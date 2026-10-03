//! COLLIERY-T-0250 — the delivery boards of a team, for the delete of the
//! team and for the report, in the database layer:
//!
//! - `delivery_boards_for_team_delete` gives each live delivery board of
//!   the team, oldest first.
//! - `teams_with_several_delivery_boards` gives each team with 2 or more
//!   live delivery boards.
//!
//! The server half is `kairos-server/tests/team_delete_boards.rs`.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). The test
//! owns a scratch database.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::models::TeamType;
use kairos_db::models::boards::NewBoard;
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::teams::NewTeam;
use kairos_db::{
    delivery_boards_for_team_delete, provision_tenant, run_public_migrations,
    teams_with_several_delivery_boards,
};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_team_delete_t0250_db_test";

fn team(conn: &mut PgConnection, name: &str, slug: &str) -> Uuid {
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
fn old_board(conn: &mut PgConnection, slug: &str, level: BoardLevel, team: Option<Uuid>) -> Uuid {
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
        .returning(boards::id)
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

#[test]
fn the_delivery_boards_of_a_team() {
    let admin_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    let mut admin = PgConnection::establish(&admin_url).unwrap_or_else(|e| {
        panic!(
            "cannot connect to compose postgres at {admin_url}: {e} \
             (is the stack up? `angreal services up`)"
        )
    });
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin)
        .expect("creating scratch database");
    let (base, _) = admin_url.rsplit_once('/').expect("a database path segment");
    let mut scratch =
        PgConnection::establish(&format!("{base}/{SCRATCH_DB}")).expect("connecting to scratch");
    let conn = &mut scratch;
    run_public_migrations(conn).expect("running public migrations");
    provision_tenant(conn, "acme", "Acme Inc").expect("provisioning acme");
    sql_query("SET search_path TO org_acme, public")
        .execute(conn)
        .expect("pinning search_path");

    // --- a tenant with the rule has no such team
    let one = team(conn, "One", "one");
    let one_board = old_board(conn, "one-delivery", BoardLevel::Delivery, Some(one));
    assert_eq!(
        teams_with_several_delivery_boards(conn).expect("report"),
        []
    );
    let boards = delivery_boards_for_team_delete(conn, one).expect("boards");
    assert_eq!(boards.iter().map(|b| b.id).collect::<Vec<_>>(), [one_board]);

    // --- old data: a team with 2 live delivery boards and a deleted one
    let two = team(conn, "Two", "two");
    let first = old_board(conn, "two-delivery", BoardLevel::Delivery, Some(two));
    let second = old_board(conn, "two-extra", BoardLevel::Delivery, Some(two));
    let gone = old_board(conn, "two-gone", BoardLevel::Delivery, Some(two));
    soft_delete_board(conn, gone);
    // A board of a different level, and a board with no team, do not count.
    old_board(conn, "one-strategy", BoardLevel::Strategy, Some(one));
    old_board(conn, "no-team", BoardLevel::Delivery, None);
    old_board(conn, "no-team-two", BoardLevel::Delivery, None);

    let boards = delivery_boards_for_team_delete(conn, two).expect("boards");
    let mut ids: Vec<Uuid> = boards.iter().map(|b| b.id).collect();
    ids.sort();
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(ids, expected, "each live delivery board, and no other");

    let report = teams_with_several_delivery_boards(conn).expect("report");
    assert_eq!(report.len(), 1, "{report:?}");
    assert_eq!(report[0].team_id, two);
    assert_eq!(report[0].team_slug, "two");
    assert_eq!(report[0].team_name, "Two");
    assert!(!report[0].team_deleted);
    let mut ids: Vec<Uuid> = report[0].boards.iter().map(|b| b.id).collect();
    ids.sort();
    assert_eq!(ids, expected);

    // --- a deleted team with live boards stays in the report, with a mark
    soft_delete_team(conn, two);
    let report = teams_with_several_delivery_boards(conn).expect("report");
    assert_eq!(report.len(), 1, "{report:?}");
    assert!(report[0].team_deleted);

    // --- with one board left, the team is not in the report
    soft_delete_board(conn, second);
    assert_eq!(
        teams_with_several_delivery_boards(conn).expect("report"),
        []
    );

    drop(scratch);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin)
        .expect("dropping scratch database");
}
