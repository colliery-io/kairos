//! COLLIERY-T-0230 — a board always has a team, over REST against the booted
//! production router.
//!
//! The rule has two forms:
//!
//! - A DELIVERY board has a delivery team (`team_id`). `POST /api/boards`
//!   refuses a delivery board with no team, and no route can clear the team
//!   of a live delivery board.
//! - A board of the ORGANIZATION (strategy, initiative, adr) has no row in
//!   `teams`. Its team is the list of the members of the board. A person
//!   joins that team when the person is added to the board.
//!
//! REST is the only surface that creates or updates a board: the MCP server
//! and the CLI have no tool or command for it. Team creation (REST and SCIM)
//! always sends the new team.
//!
//! The db-layer half is `kairos-db/tests/board_team.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_board_team_t0230_server_test`.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    spawn_server, user_token, with_database,
};
use kairos_client::Error;
use kairos_client::types_org::{
    AddBoardMemberRequest, CreateBoardRequest, CreateTeamRequest, UpdateBoardRequest,
};
use kairos_db::models::teams::NewTeam;
use kairos_db::models::{NewOrganizationMember, OrgRole, TeamType};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_board_team_t0230_server_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The `team_id` column of a board, read from the database.
fn team_of_board(conn: &mut PgConnection, board: &str) -> Option<Uuid> {
    use kairos_db::schema::boards::dsl;
    let id: Uuid = board.parse().expect("uuid");
    dsl::boards
        .filter(dsl::id.eq(id))
        .select(dsl::team_id)
        .first(conn)
        .expect("board exists")
}

#[tokio::test]
async fn a_board_always_has_a_team_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let alice_token = user_token(&http, "alice").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let server = spawn_server(router.clone()).await;
    let svc = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");

    // JIT-provision both, then make svc the org admin and alice a member.
    let _ = svc.whoami().await;
    let _ = alice.whoami().await;
    let user_id = |conn: &mut PgConnection, email: &str| -> Uuid {
        users::table
            .filter(users::email.eq(email))
            .select(users::id)
            .first(conn)
            .unwrap_or_else(|e| panic!("{email} provisioned: {e}"))
    };
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    for (user, role) in [(svc_id, OrgRole::Admin), (alice_id, OrgRole::Member)] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: user,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    // =======================================================================
    // a, b: the create of a delivery board with no team is refused, and the
    // refusal names the missing team
    // =======================================================================
    let err = svc
        .create_board(&CreateBoardRequest {
            name: "Nobody's".into(),
            slug: "nobodys-delivery".into(),
            board_level: "delivery".into(),
            team_id: None,
        })
        .await
        .expect_err("a delivery board with no team");
    match &err {
        Error::Validation {
            status, message, ..
        } => {
            assert_eq!(*status, 422);
            assert_eq!(
                message,
                "A delivery board needs a team. Send the team as team_id."
            );
        }
        other => panic!("expected 422 VALIDATION, got {other}"),
    }

    // An explicit null is the same request.
    let (status, body) = request(
        &router,
        Method::POST,
        "/api/boards",
        Some(&svc_token),
        &TENANT,
        Some(json!({
            "name": "Nobody's",
            "slug": "nobodys-delivery",
            "board_level": "delivery",
            "team_id": null,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "VALIDATION");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("team_id")),
        "the refusal names the missing team: {body}"
    );

    // A refused create leaves nothing behind.
    let boards = svc
        .list_boards(Default::default())
        .await
        .expect("listing boards");
    assert!(
        boards.items.iter().all(|b| b.slug != "nobodys-delivery"),
        "a refused create leaves no board"
    );
    assert!(
        boards
            .items
            .iter()
            .filter(|b| b.board_level == "delivery")
            .all(|b| b.team_id.is_some()),
        "every delivery board has a team"
    );

    // With a team, the same create succeeds. The team is inserted directly
    // so that it has no delivery board yet: POST /api/teams would have made
    // one, and a team has exactly one.
    let bare_team: Uuid = {
        use kairos_db::schema::teams;
        diesel::insert_into(teams::table)
            .values(NewTeam {
                name: "Data".into(),
                slug: "data".into(),
                team_type: TeamType::StreamAligned,
            })
            .returning(teams::id)
            .get_result(&mut conn)
            .expect("inserting team")
    };
    let data_board = svc
        .create_board(&CreateBoardRequest {
            name: "Data Delivery".into(),
            slug: "data-delivery".into(),
            board_level: "delivery".into(),
            team_id: Some(bare_team.to_string()),
        })
        .await
        .expect("a delivery board with a team");
    assert_eq!(
        data_board.board.team_id.as_deref(),
        Some(bare_team.to_string().as_str())
    );

    // Team creation gives the delivery board its team.
    let platform = svc
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            team_type: None,
        })
        .await
        .expect("platform");
    let platform_board = platform.delivery_board_id.clone().expect("delivery board");
    let platform_id: Uuid = platform.id.parse().expect("uuid");
    assert_eq!(team_of_board(&mut conn, &platform_board), Some(platform_id));

    // =======================================================================
    // c: a live delivery board cannot lose its team
    // =======================================================================
    // PATCH /api/boards/{id} is the only update route of a board. Its body
    // has `name` and `slug` and no `team_id`, so a request cannot address
    // the team at all.
    let uri = format!("/api/boards/{platform_board}");
    let (status, body) = request(
        &router,
        Method::PATCH,
        &uri,
        Some(&svc_token),
        &TENANT,
        Some(json!({ "team_id": null })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "VALIDATION");
    assert_eq!(team_of_board(&mut conn, &platform_board), Some(platform_id));

    // With a valid field next to it, the update runs and the team stays.
    let (status, body) = request(
        &router,
        Method::PATCH,
        &uri,
        Some(&svc_token),
        &TENANT,
        Some(json!({ "name": "Platform Work", "team_id": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Platform Work");
    assert_eq!(
        body["team_id"],
        platform.id.as_str(),
        "the update does not clear the team: {body}"
    );
    assert_eq!(team_of_board(&mut conn, &platform_board), Some(platform_id));
    // Nor can it move the board to a different team.
    let (status, body) = request(
        &router,
        Method::PATCH,
        &uri,
        Some(&svc_token),
        &TENANT,
        Some(json!({ "name": "Platform Delivery", "team_id": bare_team.to_string() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(team_of_board(&mut conn, &platform_board), Some(platform_id));
    let typed = svc
        .update_board(
            &platform_board,
            &UpdateBoardRequest {
                name: None,
                slug: Some("platform-work".into()),
            },
        )
        .await
        .expect("the typed update");
    assert_eq!(typed.team_id.as_deref(), Some(platform.id.as_str()));

    // =======================================================================
    // d, e: each board of the organization has a group of persons that a
    // reader can see, and a person joins it when added to the board
    // =======================================================================
    let boards = svc
        .list_boards(Default::default())
        .await
        .expect("listing boards");
    let organization: Vec<_> = boards
        .items
        .iter()
        .filter(|b| b.board_level != "delivery")
        .collect();
    let mut levels: Vec<&str> = organization
        .iter()
        .map(|b| b.board_level.as_str())
        .collect();
    levels.sort_unstable();
    assert_eq!(levels, ["adr", "initiative", "strategy"]);
    for board in &organization {
        assert_eq!(
            board.team_id, None,
            "a board of the organization has no row in teams"
        );
        // Any member of the tenant can read the list, not only an admin.
        let team = alice
            .list_board_members(&board.id)
            .await
            .unwrap_or_else(|e| panic!("the members of {}: {e}", board.slug));
        assert!(team.is_empty(), "nobody is on {} yet", board.slug);
    }

    let strategy = organization
        .iter()
        .find(|b| b.board_level == "strategy")
        .expect("the strategy board");
    svc.add_board_member(
        &strategy.id,
        &AddBoardMemberRequest {
            user_id: alice_id.to_string(),
            capabilities: vec!["manage_strategies".into()],
        },
    )
    .await
    .expect("adding alice to the strategy board");
    let team = alice
        .list_board_members(&strategy.id)
        .await
        .expect("the members of the strategy board");
    let emails: Vec<&str> = team.iter().map(|m| m.email.as_str()).collect();
    assert_eq!(emails, ["alice@kairos.test"]);
    // Admission is to one board. The other boards of the organization do
    // not gain her.
    for board in organization.iter().filter(|b| b.id != strategy.id) {
        let team = alice
            .list_board_members(&board.id)
            .await
            .expect("the members of another board");
        assert!(team.is_empty(), "alice is not on {}", board.slug);
    }
    // She leaves the group when she is removed from the board.
    svc.remove_board_member(&strategy.id, &alice_id.to_string())
        .await
        .expect("removing alice");
    assert!(
        alice
            .list_board_members(&strategy.id)
            .await
            .expect("the members of the strategy board")
            .is_empty()
    );

    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}

/// One section of a page: from its `## ` heading to the next one.
fn section<'a>(page: &'a str, heading: &str) -> &'a str {
    page.split(&format!("\n## {heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("no section {heading:?}"))
        .split("\n## ")
        .next()
        .expect("the section body")
}

/// The same text with each run of white space as one space, so that a line
/// break in the page does not hide a sentence.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// h: the glossary and the explanation page "Teams and boards" give the
/// rule, in both forms. A page that nothing reads goes stale, so the
/// sentences that carry the rule are read here.
#[test]
fn the_book_gives_the_rule_in_both_forms() {
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/src");
    let read = |rel: &str| {
        std::fs::read_to_string(docs.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    };

    let glossary = read("reference/glossary.md");
    let board = one_line(section(&glossary, "board"));
    for needed in [
        "Every board has a team.",
        "Kairos refuses to create a delivery board with no team.",
        "A live delivery board cannot lose its team.",
        "Its team is the list of the members of the board.",
    ] {
        assert!(board.contains(needed), "glossary, board: {needed:?}");
    }
    let delivery_board = one_line(section(&glossary, "delivery board"));
    assert!(
        delivery_board.contains("A delivery board always has a team"),
        "glossary, delivery board: {delivery_board}"
    );
    let team = one_line(section(&glossary, "team"));
    assert!(
        team.contains(
            "The team of a board of the organization is the list of the members of that board."
        ),
        "glossary, team: {team}"
    );

    let explanation = read("explanation/teams-and-boards.md");
    let rule = one_line(section(&explanation, "Every board has a team"));
    for needed in [
        "A board cannot have no team.",
        "Kairos refuses to create a delivery board with no team",
        "A live delivery board cannot lose its team",
        "Its team is the list of the members of the board.",
        "Admission is about being added to the board",
    ] {
        assert!(rule.contains(needed), "teams-and-boards: {needed:?}");
    }
    for page in [&glossary, &explanation] {
        assert!(!page.contains("not owned by a team at all"));
    }
}
