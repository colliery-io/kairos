//! KAIROS-T-0721 (COLLIERY-I-0663) — the stand-up of a team, in the
//! database layer:
//!
//! - `team_work` gives the claims of the person on the boards of the team
//!   only: a claim on the board of a different team is left out.
//! - A claim that an agent key of the person made counts for the person.
//! - The initiatives lane has the initiatives that the person owns and
//!   that have the team, from their tasks or set by hand. An initiative of
//!   a different team is left out.
//! - The tasks under an initiative of the person that the person does not
//!   claim are not in the lane.
//! - `unclaimed_work` gives the Todo and Blocked tasks of the team that no
//!   one claims. Not a claimed task, not a task in Backlog, not a done task,
//!   not a task of a different team.
//! - `team_people` gives the people of the team, and not a service
//!   account. A person who is not a member and claims a task of the team,
//!   or owns an initiative of the team, gets a turn, marked; a claim on a
//!   done task gives no turn.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

#[path = "common/scratch.rs"]
mod scratch;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::items::{self, CreateInitiative, CreateTask};
use kairos_db::models::enums::WorkClass;
use kairos_db::models::teams::{NewTeam, NewTeamMember};
use kairos_db::models::{
    BoardLevel, NewServiceAccountUser, NewUser, RelationshipType, TaskType, TeamType, User,
};
use kairos_db::my_work::{self, WorkState};
use kairos_db::{
    CodePrefix, boards, create_board, graph, item_owners, item_teams, provision_tenant,
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
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

/// A scratch database with the tenant `colliery`, pinned to its schema.
struct Scratch {
    admin: PgConnection,
    name: &'static str,
    _scratch_guard: scratch::ScratchGuard,
    conn: PgConnection,
}

impl Scratch {
    fn new(name: &'static str) -> Self {
        let name: &'static str = Box::leak(scratch::scratch_db_name(name).into_boxed_str());
        let admin_url = admin_database_url();
        let mut admin = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
            panic!(
                "cannot connect to compose postgres at {admin_url}: {e} \
                 (is the stack up? `angreal services up`)"
            )
        });
        sql_query(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .execute(&mut admin)
            .expect("dropping scratch database");
        sql_query(format!("CREATE DATABASE {name}"))
            .execute(&mut admin)
            .expect("creating scratch database");
        let mut conn = PgConnection::establish(&with_database(&admin_url, name))
            .expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "colliery", "Colliery").expect("provisioning colliery");
        sql_query("SET search_path TO org_colliery, public")
            .execute(&mut conn)
            .expect("pinning search_path");
        Scratch {
            admin,
            name,
            _scratch_guard: scratch::ScratchGuard::new(name),
            conn,
        }
    }

    fn drop_database(mut self) {
        drop(self.conn);
        sql_query(format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .execute(&mut self.admin)
        .expect("dropping scratch database");
    }
}

fn person(conn: &mut PgConnection, name: &str) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: format!("dex|{name}"),
            user_name: name.to_lowercase(),
            email: format!("{}@colliery.test", name.to_lowercase()),
            display_name: name.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

fn service_account(conn: &mut PgConnection) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewServiceAccountUser::new(
            format!("svc:{}", Uuid::new_v4()),
            "ci@colliery.test",
            "CI",
        ))
        .returning(users::id)
        .get_result(conn)
        .expect("inserting service account")
}

/// A team and its delivery board. Gives `(team, board)`.
fn team_with_board(conn: &mut PgConnection, slug: &str, prefix: &str, user: Uuid) -> (Uuid, Uuid) {
    use kairos_db::schema::teams;
    let team: Uuid = diesel::insert_into(teams::table)
        .values(NewTeam {
            name: format!("Team {slug}"),
            slug: slug.to_string(),
            team_type: TeamType::StreamAligned,
        })
        .returning(teams::id)
        .get_result(conn)
        .expect("inserting team");
    let board = create_board(
        conn,
        BoardLevel::Delivery,
        slug,
        slug,
        CodePrefix::Given(prefix),
        Some(team),
        Some(user),
    )
    .unwrap_or_else(|e| panic!("the delivery board of {slug}: {e}"));
    (team, board.id)
}

fn join(conn: &mut PgConnection, team: Uuid, user: Uuid) {
    use kairos_db::schema::team_members;
    diesel::insert_into(team_members::table)
        .values(NewTeamMember {
            team_id: team,
            user_id: user,
        })
        .execute(conn)
        .expect("adding a team member");
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

fn column(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board))
        .filter(board_columns::name.eq(name))
        .select(board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name}: {e}"))
}

fn initiative(conn: &mut PgConnection, user: Uuid, title: &str) -> Uuid {
    let board = board_id(conn, "initiatives");
    items::create_initiative(
        conn,
        CreateInitiative {
            board_id: board,
            column_id: None,
            title,
            content: "",
            complexity: None,
            bucket_type: None,
        },
        user,
    )
    .expect("creating an initiative")
    .id
}

fn task(conn: &mut PgConnection, user: Uuid, board: Uuid, title: &str) -> Uuid {
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
        user,
    )
    .expect("creating a task")
    .id
}

/// Move a task through `path`, column by column, as `actor`.
fn walk(conn: &mut PgConnection, board: Uuid, task: Uuid, path: &[&str], actor: Uuid) {
    for name in path {
        let to = column(conn, board, name);
        boards::transition_task(conn, task, to, actor)
            .unwrap_or_else(|e| panic!("transition to {name}: {e}"));
    }
}

fn link(conn: &mut PgConnection, user: Uuid, source: Uuid, target: Uuid, rel: RelationshipType) {
    graph::link_items(conn, source, target, rel, user).expect("an edge");
}

fn titles(items: &[my_work::WorkItem]) -> Vec<&str> {
    items.iter().map(|i| i.title.as_str()).collect()
}

#[test]
fn the_stand_up_shows_the_work_of_a_person_in_one_team() {
    let mut scratch = Scratch::new("kairos_team_work_t0721_test");
    let conn = &mut scratch.conn;
    let robin = person(conn, "Robin");
    let sam = person(conn, "Sam");
    let ci = service_account(conn);
    let (web, web_board) = team_with_board(conn, "web", "WEB", robin);
    let (api, api_board) = team_with_board(conn, "api", "API", robin);
    join(conn, web, robin);
    join(conn, web, sam);
    join(conn, web, ci);

    // Robin claims a task of web and a task of api.
    let web_claim = task(conn, robin, web_board, "web claim");
    walk(conn, web_board, web_claim, &["Todo", "Active"], robin);
    let api_claim = task(conn, robin, api_board, "api claim");
    walk(conn, api_board, api_claim, &["Todo", "Active"], robin);

    // An agent key of Robin claims a task of web.
    let key = Uuid::new_v4();
    let agent_claim = task(conn, robin, web_board, "agent claim");
    walk(conn, web_board, agent_claim, &["Todo"], robin);
    kairos_db::agent_mark::set(conn, Some(key)).expect("agent key");
    walk(conn, web_board, agent_claim, &["Active"], robin);
    kairos_db::agent_mark::set(conn, None).expect("no agent key");

    // Robin owns three initiatives: one with a task of web, one with web
    // set by hand, one with a task of api only.
    let from_tasks = initiative(conn, robin, "web from tasks");
    let under = task(conn, robin, web_board, "under robin, todo");
    walk(conn, web_board, under, &["Todo"], robin);
    link(conn, robin, from_tasks, under, RelationshipType::Parent);
    let by_hand = initiative(conn, robin, "web by hand");
    item_teams::link(conn, by_hand, web, robin).expect("setting the team");
    let other = initiative(conn, robin, "api only");
    link(conn, robin, other, api_claim, RelationshipType::Parent);
    for item in [from_tasks, by_hand, other] {
        item_owners::claim(conn, item, robin).expect("owning the initiative");
    }

    let work = my_work::team_work(conn, web, robin).expect("the team work");
    assert_eq!(
        titles(&work.tasks),
        vec!["web claim", "agent claim"],
        "only the claims on the boards of web, the agent claim with them"
    );
    let agent = work
        .tasks
        .iter()
        .find(|t| t.title == "agent claim")
        .expect("the agent claim");
    let claim = agent.claim.as_ref().expect("a claim");
    assert_eq!(claim.user_id, robin);
    assert_eq!(claim.agent_key_id, Some(key));
    assert!(work.tasks.iter().all(|t| t.state == WorkState::Active));
    let mut initiatives = titles(&work.initiatives);
    initiatives.sort();
    assert_eq!(
        initiatives,
        vec!["web by hand", "web from tasks"],
        "the initiatives of Robin that have the team web"
    );

    let api_work = my_work::team_work(conn, api, robin).expect("the api work");
    assert_eq!(titles(&api_work.tasks), vec!["api claim"]);
    assert_eq!(titles(&api_work.initiatives), vec!["api only"]);

    let sam_work = my_work::team_work(conn, web, sam).expect("the work of Sam");
    assert!(sam_work.tasks.is_empty() && sam_work.initiatives.is_empty());

    // The unclaimed stop of web.
    let _backlog = task(conn, sam, web_board, "in backlog");
    let blocked = task(conn, sam, web_board, "blocked todo");
    walk(conn, web_board, blocked, &["Todo"], sam);
    link(conn, sam, web_claim, blocked, RelationshipType::Blocks);
    let done = task(conn, sam, web_board, "done");
    walk(conn, web_board, done, &["Todo", "Active", "Completed"], sam);
    let api_todo = task(conn, sam, api_board, "api todo");
    walk(conn, api_board, api_todo, &["Todo"], sam);

    let unclaimed = my_work::unclaimed_work(conn, web).expect("the unclaimed stop");
    let mut names = titles(&unclaimed);
    names.sort();
    assert_eq!(
        names,
        vec!["blocked todo", "under robin, todo"],
        "the Todo and Blocked tasks of web that no one claims"
    );
    let state = |title: &str| {
        unclaimed
            .iter()
            .find(|t| t.title == title)
            .map(|t| t.state)
            .expect("the task")
    };
    assert_eq!(state("blocked todo"), WorkState::Blocked);
    assert_eq!(state("under robin, todo"), WorkState::Todo);
    assert!(unclaimed.iter().all(|t| t.claim.is_none()));

    // The people of web: Robin and Sam, not the service account.
    let people = |conn: &mut PgConnection| -> Vec<(String, bool)> {
        my_work::team_people(conn, web)
            .expect("the people")
            .into_iter()
            .map(|p| (p.display_name, p.member))
            .collect()
    };
    assert_eq!(
        people(conn),
        vec![("Robin".to_string(), true), ("Sam".to_string(), true)]
    );

    // Jamie is not a member of web, and claims a web task: Jamie gets a
    // turn, marked as not a member (Dylan, 2026-10-29). Casey owns an
    // initiative of web by hand and is not a member: a turn too.
    let jamie = person(conn, "Jamie");
    let theirs = task(conn, jamie, web_board, "jamie claim");
    walk(conn, web_board, theirs, &["Todo", "Active"], jamie);
    let casey = person(conn, "Casey");
    let caseys = initiative(conn, casey, "casey initiative");
    item_teams::link(conn, caseys, web, casey).expect("setting the team");
    item_owners::claim(conn, caseys, casey).expect("owning the initiative");
    // Dana claims a done web task only: no turn.
    let dana = person(conn, "Dana");
    let finished = task(conn, dana, web_board, "dana done");
    walk(conn, web_board, finished, &["Todo", "Active"], dana);
    walk(conn, web_board, finished, &["Completed"], dana);
    assert_eq!(
        people(conn),
        vec![
            ("Casey".to_string(), false),
            ("Jamie".to_string(), false),
            ("Robin".to_string(), true),
            ("Sam".to_string(), true),
        ]
    );
    let jamie_work = my_work::team_work(conn, web, jamie).expect("the work of Jamie");
    assert_eq!(titles(&jamie_work.tasks), vec!["jamie claim"]);

    scratch.drop_database();
}
