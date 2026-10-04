//! COLLIERY-T-3109 — the migration `document_board_required`: each
//! document has an owner board.
//!
//! The migration makes `documents.board_id` NOT NULL. It does not choose a
//! board for a document: when a document, live or archived, has no board,
//! it refuses, names the documents, and changes nothing. The operator gives
//! each document a board and runs the migration again.
//!
//! The state of a tenant before the migration is made with the down
//! migration (the column can be NULL again) and with documents that have
//! no board, written by SQL.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_db::models::enums::BoardLevel;
use kairos_db::{migrate_all_tenants, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The migration under test, verbatim: what a deployment runs is what this
/// test runs.
const UP: &str =
    include_str!("../migrations/tenant/2026-10-04-000000_document_board_required/up.sql");
const DOWN: &str =
    include_str!("../migrations/tenant/2026-10-04-000000_document_board_required/down.sql");
/// The version of the migration in `__diesel_schema_migrations`.
const VERSION: &str = "20261004000000";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

fn pin(conn: &mut PgConnection) {
    sql_query("SET search_path TO org_acme, public")
        .execute(conn)
        .expect("pinning search_path");
}

/// A scratch database with the tenant `acme`, pinned to its schema.
fn tenant(scratch_db: &str) -> (PgConnection, PgConnection) {
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {scratch_db}"))
        .execute(&mut admin_conn)
        .expect("creating scratch database");
    let scratch_url = with_database(&admin_url, scratch_db);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    pin(&mut conn);
    (admin_conn, conn)
}

fn drop_database(mut admin_conn: PgConnection, conn: PgConnection, scratch_db: &str) {
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .count
}

/// Is `documents.board_id` NOT NULL?
fn board_is_required(conn: &mut PgConnection) -> bool {
    #[derive(QueryableByName)]
    struct Nullable {
        #[diesel(sql_type = Text)]
        is_nullable: String,
    }
    sql_query(
        "SELECT is_nullable::text AS is_nullable FROM information_schema.columns \
          WHERE table_schema = 'org_acme' AND table_name = 'documents' \
            AND column_name = 'board_id'",
    )
    .get_result::<Nullable>(conn)
    .expect("reading the column")
    .is_nullable
        == "NO"
}

fn board_of_level(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::board_level.eq(level))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .expect("the board of the level")
}

/// The state of a tenant before the migration: the column can be NULL, and
/// no record says that the migration ran.
fn make_the_old_state(conn: &mut PgConnection) {
    conn.batch_execute(DOWN).expect("the down migration");
    let removed = sql_query(format!(
        "DELETE FROM __diesel_schema_migrations WHERE version = '{VERSION}'"
    ))
    .execute(conn)
    .expect("removing the record of the migration");
    assert_eq!(removed, 1, "the migration has the version {VERSION}");
    assert!(!board_is_required(conn), "the old state allows no board");
}

/// A document written by SQL, as the data of before: `board` is `None` for
/// a document with no board.
fn old_document(conn: &mut PgConnection, code: &str, board: Option<Uuid>, archived: bool) {
    sql_query(
        "INSERT INTO documents (short_code, title, board_id, created_by, updated_by, deleted_at) \
         VALUES ($1, 'Old', $2, gen_random_uuid(), gen_random_uuid(), \
                 CASE WHEN $3 THEN now() END)",
    )
    .bind::<Text, _>(code)
    .bind::<diesel::sql_types::Nullable<SqlUuid>, _>(board)
    .bind::<diesel::sql_types::Bool, _>(archived)
    .execute(conn)
    .expect("writing an old document");
}

#[test]
fn the_migration_refuses_while_a_document_has_no_board() {
    const SCRATCH_DB: &str = "kairos_document_board_required_t3109_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);
    let board = board_of_level(&mut conn, BoardLevel::Initiative);

    // A new tenant has the migration, and the column is NOT NULL.
    assert!(board_is_required(&mut conn));

    make_the_old_state(&mut conn);
    old_document(&mut conn, "ACME-D-0001", Some(board), false);
    old_document(&mut conn, "ACME-D-0002", None, false);
    old_document(&mut conn, "ACME-D-0003", None, true);

    // The migration refuses: it names the count, the archived count and
    // the codes, and it changes nothing.
    let refusal = migrate_all_tenants(&mut conn).expect_err("the migration refuses");
    let message = refusal.to_string();
    assert!(message.contains("COLLIERY-T-3109"), "{message}");
    assert!(
        message.contains("2 documents have no owner board (1 of them archived)"),
        "{message}"
    );
    assert!(
        message.contains("ACME-D-0002, ACME-D-0003 (archived)"),
        "{message}"
    );
    assert!(
        message.contains("PATCH /api/documents/{code}/board"),
        "{message}"
    );
    pin(&mut conn);
    assert!(!board_is_required(&mut conn));
    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS count FROM documents WHERE board_id IS NULL"
        ),
        2,
        "the migration chooses no board"
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS count FROM __diesel_schema_migrations \
                  WHERE version = '{VERSION}'"
            )
        ),
        0,
        "a refused migration has no record"
    );

    // An ARCHIVED document with no board also stops the migration.
    sql_query("UPDATE documents SET board_id = $1 WHERE short_code = 'ACME-D-0002'")
        .bind::<SqlUuid, _>(board)
        .execute(&mut conn)
        .expect("a board for the live document");
    let refusal = migrate_all_tenants(&mut conn).expect_err("the archived document refuses");
    let message = refusal.to_string();
    assert!(
        message.contains("1 documents have no owner board (1 of them archived)"),
        "{message}"
    );
    assert!(message.contains("ACME-D-0003 (archived)"), "{message}");
    pin(&mut conn);

    // When each document has a board, the migration passes.
    sql_query("UPDATE documents SET board_id = $1 WHERE short_code = 'ACME-D-0003'")
        .bind::<SqlUuid, _>(board)
        .execute(&mut conn)
        .expect("a board for the archived document");
    let outcomes = migrate_all_tenants(&mut conn).expect("the migration passes");
    assert_eq!(outcomes[0].applied, [VERSION], "{:?}", outcomes[0]);
    pin(&mut conn);
    assert!(board_is_required(&mut conn));
    assert_eq!(
        count(&mut conn, "SELECT count(*) AS count FROM documents"),
        3,
        "no row is removed"
    );

    // A document with no board cannot be written now.
    let refused = sql_query(
        "INSERT INTO documents (short_code, title, created_by, updated_by) \
         VALUES ('ACME-D-0004', 'New', gen_random_uuid(), gen_random_uuid())",
    )
    .execute(&mut conn);
    assert!(refused.is_err(), "a document with no board is refused");

    // The migration can run 2 times.
    conn.batch_execute(UP).expect("the second run passes");
    assert!(board_is_required(&mut conn));

    drop_database(admin_conn, conn, SCRATCH_DB);
}
