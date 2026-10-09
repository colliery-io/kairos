//! KAIROS-T-0359 — a task in Active has a claim, in the database layer:
//!
//! - the migration `task_claims` marks the live column 'Active' of each
//!   live delivery board, and no other column; a second run changes
//!   nothing;
//! - the seed of a new delivery board marks its Active column;
//! - a person who moves a task into Active gets the claim, with the agent
//!   key of the session; a move out of Active ends it; a service account
//!   that moves a task into Active makes no claim;
//! - a hand-off and a release; their refusals;
//! - an archive of the task, and an admin who removes the flag from the
//!   column, end the claim;
//! - each change has its `activity_log` row.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its
//! own scratch database.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::items::CreateTask;
use kairos_db::models::enums::{ActivityAction, BoardLevel, TaskType, WorkClass};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewServiceAccountUser, NewUser, TeamType, User};
use kairos_db::task_claims::{self, ClaimError};
use kairos_db::{boards, create_board, items, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_task_claims_t0359_test";

/// The migration under test, verbatim.
const UP: &str = include_str!("../migrations/tenant/2026-10-09-000003_task_claims/up.sql");
const DOWN: &str = include_str!("../migrations/tenant/2026-10-09-000003_task_claims/down.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

fn insert_user(conn: &mut PgConnection, external_id: &str, email: &str, name: &str) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: external_id.to_string(),
            user_name: external_id.to_string(),
            email: email.to_string(),
            display_name: name.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

fn insert_service_account(conn: &mut PgConnection) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewServiceAccountUser::new(
            format!("svc:{}", Uuid::new_v4()),
            "ci@acme.test",
            "CI",
        ))
        .returning(users::id)
        .get_result(conn)
        .expect("inserting service account")
}

fn seed_team(conn: &mut PgConnection, name: &str, slug: &str, actor: Uuid) -> Uuid {
    use kairos_db::schema::teams;
    let team_id: Uuid = diesel::insert_into(teams::table)
        .values(NewTeam {
            name: name.to_string(),
            slug: slug.to_string(),
            team_type: TeamType::StreamAligned,
        })
        .returning(teams::id)
        .get_result(conn)
        .expect("inserting team");
    create_board(
        conn,
        BoardLevel::Delivery,
        name,
        &format!("{slug}-delivery"),
        kairos_db::CodePrefix::FromSlug,
        Some(team_id),
        Some(actor),
    )
    .expect("creating delivery board")
    .id
}

/// `(name, claims)` of each live column of a board, by position.
fn column_flags(conn: &mut PgConnection, board_id: Uuid) -> Vec<(String, bool)> {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::deleted_at.is_null())
        .order(board_columns::position.asc())
        .select((board_columns::name, board_columns::claims))
        .load(conn)
        .expect("columns")
}

fn column(conn: &mut PgConnection, board_id: Uuid, name: &str) -> Uuid {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::name.eq(name))
        .select(board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name}: {e}"))
}

fn board_by_slug(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::boards;
    boards::table
        .filter(boards::slug.eq(slug))
        .select(boards::id)
        .first(conn)
        .expect("board")
}

fn new_task(conn: &mut PgConnection, board_id: Uuid, title: &str, actor: Uuid) -> Uuid {
    items::create_task(
        conn,
        CreateTask {
            board_id,
            column_id: None,
            title,
            content: "",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        actor,
    )
    .expect("creating task")
    .id
}

/// Move a task through `path`, column by column.
fn walk(conn: &mut PgConnection, board: Uuid, task: Uuid, path: &[&str], actor: Uuid) {
    for name in path {
        let to = column(conn, board, name);
        boards::transition_task(conn, task, to, actor)
            .unwrap_or_else(|e| panic!("transition to {name}: {e}"));
    }
}

/// The `details` of the activity rows of the task with `action`, oldest
/// first.
fn activity(conn: &mut PgConnection, task: Uuid, action: ActivityAction) -> Vec<String> {
    use kairos_db::schema::activity_log;
    activity_log::table
        .filter(activity_log::entity_id.eq(task))
        .filter(activity_log::action.eq(action))
        .order(activity_log::occurred_at.asc())
        .select(activity_log::details)
        .load(conn)
        .expect("activity")
}

#[test]
fn a_task_in_active_has_a_claim() {
    let admin_url = admin_database_url();
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
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let alice = insert_user(&mut conn, "dex|alice", "alice@acme.test", "Alice");
    let bob = insert_user(&mut conn, "dex|bob", "bob@acme.test", "Bob");
    let ci = insert_service_account(&mut conn);

    // --- the seed ------------------------------------------------------------
    let web = seed_team(&mut conn, "Web", "web", alice);
    assert_eq!(
        column_flags(&mut conn, web),
        vec![
            ("Backlog".to_string(), false),
            ("Todo".to_string(), false),
            ("Blocked".to_string(), false),
            ("Active".to_string(), true),
            ("Completed".to_string(), false),
        ],
        "the seed marks the Active column of a delivery board"
    );
    let initiatives = board_by_slug(&mut conn, "initiatives");
    assert!(
        column_flags(&mut conn, initiatives)
            .iter()
            .all(|(_, claims)| !claims),
        "an initiative board has no claims column"
    );

    // --- the migration, on a tenant from before it ---------------------------
    let ops = seed_team(&mut conn, "Ops", "ops", alice);
    // A removed column named Active on a delivery board.
    let removed: Uuid = {
        use kairos_db::schema::board_columns;
        diesel::insert_into(board_columns::table)
            .values((
                board_columns::board_id.eq(ops),
                board_columns::name.eq("Active"),
                board_columns::position.eq(99),
                board_columns::deleted_at.eq(Some(chrono::Utc::now())),
            ))
            .returning(board_columns::id)
            .get_result(&mut conn)
            .expect("a removed column")
    };
    conn.batch_execute(DOWN).expect("down");
    conn.batch_execute(UP).expect("up");
    for board in [web, ops] {
        let active: Vec<String> = column_flags(&mut conn, board)
            .into_iter()
            .filter(|(_, claims)| *claims)
            .map(|(name, _)| name)
            .collect();
        assert_eq!(active, vec!["Active".to_string()], "board {board}");
    }
    let removed_flag: bool = {
        use kairos_db::schema::board_columns;
        board_columns::table
            .filter(board_columns::id.eq(removed))
            .select(board_columns::claims)
            .first(&mut conn)
            .expect("removed column")
    };
    assert!(!removed_flag, "a removed column is not changed");
    assert!(
        column_flags(&mut conn, initiatives)
            .iter()
            .all(|(_, claims)| !claims),
        "the migration marks delivery boards only"
    );
    let before = column_flags(&mut conn, web);
    conn.batch_execute(UP).expect("a second run");
    assert_eq!(
        column_flags(&mut conn, web),
        before,
        "a second run changes nothing"
    );

    // --- a person moves the task to Active, with an agent key ---------------
    let key = Uuid::new_v4();
    let task = new_task(&mut conn, web, "Claimed", alice);
    walk(&mut conn, web, task, &["Todo"], alice);
    assert_eq!(task_claims::claim_of(&mut conn, task).expect("claim"), None);
    kairos_db::agent_mark::set(&mut conn, Some(key)).expect("agent key");
    walk(&mut conn, web, task, &["Active"], alice);
    kairos_db::agent_mark::set(&mut conn, None).expect("no agent key");
    let claim = task_claims::claim_of(&mut conn, task)
        .expect("claim")
        .expect("alice has the claim");
    assert_eq!(claim.user_id, alice);
    assert_eq!(claim.agent_key_id, Some(key));
    assert_eq!(
        activity(&mut conn, task, ActivityAction::Claim),
        vec![format!("claim:{alice}")]
    );

    // A move between two claims columns by a different person: the claim
    // goes to that person.
    let review = boards::add_column(&mut conn, web, "Review", 5, alice).expect("column");
    boards::set_column_claims(&mut conn, review.id, true, alice).expect("flag");
    let active = column(&mut conn, web, "Active");
    boards::add_transition(&mut conn, web, active, review.id, alice).expect("transition");
    boards::transition_task(&mut conn, task, review.id, bob).expect("to Review");
    let claim = task_claims::claim_of(&mut conn, task)
        .expect("claim")
        .expect("bob has the claim");
    assert_eq!(claim.user_id, bob);
    assert_eq!(claim.agent_key_id, None);
    assert_eq!(
        activity(&mut conn, task, ActivityAction::Claim).last(),
        Some(&format!("claim:{bob} from:{alice}"))
    );

    // The admin removes the flag: the claim ends.
    boards::set_column_claims(&mut conn, review.id, false, alice).expect("flag off");
    assert_eq!(task_claims::claim_of(&mut conn, task).expect("claim"), None);
    assert_eq!(
        activity(&mut conn, task, ActivityAction::Release).last(),
        Some(&format!("release:{bob} reason:column"))
    );

    // --- out of Active: the claim ends --------------------------------------
    let task = new_task(&mut conn, web, "Done", alice);
    walk(&mut conn, web, task, &["Todo", "Active"], alice);
    assert!(
        task_claims::claim_of(&mut conn, task)
            .expect("claim")
            .is_some()
    );
    walk(&mut conn, web, task, &["Completed"], alice);
    assert_eq!(task_claims::claim_of(&mut conn, task).expect("claim"), None);
    assert_eq!(
        activity(&mut conn, task, ActivityAction::Release),
        vec![format!("release:{alice} reason:transition")]
    );

    // --- a service account makes no claim ------------------------------------
    let task = new_task(&mut conn, web, "Machine", alice);
    walk(&mut conn, web, task, &["Todo", "Active"], ci);
    assert_eq!(task_claims::claim_of(&mut conn, task).expect("claim"), None);
    assert!(activity(&mut conn, task, ActivityAction::Claim).is_empty());
    // A claim that the task had ends at a move by a service account.
    walk(&mut conn, web, task, &["Blocked", "Active"], alice);
    assert!(
        task_claims::claim_of(&mut conn, task)
            .expect("claim")
            .is_some()
    );
    walk(&mut conn, web, task, &["Blocked", "Active"], ci);
    assert_eq!(task_claims::claim_of(&mut conn, task).expect("claim"), None);

    // --- hand-off and release -------------------------------------------------
    let task = new_task(&mut conn, web, "Handed", alice);
    let err = task_claims::release(&mut conn, task, alice).expect_err("not in Active");
    assert!(matches!(err, ClaimError::NotInClaimsColumn { .. }), "{err}");
    let err = task_claims::hand_off(&mut conn, task, bob, alice).expect_err("not in Active");
    assert!(matches!(err, ClaimError::NotInClaimsColumn { .. }), "{err}");
    walk(&mut conn, web, task, &["Todo", "Active"], alice);
    let err = task_claims::hand_off(&mut conn, task, ci, alice).expect_err("not a person");
    assert!(matches!(err, ClaimError::NotAPerson(_)), "{err}");
    let err = task_claims::hand_off(&mut conn, task, alice, alice).expect_err("the holder");
    assert!(matches!(err, ClaimError::AlreadyHolder { .. }), "{err}");
    let claim = task_claims::hand_off(&mut conn, task, bob, alice).expect("hand-off");
    assert_eq!(claim.user_id, bob);
    assert_eq!(
        activity(&mut conn, task, ActivityAction::HandOff),
        vec![format!("hand_off:{alice}->{bob}")]
    );
    let ended = task_claims::release(&mut conn, task, bob).expect("release");
    assert_eq!(ended.user_id, bob);
    assert_eq!(task_claims::claim_of(&mut conn, task).expect("claim"), None);
    let err = task_claims::release(&mut conn, task, bob).expect_err("no claim");
    assert!(matches!(err, ClaimError::NoClaim { .. }), "{err}");
    // A hand-off of a free task in Active.
    task_claims::hand_off(&mut conn, task, alice, bob).expect("hand-off of a free task");
    assert_eq!(
        activity(&mut conn, task, ActivityAction::HandOff).last(),
        Some(&format!("hand_off:none->{alice}"))
    );

    // --- an archive ends the claim -------------------------------------------
    items::soft_delete_item(
        &mut conn,
        kairos_core::short_code::ItemType::Task,
        task,
        alice,
    )
    .expect("archive");
    assert_eq!(task_claims::claim_of(&mut conn, task).expect("claim"), None);
    assert_eq!(
        activity(&mut conn, task, ActivityAction::Release).last(),
        Some(&format!("release:{alice} reason:archive"))
    );

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}
