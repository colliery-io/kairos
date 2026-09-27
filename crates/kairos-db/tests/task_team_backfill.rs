//! COLLIERY-T-0216 — the board of a task decides its team, in the database
//! layer: `items::create_task` reads the team from the board, binding a
//! repository leaves the team alone, and the backfill migration brings
//! rows written under the old rules into line.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its own
//! scratch database like the other kairos-db integration tests.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::items::CreateTask;
use kairos_db::models::enums::{BoardLevel, Forge, TaskType, WorkClass};
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{create_board, items, provision_tenant, repositories, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_task_team_t0216_test";

/// The migration under test, verbatim: what a deployment runs is what this
/// test runs.
const BACKFILL: &str =
    include_str!("../migrations/tenant/2026-09-27-000000_task_team_from_board/up.sql");

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
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

fn make_task(
    conn: &mut PgConnection,
    board: Uuid,
    title: &str,
    actor: Uuid,
) -> kairos_db::models::Task {
    items::create_task(
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
    .expect("creating task")
}

fn team_of(conn: &mut PgConnection, task: Uuid) -> Option<Uuid> {
    use kairos_db::schema::tasks::dsl;
    dsl::tasks
        .filter(dsl::id.eq(task))
        .select(dsl::team_id)
        .first(conn)
        .expect("task exists")
}

fn force_team(conn: &mut PgConnection, task: Uuid, team: Option<Uuid>) {
    use kairos_db::schema::tasks::dsl;
    diesel::update(dsl::tasks.filter(dsl::id.eq(task)))
        .set(dsl::team_id.eq(team))
        .execute(conn)
        .expect("writing the team a pre-T-0216 writer would have");
}

#[test]
fn the_board_of_a_task_decides_its_team() {
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

    let alice = {
        use kairos_db::schema::users;
        diesel::insert_into(users::table)
            .values(NewUser {
                external_id: "dex|alice".into(),
                user_name: "dex|alice".into(),
                email: "alice@acme.test".into(),
                display_name: "Alice".into(),
            })
            .returning(User::as_returning())
            .get_result(&mut conn)
            .expect("inserting user")
            .id
    };
    let (platform, platform_board) = seed_team(&mut conn, "Platform", "platform", alice);
    let (web, web_board) = seed_team(&mut conn, "Web", "web", alice);
    let teamless_board = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Nobody's",
        "nobodys-delivery",
        None,
        Some(alice),
    )
    .expect("a delivery board with no team")
    .id;

    // --- create: the team is read from the board -----------------------------
    let on_platform = make_task(&mut conn, platform_board, "On platform's board", alice);
    assert_eq!(on_platform.team_id, Some(platform));
    let on_web = make_task(&mut conn, web_board, "On web's board", alice);
    assert_eq!(on_web.team_id, Some(web));
    let on_nobodys = make_task(&mut conn, teamless_board, "On a teamless board", alice);
    assert_eq!(
        on_nobodys.team_id, None,
        "a board with no team gives a task with no team"
    );

    // --- bind: a repository changes nothing else -----------------------------
    // Owned by WEB and bound to a task on PLATFORM's board. The service layer
    // above still refuses this pairing until COLLIERY-T-0217; the db layer
    // never did, which is exactly how a task used to end up under the wrong
    // team, so this is the layer to pin it in.
    let portal = repositories::create(
        &mut conn,
        NewRepository {
            slug: "portal-web".into(),
            forge: Forge::Github,
            repo_full_name: "acme/portal-web".into(),
            repo_url: "https://github.com/acme/portal-web".into(),
            default_branch: "main".into(),
            team_id: web,
            description: String::new(),
            created_by: alice,
            updated_by: alice,
        },
    )
    .expect("web's repository");
    let bound = items::set_task_repository(&mut conn, on_platform.id, Some(portal.id), alice)
        .expect("binding");
    assert_eq!(bound.repository_id, Some(portal.id));
    assert_eq!(
        bound.team_id,
        Some(platform),
        "binding web's repository must not make this web's task"
    );
    assert_eq!(bound.board_id, platform_board);
    let cleared =
        items::set_task_repository(&mut conn, on_platform.id, None, alice).expect("clearing");
    assert_eq!(cleared.team_id, Some(platform));
    let err = items::set_task_repository(&mut conn, on_platform.id, Some(Uuid::new_v4()), alice)
        .expect_err("a repository that does not exist");
    assert!(
        matches!(err, items::ItemError::RepositoryNotFound(_)),
        "{err}"
    );

    // --- backfill: rows the old writers left behind --------------------------
    // The three shapes that existed: no team at all (the MCP tool sent none),
    // the wrong team (a repository bind overwrote it), and an archived task
    // carrying either.
    let null_team = make_task(&mut conn, platform_board, "MCP wrote no team", alice);
    force_team(&mut conn, null_team.id, None);
    let wrong_team = make_task(&mut conn, platform_board, "A bind wrote web", alice);
    force_team(&mut conn, wrong_team.id, Some(web));
    let archived = make_task(&mut conn, web_board, "Archived with the wrong team", alice);
    force_team(&mut conn, archived.id, Some(platform));
    {
        use kairos_db::schema::tasks::dsl;
        diesel::update(dsl::tasks.filter(dsl::id.eq(archived.id)))
            .set(dsl::deleted_at.eq(diesel::dsl::now))
            .execute(&mut conn)
            .expect("archiving");
    }
    let stray_on_teamless = make_task(&mut conn, teamless_board, "Teamless, stamped", alice);
    force_team(&mut conn, stray_on_teamless.id, Some(web));

    let changed = sql_query(BACKFILL)
        .execute(&mut conn)
        .expect("running the backfill");
    assert_eq!(changed, 4, "exactly the four rows that disagreed");
    assert_eq!(team_of(&mut conn, null_team.id), Some(platform));
    assert_eq!(team_of(&mut conn, wrong_team.id), Some(platform));
    assert_eq!(
        team_of(&mut conn, archived.id),
        Some(web),
        "an archived task is corrected too: a restore must not bring the wrong team back"
    );
    assert_eq!(team_of(&mut conn, stray_on_teamless.id), None);
    // Rows that were already right are untouched.
    assert_eq!(team_of(&mut conn, on_web.id), Some(web));
    assert_eq!(team_of(&mut conn, on_nobodys.id), None);

    let changed_again = sql_query(BACKFILL)
        .execute(&mut conn)
        .expect("running the backfill a second time");
    assert_eq!(changed_again, 0, "the second run changes nothing");

    // --- move: the team follows the board ------------------------------------
    let moved = kairos_db::boards::move_task(&mut conn, on_platform.id, web_board, alice)
        .expect("moving to web's board");
    assert_eq!(moved.board_id, web_board);
    assert_eq!(team_of(&mut conn, on_platform.id), Some(web));

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}
