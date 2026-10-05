//! KAIROS-T-0318 — the migration `repository_code_index_build`: each
//! repository has the setting `code_index_build`, `on` or `off`.
//!
//! Proves: a row that exists gets `on`; the CHECK refuses another value;
//! the down.sql removes the column; the up.sql runs again with no error.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`); owns
//! the scratch database `kairos_repository_code_index_build_t0318_test`.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use uuid::Uuid;

use kairos_db::{provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_repository_code_index_build_t0318_test";
const UP_SQL: &str =
    include_str!("../migrations/tenant/2026-10-05-000000_repository_code_index_build/up.sql");
const DOWN_SQL: &str =
    include_str!("../migrations/tenant/2026-10-05-000000_repository_code_index_build/down.sql");

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

fn column_count(conn: &mut PgConnection) -> i64 {
    sql_query(
        "SELECT count(*) AS count FROM information_schema.columns \
         WHERE table_schema = 'org_acme' AND table_name = 'repositories' \
           AND column_name = 'code_index_build'",
    )
    .get_result::<CountRow>(conn)
    .expect("column count")
    .count
}

fn setting(conn: &mut PgConnection, id: Uuid) -> String {
    sql_query("SELECT code_index_build AS value FROM org_acme.repositories WHERE id = $1")
        .bind::<diesel::sql_types::Uuid, _>(id)
        .get_result::<TextRow>(conn)
        .expect("the setting")
        .value
}

#[test]
fn each_repository_has_the_setting_code_index_build() {
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url)
        .expect("connecting to compose postgres (is the stack up? `angreal services up`)");
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch db");
    let mut conn = PgConnection::establish(&with_database(&admin_url, SCRATCH_DB))
        .expect("connecting to scratch db");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");

    // Given a repository made before the migration
    let team = Uuid::new_v4();
    let repo = Uuid::new_v4();
    let actor = Uuid::new_v4();
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .unwrap();
    conn.batch_execute(DOWN_SQL).expect("down.sql");
    assert_eq!(column_count(&mut conn), 0, "down.sql removes the column");
    sql_query(
        "INSERT INTO org_acme.teams (id, name, slug, team_type) \
         VALUES ($1, 'Platform', 'platform', 'platform')",
    )
    .bind::<diesel::sql_types::Uuid, _>(team)
    .execute(&mut conn)
    .expect("team");
    sql_query(
        "INSERT INTO org_acme.repositories \
         (id, slug, forge, repo_full_name, repo_url, default_branch, team_id, \
          description, created_by, updated_by) \
         VALUES ($1, 'site', 'github', 'acme/site', 'https://github.com/acme/site', \
                 'main', $2, '', $3, $3)",
    )
    .bind::<diesel::sql_types::Uuid, _>(repo)
    .bind::<diesel::sql_types::Uuid, _>(team)
    .bind::<diesel::sql_types::Uuid, _>(actor)
    .execute(&mut conn)
    .expect("repository");

    // When the migration runs (2 times: it is re-runnable)
    conn.batch_execute(UP_SQL).expect("up.sql");
    conn.batch_execute(UP_SQL).expect("up.sql again");

    // Then the repository has the setting on
    assert_eq!(column_count(&mut conn), 1);
    assert_eq!(setting(&mut conn, repo), "on");

    // And off is a value, and another value is refused
    sql_query("UPDATE org_acme.repositories SET code_index_build = 'off' WHERE id = $1")
        .bind::<diesel::sql_types::Uuid, _>(repo)
        .execute(&mut conn)
        .expect("off");
    assert_eq!(setting(&mut conn, repo), "off");
    let refused =
        sql_query("UPDATE org_acme.repositories SET code_index_build = 'maybe' WHERE id = $1")
            .bind::<diesel::sql_types::Uuid, _>(repo)
            .execute(&mut conn)
            .expect_err("the CHECK refuses maybe");
    assert!(
        refused
            .to_string()
            .contains("repositories_code_index_build_check"),
        "{refused}"
    );
    assert_eq!(setting(&mut conn, repo), "off");
}
