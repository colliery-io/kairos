//! The `2026-09-22-000000_repositories` migration on POPULATED tables
//! (KAIROS-T-0113). The provisioning upgrade-path test runs it on an empty
//! `forge_connections`; this one seeds the pre-migration shape with the
//! rows that matter — slug collisions across forges and case, a
//! soft-deleted team-less connection, links on a connection — and proves:
//! backfilled slugs are distinct, valid and equal to the Rust derivation;
//! every connection (live or dead) gets a repository; links still resolve;
//! the fail-loud RAISE fires for a live team-less connection; and the DOWN
//! migration restores the old shape (then up re-applies cleanly).
//!
//! Runs against the LIVE compose Postgres (`angreal services up`); owns
//! the scratch database `kairos_repositories_migration_test`.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use uuid::Uuid;

use kairos_core::repositories::{is_valid_slug, slug_from_full_name};
use kairos_db::{migrate_all_tenants, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_repositories_migration_test";
const DOWN_SQL: &str = include_str!("../migrations/tenant/2026-09-22-000000_repositories/down.sql");
/// The diesel bookkeeping version of the migration above — its directory
/// timestamp with the separators stripped. Named rather than taken as
/// `max(version)`: this test reverts exactly ONE migration, and
/// "the newest one" stopped being this one as soon as a later tenant
/// migration landed (KAIROS-T-0161's was the first to prove it).
const REPOSITORIES_VERSION: &str = "20260922000000";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("database url has a path");
    format!("{base}/{db_name}")
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct TextRow {
    #[diesel(sql_type = Text)]
    value: String,
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("count query failed: {sql}: {e}"))
        .count
}

fn texts(conn: &mut PgConnection, sql: &str) -> Vec<String> {
    sql_query(sql)
        .load::<TextRow>(conn)
        .unwrap_or_else(|e| panic!("query failed: {sql}: {e}"))
        .into_iter()
        .map(|r| r.value)
        .collect()
}

/// Put `org_acme` back into the pre-`repositories` shape: run the down
/// migration and forget its bookkeeping row, so `migrate_all_tenants`
/// re-applies exactly that one.
fn revert_repositories_migration(conn: &mut PgConnection) {
    sql_query("SET search_path TO org_acme, public")
        .execute(conn)
        .expect("pinning search_path");
    conn.batch_execute(DOWN_SQL).expect("running down.sql");
    let forgotten = sql_query("DELETE FROM org_acme.__diesel_schema_migrations WHERE version = $1")
        .bind::<Text, _>(REPOSITORIES_VERSION)
        .execute(conn)
        .expect("forgetting the repositories migration");
    assert_eq!(
        forgotten, 1,
        "the repositories migration must have been applied to org_acme"
    );
    sql_query("SET search_path TO public")
        .execute(conn)
        .expect("resetting search_path");
}

/// Insert one OLD-shape connection (the columns the migration drops).
fn old_connection(
    conn: &mut PgConnection,
    forge: &str,
    full_name: &str,
    team: Option<Uuid>,
    deleted: bool,
    actor: Uuid,
) -> Uuid {
    let id = Uuid::new_v4();
    sql_query(
        "INSERT INTO org_acme.forge_connections \
             (id, forge, repo_full_name, repo_url, team_id, created_by, deleted_at) \
         VALUES ($1, $2, $3, $4, $5, $6, CASE WHEN $7 THEN now() ELSE NULL END)",
    )
    .bind::<diesel::sql_types::Uuid, _>(id)
    .bind::<Text, _>(forge)
    .bind::<Text, _>(full_name)
    .bind::<Text, _>(format!("https://{forge}.com/{full_name}"))
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Uuid>, _>(team)
    .bind::<diesel::sql_types::Uuid, _>(actor)
    .bind::<diesel::sql_types::Bool, _>(deleted)
    .execute(conn)
    .expect("inserting an old-shape connection");
    id
}

#[test]
fn repositories_migration_on_populated_tables() {
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url)
        .expect("connecting to compose postgres (is the stack up? `angreal services up`)");
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch db");
    let scratch_url = with_database(&admin_url, SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch db");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");

    // A user for created_by, and two teams.
    let actor = Uuid::new_v4();
    sql_query(
        "INSERT INTO public.users (id, external_id, user_name, email, display_name) \
         VALUES ($1, 'mig:actor', 'mig:actor', 'mig@kairos.test', 'Migration Actor')",
    )
    .bind::<diesel::sql_types::Uuid, _>(actor)
    .execute(&mut conn)
    .expect("actor");
    let platform = Uuid::new_v4();
    let web = Uuid::new_v4();
    for (id, slug) in [(platform, "platform"), (web, "web")] {
        sql_query(
            "INSERT INTO org_acme.teams (id, name, slug, team_type) VALUES ($1, $2, $2, 'platform')",
        )
        .bind::<diesel::sql_types::Uuid, _>(id)
        .bind::<Text, _>(slug)
        .execute(&mut conn)
        .expect("team");
    }

    // ---- 1. back to the pre-migration shape, then populate it -----------
    revert_repositories_migration(&mut conn);
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM information_schema.tables \
             WHERE table_schema = 'org_acme' AND table_name = 'repositories'"
        ),
        0,
        "down.sql removed repositories"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM information_schema.columns \
             WHERE table_schema = 'org_acme' AND table_name = 'forge_connections' \
               AND column_name IN ('repo_full_name', 'repo_url', 'team_id')"
        ),
        3,
        "down.sql restored the three columns"
    );

    // Collisions: same name on two forges; case-only twins on one forge;
    // a name whose derivation needs trimming; a soft-deleted team-less
    // connection; and a soft-deleted twin of a live one.
    let gh_foo = old_connection(
        &mut conn,
        "github",
        "acme/foo",
        Some(platform),
        false,
        actor,
    );
    let gl_foo = old_connection(&mut conn, "gitlab", "acme/foo", Some(web), false, actor);
    let gh_foo_case = old_connection(
        &mut conn,
        "github",
        "Acme/Foo",
        Some(platform),
        false,
        actor,
    );
    let weird = old_connection(
        &mut conn,
        "github",
        "/weird//name/",
        Some(platform),
        false,
        actor,
    );
    let dead_orphan = old_connection(&mut conn, "gitlab", "acme/dead", None, true, actor);
    let dead_twin = old_connection(&mut conn, "github", "acme/foo", Some(platform), true, actor);
    // A link on the live github connection.
    sql_query(
        "INSERT INTO org_acme.item_links \
             (item_id, connection_id, kind, external_id, title, url, state, forge_updated_at) \
         VALUES (gen_random_uuid(), $1, 'pull_request', '7', 'PR', 'https://x', 'open', now())",
    )
    .bind::<diesel::sql_types::Uuid, _>(gh_foo)
    .execute(&mut conn)
    .expect("link");

    // ---- 2. migrate ---------------------------------------------------------
    let outcomes = migrate_all_tenants(&mut conn).expect("fleet migration over populated tables");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].applied.len(),
        1,
        "exactly the repositories migration"
    );

    let mut slugs = texts(
        &mut conn,
        "SELECT slug AS value FROM org_acme.repositories WHERE deleted_at IS NULL",
    );
    slugs.sort();
    // acme/foo x3 (github, gitlab, github-case) collide on `acme-foo`:
    // gitlab is alone on its forge -> `-gitlab`; the two github ones get
    // `-2`/`-3`. `/weird//name/` trims to `weird-name`.
    assert_eq!(
        slugs,
        ["acme-foo-2", "acme-foo-3", "acme-foo-gitlab", "weird-name"],
        "collision-free, valid slugs"
    );
    for slug in &slugs {
        assert!(is_valid_slug(slug), "{slug} passes the Rust vocabulary");
    }
    // The uncontested derivation equals the Rust one.
    let rust = slug_from_full_name("/weird//name/");
    assert_eq!(rust, "weird-name");
    assert!(slugs.contains(&rust));
    // Every connection, live or dead, points at a repository; the dead
    // orphan got a soft-deleted repo of its own with a borrowed team.
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM org_acme.forge_connections WHERE repository_id IS NULL"
        ),
        0
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM org_acme.repositories WHERE deleted_at IS NOT NULL"
        ),
        1,
        "the dead orphan's twin"
    );
    // The soft-deleted twin of a LIVE pair shares the live repository.
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) FROM org_acme.forge_connections a \
                 JOIN org_acme.forge_connections b ON a.repository_id = b.repository_id \
                 WHERE a.id = '{gh_foo}' AND b.id = '{dead_twin}'"
            ),
        ),
        1
    );
    // Links still resolve through the join.
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) FROM org_acme.item_links l \
                 JOIN org_acme.forge_connections c ON c.id = l.connection_id \
                 JOIN org_acme.repositories r ON r.id = c.repository_id \
                 WHERE c.id = '{gh_foo}' AND r.slug LIKE 'acme-foo-%'"
            ),
        ),
        1
    );
    let _ = (gl_foo, gh_foo_case, weird, dead_orphan);

    // ---- 3. down restores the old shape with the right values ------------
    revert_repositories_migration(&mut conn);
    let mut names = texts(
        &mut conn,
        "SELECT repo_full_name AS value FROM org_acme.forge_connections",
    );
    names.sort();
    assert_eq!(
        names,
        [
            "/weird//name/",
            "Acme/Foo",
            "acme/dead",
            "acme/foo",
            "acme/foo",
            "acme/foo"
        ]
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM org_acme.forge_connections WHERE team_id IS NULL"
        ),
        0,
        "down copies the (borrowed) team back onto the dead orphan"
    );

    // ---- 4. up re-applies cleanly (re-runnability) -------------------------
    let outcomes = migrate_all_tenants(&mut conn).expect("re-applying up after down");
    assert_eq!(outcomes[0].applied.len(), 1);
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) FROM org_acme.repositories WHERE deleted_at IS NULL"
        ),
        4
    );

    // ---- 5. the fail-loud case: a LIVE team-less connection -------------
    revert_repositories_migration(&mut conn);
    let orphan = old_connection(&mut conn, "github", "acme/orphan", None, false, actor);
    let err = migrate_all_tenants(&mut conn).expect_err("a live team-less connection must fail");
    let message = err.to_string();
    assert!(
        message.contains("KAIROS-T-0103") && message.contains(&orphan.to_string()),
        "the RAISE names the offending connection: {message}"
    );

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db after test");
}
