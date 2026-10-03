//! COLLIERY-T-0230 — a delivery board always has a team, in the database
//! layer: `create_board` refuses a delivery board with no team, and leaves
//! the boards of the organization (strategy, initiative, adr) as they are.
//!
//! `create_board` is the one function that REST, team creation, SCIM, the
//! demo seed and tenant provisioning all call. The rule is here so that no
//! entry point can forget it.
//!
//! Against real Postgres from the compose stack (A-0012 tier 2), in its own
//! scratch database like the other kairos-db integration tests.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use uuid::Uuid;

use kairos_db::boards::BoardError;
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewUser, TeamType, User};
use kairos_db::{create_board, provision_tenant, run_public_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";
const SCRATCH_DB: &str = "kairos_board_team_t0230_test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
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

#[test]
fn a_delivery_board_always_has_a_team() {
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
    // Provisioning creates the three boards of the organization with no
    // team. It must still succeed: the rule is about delivery boards only.
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

    // --- a delivery board with no team is refused ----------------------------
    let err = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Nobody's",
        "nobodys-delivery",
        kairos_db::CodePrefix::FromSlug,
        None,
        Some(alice),
    )
    .expect_err("a delivery board with no team");
    assert!(matches!(err, BoardError::DeliveryBoardNeedsTeam), "{err}");
    assert!(
        err.to_string().contains("team"),
        "the refusal names the missing team: {err}"
    );
    assert_eq!(
        boards_with_slug(&mut conn, "nobodys-delivery"),
        0,
        "a refused create leaves no board"
    );

    // --- a delivery board with a team is created -----------------------------
    let team_id: Uuid = {
        use kairos_db::schema::teams;
        diesel::insert_into(teams::table)
            .values(NewTeam {
                name: "Platform".into(),
                slug: "platform".into(),
                team_type: TeamType::StreamAligned,
            })
            .returning(teams::id)
            .get_result(&mut conn)
            .expect("inserting team")
    };
    let delivery = create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        kairos_db::CodePrefix::FromSlug,
        Some(team_id),
        Some(alice),
    )
    .expect("a delivery board with a team");
    assert_eq!(delivery.team_id, Some(team_id));

    // --- the boards of the organization have no row in `teams` ---------------
    // Their team is the list of the members of the board. The data model
    // does not change for them.
    for (level, slug) in [
        (BoardLevel::Strategy, "second-strategy"),
        (BoardLevel::Initiative, "second-initiatives"),
        (BoardLevel::Adr, "second-adrs"),
    ] {
        let board = create_board(
            &mut conn,
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

    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch database");
}
