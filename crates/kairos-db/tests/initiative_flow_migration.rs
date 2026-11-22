//! KAIROS-T-0773 (COLLIERY-I-0678): the default initiative flow puts
//! Decompose before Ready.
//!
//! - A new initiative board takes the new order (the seed of
//!   `system_board_defaults`).
//! - The public migration `initiative_flow_decompose_first` changes the
//!   default row only when it is the old default.
//! - The tenant migration `initiative_boards_decompose_first` reorders each
//!   initiative board that has the old default exactly. Each item stays in
//!   its column. A board that someone changed is left as it is. The
//!   migration can run 2 times.
//!
//! The old state is made with the down migrations: that is the state of a
//! tenant before the migration.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_db::items::{self, CreateInitiative};
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::{NewUser, User};
use kairos_db::{CodePrefix, create_board, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The migrations under test, verbatim.
const TENANT_UP: &str =
    include_str!("../migrations/tenant/2026-11-22-000100_initiative_boards_decompose_first/up.sql");
const TENANT_DOWN: &str = include_str!(
    "../migrations/tenant/2026-11-22-000100_initiative_boards_decompose_first/down.sql"
);
const PUBLIC_UP: &str =
    include_str!("../migrations/public/2026-11-22-000100_initiative_flow_decompose_first/up.sql");
const PUBLIC_DOWN: &str =
    include_str!("../migrations/public/2026-11-22-000100_initiative_flow_decompose_first/down.sql");

const OLD: [&str; 7] = [
    "Discovery",
    "Design",
    "Ready",
    "Decompose",
    "Active",
    "Monitoring",
    "Completed",
];
const NEW: [&str; 7] = [
    "Discovery",
    "Design",
    "Decompose",
    "Ready",
    "Active",
    "Monitoring",
    "Completed",
];
const NEW_PAIRS: [(&str, &str); 6] = [
    ("Active", "Monitoring"),
    ("Decompose", "Ready"),
    ("Design", "Decompose"),
    ("Discovery", "Design"),
    ("Monitoring", "Completed"),
    ("Ready", "Active"),
];
const OLD_PAIRS: [(&str, &str); 6] = [
    ("Active", "Monitoring"),
    ("Decompose", "Active"),
    ("Design", "Ready"),
    ("Discovery", "Design"),
    ("Monitoring", "Completed"),
    ("Ready", "Decompose"),
];

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

/// A scratch database with the tenant `colliery`, pinned to its schema, and
/// a user.
fn tenant(scratch_db: &str) -> (PgConnection, PgConnection, Uuid, String) {
    let scratch_db = scratch::scratch_db_name(scratch_db);
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
    let mut conn = PgConnection::establish(&with_database(&admin_url, &scratch_db))
        .expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "colliery", "Colliery").expect("provisioning colliery");
    sql_query("SET search_path TO org_colliery, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let user = {
        use kairos_db::schema::users;
        diesel::insert_into(users::table)
            .values(NewUser {
                external_id: "dex|robin".into(),
                user_name: "dex|robin".into(),
                email: "robin@colliery.test".into(),
                display_name: "Robin".into(),
            })
            .returning(User::as_returning())
            .get_result(&mut conn)
            .expect("inserting user")
            .id
    };
    (admin_conn, conn, user, scratch_db)
}

fn drop_database(mut admin_conn: PgConnection, conn: PgConnection, scratch_db: &str) {
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}

fn board_id(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::slug.eq(slug))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("the board {slug}: {e}"))
}

fn columns(conn: &mut PgConnection, board: Uuid) -> Vec<String> {
    use kairos_db::schema::board_columns::dsl;
    dsl::board_columns
        .filter(dsl::board_id.eq(board))
        .order(dsl::position.asc())
        .select(dsl::name)
        .load(conn)
        .expect("columns")
}

#[derive(QueryableByName)]
struct Pair {
    #[diesel(sql_type = Text)]
    from_name: String,
    #[diesel(sql_type = Text)]
    to_name: String,
}

fn pairs(conn: &mut PgConnection, board: Uuid) -> Vec<(String, String)> {
    sql_query(
        "SELECT f.name AS from_name, t.name AS to_name FROM board_transitions x \
         JOIN board_columns f ON f.id = x.from_column_id \
         JOIN board_columns t ON t.id = x.to_column_id \
         WHERE x.board_id = $1 ORDER BY f.name, t.name",
    )
    .bind::<SqlUuid, _>(board)
    .load::<Pair>(conn)
    .expect("transitions")
    .into_iter()
    .map(|p| (p.from_name, p.to_name))
    .collect()
}

fn owned(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn column_id(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid {
    use kairos_db::schema::board_columns::dsl;
    dsl::board_columns
        .filter(dsl::board_id.eq(board))
        .filter(dsl::name.eq(name))
        .select(dsl::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("the column {name}: {e}"))
}

/// An initiative in the column `name`, and its column read back by name.
fn initiative_in(conn: &mut PgConnection, board: Uuid, name: &str, actor: Uuid) -> Uuid {
    let column = column_id(conn, board, name);
    items::create_initiative(
        conn,
        CreateInitiative {
            board_id: board,
            column_id: Some(column),
            title: name,
            content: "",
            complexity: None,
            bucket_type: None,
        },
        actor,
    )
    .expect("creating an initiative")
    .id
}

fn column_of(conn: &mut PgConnection, initiative: Uuid) -> String {
    use kairos_db::schema::{board_columns, initiatives};
    initiatives::table
        .inner_join(board_columns::table.on(board_columns::id.eq(initiatives::column_id)))
        .filter(initiatives::id.eq(initiative))
        .select(board_columns::name)
        .first(conn)
        .expect("the column of the initiative")
}

#[test]
fn a_new_initiative_board_has_decompose_before_ready() {
    let (admin, mut conn, user, db) = tenant("initiative_flow_new_board");
    let seeded = board_id(&mut conn, "initiatives");
    assert_eq!(columns(&mut conn, seeded), NEW);
    assert_eq!(pairs(&mut conn, seeded), owned(&NEW_PAIRS));
    let board = create_board(
        &mut conn,
        BoardLevel::Initiative,
        "Platform initiatives",
        "platform-initiatives",
        CodePrefix::FromSlug,
        None,
        Some(user),
    )
    .expect("an initiative board");
    assert_eq!(columns(&mut conn, board.id), NEW);
    assert_eq!(pairs(&mut conn, board.id), owned(&NEW_PAIRS));
    drop_database(admin, conn, &db);
}

#[test]
fn the_public_default_changes_only_from_the_old_default() {
    let (admin, mut conn, _user, db) = tenant("initiative_flow_public_default");
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Text)]
        columns: String,
    }
    let read = |conn: &mut PgConnection| {
        sql_query(
            "SELECT columns FROM public.system_board_defaults WHERE board_level = 'initiative'",
        )
        .get_result::<Row>(conn)
        .expect("the default")
        .columns
    };
    conn.batch_execute(PUBLIC_DOWN).expect("down");
    assert_eq!(read(&mut conn), OLD.join("\n"));
    conn.batch_execute(PUBLIC_UP).expect("up");
    assert_eq!(read(&mut conn), NEW.join("\n"));
    conn.batch_execute(PUBLIC_UP).expect("up again");
    assert_eq!(read(&mut conn), NEW.join("\n"));

    // An operator's own default stays.
    conn.batch_execute(
        "UPDATE public.system_board_defaults SET columns = E'Idea\\nDoing\\nDone', \
         transitions = E'Idea -> Doing\\nDoing -> Done' WHERE board_level = 'initiative'",
    )
    .expect("an own default");
    conn.batch_execute(PUBLIC_UP)
        .expect("up over an own default");
    assert_eq!(read(&mut conn), "Idea\nDoing\nDone");
    drop_database(admin, conn, &db);
}

#[test]
fn an_old_board_is_reordered_and_its_items_stay_in_their_column() {
    let (admin, mut conn, user, db) = tenant("initiative_flow_old_board");
    let board = board_id(&mut conn, "initiatives");
    conn.batch_execute(TENANT_DOWN)
        .expect("down: the old order");
    assert_eq!(columns(&mut conn, board), OLD);
    assert_eq!(pairs(&mut conn, board), owned(&OLD_PAIRS));

    let in_ready = initiative_in(&mut conn, board, "Ready", user);
    let in_decompose = initiative_in(&mut conn, board, "Decompose", user);
    let in_active = initiative_in(&mut conn, board, "Active", user);

    conn.batch_execute(TENANT_UP).expect("up");
    assert_eq!(columns(&mut conn, board), NEW);
    assert_eq!(pairs(&mut conn, board), owned(&NEW_PAIRS));
    assert_eq!(column_of(&mut conn, in_ready), "Ready");
    assert_eq!(column_of(&mut conn, in_decompose), "Decompose");
    assert_eq!(column_of(&mut conn, in_active), "Active");

    conn.batch_execute(TENANT_UP).expect("up again: no change");
    assert_eq!(columns(&mut conn, board), NEW);
    assert_eq!(pairs(&mut conn, board), owned(&NEW_PAIRS));
    drop_database(admin, conn, &db);
}

#[test]
fn a_changed_board_is_left_as_it_is() {
    let (admin, mut conn, _user, db) = tenant("initiative_flow_changed_board");
    let board = board_id(&mut conn, "initiatives");
    conn.batch_execute(TENANT_DOWN)
        .expect("down: the old order");
    // Someone added a transition back from Ready to Design.
    let design = column_id(&mut conn, board, "Design");
    let ready = column_id(&mut conn, board, "Ready");
    sql_query(
        "INSERT INTO board_transitions (board_id, from_column_id, to_column_id) VALUES ($1, $2, $3)",
    )
    .bind::<SqlUuid, _>(board)
    .bind::<SqlUuid, _>(ready)
    .bind::<SqlUuid, _>(design)
    .execute(&mut conn)
    .expect("an own transition");
    let before = pairs(&mut conn, board);

    conn.batch_execute(TENANT_UP).expect("up");
    assert_eq!(columns(&mut conn, board), OLD);
    assert_eq!(pairs(&mut conn, board), before);
    drop_database(admin, conn, &db);
}
