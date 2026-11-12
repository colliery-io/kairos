//! KAIROS-T-0764 (COLLIERY-I-0674, KAIROS-A-0084) — the AdHoc stand-up, in
//! the database layer:
//!
//! - A group has a name, unique in the organization with no regard to
//!   case, and people. Each member can edit each group; only the creator
//!   deletes it (`DeleteOutcome::NotCreator` for the others, and nothing
//!   changes).
//! - `person_work` gives the work of a person by ownership on each board:
//!   the claims on the boards of two teams (an agent claim counts for the
//!   person), and the initiatives of both teams. Not the tasks under the
//!   initiatives that the person does not claim, and not the tasks that no
//!   one claims (there is no Unclaimed stop).
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
use kairos_db::my_work;
use kairos_db::standup_groups::{self, DeleteOutcome, GroupError};
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
fn a_group_has_rights_and_its_stop_spans_the_boards_of_two_teams() {
    let mut scratch = Scratch::new("kairos_standup_groups_t0764_test");
    let conn = &mut scratch.conn;
    let robin = person(conn, "Robin");
    let sam = person(conn, "Sam");
    let jamie = person(conn, "Jamie");
    let _ci = service_account(conn);
    let (web, web_board) = team_with_board(conn, "web", "WEB", robin);
    let (api, api_board) = team_with_board(conn, "api", "API", robin);
    join(conn, web, robin);
    join(conn, api, sam);

    // --- create, read, edit by a different member, delete by the creator.
    let group =
        standup_groups::create(conn, "Release crew", robin, &[sam, robin, sam]).expect("a group");
    assert_eq!(group.name, "Release crew");
    assert_eq!(group.created_by, robin);
    let people = |conn: &mut PgConnection, id: Uuid| -> Vec<String> {
        standup_groups::people_of(conn, &[id])
            .expect("the people")
            .remove(&id)
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.display_name)
            .collect()
    };
    assert_eq!(people(conn, group.id), vec!["Robin", "Sam"]);
    assert!(matches!(
        standup_groups::create(conn, "release CREW", sam, &[]),
        Err(GroupError::NameTaken)
    ));

    // Sam did not create the group, and edits its name and its people.
    let renamed = standup_groups::update(conn, group.id, Some("Launch"), Some(&[sam, jamie]))
        .expect("Sam edits the group");
    assert_eq!(renamed.name, "Launch");
    assert_eq!(renamed.created_by, robin, "the creator does not change");
    assert_eq!(people(conn, group.id), vec!["Jamie", "Sam"]);
    let same = standup_groups::update(conn, group.id, None, None).expect("no change");
    assert_eq!(same.name, "Launch");
    assert_eq!(people(conn, group.id), vec!["Jamie", "Sam"]);
    assert!(standup_groups::has_person(conn, group.id, jamie).expect("has"));
    assert!(!standup_groups::has_person(conn, group.id, robin).expect("has not"));
    let other = standup_groups::create(conn, "Other", sam, &[robin]).expect("a second group");
    assert!(matches!(
        standup_groups::update(conn, other.id, Some("LAUNCH"), None),
        Err(GroupError::NameTaken)
    ));
    assert!(matches!(
        standup_groups::update(conn, Uuid::new_v4(), Some("x"), None),
        Err(GroupError::NotFound)
    ));
    let names: Vec<String> = standup_groups::list(conn)
        .expect("the list")
        .into_iter()
        .map(|g| g.name)
        .collect();
    assert_eq!(names, vec!["Launch", "Other"]);

    // Only the creator deletes.
    assert_eq!(
        standup_groups::delete(conn, group.id, sam).expect("refused"),
        DeleteOutcome::NotCreator
    );
    assert!(standup_groups::get(conn, group.id).expect("get").is_some());
    assert_eq!(
        standup_groups::delete(conn, group.id, robin).expect("deleted"),
        DeleteOutcome::Deleted
    );
    assert!(standup_groups::get(conn, group.id).expect("get").is_none());
    assert!(people(conn, group.id).is_empty(), "the people go with it");
    assert_eq!(
        standup_groups::delete(conn, group.id, robin).expect("gone"),
        DeleteOutcome::NotFound
    );

    // --- the stop of Robin spans the boards of web and api.
    let web_claim = task(conn, robin, web_board, "web claim");
    walk(conn, web_board, web_claim, &["Todo", "Active"], robin);
    let api_claim = task(conn, robin, api_board, "api claim");
    walk(conn, api_board, api_claim, &["Todo", "Active"], robin);
    let key = Uuid::new_v4();
    let agent_claim = task(conn, robin, api_board, "agent claim");
    walk(conn, api_board, agent_claim, &["Todo"], robin);
    kairos_db::agent_mark::set(conn, Some(key)).expect("agent key");
    walk(conn, api_board, agent_claim, &["Active"], robin);
    kairos_db::agent_mark::set(conn, None).expect("no agent key");
    let sams = task(conn, sam, api_board, "sam claim");
    walk(conn, api_board, sams, &["Todo", "Active"], sam);

    let web_initiative = initiative(conn, robin, "web initiative");
    item_teams::link(conn, web_initiative, web, robin).expect("the team");
    let api_initiative = initiative(conn, robin, "api initiative");
    let under = task(conn, robin, api_board, "under robin, todo");
    walk(conn, api_board, under, &["Todo"], robin);
    link(conn, robin, api_initiative, under, RelationshipType::Parent);
    for item in [web_initiative, api_initiative] {
        item_owners::claim(conn, item, robin).expect("owning the initiative");
    }
    let _unclaimed = task(conn, sam, web_board, "no claim");

    let work = my_work::person_work(conn, robin).expect("the stop of Robin");
    let mut tasks = titles(&work.tasks);
    tasks.sort();
    assert_eq!(
        tasks,
        vec!["agent claim", "api claim", "web claim"],
        "the claims on the boards of both teams; not the task under the \
         initiative that no one claims, not the claim of Sam"
    );
    assert!(
        work.tasks
            .iter()
            .all(|t| t.claim.as_ref().is_some_and(|c| c.user_id == robin))
    );
    let mut boards: Vec<&str> = work.tasks.iter().map(|t| t.board_slug.as_str()).collect();
    boards.sort();
    boards.dedup();
    assert_eq!(boards, vec!["api", "web"]);
    let mut initiatives = titles(&work.initiatives);
    initiatives.sort();
    assert_eq!(initiatives, vec!["api initiative", "web initiative"]);
    assert!(
        work.tasks
            .iter()
            .chain(work.initiatives.iter())
            .all(|i| !i.reasons.contains(&my_work::WorkReason::Unclaimed)),
        "no Unclaimed stop"
    );

    // The team stand-up keeps its filter.
    let web_work = my_work::team_work(conn, web, robin).expect("the web work");
    assert_eq!(titles(&web_work.tasks), vec!["web claim"]);
    assert_eq!(titles(&web_work.initiatives), vec!["web initiative"]);

    let jamie_work = my_work::person_work(conn, jamie).expect("the stop of Jamie");
    assert!(jamie_work.tasks.is_empty() && jamie_work.initiatives.is_empty());

    scratch.drop_database();
}
