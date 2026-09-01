//! `kairos-server` — the axum binary: `/api/*` JSON routes, tower
//! middleware (tenant resolution, OIDC bearer validation, tracing, CORS),
//! the `/mcp` endpoint, and serving the Leptos frontend at `/`.
//!
//! Startup (KAIROS-T-0007): before doing anything else (and, once the axum
//! server lands, before binding), pending public-schema migrations are
//! applied against `DATABASE_URL`. A missing or unreachable `DATABASE_URL`
//! fails fast with a clear message and a non-zero exit.
//!
//! Subcommands (KAIROS-T-0007/T-0008) — each is what the matching
//! `angreal db <name>` task invokes, so dev tooling, tests, and server boot
//! share one code path (the embedded migration trees, per KAIROS-A-0009):
//!
//! - `serve` — run the axum server (KAIROS-T-0017): binds
//!   `KAIROS_BIND_ADDR` (default 127.0.0.1:8080), applies pending public
//!   migrations first (below), initializes tracing per KAIROS-A-0013
//!   (`KAIROS_LOG_LEVEL` / `KAIROS_LOG_FORMAT`), serves `/healthz` and the
//!   auth+tenant-protected `/api` routes, and drains gracefully on ctrl-c
//! - `migrate` — run pending public-schema migrations and exit
//! - `create-tenant --slug <slug> [--name <name>]` — provision a tenant
//!   (org row + `org_{slug}` schema + tenant migrations + seeded defaults)
//! - `drop-tenant --slug <slug> --confirm` — destroy a tenant (schema
//!   CASCADE + org row); refuses without `--confirm`
//! - `migrate-tenants` — run pending tenant migrations in every tenant schema
//! - `list-tenants` — list provisioned tenants
//! - `seed-demo [--force]` — seed the `demo` tenant fixture (KAIROS-T-0035,
//!   A-0012): fixture users/teams/boards/items; existing demo tenant is a
//!   polite no-op without `--force`
//!
//! The rest of main is still the KAIROS-T-0002 placeholder: prints name and
//! version and exits 0.

use std::process::ExitCode;

use diesel::pg::PgConnection;

/// Establish the migration connection from `DATABASE_URL` and apply pending
/// public-schema migrations, printing what was applied. Every subcommand
/// starts here so the `public.organizations` / `system_*` tables always
/// exist before tenant operations run. Migrations use a dedicated
/// synchronous connection (`diesel_migrations` is sync); see
/// `kairos_db::migrations` for the rationale.
fn connect_and_migrate_public() -> Result<PgConnection, String> {
    let database_url = std::env::var("DATABASE_URL").map_err(|_| {
        "DATABASE_URL is not set; it is required to run schema migrations \
         (e.g. postgres://kairos:kairos@localhost:41432/kairos)"
            .to_string()
    })?;

    let mut conn = kairos_db::establish_migration_connection(&database_url)
        .map_err(|e| format!("cannot reach database at DATABASE_URL: {e}"))?;

    let applied = kairos_db::run_public_migrations(&mut conn)
        .map_err(|e| format!("public schema migration failed: {e}"))?;

    if applied.is_empty() {
        println!("public schema migrations: up to date (no pending migrations)");
    } else {
        for version in &applied {
            println!("applied public schema migration: {version}");
        }
    }
    Ok(conn)
}

/// Value of `--flag <value>` in `args`, if present.
fn flag_value(args: &[String], flag: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == flag) {
        None => Ok(None),
        Some(i) => match args.get(i + 1) {
            Some(v) if !v.starts_with("--") => Ok(Some(v.clone())),
            _ => Err(format!("{flag} requires a value")),
        },
    }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn create_tenant(conn: &mut PgConnection, args: &[String]) -> Result<(), String> {
    let slug = flag_value(args, "--slug")?
        .ok_or("create-tenant requires --slug <slug> (e.g. create-tenant --slug acme)")?;
    let name = flag_value(args, "--name")?.unwrap_or_else(|| slug.clone());

    let report = kairos_db::provision_tenant(conn, &slug, &name)
        .map_err(|e| format!("create-tenant: {e}"))?;

    println!(
        "provisioned tenant '{}' (schema {}): {} tenant migration(s) applied, \
         boards created: {}, {} template(s) and {} metadata definition(s) copied from system defaults",
        report.slug,
        report.schema,
        report.migrations_applied.len(),
        report.boards_created.join(", "),
        report.templates_copied,
        report.metadata_definitions_copied,
    );
    Ok(())
}

fn drop_tenant(conn: &mut PgConnection, args: &[String]) -> Result<(), String> {
    let slug = flag_value(args, "--slug")?
        .ok_or("drop-tenant requires --slug <slug> (e.g. drop-tenant --slug acme --confirm)")?;
    let confirm = has_flag(args, "--confirm");

    kairos_db::drop_tenant(conn, &slug, confirm).map_err(|e| format!("drop-tenant: {e}"))?;
    println!(
        "dropped tenant '{slug}': schema org_{slug} removed (CASCADE), organization row deleted"
    );
    Ok(())
}

fn migrate_tenants(conn: &mut PgConnection) -> Result<(), String> {
    let outcomes =
        kairos_db::migrate_all_tenants(conn).map_err(|e| format!("migrate-tenants: {e}"))?;
    if outcomes.is_empty() {
        println!("no tenants provisioned - nothing to migrate");
        return Ok(());
    }
    for outcome in &outcomes {
        if outcome.applied.is_empty() {
            println!(
                "{}: up to date (no pending tenant migrations)",
                outcome.schema
            );
        } else {
            for version in &outcome.applied {
                println!("{}: applied tenant migration {version}", outcome.schema);
            }
        }
    }
    println!(
        "tenant migrations complete across {} tenant(s)",
        outcomes.len()
    );
    Ok(())
}

fn list_tenants(conn: &mut PgConnection) -> Result<(), String> {
    let tenants = kairos_db::list_tenants(conn).map_err(|e| format!("list-tenants: {e}"))?;
    if tenants.is_empty() {
        println!("no tenants provisioned");
        return Ok(());
    }
    println!("{:<24} {:<32} SCHEMA", "SLUG", "NAME");
    for t in &tenants {
        let schema = if t.schema_exists {
            format!("org_{}", t.slug)
        } else {
            format!("org_{} (MISSING)", t.slug)
        };
        println!("{:<24} {:<32} {schema}", t.slug, t.name);
    }
    Ok(())
}

/// The `seed-demo` subcommand (KAIROS-T-0035, KAIROS-A-0012 fixtures):
/// provision the `demo` tenant with the fixture users/teams/boards/items
/// via [`kairos_db::seed_demo`]. Without `--force`, an existing demo
/// tenant is a polite no-op (exit 0) naming the flag; with `--force` the
/// demo tenant is dropped and reseeded.
fn seed_demo(conn: &mut PgConnection, args: &[String]) -> Result<(), String> {
    let force = has_flag(args, "--force");
    match kairos_db::seed_demo(conn, force) {
        Err(kairos_db::SeedError::AlreadySeeded) => {
            println!(
                "tenant 'demo' is already seeded - nothing to do \
                 (pass --force to drop and recreate the demo tenant)"
            );
            Ok(())
        }
        Err(e) => Err(format!("seed-demo: {e}")),
        Ok(report) => {
            if report.recreated {
                println!("dropped existing tenant 'demo' (--force) before reseeding");
            }
            println!(
                "seeded tenant '{}' (schema {}): {} users (alice=org admin, bob, carol), \
                 {} teams, {} boards, {} delivery stream(s), {} strategy, {} initiatives \
                 (2 buckets), {} tasks, {} document, {} ADRs, {} relationship edges, \
                 {} metadata stamp(s)",
                report.slug,
                report.schema,
                report.users,
                report.teams,
                report.boards,
                report.streams,
                report.strategies,
                report.initiatives,
                report.tasks,
                report.documents,
                report.adrs,
                report.edges,
                report.metadata_stamps,
            );
            println!("short codes: {}", report.short_codes.join(", "));
            Ok(())
        }
    }
}

/// The `serve` subcommand (KAIROS-T-0017): fail-fast config, tracing init
/// per KAIROS-A-0013, then the axum serve loop on a fresh tokio runtime
/// (main stays sync because the migration path is sync, KAIROS-T-0007).
fn serve() -> Result<(), String> {
    let config = kairos_server::config::AppConfig::from_env().map_err(|e| e.to_string())?;
    kairos_server::init_tracing(&config);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start tokio runtime: {e}"))?;
    runtime.block_on(kairos_server::app::serve(config))
}

fn run() -> Result<bool, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let subcommand = args.first().map(String::as_str);

    // Every path (including plain server startup) first applies pending
    // public migrations on a dedicated sync connection (KAIROS-T-0007).
    let mut conn = connect_and_migrate_public()?;

    match subcommand {
        None => Ok(false), // fall through to placeholder output
        Some("serve") => {
            drop(conn); // the server builds its own async pool
            serve().map(|_| true)
        }
        Some("migrate") => Ok(true),
        Some("create-tenant") => create_tenant(&mut conn, &args[1..]).map(|_| true),
        Some("drop-tenant") => drop_tenant(&mut conn, &args[1..]).map(|_| true),
        Some("migrate-tenants") => migrate_tenants(&mut conn).map(|_| true),
        Some("list-tenants") => list_tenants(&mut conn).map(|_| true),
        Some("seed-demo") => seed_demo(&mut conn, &args[1..]).map(|_| true),
        Some(other) => Err(format!(
            "unknown subcommand {other:?}; expected one of: serve, migrate, create-tenant, \
             drop-tenant, migrate-tenants, list-tenants, seed-demo"
        )),
    }
}

fn main() -> ExitCode {
    match run() {
        Err(message) => {
            eprintln!("kairos-server: {message}");
            ExitCode::FAILURE
        }
        Ok(true) => ExitCode::SUCCESS, // subcommand handled, exit
        Ok(false) => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{flag_value, has_flag};

    #[test]
    fn smoke() {
        assert_eq!(env!("CARGO_PKG_NAME"), "kairos-server");
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flag_value_parses_pairs() {
        let a = args(&["--slug", "acme", "--name", "Acme Inc"]);
        assert_eq!(flag_value(&a, "--slug").unwrap().as_deref(), Some("acme"));
        assert_eq!(
            flag_value(&a, "--name").unwrap().as_deref(),
            Some("Acme Inc")
        );
        assert_eq!(flag_value(&a, "--missing").unwrap(), None);
    }

    #[test]
    fn flag_value_rejects_missing_value() {
        assert!(flag_value(&args(&["--slug"]), "--slug").is_err());
        assert!(flag_value(&args(&["--slug", "--confirm"]), "--slug").is_err());
    }

    #[test]
    fn has_flag_detects_presence() {
        let a = args(&["--slug", "acme", "--confirm"]);
        assert!(has_flag(&a, "--confirm"));
        assert!(!has_flag(&a, "--force"));
    }
}
