//! Embedded Diesel migrations (KAIROS-A-0009, KAIROS-T-0007, KAIROS-T-0008).
//!
//! Two trees, both transcribed from KAIROS-S-0004 and compiled into the
//! binary with `embed_migrations!` so the server can apply pending
//! migrations at startup and tests can migrate scratch databases without
//! any external tooling:
//!
//! - `migrations/public/` — the shared public schema (T-0007).
//! - `migrations/tenant/` — the per-tenant schema (T-0008). This tree is
//!   written with UNQUALIFIED names and is executed with `search_path`
//!   pinned to a target `org_{slug}` schema (see [`crate::tenant`]), so the
//!   same migrations provision and upgrade every tenant schema, each with
//!   its own `__diesel_schema_migrations` bookkeeping table.
//!
//! # Why a sync `PgConnection` when the runtime pool is async?
//!
//! `diesel_migrations` is synchronous — its `MigrationHarness` is only
//! implemented for blocking `diesel` connections, not `diesel-async`'s
//! `AsyncPgConnection`. Migrations run once at startup (or from tests /
//! `angreal db migrate`) before the async pool serves any traffic, so we
//! deliberately establish a dedicated short-lived sync [`PgConnection`]
//! for the migration path and drop it afterwards. The bb8/diesel-async
//! pool (per KAIROS-A-0009) remains the runtime data path.

use diesel::Connection;
use diesel::pg::PgConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

/// The embedded public-schema migration tree (`crates/kairos-db/migrations/public`).
pub const PUBLIC_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/public");

/// The embedded tenant-schema migration tree (`crates/kairos-db/migrations/tenant`).
///
/// Always run with `search_path` pinned to the target tenant schema — use
/// [`crate::tenant::provision_tenant`] / [`crate::tenant::migrate_all_tenants`]
/// rather than running this tree directly.
pub const TENANT_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/tenant");

/// Errors from establishing a migration connection or applying migrations.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// Could not connect to the database (bad URL, server unreachable,
    /// authentication failure, ...).
    #[error("failed to connect to database for migrations: {0}")]
    Connection(#[from] diesel::ConnectionError),
    /// A migration failed to apply.
    #[error("failed to run public schema migrations: {0}")]
    Migration(String),
    /// A required PostgreSQL extension is not installable on this server
    /// (KAIROS-T-0187). Checked before any migration runs, because the
    /// alternative is PostgreSQL's own `extension "vector" is not available`
    /// arriving from the middle of a migration run with no hint what to do.
    #[error(
        "required PostgreSQL extension {name:?} is not available on this server. \
         {hint} See https://colliery-io.github.io/kairos/how-to/install-with-helm.html \
         for the database Kairos needs."
    )]
    MissingExtension {
        /// The extension that could not be found.
        name: &'static str,
        /// What the operator should do about it.
        hint: &'static str,
    },
}

/// Extensions Kairos requires of the database it is pointed at, with the
/// sentence an operator needs when one is missing.
///
/// `vector` is required rather than optional per KAIROS-A-0021 rule 2. The
/// alternative — a schema that differs between deployments, so that every later
/// migration and every read path has to ask whether the embedding tables exist —
/// is a permanent fork of the schema maintained to spare one `CREATE EXTENSION`.
/// pgvector is available on RDS, Cloud SQL and Azure Database for PostgreSQL, so
/// "bring your own Postgres" survives the requirement.
const REQUIRED_EXTENSIONS: &[(&str, &str)] = &[(
    "vector",
    "Install pgvector (package `postgresql-16-pgvector` on Debian/Ubuntu, \
     `pgvector` in Homebrew, or the `pgvector/pgvector` image), or enable it \
     on your managed instance — it ships with RDS, Cloud SQL and Azure.",
)];

/// Check that every extension in [`REQUIRED_EXTENSIONS`] can be created on this
/// server, before any migration tries to.
///
/// Read-only. `pg_available_extensions` lists what the server has files for,
/// which is what `CREATE EXTENSION` needs — an extension already installed is
/// listed too, so this passes on a database that is already set up.
pub fn check_required_extensions(conn: &mut PgConnection) -> Result<(), MigrationError> {
    check_extensions(conn, REQUIRED_EXTENSIONS)
}

/// [`check_required_extensions`] against an explicit list.
///
/// Separate from the wrapper so the absent case is testable: on a correctly
/// provisioned server every required extension is present by definition, so a
/// test that can only ask about `vector` can never exercise the branch that
/// matters. Integration tests pass a name no server has.
pub fn check_extensions(
    conn: &mut PgConnection,
    required: &[(&'static str, &'static str)],
) -> Result<(), MigrationError> {
    use diesel::sql_types::Text;
    use diesel::{QueryableByName, RunQueryDsl, sql_query};

    #[derive(QueryableByName)]
    struct NameRow {
        #[diesel(sql_type = Text)]
        name: String,
    }

    for (name, hint) in required {
        let found: Vec<NameRow> =
            sql_query("SELECT name::text AS name FROM pg_available_extensions WHERE name = $1")
                .bind::<Text, _>(name)
                .load(conn)
                .map_err(|e| MigrationError::Migration(e.to_string()))?;
        // Compare the returned name rather than just counting rows: it uses the
        // value the server actually gave us, so a mis-bound parameter reads as
        // missing rather than as present.
        if !found.iter().any(|row| row.name == *name) {
            return Err(MigrationError::MissingExtension { name, hint });
        }
    }
    Ok(())
}

/// Establish the dedicated synchronous connection used for running
/// migrations (see module docs for why this is sync while the runtime
/// pool is async).
pub fn establish_migration_connection(database_url: &str) -> Result<PgConnection, MigrationError> {
    Ok(PgConnection::establish(database_url)?)
}

/// Run all pending public-schema migrations on `conn`.
///
/// Returns the list of migration versions applied by this call. Already
/// applied migrations are skipped (tracked in `__diesel_schema_migrations`),
/// so calling this repeatedly is idempotent: a second run returns an empty
/// list and changes nothing.
///
/// Used by `kairos-server` at startup (and via its `migrate` subcommand,
/// which backs `angreal db migrate`) and by integration tests.
pub fn run_public_migrations(conn: &mut PgConnection) -> Result<Vec<String>, MigrationError> {
    // Before anything is applied: fail with a sentence an operator can act on
    // rather than from inside a migration with PostgreSQL's own wording
    // (KAIROS-T-0187). It lives here rather than at the call sites so that the
    // server, `angreal db migrate` and every integration test get it without
    // each remembering to ask.
    check_required_extensions(conn)?;

    let applied = conn
        .run_pending_migrations(PUBLIC_MIGRATIONS)
        .map_err(|e| MigrationError::Migration(e.to_string()))?;
    Ok(applied.iter().map(|v| v.to_string()).collect())
}

/// Whether the embedded public-schema migration tree has any migration not
/// yet applied to `conn`. Backs the `/readyz` pending-migration check
/// (KAIROS-A-0013, KAIROS-T-0049): a binary whose embedded migrations are
/// ahead of the database is not ready to serve. Read-only — never applies
/// anything.
pub fn has_pending_public_migrations(conn: &mut PgConnection) -> Result<bool, MigrationError> {
    conn.has_pending_migration(PUBLIC_MIGRATIONS)
        .map_err(|e| MigrationError::Migration(e.to_string()))
}
