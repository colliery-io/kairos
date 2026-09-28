//! COLLIERY-T-0255 — two live boards cannot have the same slug, in the
//! database layer: the migration `boards_live_slug_uniqueness` on a tenant
//! WITH two live boards of one slug and on a tenant WITHOUT, and the check
//! of `create_board`.
//!
//! The migration runs later on tenants with real data, so the test is about
//! what it does to data:
//!
//! - the oldest board keeps the slug, and each later board gets `<slug>-2`,
//!   `<slug>-3`, ... with a number that no live board has,
//! - a deleted board is not read and not changed,
//! - a board with a slug of its own is not changed,
//! - the index is there after the migration, and a second run changes
//!   nothing.
//!
//! The boards of the old data are written directly to the table, with the
//! index removed: that is the state of a tenant before the migration.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_db::models::enums::BoardLevel;
use kairos_db::{BoardError, boards, create_board, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The migration under test, verbatim: what a deployment runs is what this
/// test runs.
const UP: &str =
    include_str!("../migrations/tenant/2026-09-28-000000_boards_live_slug_uniqueness/up.sql");
const DOWN: &str =
    include_str!("../migrations/tenant/2026-09-28-000000_boards_live_slug_uniqueness/down.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
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
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    (admin_conn, conn)
}

fn drop_database(mut admin_conn: PgConnection, conn: PgConnection, scratch_db: &str) {
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

/// The state of a tenant before the migration: no index on the slug.
fn remove_the_index(conn: &mut PgConnection) {
    conn.batch_execute(DOWN).expect("the down migration");
    assert!(
        !index_is_there(conn),
        "the down migration removes the index"
    );
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn index_is_there(conn: &mut PgConnection) -> bool {
    sql_query(
        "SELECT count(*) AS count FROM pg_indexes \
          WHERE schemaname = 'org_acme' AND indexname = 'boards_live_slug_key'",
    )
    .get_result::<Count>(conn)
    .expect("reading pg_indexes")
    .count
        == 1
}

/// One board of old data, written directly to the table. `created` is the
/// day of 2026-01 that the board was made. Returns its id.
fn old_board(conn: &mut PgConnection, name: &str, slug: &str, created: u32, live: bool) -> Uuid {
    let id = Uuid::new_v4();
    old_board_with_id(conn, id, name, slug, created, live);
    id
}

fn old_board_with_id(
    conn: &mut PgConnection,
    id: Uuid,
    name: &str,
    slug: &str,
    created: u32,
    live: bool,
) {
    let at = format!("2026-01-{created:02}T00:00:00Z");
    sql_query(
        "INSERT INTO boards (id, name, slug, board_level, deleted_at, created_at, updated_at) \
         VALUES ($1, $2, $3, 'initiative', \
                 CASE WHEN $4 THEN NULL ELSE $5::timestamptz END, \
                 $5::timestamptz, $5::timestamptz)",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(name)
    .bind::<Text, _>(slug)
    .bind::<diesel::sql_types::Bool, _>(live)
    .bind::<Text, _>(at)
    .execute(conn)
    .unwrap_or_else(|e| panic!("writing the old board {name}: {e}"));
}

#[derive(QueryableByName, Debug, Clone, PartialEq)]
struct Row {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    slug: String,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    #[diesel(sql_type = Timestamptz)]
    created_at: chrono::DateTime<chrono::Utc>,
    #[diesel(sql_type = Timestamptz)]
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// Each board of the tenant, live or not, in the order of its id.
fn all_boards(conn: &mut PgConnection) -> Vec<Row> {
    sql_query("SELECT id, name, slug, deleted_at, created_at, updated_at FROM boards ORDER BY id")
        .load(conn)
        .expect("reading the boards")
}

fn board(conn: &mut PgConnection, id: Uuid) -> Row {
    all_boards(conn)
        .into_iter()
        .find(|row| row.id == id)
        .expect("the board is there")
}

/// The number of live boards of each slug that more than one live board
/// has. Empty when the rule holds.
fn slugs_of_two_live_boards(conn: &mut PgConnection) -> Vec<String> {
    #[derive(QueryableByName)]
    struct Slug {
        #[diesel(sql_type = Text)]
        slug: String,
    }
    sql_query(
        "SELECT slug FROM boards WHERE deleted_at IS NULL \
          GROUP BY slug HAVING count(*) > 1 ORDER BY slug",
    )
    .load::<Slug>(conn)
    .expect("reading the slugs")
    .into_iter()
    .map(|row| row.slug)
    .collect()
}

#[test]
fn the_migration_gives_each_later_board_a_slug_of_its_own() {
    const SCRATCH_DB: &str = "kairos_board_slug_dup_t0255_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);
    remove_the_index(&mut conn);

    // ------------------------------------------------------------------
    // The old data
    // ------------------------------------------------------------------
    // Three live boards with the slug `alpha`, and a live board that has
    // `alpha-2` already.
    let alpha_first = old_board(&mut conn, "Alpha, the first", "alpha", 1, true);
    let alpha_second = old_board(&mut conn, "Alpha, the second", "alpha", 10, true);
    let alpha_third = old_board(&mut conn, "Alpha, the third", "alpha", 20, true);
    let alpha_2 = old_board(&mut conn, "Alpha 2", "alpha-2", 5, true);
    // A deleted board with the slug, older than each live board. It is not
    // a board of the tenant: it does not keep the slug.
    let alpha_deleted = old_board(&mut conn, "Alpha, deleted", "alpha", 1, false);
    // Two live boards of the same age: the id decides.
    let (low, high) = {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        if a < b { (a, b) } else { (b, a) }
    };
    old_board_with_id(&mut conn, high, "Beta, the higher id", "beta", 3, true);
    old_board_with_id(&mut conn, low, "Beta, the lower id", "beta", 3, true);
    // A live board and a deleted board with one slug: no conflict.
    let gamma_live = old_board(&mut conn, "Gamma", "gamma", 7, true);
    let gamma_deleted = old_board(&mut conn, "Gamma, deleted", "gamma", 2, false);
    // A board with columns: the columns stay with the board.
    let columns_before = {
        sql_query(format!(
            "INSERT INTO board_columns (board_id, name, position) \
             VALUES ('{alpha_second}', 'Todo', 0), ('{alpha_second}', 'Done', 1)"
        ))
        .execute(&mut conn)
        .expect("writing the columns");
        2
    };

    assert_eq!(slugs_of_two_live_boards(&mut conn), ["alpha", "beta"]);
    let before = all_boards(&mut conn);

    // ------------------------------------------------------------------
    // The migration
    // ------------------------------------------------------------------
    conn.batch_execute(UP)
        .expect("the migration passes on a tenant with two live boards of one slug");

    assert!(index_is_there(&mut conn));
    assert!(slugs_of_two_live_boards(&mut conn).is_empty());

    // The oldest keeps the slug. It is not changed at all.
    assert_eq!(
        board(&mut conn, alpha_first),
        *before.iter().find(|row| row.id == alpha_first).unwrap()
    );
    // `alpha-2` is the slug of a live board, so the second gets `alpha-3`,
    // and the third gets `alpha-4`.
    assert_eq!(board(&mut conn, alpha_second).slug, "alpha-3");
    assert_eq!(board(&mut conn, alpha_third).slug, "alpha-4");
    assert_eq!(board(&mut conn, alpha_2).slug, "alpha-2");
    // The same age: the lower id keeps the slug.
    assert_eq!(board(&mut conn, low).slug, "beta");
    assert_eq!(board(&mut conn, high).slug, "beta-2");

    // Only the slug and `updated_at` of a board with a new slug change.
    let after = all_boards(&mut conn);
    assert_eq!(after.len(), before.len(), "no board is added or removed");
    let renamed = [alpha_second, alpha_third, high];
    for (old, new) in before.iter().zip(&after) {
        assert_eq!(old.id, new.id);
        if renamed.contains(&old.id) {
            assert_eq!(new.name, old.name, "the name stays");
            assert_eq!(new.created_at, old.created_at, "the age stays");
            assert_eq!(new.deleted_at, None, "the board stays live");
            assert!(new.updated_at > old.updated_at, "{new:?}");
        } else {
            assert_eq!(new, old, "a board with a slug of its own is not changed");
        }
    }
    // A deleted board is not changed, and it has the slug of a live board.
    assert_eq!(board(&mut conn, alpha_deleted).slug, "alpha");
    assert_eq!(board(&mut conn, gamma_deleted).slug, "gamma");
    assert_eq!(board(&mut conn, gamma_live).slug, "gamma");
    // The columns stay with the board.
    let columns = sql_query(format!(
        "SELECT count(*) AS count FROM board_columns WHERE board_id = '{alpha_second}'"
    ))
    .get_result::<Count>(&mut conn)
    .expect("counting the columns")
    .count;
    assert_eq!(columns, columns_before);

    // ------------------------------------------------------------------
    // The rule after the migration
    // ------------------------------------------------------------------
    // The index refuses a second live board with a slug.
    let refused = conn
        .transaction::<_, diesel::result::Error, _>(|conn| {
            old_board_insert(conn, "alpha", true).map(|_| ())
        })
        .expect_err("the index refuses the slug of a live board");
    assert!(
        matches!(
            refused,
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                ref info
            ) if info.constraint_name() == Some("boards_live_slug_key")
        ),
        "{refused:?}"
    );
    // The refusal of the index has the form of the check: it names the
    // board that has the slug.
    match boards::slug_violation(&mut conn, "alpha", None, refused) {
        BoardError::SlugTaken {
            slug,
            board_id,
            board_name,
        } => {
            assert_eq!(slug, "alpha");
            assert_eq!(board_id, alpha_first);
            assert_eq!(board_name, "Alpha, the first");
        }
        other => panic!("expected SlugTaken, got {other:?}"),
    }
    // A deleted board with the slug of a live board is permitted.
    old_board_insert(&mut conn, "alpha", false).expect("a deleted board does not keep a slug");

    // A second run changes nothing.
    let once = all_boards(&mut conn);
    conn.batch_execute(UP).expect("the second run passes");
    assert_eq!(all_boards(&mut conn), once);
    assert!(index_is_there(&mut conn));

    // The down migration removes the index, and it changes no slug.
    conn.batch_execute(DOWN).expect("the down migration");
    assert!(!index_is_there(&mut conn));
    assert_eq!(all_boards(&mut conn), once);

    drop_database(admin_conn, conn, SCRATCH_DB);
}

/// An insert that gives the error of the database to the caller.
fn old_board_insert(
    conn: &mut PgConnection,
    slug: &str,
    live: bool,
) -> Result<usize, diesel::result::Error> {
    sql_query(
        "INSERT INTO boards (name, slug, board_level, deleted_at) \
         VALUES ('A new board', $1, 'initiative', CASE WHEN $2 THEN NULL ELSE now() END)",
    )
    .bind::<Text, _>(slug)
    .bind::<diesel::sql_types::Bool, _>(live)
    .execute(conn)
}

#[test]
fn the_migration_changes_nothing_on_a_tenant_without_duplicates() {
    const SCRATCH_DB: &str = "kairos_board_slug_clean_t0255_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);

    // The boards of a new tenant, and 2 more. One is deleted and has the
    // slug of a live board.
    let live = create_board(
        &mut conn,
        BoardLevel::Initiative,
        "Roadmap",
        "roadmap",
        None,
        None,
    )
    .expect("creating a board");
    old_board(&mut conn, "Roadmap, deleted", "roadmap", 1, false);
    remove_the_index(&mut conn);
    let before = all_boards(&mut conn);
    assert!(before.len() >= 3, "{before:?}");
    assert!(slugs_of_two_live_boards(&mut conn).is_empty());

    conn.batch_execute(UP).expect("the migration passes");

    assert_eq!(all_boards(&mut conn), before, "no board is changed");
    assert!(index_is_there(&mut conn));
    assert_eq!(board(&mut conn, live.id).slug, "roadmap");

    drop_database(admin_conn, conn, SCRATCH_DB);
}

#[test]
fn create_board_refuses_the_slug_of_a_live_board() {
    const SCRATCH_DB: &str = "kairos_board_slug_create_t0255_test";
    let (admin_conn, mut conn) = tenant(SCRATCH_DB);
    assert!(
        index_is_there(&mut conn),
        "a new tenant has the index from its first migration run"
    );

    let first = create_board(
        &mut conn,
        BoardLevel::Initiative,
        "Roadmap",
        "roadmap",
        None,
        None,
    )
    .expect("creating a board");
    let before = all_boards(&mut conn).len();
    match create_board(
        &mut conn,
        BoardLevel::Strategy,
        "A second roadmap",
        "roadmap",
        None,
        None,
    ) {
        Err(
            ref e @ BoardError::SlugTaken {
                ref slug,
                board_id,
                ref board_name,
            },
        ) => {
            assert_eq!(slug, "roadmap");
            assert_eq!(board_id, first.id);
            assert_eq!(board_name, "Roadmap");
            assert_eq!(
                e.to_string(),
                "The live board \"Roadmap\" has the slug \"roadmap\". Two live boards cannot \
                 have the same slug. Send a different slug."
            );
        }
        other => panic!("expected SlugTaken, got {other:?}"),
    }
    assert_eq!(all_boards(&mut conn).len(), before, "no board is written");

    // The check for an update: a board is not in conflict with itself.
    boards::check_board_slug(&mut conn, "roadmap", Some(first.id))
        .expect("the board keeps its slug");
    assert!(matches!(
        boards::check_board_slug(&mut conn, "roadmap", None),
        Err(BoardError::SlugTaken { .. })
    ));

    // A deleted board does not keep its slug.
    sql_query(format!(
        "UPDATE boards SET deleted_at = now() WHERE id = '{}'",
        first.id
    ))
    .execute(&mut conn)
    .expect("deleting the board");
    let second = create_board(
        &mut conn,
        BoardLevel::Strategy,
        "A second roadmap",
        "roadmap",
        None,
        None,
    )
    .expect("the slug is free");
    assert_ne!(second.id, first.id);

    drop_database(admin_conn, conn, SCRATCH_DB);
}
