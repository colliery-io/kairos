//! KAIROS-I-0012 — `boards::move_task`: a task moves to another delivery
//! board, landing in its entry column and following its team, with a
//! `board_move` activity row and an `item_moved` event for BOTH boards.
//! The refusals are the interesting part: same board, a non-delivery
//! target, and the KAIROS-T-0104 rule that a repository-bound task lives
//! on its owner's board.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its
//! own scratch database like the other kairos-db integration tests.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::items::CreateTask;
use kairos_db::models::enums::{ActivityAction, BoardLevel, Forge, TaskType, WorkClass};
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{
    BoardError, boards, create_board, items, provision_tenant, repositories, run_public_migrations,
};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_board_move_test";

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
            email: email.to_string(),
            display_name: name.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

fn seed_team(conn: &mut PgConnection, name: &str, slug: &str, actor: Uuid) -> (Uuid, Uuid) {
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
    let board = create_board(
        conn,
        BoardLevel::Delivery,
        name,
        &format!("{slug}-delivery"),
        Some(team_id),
        Some(actor),
    )
    .expect("creating delivery board")
    .id;
    (team_id, board)
}

fn column_name(conn: &mut PgConnection, column_id: Uuid) -> String {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::id.eq(column_id))
        .select(board_columns::name)
        .first(conn)
        .expect("column exists")
}

fn activity_details(conn: &mut PgConnection, item: Uuid) -> Vec<String> {
    use kairos_db::schema::activity_log;
    activity_log::table
        .filter(activity_log::entity_id.eq(item))
        .filter(activity_log::action.eq(ActivityAction::BoardMove))
        .select(activity_log::details)
        .load(conn)
        .expect("loading activity")
}

#[test]
fn move_task_between_delivery_boards() {
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
    let (platform_id, platform_board) = seed_team(&mut conn, "Platform", "platform", alice);
    let (_web_id, web_board) = seed_team(&mut conn, "Web", "web", alice);
    let initiatives_board: Uuid = {
        use kairos_db::schema::boards;
        boards::table
            .filter(boards::slug.eq("initiatives"))
            .select(boards::id)
            .first(&mut conn)
            .expect("initiatives board")
    };

    let task = items::create_task(
        &mut conn,
        CreateTask {
            board_id: platform_board,
            column_id: None,
            title: "Bulk invoice export",
            content: "",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            team_id: Some(platform_id),
            repository_id: None,
        },
        alice,
    )
    .expect("creating task");

    // --- refusals ------------------------------------------------------------
    let err = boards::move_task(&mut conn, task.id, platform_board, alice)
        .expect_err("the task is already there");
    assert!(matches!(err, BoardError::SameBoard(_)), "{err}");

    let err = boards::move_task(&mut conn, task.id, initiatives_board, alice)
        .expect_err("tasks move between delivery boards only");
    assert!(matches!(err, BoardError::NotDeliveryBoard(_)), "{err}");

    // --- the happy path ------------------------------------------------------
    let moved = boards::move_task(&mut conn, task.id, web_board, alice).expect("moving to web");
    assert_eq!(moved.from_board_id, platform_board);
    assert_eq!(moved.board_id, web_board);
    assert_eq!(
        column_name(&mut conn, moved.column_id),
        "Backlog",
        "the task lands in the target's entry column"
    );
    assert_eq!(
        boards::entry_column(&mut conn, web_board).expect("entry column"),
        Some(moved.column_id)
    );
    // The team follows the board, so routing and team lenses stay honest.
    let stored: (Uuid, Uuid, Option<Uuid>) = {
        use kairos_db::schema::tasks::dsl;
        dsl::tasks
            .filter(dsl::id.eq(task.id))
            .select((dsl::board_id, dsl::column_id, dsl::team_id))
            .first(&mut conn)
            .expect("task row")
    };
    assert_eq!(stored, (web_board, moved.column_id, moved.team_id));
    assert_ne!(moved.team_id, Some(platform_id));

    let details = activity_details(&mut conn, task.id);
    assert_eq!(details.len(), 1, "one board_move row: {details:?}");
    assert!(
        details[0].contains(&format!("board:{platform_board}->{web_board}")),
        "{details:?}"
    );
    // The two `item_moved` notifications (one per board) go over NOTIFY,
    // so they are asserted through a real socket in kairos-server's
    // tests/ws_events.rs, not here.

    // --- the KAIROS-T-0104 rule ---------------------------------------------
    let payments = repositories::create(
        &mut conn,
        NewRepository {
            slug: "payments-api".to_string(),
            forge: Forge::Github,
            repo_full_name: "acme/payments-api".to_string(),
            repo_url: "https://github.com/acme/payments-api".to_string(),
            default_branch: "main".to_string(),
            team_id: platform_id,
            description: String::new(),
            created_by: alice,
            updated_by: alice,
        },
    )
    .expect("registering the repository");
    // Back to platform (the owner's board) before binding, so the refusal
    // below is about the repository rule and not "already there".
    let back = boards::move_task(&mut conn, task.id, platform_board, alice)
        .expect("back to the platform board");
    assert_eq!(back.board_id, platform_board);
    assert_eq!(back.team_id, Some(platform_id));
    items::set_task_repository(&mut conn, task.id, Some(payments.id), alice)
        .expect("binding the repository");

    let err = boards::move_task(&mut conn, task.id, web_board, alice)
        .expect_err("a bound task may not leave its repository's owner");
    match err {
        BoardError::RepositoryOwnerMismatch {
            repository,
            owner_board_id,
            ..
        } => {
            assert_eq!(repository, "payments-api");
            assert_eq!(owner_board_id, Some(platform_board));
        }
        other => panic!("expected RepositoryOwnerMismatch, got {other}"),
    }
    // …and the binding did not move it: it is still on the owner's board.
    let placement: Uuid = {
        use kairos_db::schema::tasks::dsl;
        dsl::tasks
            .filter(dsl::id.eq(task.id))
            .select(dsl::board_id)
            .first(&mut conn)
            .expect("task row")
    };
    assert_eq!(placement, platform_board);

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}
