//! COLLIERY-T-3100 — a retired short code still finds its item and names
//! the current code, in the database layer.
//!
//! The scenarios of the task:
//!
//! - a retired code names the item and its current code (`current_code`;
//!   `get_item` over MCP and REST is in `kairos-server/tests/retired_codes.rs`),
//! - a retired code is never issued again: the sequence moves past it, a
//!   new sequence starts after it, and the trigger refuses it on a direct
//!   write,
//! - search finds an item by a retired code, with its current code.
//!
//! The rename of COLLIERY-T-3101 does not exist yet, so the tests change
//! the code of the item directly and then call `retire_code`, in that
//! order, as the rename will.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::search::{SearchFilter, SearchRequest};
use kairos_db::items::{self, CreateTask};
use kairos_db::models::enums::{BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::retired_codes::{self, RetireError};
use kairos_db::{CodePrefix, create_board, provision_tenant, run_public_migrations, search};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

/// The fixture of the scenarios: the tenant `colliery` with the delivery
/// boards `colliery-io-delivery` (COLLIERY) and `skadi` (SKADI), and one
/// task whose code changed from `COLLIERY-T-2430` to `SKADI-T-0577`.
struct Fixture {
    admin: PgConnection,
    conn: PgConnection,
    db: &'static str,
    user: Uuid,
    colliery: Uuid,
    item: Uuid,
}

impl Fixture {
    fn new(db: &'static str) -> Self {
        let admin_url = admin_database_url();
        let mut admin = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
            panic!(
                "cannot connect to compose postgres at {admin_url}: {e} \
                 (is the stack up? `angreal services up`)"
            )
        });
        sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
            .execute(&mut admin)
            .expect("dropping scratch database");
        sql_query(format!("CREATE DATABASE {db}"))
            .execute(&mut admin)
            .expect("creating scratch database");
        let mut conn = PgConnection::establish(&with_database(&admin_url, db))
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
                    external_id: "dex|dylan".into(),
                    user_name: "dex|dylan".into(),
                    email: "dylan@colliery.test".into(),
                    display_name: "Dylan".into(),
                })
                .returning(User::as_returning())
                .get_result(&mut conn)
                .expect("inserting user")
                .id
        };
        let colliery_team = team(&mut conn, "colliery-io");
        let colliery = create_board(
            &mut conn,
            BoardLevel::Delivery,
            "Colliery Delivery",
            "colliery-io-delivery",
            CodePrefix::Given("COLLIERY"),
            Some(colliery_team),
            Some(user),
        )
        .expect("creating colliery-io-delivery")
        .id;
        let skadi_team = team(&mut conn, "skadi");
        create_board(
            &mut conn,
            BoardLevel::Delivery,
            "Skadi",
            "skadi",
            CodePrefix::Given("SKADI"),
            Some(skadi_team),
            Some(user),
        )
        .expect("creating skadi");

        // The COLLIERY task sequence is at 2429, so the next task is 2430.
        set_sequence(&mut conn, "COLLIERY", 2429);
        let (item, code) = task(&mut conn, colliery, user, "Find the downloads");
        assert_eq!(code, "COLLIERY-T-2430");
        // The code changes, then the old code is retired (COLLIERY-T-3101).
        set_code(&mut conn, item, "SKADI-T-0577").expect("changing the code");
        retired_codes::retire_code(&mut conn, "COLLIERY-T-2430", item, "moved to skadi")
            .expect("retiring COLLIERY-T-2430");

        Fixture {
            admin,
            conn,
            db,
            user,
            colliery,
            item,
        }
    }

    fn drop(self) {
        let Fixture {
            mut admin,
            conn,
            db,
            ..
        } = self;
        drop(conn);
        sql_query(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
            .execute(&mut admin)
            .expect("dropping scratch database");
    }
}

fn team(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::teams;
    diesel::insert_into(teams::table)
        .values(NewTeam {
            name: slug.to_string(),
            slug: slug.to_string(),
            team_type: TeamType::StreamAligned,
        })
        .returning(teams::id)
        .get_result(conn)
        .expect("inserting team")
}

fn task(conn: &mut PgConnection, board: Uuid, actor: Uuid, title: &str) -> (Uuid, String) {
    let task = items::create_task(
        conn,
        CreateTask {
            board_id: board,
            column_id: None,
            title,
            content: "",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        actor,
    )
    .expect("creating a task");
    (task.id, task.short_code)
}

fn set_sequence(conn: &mut PgConnection, prefix: &str, last: i64) {
    items::ensure_code_sequence(conn, prefix, kairos_core::short_code::ItemType::Task)
        .expect("making the sequence row");
    sql_query(
        "UPDATE short_code_sequences SET last_number = $2 \
          WHERE code_prefix = $1 AND item_type = 'T'",
    )
    .bind::<Text, _>(prefix)
    .bind::<diesel::sql_types::BigInt, _>(last)
    .execute(conn)
    .expect("setting the sequence");
}

fn set_code(conn: &mut PgConnection, item: Uuid, code: &str) -> QueryResult<usize> {
    sql_query("UPDATE tasks SET short_code = $2 WHERE id = $1")
        .bind::<SqlUuid, _>(item)
        .bind::<Text, _>(code)
        .execute(conn)
}

fn search_for(conn: &mut PgConnection, q: &str, include_deleted: bool) -> Vec<String> {
    let request = SearchRequest {
        q: Some(q.to_string()),
        filter: include_deleted.then(|| SearchFilter {
            include_deleted: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    search::execute_search(conn, &request)
        .expect("searching")
        .tasks
        .into_iter()
        .map(|t| t.short_code)
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario: get_item follows a retired code (the database part)
// ---------------------------------------------------------------------------

#[test]
fn a_retired_code_names_the_item_and_its_current_code() {
    let mut f = Fixture::new("kairos_retired_codes_current_t3100");

    let retired = retired_codes::current_code(&mut f.conn, "COLLIERY-T-2430")
        .expect("reading the retired code")
        .expect("COLLIERY-T-2430 is retired");
    assert_eq!(retired.item_id, f.item);
    assert_eq!(retired.reason, "moved to skadi");
    assert_eq!(
        retired.current,
        Some((
            "SKADI-T-0577".to_string(),
            kairos_core::short_code::ItemType::Task
        ))
    );
    assert_eq!(
        retired_codes::codes_of_item(&mut f.conn, f.item).expect("reading the codes"),
        vec!["COLLIERY-T-2430".to_string()]
    );

    // A live code and an unknown code are not retired.
    assert!(
        retired_codes::current_code(&mut f.conn, "SKADI-T-0577")
            .expect("reading")
            .is_none()
    );
    assert!(
        retired_codes::current_code(&mut f.conn, "COLLIERY-T-9999")
            .expect("reading")
            .is_none()
    );

    // An item that is removed for real: the code stays retired, and it
    // names no current code.
    sql_query("DELETE FROM tasks WHERE id = $1")
        .bind::<SqlUuid, _>(f.item)
        .execute(&mut f.conn)
        .expect("removing the task");
    let retired = retired_codes::current_code(&mut f.conn, "COLLIERY-T-2430")
        .expect("reading")
        .expect("the code stays retired");
    assert_eq!(retired.current, None);
    assert!(retired_codes::is_retired(&mut f.conn, "COLLIERY-T-2430").expect("reading"));

    f.drop();
}

#[test]
fn retire_code_refuses_bad_input_and_names_it() {
    let mut f = Fixture::new("kairos_retired_codes_refusals_t3100");
    let item = f.item;

    // The current code of an item cannot be retired.
    match retired_codes::retire_code(&mut f.conn, "SKADI-T-0577", item, "test") {
        Err(RetireError::InUse(code)) => assert_eq!(code, "SKADI-T-0577"),
        other => panic!("expected InUse, got {other:?}"),
    }
    // A code is retired one time.
    match retired_codes::retire_code(&mut f.conn, "COLLIERY-T-2430", item, "again") {
        Err(RetireError::AlreadyRetired(code)) => assert_eq!(code, "COLLIERY-T-2430"),
        other => panic!("expected AlreadyRetired, got {other:?}"),
    }
    // Not a short code.
    for bad in ["colliery-t-2430", "COLLIERY-T-24", "COLLIERY-X-2430", ""] {
        match retired_codes::retire_code(&mut f.conn, bad, item, "test") {
            Err(RetireError::NotAShortCode(code)) => assert_eq!(code, bad),
            other => panic!("expected NotAShortCode for {bad:?}, got {other:?}"),
        }
    }
    // A reason is required.
    match retired_codes::retire_code(&mut f.conn, "COLLIERY-T-0001", item, "  ") {
        Err(RetireError::NoReason) => {}
        other => panic!("expected NoReason, got {other:?}"),
    }

    f.drop();
}

// ---------------------------------------------------------------------------
// Scenario: A retired code is never issued again
// ---------------------------------------------------------------------------

#[test]
fn a_retired_code_is_never_issued_again() {
    let mut f = Fixture::new("kairos_retired_codes_never_again_t3100");
    let (colliery, user) = (f.colliery, f.user);

    // When the COLLIERY task sequence reaches 2430 ...
    set_sequence(&mut f.conn, "COLLIERY", 2429);
    let (_, code) = task(&mut f.conn, colliery, user, "The next task");
    // ... then the new task does not get COLLIERY-T-2430.
    assert_eq!(code, "COLLIERY-T-2431");

    // A sequence that starts again (no row) starts after the retired code
    // too: here the highest code is the retired one.
    sql_query("DELETE FROM tasks WHERE short_code = 'COLLIERY-T-2431'")
        .execute(&mut f.conn)
        .expect("removing COLLIERY-T-2431");
    sql_query("DELETE FROM short_code_sequences WHERE code_prefix = 'COLLIERY'")
        .execute(&mut f.conn)
        .expect("removing the COLLIERY sequences");
    let (other, code) = task(&mut f.conn, colliery, user, "After a new start");
    assert_eq!(code, "COLLIERY-T-2431");

    // A direct write cannot give an item the retired code: the trigger
    // refuses it, on an insert and on an update.
    let err = set_code(&mut f.conn, other, "COLLIERY-T-2430")
        .expect_err("the trigger refuses a retired code");
    assert!(
        err.to_string()
            .contains("the short code COLLIERY-T-2430 is retired"),
        "{err}"
    );
    let err = sql_query(
        "INSERT INTO tasks (short_code, title, content, board_id, column_id, task_type, \
                            team_id, created_by, updated_by, work_class) \
         SELECT 'COLLIERY-T-2430', 'x', '', board_id, column_id, task_type, \
                team_id, created_by, updated_by, work_class \
           FROM tasks WHERE id = $1",
    )
    .bind::<SqlUuid, _>(other)
    .execute(&mut f.conn)
    .expect_err("the trigger refuses a retired code on an insert");
    assert!(err.to_string().contains("is retired"), "{err}");

    f.drop();
}

// ---------------------------------------------------------------------------
// Scenario: Search finds an item by a retired code
// ---------------------------------------------------------------------------

#[test]
fn search_finds_an_item_by_a_retired_code() {
    let mut f = Fixture::new("kairos_retired_codes_search_t3100");
    let (colliery, user) = (f.colliery, f.user);
    // Another task that names the old code in its text.
    let (mention, mention_code) = task(&mut f.conn, colliery, user, "Notes");
    sql_query("UPDATE tasks SET content = 'See COLLIERY-T-2430 for the downloads.' WHERE id = $1")
        .bind::<SqlUuid, _>(mention)
        .execute(&mut f.conn)
        .expect("writing the mention");

    // The result has the item with its current code, first.
    let found = search_for(&mut f.conn, "COLLIERY-T-2430", false);
    assert_eq!(found.first().map(String::as_str), Some("SKADI-T-0577"));
    assert!(found.contains(&mention_code), "{found:?}");
    // The case of the query does not matter.
    let found = search_for(&mut f.conn, "  colliery-t-2430 ", false);
    assert_eq!(found.first().map(String::as_str), Some("SKADI-T-0577"));
    // The current code finds the item too.
    let found = search_for(&mut f.conn, "SKADI-T-0577", false);
    assert_eq!(found, vec!["SKADI-T-0577".to_string()]);

    // An archived item is found only when the search includes archived work.
    sql_query("UPDATE tasks SET deleted_at = now() WHERE id = $1")
        .bind::<SqlUuid, _>(f.item)
        .execute(&mut f.conn)
        .expect("archiving the task");
    let found = search_for(&mut f.conn, "COLLIERY-T-2430", false);
    assert!(!found.contains(&"SKADI-T-0577".to_string()), "{found:?}");
    let found = search_for(&mut f.conn, "COLLIERY-T-2430", true);
    assert_eq!(found.first().map(String::as_str), Some("SKADI-T-0577"));

    f.drop();
}
