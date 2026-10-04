//! COLLIERY-T-3102 — team ADR boards, in the database layer:
//!
//! - Scenario: A team can have an ADR board. `create_board` accepts a team
//!   for an ADR board, and an ADR on it gets the prefix of the team.
//! - Scenario: A team has at most one ADR board. A second one is refused,
//!   and the refusal names the board that exists. Two creates at the same
//!   time give one board.
//! - The prefix of a team ADR board is the prefix of the team.
//! - Scenario: The ADRs of colliery-io keep their codes. The migration
//!   `team_adr_boards` gives the board `adrs` the team whose delivery board
//!   has its prefix. No code changes, and it can run 2 times.
//! - The delete of a team removes its ADR board too
//!   (`boards_for_team_delete`).
//! - COLLIERY-T-3108: the team rule gives a member `manage_adrs` on the ADR
//!   board of the team, and not on the ADR board of the organization.
//!
//! The REST half is in `kairos-server/tests/board_team_rules.rs` and
//! `kairos-server/tests/team_adr_writes.rs`.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2). Each test
//! owns a scratch database.

use std::sync::{Arc, Barrier};

use diesel::connection::SimpleConnection;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::items::{self, CreateAdr};
use kairos_db::models::boards::Board;
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::teams::{NewTeam, NewTeamMember};
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{
    BoardError, CodePrefix, boards_for_team_delete, create_board, provision_tenant,
    run_public_migrations,
};

/// The migration under test, verbatim: what a deployment runs is what this
/// test runs.
const UP: &str = include_str!("../migrations/tenant/2026-10-03-000003_team_adr_boards/up.sql");

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
    url: String,
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
        let url = with_database(&admin_url, name);
        let mut conn = PgConnection::establish(&url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "colliery", "Colliery").expect("provisioning colliery");
        pin(&mut conn);
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
            url,
            conn,
            user,
        }
    }

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
    sql_query("SET search_path TO org_colliery, public")
        .execute(conn)
        .expect("pinning search_path");
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

/// A team and its delivery board, with `prefix`.
fn team_with_board(conn: &mut PgConnection, slug: &str, prefix: &str, user: Uuid) -> Uuid {
    let id = team(conn, slug);
    create_board(
        conn,
        BoardLevel::Delivery,
        slug,
        slug,
        CodePrefix::Given(prefix),
        Some(id),
        Some(user),
    )
    .unwrap_or_else(|e| panic!("the delivery board of {slug}: {e}"));
    id
}

fn board(conn: &mut PgConnection, slug: &str) -> Board {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::slug.eq(slug))
        .filter(dsl::deleted_at.is_null())
        .select(Board::as_select())
        .first(conn)
        .unwrap_or_else(|e| panic!("the board {slug}: {e}"))
}

fn adr(conn: &mut PgConnection, board: Uuid, user: Uuid) -> String {
    items::create_adr(
        conn,
        CreateAdr {
            board_id: Some(board),
            column_id: None,
            title: "A decision",
            content: "",
            decision_maker: None,
            decision_date: None,
        },
        user,
    )
    .expect("creating an ADR")
    .short_code
}

fn live_adr_boards(conn: &mut PgConnection, team: Uuid) -> i64 {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::team_id.eq(team))
        .filter(dsl::board_level.eq(BoardLevel::Adr))
        .filter(dsl::deleted_at.is_null())
        .count()
        .get_result(conn)
        .expect("counting boards")
}

// Scenario: A team can have an ADR board
// Scenario: A team has at most one ADR board
#[test]
fn a_team_has_one_adr_board_with_the_prefix_of_the_team() {
    let mut scratch = Scratch::new("kairos_team_adr_boards_t3102_test");
    let user = scratch.user;
    let conn = &mut scratch.conn;
    let skadi = team_with_board(conn, "skadi", "SKADI", user);

    // --- the prefix of a team ADR board is the prefix of the team
    let err = create_board(
        conn,
        BoardLevel::Adr,
        "Skadi ADRs",
        "skadi-adrs",
        CodePrefix::Given("SKADIA"),
        Some(skadi),
        Some(user),
    )
    .expect_err("a prefix that is not the prefix of the team");
    assert!(
        matches!(&err, BoardError::TeamAdrBoardPrefix { expected, given }
            if expected == "SKADI" && given == "SKADIA"),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "The ADR board of the team must have the prefix of the team, \"SKADI\", not \
         \"SKADIA\". Send \"SKADI\" as code_prefix."
    );
    assert_eq!(live_adr_boards(conn, skadi), 0);

    // --- When an admin creates an ADR board for the team "skadi" with
    // --- prefix "SKADI", Then the board exists, and an ADR on it gets the
    // --- code "SKADI-A-0001".
    let created = create_board(
        conn,
        BoardLevel::Adr,
        "Skadi ADRs",
        "skadi-adrs",
        CodePrefix::Given("SKADI"),
        Some(skadi),
        Some(user),
    )
    .expect("the ADR board of the team");
    assert_eq!(created.team_id, Some(skadi));
    assert_eq!(created.code_prefix, "SKADI");
    assert_eq!(created.board_level, BoardLevel::Adr);
    assert_eq!(adr(conn, created.id, user), "SKADI-A-0001");
    assert_eq!(adr(conn, created.id, user), "SKADI-A-0002");
    // The ADR board of the organization keeps the tenant prefix.
    let org_adrs = board(conn, "adrs");
    assert_eq!(org_adrs.team_id, None);
    assert_eq!(adr(conn, org_adrs.id, user), "COLLIERY-A-0001");

    // --- Given the team "skadi" has an ADR board, When an admin creates a
    // --- second ADR board for "skadi", Then the create is refused, and the
    // --- error names the board that exists.
    let err = create_board(
        conn,
        BoardLevel::Adr,
        "Skadi ADRs 2",
        "skadi-adrs-2",
        CodePrefix::Given("SKADI"),
        Some(skadi),
        Some(user),
    )
    .expect_err("a second ADR board");
    match &err {
        BoardError::TeamHasAdrBoard {
            team_id,
            board_id,
            board_name,
            board_slug,
        } => {
            assert_eq!(*team_id, skadi);
            assert_eq!(*board_id, created.id);
            assert_eq!(board_name, "Skadi ADRs");
            assert_eq!(board_slug, "skadi-adrs");
        }
        other => panic!("expected TeamHasAdrBoard, got {other}"),
    }
    assert_eq!(
        err.to_string(),
        "The team has the ADR board \"skadi-adrs\". A team has only one ADR board. Use that \
         board."
    );
    assert_eq!(live_adr_boards(conn, skadi), 1);

    // --- with no prefix from the caller, the board takes the prefix of
    // --- the team
    let crt = team_with_board(conn, "crt", "CRT", user);
    let crt_adrs = create_board(
        conn,
        BoardLevel::Adr,
        "crt ADRs",
        "crt-adrs",
        CodePrefix::FromSlug,
        Some(crt),
        Some(user),
    )
    .expect("the ADR board of crt");
    assert_eq!(crt_adrs.code_prefix, "CRT");

    // --- a strategy or initiative board still has no team
    for level in [BoardLevel::Strategy, BoardLevel::Initiative] {
        let err = create_board(
            conn,
            level,
            "Team board",
            "team-board",
            CodePrefix::Given("TEAMB"),
            Some(crt),
            Some(user),
        )
        .expect_err("a team for a board of the organization");
        assert!(
            matches!(err, BoardError::OrganizationBoardHasNoTeam(l) if l == level),
            "{err}"
        );
    }

    // --- the delete of a team removes its delivery board and its ADR board
    let doomed: Vec<String> = boards_for_team_delete(conn, crt)
        .expect("the boards of the team")
        .into_iter()
        .map(|board| board.slug)
        .collect();
    assert_eq!(doomed, vec!["crt".to_string(), "crt-adrs".to_string()]);

    // --- two creates at the same time give one ADR board
    for round in 0..6 {
        let slug = format!("race{round}");
        let race = team_with_board(&mut scratch.conn, &slug, &format!("RACE{round}"), user);
        let barrier = Arc::new(Barrier::new(2));
        let writers: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|writer| {
                let mut conn = scratch.connect();
                let barrier = barrier.clone();
                let slug = slug.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    create_board(
                        &mut conn,
                        BoardLevel::Adr,
                        &format!("{slug} {writer}"),
                        &format!("{slug}-adrs-{writer}"),
                        CodePrefix::FromSlug,
                        Some(race),
                        Some(user),
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
            .filter(|r| matches!(r, Err(BoardError::TeamHasAdrBoard { .. })))
            .count();
        assert_eq!((created, refused), (1, 1), "round {round}: {results:?}");
        assert_eq!(live_adr_boards(&mut scratch.conn, race), 1);
    }

    scratch.drop_database();
}

// Scenario: The ADRs of colliery-io keep their codes
#[test]
fn the_migration_gives_adrs_to_colliery_io_and_keeps_each_code() {
    let mut scratch = Scratch::new("kairos_team_adr_boards_migration_t3102");
    let user = scratch.user;
    let conn = &mut scratch.conn;

    // Given the tenant as the live deployment has it: the shared ADR board
    // `adrs` and the delivery board of colliery-io have the prefix
    // COLLIERY, and other teams have prefixes of their own.
    let colliery_io = team_with_board(conn, "colliery-io-delivery", "COLLIERY", user);
    let skadi = team_with_board(conn, "skadi", "SKADI", user);
    let adrs = board(conn, "adrs");
    assert_eq!(adrs.team_id, None);
    let codes: Vec<String> = (0..3).map(|_| adr(conn, adrs.id, user)).collect();
    assert_eq!(
        codes,
        vec!["COLLIERY-A-0001", "COLLIERY-A-0002", "COLLIERY-A-0003"]
    );
    // An ADR board of the organization with a prefix that no team has.
    let other = create_board(
        conn,
        BoardLevel::Adr,
        "Platform decisions",
        "platform-decisions",
        CodePrefix::Given("PLATDEC"),
        None,
        Some(user),
    )
    .expect("an ADR board of the organization");

    // When the migration runs.
    conn.batch_execute(UP).expect("the up migration");

    // Then `adrs` is the ADR board of colliery-io, and each code is the same.
    let adrs = board(conn, "adrs");
    assert_eq!(adrs.team_id, Some(colliery_io));
    assert_eq!(adrs.code_prefix, "COLLIERY");
    let after: Vec<String> = {
        use kairos_db::schema::adrs::dsl;
        dsl::adrs
            .filter(dsl::board_id.eq(adrs.id))
            .order(dsl::short_code.asc())
            .select(dsl::short_code)
            .load(conn)
            .expect("reading the codes")
    };
    assert_eq!(after, codes);
    assert_eq!(adr(conn, adrs.id, user), "COLLIERY-A-0004");
    // A board with a prefix that names no team keeps no team.
    assert_eq!(board(conn, "platform-decisions").team_id, None);
    assert_eq!(other.team_id, None);
    assert_eq!(live_adr_boards(conn, skadi), 0);

    // The migration can run 2 times: nothing changes.
    conn.batch_execute(UP)
        .expect("the second run of the up migration");
    assert_eq!(board(conn, "adrs").team_id, Some(colliery_io));
    assert_eq!(live_adr_boards(conn, colliery_io), 1);
    assert_eq!(board(conn, "platform-decisions").team_id, None);

    // A team that has an ADR board gets no second one. Only old data can
    // hold this: a team ADR board with a different prefix, written
    // directly, and an ADR board of the organization with the prefix of
    // the team.
    {
        use kairos_db::models::boards::NewBoard;
        use kairos_db::schema::boards;
        for (slug, team_id, prefix) in [
            ("skadi-old-adrs", Some(skadi), "SKADIOLD"),
            ("skadi-shared-adrs", None, "SKADI"),
        ] {
            diesel::insert_into(boards::table)
                .values(NewBoard {
                    name: slug.into(),
                    slug: slug.into(),
                    board_level: BoardLevel::Adr,
                    team_id,
                    code_prefix: prefix.into(),
                })
                .execute(conn)
                .expect("inserting an old board");
        }
    }
    conn.batch_execute(UP)
        .expect("the third run of the up migration");
    assert_eq!(board(conn, "skadi-shared-adrs").team_id, None);
    assert_eq!(live_adr_boards(conn, skadi), 1);

    scratch.drop_database();
}

/// A second user with no grant.
fn user(conn: &mut PgConnection, name: &str) -> Uuid {
    use kairos_db::schema::users;
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id: format!("dex|{name}"),
            user_name: format!("dex|{name}"),
            email: format!("{name}@colliery.test"),
            display_name: name.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("inserting user")
        .id
}

fn join(conn: &mut PgConnection, team: Uuid, user: Uuid) {
    use kairos_db::schema::team_members;
    diesel::insert_into(team_members::table)
        .values(NewTeamMember {
            team_id: team,
            user_id: user,
        })
        .execute(conn)
        .expect("joining the team");
}

// COLLIERY-T-3108: a member of a team writes ADRs on the ADR board of the
// team with no grant. The team rule is computed when it is read
// (`check_capability`); nothing is stored.
#[test]
fn the_team_rule_gives_manage_adrs_on_the_adr_board_of_the_team() {
    use kairos_core::abac::{
        CONFIGURE_BOARDS, MANAGE_ADRS, TEAM_ADR_BOARD_IMPLIED_CAPABILITIES,
        TEAM_IMPLIED_CAPABILITIES,
    };
    use kairos_db::abac::check_capability;

    let mut scratch = Scratch::new("kairos_team_adr_writes_t3108_db_test");
    let admin = scratch.user;
    let conn = &mut scratch.conn;
    let skadi = team_with_board(conn, "skadi", "SKADI", admin);
    let crt = team_with_board(conn, "crt", "CRT", admin);
    let skadi_adrs = create_board(
        conn,
        BoardLevel::Adr,
        "Skadi ADRs",
        "skadi-adrs",
        CodePrefix::Given("SKADI"),
        Some(skadi),
        Some(admin),
    )
    .expect("the ADR board of skadi")
    .id;
    let skadi_delivery = board(conn, "skadi").id;
    let org_adrs = board(conn, "adrs");
    assert_eq!(org_adrs.team_id, None, "the ADR board of the organization");

    let alice = user(conn, "alice");
    let bob = user(conn, "bob");
    join(conn, skadi, alice);
    join(conn, crt, bob);
    let mut can = |board, user, cap: &str| check_capability(conn, board, user, cap).expect("check");

    // AC1: the ADR board of the team gives the member manage_adrs, and the
    // delivery set, with no grant.
    for cap in TEAM_ADR_BOARD_IMPLIED_CAPABILITIES {
        assert!(can(skadi_adrs, alice, cap), "{cap:?} on skadi-adrs");
    }
    assert!(!can(skadi_adrs, alice, CONFIGURE_BOARDS));
    // The delivery board of the team does not give manage_adrs.
    for cap in TEAM_IMPLIED_CAPABILITIES {
        assert!(can(skadi_delivery, alice, cap), "{cap:?} on skadi");
    }
    assert!(!can(skadi_delivery, alice, MANAGE_ADRS));

    // AC2: a member of a different team gets nothing on it.
    for cap in TEAM_ADR_BOARD_IMPLIED_CAPABILITIES {
        assert!(!can(skadi_adrs, bob, cap), "bob, {cap:?} on skadi-adrs");
    }

    // AC3: the ADR board of the organization has no team. manage_adrs there
    // needs a grant.
    assert!(!can(org_adrs.id, alice, MANAGE_ADRS));
    assert!(!can(org_adrs.id, bob, MANAGE_ADRS));

    // Leave the team: the capability goes with it. Nothing was stored.
    {
        use kairos_db::schema::team_members::dsl;
        diesel::delete(dsl::team_members.filter(dsl::user_id.eq(alice)))
            .execute(conn)
            .expect("leaving the team");
    }
    assert!(!check_capability(conn, skadi_adrs, alice, MANAGE_ADRS).expect("check"));

    scratch.drop_database();
}
