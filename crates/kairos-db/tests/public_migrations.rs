//! Integration test for the embedded public-schema migrations
//! (KAIROS-T-0007, DDL per KAIROS-S-0004).
//!
//! Runs against the real compose Postgres (`angreal services up` /
//! `angreal test integration` — the database is never mocked, per
//! KAIROS-A-0012). Connection details come from `DATABASE_URL` if set,
//! otherwise the same default the angreal tooling uses
//! (`.angreal/task_db.py`): `postgres://kairos:kairos@localhost:41432/kairos`.
//!
//! For isolation the test drops and recreates a dedicated scratch database
//! (`kairos_public_migrations_test`) on the same server, so it never
//! touches the dev `kairos` database or interferes with other tests.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;

use kairos_db::run_public_migrations;

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const SCRATCH_DB: &str = "kairos_public_migrations_test";

/// The public-schema tables: the 8 defined by KAIROS-S-0004 plus the
/// KAIROS-T-0078 system metadata scopes.
const EXPECTED_TABLES: [&str; 9] = [
    "organization_members",
    "organizations",
    "system_board_defaults",
    "system_metadata_definition_scopes",
    "system_metadata_definitions",
    "system_metadata_enum_options",
    "system_template_metadata",
    "system_templates",
    "users",
];

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Replace the database name (final path segment) in a postgres URL.
fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

#[derive(QueryableByName)]
struct TableName {
    #[diesel(sql_type = diesel::sql_types::Text)]
    table_name: String,
}

fn public_base_tables(conn: &mut PgConnection) -> Vec<String> {
    let mut tables: Vec<String> = sql_query(
        "SELECT table_name::text AS table_name \
         FROM information_schema.tables \
         WHERE table_schema = 'public' \
           AND table_type = 'BASE TABLE' \
           AND table_name <> '__diesel_schema_migrations'",
    )
    .load::<TableName>(conn)
    .expect("querying information_schema.tables")
    .into_iter()
    .map(|t| t.table_name)
    .collect();
    tables.sort();
    tables
}

/// Expect a database error of `kind` from `result`.
fn assert_database_error_kind<T: std::fmt::Debug>(
    result: Result<T, DieselError>,
    kind: DatabaseErrorKind,
    context: &str,
) {
    match result {
        Err(DieselError::DatabaseError(actual, _)) if actual == kind => {}
        other => panic!("{context}: expected {kind:?}, got {other:?}"),
    }
}

#[test]
fn public_migrations_from_empty_database() {
    let admin_url = admin_database_url();

    // Drop + recreate the scratch database for isolation. CREATE/DROP
    // DATABASE must run on a connection to another database.
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");

    let scratch_url = with_database(&admin_url, SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");

    // (a) From empty, the runner creates all 8 S-0004 public tables.
    let applied = run_public_migrations(&mut conn).expect("running public migrations from empty");
    assert!(
        !applied.is_empty(),
        "first run against an empty database should apply at least one migration"
    );
    assert_eq!(
        public_base_tables(&mut conn),
        EXPECTED_TABLES,
        "public schema should contain exactly the expected public tables"
    );

    // (b) Re-running is a no-op (idempotent).
    let reapplied = run_public_migrations(&mut conn).expect("re-running public migrations");
    assert!(
        reapplied.is_empty(),
        "second run should apply nothing, applied: {reapplied:?}"
    );
    assert_eq!(public_base_tables(&mut conn), EXPECTED_TABLES);

    // (c) Duplicate organization slug violates the UNIQUE constraint.
    sql_query("INSERT INTO organizations (name, slug) VALUES ('Acme', 'acme')")
        .execute(&mut conn)
        .expect("inserting a valid organization");
    assert_database_error_kind(
        sql_query("INSERT INTO organizations (name, slug) VALUES ('Acme Two', 'acme')")
            .execute(&mut conn),
        DatabaseErrorKind::UniqueViolation,
        "duplicate organization slug",
    );

    // (d) An invalid slug (uppercase) violates the CHECK constraint
    //     (slug ~ '^[a-z][a-z0-9_-]{1,62}$').
    assert_database_error_kind(
        sql_query("INSERT INTO organizations (name, slug) VALUES ('Shouty', 'ACME')")
            .execute(&mut conn),
        DatabaseErrorKind::CheckViolation,
        "uppercase organization slug",
    );

    // Clean up the scratch database.
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database after test");
}
