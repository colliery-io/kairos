//! COLLIERY-T-0240, T-0241, T-0242, T-0243 — the rules of the team of a
//! board, in the database layer:
//!
//! - T-0240: a team has one delivery board. `create_board` refuses a second
//!   one, and two creates at the same time give one board.
//! - T-0241: `check_board_delete` refuses the delete of the only delivery
//!   board of a team.
//! - T-0242: `create_board` refuses a board of the organization with a team.
//! - T-0243: `check_board_team` refuses a change of the team of a board.
//!
//! COLLIERY-T-0230 (`board_team.rs`) is the rule that these complete: a
//! delivery board always has a team.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use std::sync::{Arc, Barrier};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::boards::BoardError;
use kairos_db::models::boards::{Board, NewBoard};
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{
    check_board_delete, check_board_team, create_board, provision_tenant, run_public_migrations,
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

/// A scratch database with the tenant `acme`, and a user to act as.
struct Scratch {
    admin: PgConnection,
    name: &'static str,
    url: String,
    conn: PgConnection,
    alice: Uuid,
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
        let url = with_database(&admin_url, name);
        let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
        pin(&mut conn);
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
        Scratch {
            admin,
            name,
            url,
            conn,
            alice,
        }
    }

    /// A second connection to the tenant, for a concurrent writer.
    fn connect(&self) -> PgConnection {
        let mut conn = PgConnection::establish(&self.url).expect("a second connection");
        pin(&mut conn);
        conn
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

fn pin(conn: &mut PgConnection) {
    sql_query("SET search_path TO org_acme, public")
        .execute(conn)
        .expect("pinning search_path");
}

/// A team with no delivery board.
fn bare_team(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid {
    use kairos_db::schema::teams;
    diesel::insert_into(teams::table)
        .values(NewTeam {
            name: name.into(),
            slug: slug.into(),
            team_type: TeamType::StreamAligned,
        })
        .returning(teams::id)
        .get_result(conn)
        .expect("inserting team")
}

/// A board row written with no rule, as old data has it.
fn old_board(conn: &mut PgConnection, slug: &str, level: BoardLevel, team: Option<Uuid>) -> Board {
    use kairos_db::schema::boards;
    diesel::insert_into(boards::table)
        .values(NewBoard {
            name: slug.into(),
            slug: slug.into(),
            board_level: level,
            team_id: team,
            // A prefix of its own (COLLIERY-T-3099).
            code_prefix: format!("B{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
                .to_uppercase(),
        })
        .returning(Board::as_returning())
        .get_result(conn)
        .expect("inserting board")
}

fn soft_delete_board(conn: &mut PgConnection, board: Uuid) {
    use kairos_db::schema::boards::dsl;
    diesel::update(dsl::boards.filter(dsl::id.eq(board)))
        .set(dsl::deleted_at.eq(diesel::dsl::now))
        .execute(conn)
        .expect("deleting board");
}

fn soft_delete_team(conn: &mut PgConnection, team: Uuid) {
    use kairos_db::schema::teams::dsl;
    diesel::update(dsl::teams.filter(dsl::id.eq(team)))
        .set(dsl::deleted_at.eq(diesel::dsl::now))
        .execute(conn)
        .expect("deleting team");
}

/// How many LIVE delivery boards the team has.
fn live_delivery_boards(conn: &mut PgConnection, team: Uuid) -> i64 {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::team_id.eq(team))
        .filter(dsl::board_level.eq(BoardLevel::Delivery))
        .filter(dsl::deleted_at.is_null())
        .count()
        .get_result(conn)
        .expect("counting boards")
}

/// How many board rows have this slug, live or not.
fn boards_with_slug(conn: &mut PgConnection, slug: &str) -> i64 {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::slug.eq(slug))
        .count()
        .get_result(conn)
        .expect("counting boards")
}

/// COLLIERY-T-0240.
#[test]
fn a_team_has_one_delivery_board() {
    let mut scratch = Scratch::new("kairos_board_rules_t0240_test");
    let alice = scratch.alice;
    let conn = &mut scratch.conn;

    let platform = bare_team(conn, "Platform", "platform");
    let first = create_board(
        conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        kairos_db::CodePrefix::FromSlug,
        Some(platform),
        Some(alice),
    )
    .expect("the first delivery board of the team");

    // --- the second delivery board is refused, and the refusal names the
    // --- board that the team has
    let err = create_board(
        conn,
        BoardLevel::Delivery,
        "Platform Two",
        "platform-two",
        kairos_db::CodePrefix::FromSlug,
        Some(platform),
        Some(alice),
    )
    .expect_err("a second delivery board");
    match &err {
        BoardError::TeamHasDeliveryBoard {
            team_id,
            board_id,
            board_name,
            board_slug,
        } => {
            assert_eq!(*team_id, platform);
            assert_eq!(*board_id, first.id);
            assert_eq!(board_name, "Platform Delivery");
            assert_eq!(board_slug, "platform-delivery");
        }
        other => panic!("expected TeamHasDeliveryBoard, got {other}"),
    }
    assert_eq!(
        err.to_string(),
        "The team has the delivery board \"Platform Delivery\". A team has only one delivery \
         board. Use that board."
    );
    assert_eq!(boards_with_slug(conn, "platform-two"), 0);
    assert_eq!(live_delivery_boards(conn, platform), 1);

    // --- a different team is not refused
    let data = bare_team(conn, "Data", "data");
    create_board(
        conn,
        BoardLevel::Delivery,
        "Data Delivery",
        "data-delivery",
        kairos_db::CodePrefix::FromSlug,
        Some(data),
        Some(alice),
    )
    .expect("the delivery board of a different team");

    // --- a deleted board does not count
    soft_delete_board(conn, first.id);
    let second = create_board(
        conn,
        BoardLevel::Delivery,
        "Platform Work",
        "platform-work",
        kairos_db::CodePrefix::FromSlug,
        Some(platform),
        Some(alice),
    )
    .expect("a delivery board after the delete of the first");
    assert_eq!(second.team_id, Some(platform));
    assert_eq!(live_delivery_boards(conn, platform), 1);

    // --- two creates at the same time give one board
    for round in 0..8 {
        let team = bare_team(
            &mut scratch.conn,
            &format!("Race {round}"),
            &format!("race-{round}"),
        );
        let barrier = Arc::new(Barrier::new(2));
        let writers: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|writer| {
                let mut conn = scratch.connect();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    create_board(
                        &mut conn,
                        BoardLevel::Delivery,
                        &format!("Race {round} {writer}"),
                        &format!("race-{round}-{writer}"),
                        kairos_db::CodePrefix::FromSlug,
                        Some(team),
                        Some(alice),
                    )
                })
            })
            .collect();
        let results: Vec<_> = writers
            .into_iter()
            .map(|writer| writer.join().expect("the writer does not panic"))
            .collect();
        let created = results.iter().filter(|r| r.is_ok()).count();
        let refused = results
            .iter()
            .filter(|r| matches!(r, Err(BoardError::TeamHasDeliveryBoard { .. })))
            .count();
        assert_eq!((created, refused), (1, 1), "round {round}: {results:?}");
        assert_eq!(live_delivery_boards(&mut scratch.conn, team), 1);
    }

    scratch.drop_database();
}

/// COLLIERY-T-0241.
#[test]
fn the_only_delivery_board_of_a_team_is_not_deleted() {
    let mut scratch = Scratch::new("kairos_board_rules_t0241_test");
    let alice = scratch.alice;
    let conn = &mut scratch.conn;

    let platform = bare_team(conn, "Platform", "platform");
    let board = create_board(
        conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        kairos_db::CodePrefix::FromSlug,
        Some(platform),
        Some(alice),
    )
    .expect("the delivery board");

    let err = check_board_delete(conn, &board).expect_err("the only delivery board");
    match &err {
        BoardError::LastDeliveryBoard {
            board_id,
            team_id,
            team_name,
        } => {
            assert_eq!(*board_id, board.id);
            assert_eq!(*team_id, platform);
            assert_eq!(team_name, "Platform");
        }
        other => panic!("expected LastDeliveryBoard, got {other}"),
    }
    assert_eq!(
        err.to_string(),
        "This board is the only delivery board of the team \"Platform\". A team always has a \
         delivery board. To remove the board, delete the team."
    );

    // --- old data: a team with two delivery boards can lose one, and then
    // --- the rule applies to the board that stays
    let extra = old_board(conn, "platform-extra", BoardLevel::Delivery, Some(platform));
    check_board_delete(conn, &extra).expect("one of two delivery boards");
    check_board_delete(conn, &board).expect("the other of two delivery boards");
    soft_delete_board(conn, extra.id);
    let err = check_board_delete(conn, &board).expect_err("the board that stays");
    assert!(matches!(err, BoardError::LastDeliveryBoard { .. }), "{err}");

    // --- a board of the organization has no delivery team
    for (level, slug) in [
        (BoardLevel::Strategy, "second-strategy"),
        (BoardLevel::Initiative, "second-initiatives"),
        (BoardLevel::Adr, "second-adrs"),
    ] {
        let board = create_board(
            conn,
            level,
            slug,
            slug,
            kairos_db::CodePrefix::FromSlug,
            None,
            Some(alice),
        )
        .unwrap_or_else(|e| panic!("a {level} board: {e}"));
        check_board_delete(conn, &board).unwrap_or_else(|e| panic!("a {level} board: {e}"));
    }

    // --- old data: a delivery board with no team
    let teamless = old_board(conn, "teamless", BoardLevel::Delivery, None);
    check_board_delete(conn, &teamless).expect("a delivery board with no team");

    // --- the board of a deleted team. The delete of a team removes the
    // --- team and the board together, so the team goes first.
    soft_delete_team(conn, platform);
    check_board_delete(conn, &board).expect("the board of a deleted team");

    scratch.drop_database();
}

/// COLLIERY-T-0242.
#[test]
fn a_board_of_the_organization_has_no_team_id() {
    let mut scratch = Scratch::new("kairos_board_rules_t0242_test");
    let alice = scratch.alice;
    let conn = &mut scratch.conn;

    let platform = bare_team(conn, "Platform", "platform");
    for (level, slug, name) in [
        (BoardLevel::Strategy, "team-strategy", "strategy"),
        (BoardLevel::Initiative, "team-initiatives", "initiative"),
        (BoardLevel::Adr, "team-adrs", "adr"),
    ] {
        let err = create_board(
            conn,
            level,
            slug,
            slug,
            kairos_db::CodePrefix::FromSlug,
            Some(platform),
            Some(alice),
        )
        .expect_err("a board of the organization with a team");
        assert!(
            matches!(err, BoardError::OrganizationBoardHasNoTeam(l) if l == level),
            "{level}: {err}"
        );
        assert_eq!(
            err.to_string(),
            format!(
                "Only a delivery board has a team. Do not send team_id for a board of level \
                 {name}."
            )
        );
        assert_eq!(boards_with_slug(conn, slug), 0, "{level}");
        // With no team, the same create succeeds.
        let board = create_board(
            conn,
            level,
            slug,
            slug,
            kairos_db::CodePrefix::FromSlug,
            None,
            Some(alice),
        )
        .unwrap_or_else(|e| panic!("a {level} board with no team: {e}"));
        assert_eq!(board.team_id, None);
    }
    // The team did not use its one delivery board.
    assert_eq!(live_delivery_boards(conn, platform), 0);

    scratch.drop_database();
}

/// COLLIERY-T-0243.
#[test]
fn the_team_of_a_board_does_not_change() {
    let mut scratch = Scratch::new("kairos_board_rules_t0243_test");
    let alice = scratch.alice;
    let conn = &mut scratch.conn;

    let platform = bare_team(conn, "Platform", "platform");
    let data = bare_team(conn, "Data", "data");
    let delivery = create_board(
        conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        kairos_db::CodePrefix::FromSlug,
        Some(platform),
        Some(alice),
    )
    .expect("the delivery board");
    let strategy = create_board(
        conn,
        BoardLevel::Strategy,
        "Second Strategy",
        "second-strategy",
        kairos_db::CodePrefix::FromSlug,
        None,
        Some(alice),
    )
    .expect("a strategy board");

    // No team in the update, and the team that the board has.
    check_board_team(&delivery, None).expect("no team_id");
    check_board_team(&delivery, Some(Some(platform))).expect("the same team");
    check_board_team(&strategy, None).expect("no team_id");
    check_board_team(&strategy, Some(None)).expect("the same null");

    // A different team, and a null for a board that has a team.
    for (board, sent) in [
        (&delivery, Some(data)),
        (&delivery, None),
        (&strategy, Some(platform)),
    ] {
        let err = check_board_team(board, Some(sent)).expect_err("a different team");
        match &err {
            BoardError::BoardTeamIsFixed { board_id, team_id } => {
                assert_eq!(*board_id, board.id);
                assert_eq!(*team_id, board.team_id);
            }
            other => panic!("expected BoardTeamIsFixed, got {other}"),
        }
        assert_eq!(
            err.to_string(),
            "The team of a board does not change. To give the work to a different team, move \
             the task to the board of that team."
        );
    }

    scratch.drop_database();
}
