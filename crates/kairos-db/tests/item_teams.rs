//! KAIROS-T-0320 (COLLIERY-I-0602) — the teams of an initiative or a
//! strategy, in the database layer:
//!
//! - An initiative gets the team of the board of each live task below it.
//! - A strategy gets the teams of its initiatives: two levels down.
//! - A team can be set by hand, and it stays when tasks come and go.
//! - A team from tasks and set by hand is one team, with both marks.
//! - An item with no task and no hand-set team has no team.
//! - An archived task, initiative, board or team gives no team.
//! - `board_teams` gives the teams of each item of a board in one query.
//! - The link is refused on a task, a document or an ADR, and for an
//!   archived team. Set and clear write activity rows; a second set and a
//!   clear of nothing are typed errors.
//! - The migration: the CHECK keeps a team link to an initiative or a
//!   strategy, and a repository link to a document or an ADR. The
//!   down.sql removes the team links and puts back the old CHECKs.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::short_code::ItemType;
use kairos_db::item_teams::{self, ItemTeam, ItemTeamError};
use kairos_db::items::{self, CreateAdr, CreateInitiative, CreateStrategy, CreateTask};
use kairos_db::models::enums::WorkClass;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{BoardLevel, NewUser, RelationshipType, TaskType, TeamType, User};
use kairos_db::{
    CodePrefix, create_board, graph, provision_tenant, run_public_migrations, soft_delete_item,
};

const UP: &str = include_str!("../migrations/tenant/2026-10-06-000000_item_teams/up.sql");
const DOWN: &str = include_str!("../migrations/tenant/2026-10-06-000000_item_teams/down.sql");

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

/// A scratch database with the tenant `colliery`, pinned to its schema.
struct Scratch {
    admin: PgConnection,
    name: &'static str,
    conn: PgConnection,
    user: Uuid,
}

impl Scratch {
    fn new(name: &'static str) -> Self {
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
        Scratch {
            admin,
            name,
            conn,
            user,
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

fn board_id(conn: &mut PgConnection, slug: &str) -> Uuid {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::slug.eq(slug))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("the board {slug}: {e}"))
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

fn strategy(conn: &mut PgConnection, user: Uuid) -> Uuid {
    let board = board_id(conn, "strategy");
    items::create_strategy(
        conn,
        CreateStrategy {
            board_id: board,
            column_id: None,
            title: "A strategy",
            content: "",
            hypothesis: None,
        },
        user,
    )
    .expect("creating a strategy")
    .id
}

fn task_under(conn: &mut PgConnection, user: Uuid, board: Uuid, parent: Uuid) -> Uuid {
    let task = items::create_task(
        conn,
        CreateTask {
            board_id: board,
            column_id: None,
            title: "A task",
            content: "",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        user,
    )
    .expect("creating a task")
    .id;
    child(conn, user, parent, task);
    task
}

fn child(conn: &mut PgConnection, user: Uuid, parent: Uuid, child: Uuid) {
    graph::link_items(conn, parent, child, RelationshipType::Parent, user)
        .expect("the parent edge");
}

/// `(slug, from_tasks, set_by_hand)` of each team, by slug.
fn summary(teams: &[ItemTeam]) -> Vec<(&str, bool, bool)> {
    teams
        .iter()
        .map(|t| (t.slug.as_str(), t.from_tasks, t.set_by_hand))
        .collect()
}

fn teams(conn: &mut PgConnection, item: Uuid) -> Vec<(String, bool, bool)> {
    summary(&item_teams::teams_of(conn, item).expect("the teams"))
        .into_iter()
        .map(|(s, f, h)| (s.to_string(), f, h))
        .collect()
}

fn t(slug: &str, from_tasks: bool, set_by_hand: bool) -> (String, bool, bool) {
    (slug.to_string(), from_tasks, set_by_hand)
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn activity_rows(conn: &mut PgConnection, item: Uuid, details: &str) -> i64 {
    sql_query(
        "SELECT count(*) AS count FROM activity_log \
          WHERE entity_id = $1 AND details = $2",
    )
    .bind::<SqlUuid, _>(item)
    .bind::<diesel::sql_types::Text, _>(details)
    .get_result::<CountRow>(conn)
    .expect("counting activity")
    .count
}

#[test]
fn an_item_gets_its_teams_from_its_tasks_and_by_hand() {
    let mut scratch = Scratch::new("kairos_item_teams_t0320_test");
    let user = scratch.user;
    let conn = &mut scratch.conn;
    let (skadi, skadi_board) = team_with_board(conn, "skadi", "SKADI", user);
    let (_kairos, kairos_board) = team_with_board(conn, "kairos", "KAIROS", user);
    let (mimir, _mimir_board) = team_with_board(conn, "mimir", "MIMIR", user);

    // --- No team: an initiative with no task and no hand-set team.
    let empty = initiative(conn, user, "Empty");
    assert_eq!(teams(conn, empty), vec![]);

    // --- Teams from tasks: the team of the board of each task, one time
    // --- for each team, by slug.
    let i1 = initiative(conn, user, "Two teams");
    task_under(conn, user, skadi_board, i1);
    task_under(conn, user, skadi_board, i1);
    let kairos_task = task_under(conn, user, kairos_board, i1);
    assert_eq!(
        teams(conn, i1),
        vec![t("kairos", true, false), t("skadi", true, false)]
    );

    // --- Two levels down: a strategy gets the teams of its initiatives.
    let s = strategy(conn, user);
    child(conn, user, s, i1);
    child(conn, user, s, empty);
    assert_eq!(
        teams(conn, s),
        vec![t("kairos", true, false), t("skadi", true, false)]
    );

    // --- A team set by hand, before decomposition.
    let set = item_teams::link(conn, empty, mimir, user).expect("set mimir by hand");
    assert_eq!(
        (set.slug.as_str(), set.from_tasks, set.set_by_hand),
        ("mimir", false, true)
    );
    assert_eq!(teams(conn, empty), vec![t("mimir", false, true)]);
    // A hand-set team of an initiative is not a team of its strategy: the
    // strategy gets teams from tasks only, and from its own hand-set teams.
    assert_eq!(
        teams(conn, s),
        vec![t("kairos", true, false), t("skadi", true, false)]
    );
    let details = format!("relationship:impacts:{}->team:mimir", code(conn, empty));
    assert_eq!(activity_rows(conn, empty, &details), 1);

    // --- A hand-set team stays when tasks come.
    task_under(conn, user, skadi_board, empty);
    assert_eq!(
        teams(conn, empty),
        vec![t("mimir", false, true), t("skadi", true, false)]
    );

    // --- From tasks and set by hand: one team, both marks.
    item_teams::link(conn, i1, skadi, user).expect("set skadi by hand on i1");
    assert_eq!(
        teams(conn, i1),
        vec![t("kairos", true, false), t("skadi", true, true)]
    );

    // --- A strategy can have a team set by hand too.
    item_teams::link(conn, s, mimir, user).expect("set mimir on the strategy");
    assert_eq!(
        teams(conn, s),
        vec![
            t("kairos", true, false),
            t("mimir", false, true),
            t("skadi", true, false)
        ]
    );

    // --- A second set and a clear of nothing are typed errors.
    let err = item_teams::link(conn, empty, mimir, user).expect_err("set twice");
    assert!(matches!(err, ItemTeamError::AlreadyLinked { .. }), "{err}");
    let err = item_teams::unlink(conn, i1, mimir, user).expect_err("clear nothing");
    assert!(matches!(err, ItemTeamError::NotLinked { .. }), "{err}");

    // --- Clear removes the hand-set link only: a team from tasks stays.
    item_teams::unlink(conn, i1, skadi, user).expect("clear skadi on i1");
    assert_eq!(
        teams(conn, i1),
        vec![t("kairos", true, false), t("skadi", true, false)]
    );
    let details = format!("relationship:impacts:{}->team:skadi", code(conn, i1));
    assert_eq!(
        activity_rows(conn, i1, &details),
        2,
        "one row for the set and one for the clear"
    );

    // --- An archived task gives no team.
    soft_delete_item(conn, ItemType::Task, kairos_task, user).expect("archive the kairos task");
    assert_eq!(teams(conn, i1), vec![t("skadi", true, false)]);

    // --- An archived initiative gives its strategy no team.
    soft_delete_item(conn, ItemType::Initiative, i1, user).expect("archive i1");
    assert_eq!(
        teams(conn, s),
        vec![t("mimir", false, true), t("skadi", true, false)],
        "skadi now comes from the task under `empty`"
    );

    // --- An archived team gives no team, from tasks or by hand. Its link
    // --- stays, and it can be cleared.
    sql_query("UPDATE teams SET deleted_at = now() WHERE id = $1")
        .bind::<SqlUuid, _>(mimir)
        .execute(conn)
        .expect("archive mimir");
    assert_eq!(teams(conn, empty), vec![t("skadi", true, false)]);
    let live = initiative(conn, user, "Live");
    let err = item_teams::link(conn, live, mimir, user).expect_err("a link to an archived team");
    assert!(matches!(err, ItemTeamError::TeamNotFound(_)), "{err}");
    item_teams::unlink(conn, empty, mimir, user).expect("clear a link to an archived team");

    // --- A task on an archived board gives no team.
    sql_query("UPDATE boards SET deleted_at = now() WHERE id = $1")
        .bind::<SqlUuid, _>(skadi_board)
        .execute(conn)
        .expect("archive the skadi board");
    assert_eq!(teams(conn, empty), vec![]);

    scratch.drop_database();
}

fn code(conn: &mut PgConnection, item: Uuid) -> String {
    #[derive(QueryableByName)]
    struct Code {
        #[diesel(sql_type = diesel::sql_types::Text)]
        short_code: String,
    }
    sql_query("SELECT short_code FROM entity_directory WHERE id = $1")
        .bind::<SqlUuid, _>(item)
        .get_result::<Code>(conn)
        .expect("the short code")
        .short_code
}

#[test]
fn a_board_gives_the_teams_of_each_item_in_one_query() {
    let mut scratch = Scratch::new("kairos_item_teams_board_t0320_test");
    let user = scratch.user;
    let conn = &mut scratch.conn;
    let (_skadi, skadi_board) = team_with_board(conn, "skadi", "SKADI", user);
    let (weir, weir_board) = team_with_board(conn, "weir", "WEIR", user);

    let i1 = initiative(conn, user, "One");
    task_under(conn, user, skadi_board, i1);
    let i2 = initiative(conn, user, "Two");
    item_teams::link(conn, i2, weir, user).expect("set weir");
    task_under(conn, user, weir_board, i2);
    let none = initiative(conn, user, "None");

    let initiatives = board_id(conn, "initiatives");
    let by_item = item_teams::board_teams(conn, initiatives).expect("the board teams");
    assert_eq!(summary(&by_item[&i1]), vec![("skadi", true, false)]);
    assert_eq!(summary(&by_item[&i2]), vec![("weir", true, true)]);
    assert!(
        !by_item.contains_key(&none),
        "an item with no team has no entry"
    );
    assert_eq!(by_item.len(), 2);

    // The strategy board reads its own items, two levels down.
    let s = strategy(conn, user);
    child(conn, user, s, i1);
    child(conn, user, s, i2);
    let strategies = board_id(conn, "strategy");
    let by_item = item_teams::board_teams(conn, strategies).expect("the strategy board teams");
    assert_eq!(
        summary(&by_item[&s]),
        vec![("skadi", true, false), ("weir", true, false)]
    );

    // `teams_of_items` gives the same answer for a list.
    let many = item_teams::teams_of_items(conn, &[i1, i2, none]).expect("teams of items");
    assert_eq!(summary(&many[&i1]), vec![("skadi", true, false)]);
    assert!(!many.contains_key(&none));

    scratch.drop_database();
}

#[test]
fn only_an_initiative_or_a_strategy_has_a_team() {
    let mut scratch = Scratch::new("kairos_item_teams_subject_t0320_test");
    let user = scratch.user;
    let conn = &mut scratch.conn;
    let (skadi, skadi_board) = team_with_board(conn, "skadi", "SKADI", user);
    let parent = initiative(conn, user, "Parent");
    let task = task_under(conn, user, skadi_board, parent);

    let err = item_teams::link(conn, task, skadi, user).expect_err("a task");
    assert!(
        matches!(
            &err,
            ItemTeamError::SubjectType {
                item_type: ItemType::Task,
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "SKADI-T-0001 is a task. Only an initiative or a strategy can have a team."
    );

    let adrs = board_id(conn, "adrs");
    let adr = items::create_adr(
        conn,
        CreateAdr {
            board_id: Some(adrs),
            column_id: None,
            title: "A decision",
            content: "",
            decision_maker: None,
            decision_date: None,
        },
        user,
    )
    .expect("an ADR")
    .id;
    let err = item_teams::link(conn, adr, skadi, user).expect_err("an ADR");
    assert!(matches!(err, ItemTeamError::SubjectType { .. }), "{err}");

    let err = item_teams::link(conn, parent, Uuid::new_v4(), user).expect_err("no team");
    assert!(matches!(err, ItemTeamError::TeamNotFound(_)), "{err}");

    // The CHECK keeps the pairs apart, also for a write that skips the
    // service: an ADR cannot impact a team, an initiative cannot impact a
    // repository.
    let bad = sql_query(
        "INSERT INTO item_impacts (item_id, item_type, target_kind, target_id, created_by) \
         VALUES ($1, 'adr', 'team', $2, $3)",
    )
    .bind::<SqlUuid, _>(adr)
    .bind::<SqlUuid, _>(skadi)
    .bind::<SqlUuid, _>(user)
    .execute(conn);
    assert!(bad.is_err(), "an ADR that impacts a team");
    let bad = sql_query(
        "INSERT INTO item_impacts (item_id, item_type, target_kind, target_id, created_by) \
         VALUES ($1, 'initiative', 'repository', $2, $3)",
    )
    .bind::<SqlUuid, _>(parent)
    .bind::<SqlUuid, _>(Uuid::new_v4())
    .bind::<SqlUuid, _>(user)
    .execute(conn);
    assert!(bad.is_err(), "an initiative that impacts a repository");

    scratch.drop_database();
}

#[test]
fn the_migration_runs_again_and_its_down_removes_the_team_links() {
    let mut scratch = Scratch::new("kairos_item_teams_migration_t0320_test");
    let user = scratch.user;
    let conn = &mut scratch.conn;
    let (skadi, _) = team_with_board(conn, "skadi", "SKADI", user);
    let i1 = initiative(conn, user, "One");
    item_teams::link(conn, i1, skadi, user).expect("set skadi");

    // Up again: no error, no change.
    conn.batch_execute(UP).expect("up.sql runs again");
    assert_eq!(item_teams::teams_of(conn, i1).expect("teams").len(), 1);

    // Down: the team links go, and a team link is refused again.
    conn.batch_execute(DOWN).expect("down.sql");
    assert_eq!(item_teams::teams_of(conn, i1).expect("teams").len(), 0);
    let refused = sql_query(
        "INSERT INTO item_impacts (item_id, item_type, target_kind, target_id, created_by) \
         VALUES ($1, 'initiative', 'team', $2, $3)",
    )
    .bind::<SqlUuid, _>(i1)
    .bind::<SqlUuid, _>(skadi)
    .bind::<SqlUuid, _>(user)
    .execute(conn);
    assert!(refused.is_err(), "the old CHECK refuses a team link");

    // Up after down: the team link works again.
    conn.batch_execute(UP).expect("up.sql after down.sql");
    item_teams::link(conn, i1, skadi, user).expect("set skadi again");

    scratch.drop_database();
}
