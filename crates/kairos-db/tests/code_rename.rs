//! COLLIERY-T-3101 — a move to another board can rename the item, and the
//! links change one time. The database layer
//! (`kairos_db::code_rename`, `boards::move_task_with`).
//!
//! The scenarios of the task:
//!
//! - a move with rename gives the next code of the target board, the text
//!   of the other items names the new code, and the old code is retired
//!   and finds the moved task,
//! - a move with no rename keeps the code,
//! - a code in a URL does not change,
//! - the history records the rename (the activity log row, with the
//!   actor; MCP `get_history` is in `kairos-server/tests/rename_on_move.rs`).
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::short_code::ItemType;
use kairos_db::code_rename::{self, RenameError};
use kairos_db::items::{self, ContentUpdate, CreateTask};
use kairos_db::models::enums::{ActivityAction, BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{
    BoardError, CodePrefix, boards, create_board, provision_tenant, retired_codes,
    run_public_migrations,
};

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

/// The tenant `colliery` with the delivery boards `colliery-io-delivery`
/// (COLLIERY) and `skadi` (SKADI). The COLLIERY task sequence is at 99, so
/// the first task is COLLIERY-T-0100.
struct Fixture {
    admin: PgConnection,
    conn: PgConnection,
    db: &'static str,
    user: Uuid,
    colliery: Uuid,
    skadi: Uuid,
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
        let skadi = create_board(
            &mut conn,
            BoardLevel::Delivery,
            "Skadi",
            "skadi",
            CodePrefix::Given("SKADI"),
            Some(skadi_team),
            Some(user),
        )
        .expect("creating skadi")
        .id;
        items::ensure_code_sequence(&mut conn, "COLLIERY", ItemType::Task)
            .expect("making the sequence row");
        sql_query(
            "UPDATE short_code_sequences SET last_number = 99 \
              WHERE code_prefix = 'COLLIERY' AND item_type = 'T'",
        )
        .execute(&mut conn)
        .expect("setting the sequence");
        Fixture {
            admin,
            conn,
            db,
            user,
            colliery,
            skadi,
        }
    }

    fn task(&mut self, title: &str, content: &str) -> (Uuid, String) {
        let task = items::create_task(
            &mut self.conn,
            CreateTask {
                board_id: self.colliery,
                column_id: None,
                title,
                content,
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                repository_id: None,
            },
            self.user,
        )
        .expect("creating a task");
        (task.id, task.short_code)
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

/// (short code, title, content, version) of a task, live or archived.
fn task_row(conn: &mut PgConnection, id: Uuid) -> (String, String, String, i32) {
    use kairos_db::schema::tasks::dsl;
    dsl::tasks
        .filter(dsl::id.eq(id))
        .select((dsl::short_code, dsl::title, dsl::content, dsl::version))
        .first(conn)
        .expect("reading the task")
}

/// (version, title, content, editor) of the newest history row.
fn newest_history(conn: &mut PgConnection, id: Uuid) -> (i32, String, String, Uuid) {
    use kairos_db::schema::item_history::dsl;
    dsl::item_history
        .filter(dsl::item_id.eq(id))
        .order(dsl::version.desc())
        .select((dsl::version, dsl::title, dsl::content, dsl::edited_by))
        .first(conn)
        .expect("reading the history")
}

// ---------------------------------------------------------------------------
// Scenario: A move with rename gives a new code and updates the references
// Scenario: A code in a URL does not change
// Scenario: The history records the rename
// ---------------------------------------------------------------------------

#[test]
fn a_move_with_rename_gives_a_new_code_and_updates_the_references() {
    let mut f = Fixture::new("kairos_code_rename_move_t3101");
    let (moved, code) = f.task("Find the downloads", "This task is COLLIERY-T-0100.");
    assert_eq!(code, "COLLIERY-T-0100");
    let (other, _) = f.task(
        "After COLLIERY-T-0100",
        "Blocked by COLLIERY-T-0100. See [[COLLIERY-T-0100]] and \
         https://example.com/COLLIERY-T-0100.md for the file.",
    );
    let (archived, _) = f.task("Old note", "It named COLLIERY-T-0100.");
    items::soft_delete_item(&mut f.conn, ItemType::Task, archived, f.user)
        .expect("archiving a task");
    let (untouched, _) = f.task("No reference", "COLLIERY-T-01000 is a different code.");

    let result =
        boards::move_task_with(&mut f.conn, moved, f.skadi, f.user, true).expect("the move");
    let rename = result.rename.expect("a rename");
    assert_eq!(result.board_id, f.skadi);

    // Its code is the next SKADI task code.
    assert_eq!(rename.old_code, "COLLIERY-T-0100");
    assert_eq!(rename.new_code, "SKADI-T-0001");
    let (now, _, content, version) = task_row(&mut f.conn, moved);
    assert_eq!(now, "SKADI-T-0001");
    assert_eq!(
        content, "This task is SKADI-T-0001.",
        "its own text changes too"
    );
    assert_eq!(version, 2);

    // The other task's text names the new code. The URL does not change.
    let (_, title, content, version) = task_row(&mut f.conn, other);
    assert_eq!(title, "After SKADI-T-0001");
    assert_eq!(
        content,
        "Blocked by SKADI-T-0001. See [[SKADI-T-0001]] and \
         https://example.com/COLLIERY-T-0100.md for the file."
    );
    assert_eq!(version, 2, "the change is a new version");
    let (history_version, history_title, history_content, editor) =
        newest_history(&mut f.conn, other);
    assert_eq!(history_version, 2);
    assert_eq!(history_title, title);
    assert_eq!(history_content, content);
    assert_eq!(editor, f.user, "the history names who did the move");

    // An archived item changes too, so that a restore does not bring the
    // old code back. An item with a longer code that starts the same does
    // not change.
    let (_, _, content, _) = task_row(&mut f.conn, archived);
    assert_eq!(content, "It named SKADI-T-0001.");
    let (_, _, content, version) = task_row(&mut f.conn, untouched);
    assert_eq!(content, "COLLIERY-T-01000 is a different code.");
    assert_eq!(version, 1);
    let mut changed = rename.references.clone();
    changed.sort();
    assert_eq!(changed.len(), 3, "{changed:?}");

    // COLLIERY-T-0100 is retired, and it finds the moved task.
    let retired = retired_codes::current_code(&mut f.conn, "COLLIERY-T-0100")
        .expect("reading the retired code")
        .expect("COLLIERY-T-0100 is retired");
    assert_eq!(retired.item_id, moved);
    assert_eq!(
        retired.current,
        Some(("SKADI-T-0001".to_string(), ItemType::Task))
    );
    assert_eq!(retired.reason, "The task moved to the board skadi.");

    // The history records the rename: the old code, the new code, who.
    {
        use kairos_db::schema::activity_log::dsl;
        let rows: Vec<(String, Uuid)> = dsl::activity_log
            .filter(dsl::entity_id.eq(moved))
            .filter(dsl::action.eq(ActivityAction::Rename))
            .select((dsl::details, dsl::actor_id))
            .load(&mut f.conn)
            .expect("reading the activity log");
        assert_eq!(
            rows,
            vec![("code:COLLIERY-T-0100->SKADI-T-0001".to_string(), f.user)]
        );
    }

    // The replace happens one time: the old code is never issued again,
    // and an edit that writes the old code later is not changed again.
    let (fresh, fresh_code) = f.task("New", "");
    assert_eq!(
        fresh_code, "COLLIERY-T-0104",
        "the sequence of COLLIERY goes on"
    );
    let _ = fresh;
    let (_, _, _, version) = task_row(&mut f.conn, other);
    items::update_item_content(
        &mut f.conn,
        ItemType::Task,
        other,
        ContentUpdate {
            new_title: None,
            new_content: "Now it names COLLIERY-T-0100 again.",
            expected_version: version,
        },
        f.user,
    )
    .expect("an edit");
    let (_, _, content, _) = task_row(&mut f.conn, other);
    assert_eq!(content, "Now it names COLLIERY-T-0100 again.");

    f.drop();
}

// ---------------------------------------------------------------------------
// Scenario: A move with no rename keeps the code
// ---------------------------------------------------------------------------

#[test]
fn a_move_with_no_rename_keeps_the_code() {
    let mut f = Fixture::new("kairos_code_rename_keep_t3101");
    let (moved, code) = f.task("Find the downloads", "");
    let (other, _) = f.task("After", "Blocked by COLLIERY-T-0100.");

    let result =
        boards::move_task_with(&mut f.conn, moved, f.skadi, f.user, false).expect("the move");
    assert_eq!(result.rename, None);
    assert_eq!(result.board_id, f.skadi);
    let (now, _, _, version) = task_row(&mut f.conn, moved);
    assert_eq!(now, code, "its code does not change");
    assert_eq!(version, 1);
    let (_, _, content, version) = task_row(&mut f.conn, other);
    assert_eq!(content, "Blocked by COLLIERY-T-0100.");
    assert_eq!(version, 1);
    assert!(
        !retired_codes::is_retired(&mut f.conn, &code).expect("reading"),
        "no code is retired"
    );

    f.drop();
}

// ---------------------------------------------------------------------------
// A rename to the prefix that the code has already is refused, and nothing
// changes
// ---------------------------------------------------------------------------

#[test]
fn a_rename_to_the_same_prefix_is_refused_and_changes_nothing() {
    let mut f = Fixture::new("kairos_code_rename_same_t3101");
    let (task, code) = f.task("Find the downloads", "");
    let (other, _) = f.task("After", "Blocked by COLLIERY-T-0100.");

    let err = code_rename::rename_item(
        &mut f.conn,
        ItemType::Task,
        task,
        Some(f.colliery),
        f.user,
        "test",
    )
    .expect_err("the same prefix");
    match err {
        RenameError::NotNeeded { code, prefix } => {
            assert_eq!(code, "COLLIERY-T-0100");
            assert_eq!(prefix, "COLLIERY");
        }
        other => panic!("expected NotNeeded, got {other:?}"),
    }
    let (now, _, _, _) = task_row(&mut f.conn, task);
    assert_eq!(now, code);
    let (_, _, content, version) = task_row(&mut f.conn, other);
    assert_eq!(content, "Blocked by COLLIERY-T-0100.");
    assert_eq!(version, 1);

    // A move with rename of a task that is not there: the move refuses,
    // and the rename does not start.
    let err = boards::move_task_with(&mut f.conn, Uuid::new_v4(), f.skadi, f.user, true)
        .expect_err("no such task");
    assert!(matches!(err, BoardError::ItemNotFound { .. }), "{err:?}");

    // A direct write of a retired code is refused by the trigger, so the
    // rename cannot give an item a code that is retired.
    retired_codes::retire_code(&mut f.conn, "SKADI-T-0001", Uuid::new_v4(), "test")
        .expect("retiring SKADI-T-0001 first");
    let result =
        boards::move_task_with(&mut f.conn, task, f.skadi, f.user, true).expect("the move");
    assert_eq!(
        result.rename.expect("a rename").new_code,
        "SKADI-T-0002",
        "the sequence moves past a retired code"
    );
    let row: (String,) = {
        #[derive(QueryableByName)]
        struct Code {
            #[diesel(sql_type = Text)]
            short_code: String,
        }
        let code: Code = sql_query("SELECT short_code FROM tasks WHERE id = $1")
            .bind::<SqlUuid, _>(task)
            .get_result(&mut f.conn)
            .expect("reading");
        (code.short_code,)
    };
    assert_eq!(row.0, "SKADI-T-0002");

    f.drop();
}
