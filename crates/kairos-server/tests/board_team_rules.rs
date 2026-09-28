//! COLLIERY-T-0240, T-0241, T-0242, T-0243 — the rules of the team of a
//! board, over REST against the booted production router:
//!
//! - T-0240: `POST /api/boards` does not give a team a second delivery
//!   board (422 `TEAM_HAS_DELIVERY_BOARD`).
//! - T-0241: `DELETE /api/boards/{id}` does not delete the only delivery
//!   board of a team (422 `LAST_DELIVERY_BOARD`). `DELETE /api/teams/{id}`
//!   removes the team and its board together, as before.
//! - T-0242: `POST /api/boards` does not give a team to a board of the
//!   organization (422 `VALIDATION`).
//! - T-0243: `PATCH /api/boards/{id}` does not change the team of a board
//!   (422 `BOARD_TEAM_IS_FIXED`).
//!
//! REST is the only surface that creates, updates or deletes a board alone:
//! the MCP server and the CLI have no tool or command for it.
//!
//! The db-layer half is `kairos-db/tests/board_team_rules.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, TestServer, base_config, drop_scratch_db, error_code, recreate_scratch_db,
    request, seed_team, spawn_server, user_token, with_database,
};
use kairos_client::types_org::{CreateTeamRequest, Team, UpdateBoardRequest};
use kairos_client::{Error, KairosClient};
use kairos_db::models::boards::NewBoard;
use kairos_db::models::enums::BoardLevel;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The booted router on a scratch database, with `svc` as the org admin.
struct Stack {
    scratch_db: &'static str,
    admin_conn: PgConnection,
    /// Pinned to the tenant schema.
    conn: PgConnection,
    pool: TenantPool,
    router: Router,
    server: TestServer,
    token: String,
    svc: KairosClient,
}

impl Stack {
    async fn boot(scratch_db: &'static str) -> Self {
        let admin_conn = recreate_scratch_db(scratch_db);
        let scratch_url = with_database(&common::admin_database_url(), scratch_db);
        let mut conn =
            PgConnection::establish(&scratch_url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("acme org row");

        let http = reqwest::Client::new();
        let token = user_token(&http, "svc").await;
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(
            Authenticator::discover(ISSUER, AUDIENCE)
                .await
                .expect("OIDC discovery against live Dex"),
        );
        let router = app::router(app::state_with(
            base_config(&scratch_url),
            pool.clone(),
            auth,
        ));
        let server = spawn_server(router.clone()).await;
        let svc = server.client(&token, "acme");

        // JIT-provision svc, then make it the org admin.
        let _ = svc.whoami().await;
        let svc_id: Uuid = users::table
            .filter(users::email.eq("svc@kairos.test"))
            .select(users::id)
            .first(&mut conn)
            .expect("svc provisioned");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: svc_id,
                role: OrgRole::Admin,
            })
            .execute(&mut conn)
            .expect("granting membership");
        diesel::sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");

        Stack {
            scratch_db,
            admin_conn,
            conn,
            pool,
            router,
            server,
            token,
            svc,
        }
    }

    /// One request as the org admin.
    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        request(&self.router, method, uri, Some(&self.token), &TENANT, body).await
    }

    /// `POST /api/teams`: a team with its delivery board.
    async fn team(&self, name: &str, slug: &str) -> Team {
        self.svc
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                team_type: None,
            })
            .await
            .unwrap_or_else(|e| panic!("team {slug}: {e}"))
    }

    /// The id of the first live board of a level.
    async fn board_of_level(&self, level: &str) -> String {
        self.svc
            .list_boards(Default::default())
            .await
            .expect("listing boards")
            .items
            .into_iter()
            .find(|b| b.board_level == level)
            .unwrap_or_else(|| panic!("a {level} board"))
            .id
    }

    fn shutdown(mut self) {
        drop(self.server);
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

/// `(name, team_id, live)` of a board, read from the database.
fn board_row(conn: &mut PgConnection, board: &str) -> (String, Option<Uuid>, bool) {
    use kairos_db::schema::boards::dsl;
    let id: Uuid = board.parse().expect("uuid");
    let (name, team, deleted): (String, Option<Uuid>, Option<chrono::DateTime<chrono::Utc>>) =
        dsl::boards
            .filter(dsl::id.eq(id))
            .select((dsl::name, dsl::team_id, dsl::deleted_at))
            .first(conn)
            .expect("board exists");
    (name, team, deleted.is_none())
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

/// A delivery board row written with no rule, as old data has it.
fn old_delivery_board(conn: &mut PgConnection, slug: &str, team: Uuid) -> String {
    use kairos_db::schema::boards;
    let id: Uuid = diesel::insert_into(boards::table)
        .values(NewBoard {
            name: slug.into(),
            slug: slug.into(),
            board_level: BoardLevel::Delivery,
            team_id: Some(team),
        })
        .returning(boards::id)
        .get_result(conn)
        .expect("inserting board");
    id.to_string()
}

fn soft_delete_board(conn: &mut PgConnection, board: &str) {
    use kairos_db::schema::boards::dsl;
    let id: Uuid = board.parse().expect("uuid");
    diesel::update(dsl::boards.filter(dsl::id.eq(id)))
        .set(dsl::deleted_at.eq(diesel::dsl::now))
        .execute(conn)
        .expect("deleting board");
}

fn message(body: &Value) -> &str {
    body["error"]["message"].as_str().unwrap_or_default()
}

fn delivery_board(name: &str, slug: &str, team: impl ToString) -> Value {
    json!({
        "name": name,
        "slug": slug,
        "board_level": "delivery",
        "team_id": team.to_string(),
    })
}

/// COLLIERY-T-0240.
#[tokio::test]
async fn a_team_has_one_delivery_board_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_rules_t0240_server_test").await;

    // Team creation gives the team its delivery board, as before.
    let platform = stack.team("Platform", "platform").await;
    let platform_board = platform.delivery_board_id.clone().expect("delivery board");
    let platform_id: Uuid = platform.id.parse().expect("uuid");

    // --- the second delivery board is refused, and the refusal names the
    // --- board that the team has
    let (status, body) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(delivery_board("Platform Two", "platform-two", platform_id)),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "TEAM_HAS_DELIVERY_BOARD");
    assert_eq!(
        message(&body),
        "The team has the delivery board \"Platform Delivery\". A team has only one delivery \
         board. Use that board."
    );
    let details = &body["error"]["details"];
    assert_eq!(details["team_id"], platform.id.as_str(), "{body}");
    assert_eq!(details["board"]["id"], platform_board.as_str(), "{body}");
    assert_eq!(details["board"]["slug"], "platform-delivery", "{body}");
    assert_eq!(boards_with_slug(&mut stack.conn, "platform-two"), 0);
    assert_eq!(live_delivery_boards(&mut stack.conn, platform_id), 1);

    // --- a team with no delivery board gets one
    let data = seed_team(&mut stack.conn, "Data", "data");
    let (status, body) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(delivery_board("Data Delivery", "data-delivery", data)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let data_board = body["id"].as_str().expect("board id").to_string();

    // --- a deleted board does not count
    soft_delete_board(&mut stack.conn, &data_board);
    let (status, body) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(delivery_board("Data Work", "data-work", data)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(live_delivery_boards(&mut stack.conn, data), 1);

    // --- two creates at the same time give one board
    for round in 0..4 {
        let team = seed_team(
            &mut stack.conn,
            &format!("Race {round}"),
            &format!("race-{round}"),
        );
        let (a, b) = tokio::join!(
            stack.send(
                Method::POST,
                "/api/boards",
                Some(delivery_board("Race A", &format!("race-{round}-a"), team)),
            ),
            stack.send(
                Method::POST,
                "/api/boards",
                Some(delivery_board("Race B", &format!("race-{round}-b"), team)),
            ),
        );
        let mut statuses = [a.0, b.0];
        statuses.sort();
        assert_eq!(
            statuses,
            [StatusCode::CREATED, StatusCode::UNPROCESSABLE_ENTITY],
            "round {round}: {} / {}",
            a.1,
            b.1
        );
        assert_eq!(live_delivery_boards(&mut stack.conn, team), 1);
    }

    stack.shutdown();
}

/// COLLIERY-T-0241.
#[tokio::test]
async fn the_only_delivery_board_of_a_team_is_not_deleted_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_rules_t0241_server_test").await;

    let platform = stack.team("Platform", "platform").await;
    let platform_board = platform.delivery_board_id.clone().expect("delivery board");
    let platform_id: Uuid = platform.id.parse().expect("uuid");

    // --- the delete of the only delivery board is refused, and the refusal
    // --- tells the caller what to do
    let uri = format!("/api/boards/{platform_board}");
    let (status, body) = stack.send(Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "LAST_DELIVERY_BOARD");
    assert_eq!(
        message(&body),
        "This board is the only delivery board of the team \"Platform\". A team always has a \
         delivery board. To remove the board, delete the team."
    );
    let details = &body["error"]["details"];
    assert_eq!(details["board_id"], platform_board.as_str(), "{body}");
    assert_eq!(details["team"]["id"], platform.id.as_str(), "{body}");
    assert!(board_row(&mut stack.conn, &platform_board).2, "still live");
    assert_eq!(live_delivery_boards(&mut stack.conn, platform_id), 1);
    // The typed client gives the same text.
    let err = stack
        .svc
        .delete_board(&platform_board)
        .await
        .expect_err("the only delivery board");
    assert!(err.to_string().contains("delete the team"), "{err}");

    // --- old data: a team with two delivery boards can lose one. The rule
    // --- then applies to the board that stays.
    let extra = old_delivery_board(&mut stack.conn, "platform-extra", platform_id);
    let (status, body) = stack
        .send(Method::DELETE, &format!("/api/boards/{extra}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = stack.send(Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "LAST_DELIVERY_BOARD");

    // --- a board of the organization is deleted as before
    let (status, body) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(json!({
                "name": "Second Strategy",
                "slug": "second-strategy",
                "board_level": "strategy",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let second = body["id"].as_str().expect("board id").to_string();
    let (status, body) = stack
        .send(Method::DELETE, &format!("/api/boards/{second}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // --- the delete of the team removes the team and its board together
    let deleted = stack
        .svc
        .delete_team(&platform.id)
        .await
        .expect("the delete of the team");
    assert!(deleted.deleted);
    assert!(!board_row(&mut stack.conn, &platform_board).2, "deleted");
    assert_eq!(live_delivery_boards(&mut stack.conn, platform_id), 0);

    stack.shutdown();
}

/// COLLIERY-T-0242.
#[tokio::test]
async fn a_board_of_the_organization_has_no_team_id_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_rules_t0242_server_test").await;

    let data = seed_team(&mut stack.conn, "Data", "data");
    for level in ["strategy", "initiative", "adr"] {
        let slug = format!("team-{level}");
        let (status, body) = stack
            .send(
                Method::POST,
                "/api/boards",
                Some(json!({
                    "name": "With a team",
                    "slug": slug,
                    "board_level": level,
                    "team_id": data.to_string(),
                })),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{level}: {body}");
        assert_eq!(error_code(&body), "VALIDATION", "{level}");
        assert_eq!(
            message(&body),
            format!(
                "Only a delivery board has a team. Do not send team_id for a board of level \
                 {level}."
            )
        );
        assert_eq!(body["error"]["details"]["field"], "team_id", "{body}");
        assert_eq!(boards_with_slug(&mut stack.conn, &slug), 0, "{level}");

        // With no team, and with a null, the same create succeeds.
        let (status, body) = stack
            .send(
                Method::POST,
                "/api/boards",
                Some(json!({
                    "name": "With no team",
                    "slug": slug,
                    "board_level": level,
                    "team_id": null,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{level}: {body}");
        assert_eq!(body["team_id"], Value::Null, "{level}: {body}");
    }

    stack.shutdown();
}

/// COLLIERY-T-0243.
#[tokio::test]
async fn the_team_of_a_board_does_not_change_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_rules_t0243_server_test").await;

    let platform = stack.team("Platform", "platform").await;
    let platform_board = platform.delivery_board_id.clone().expect("delivery board");
    let platform_id: Uuid = platform.id.parse().expect("uuid");
    let data = stack.team("Data", "data").await;
    let uri = format!("/api/boards/{platform_board}");
    let unchanged = ("Platform Delivery".to_string(), Some(platform_id), true);

    // --- a different team is refused, and the update writes nothing
    for body in [
        json!({ "name": "Moved", "team_id": data.id }),
        json!({ "name": "Moved", "team_id": null }),
        json!({ "team_id": data.id }),
    ] {
        let (status, answer) = stack.send(Method::PATCH, &uri, Some(body.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {answer}");
        assert_eq!(error_code(&answer), "BOARD_TEAM_IS_FIXED", "{body}");
        assert_eq!(
            message(&answer),
            "The team of a board does not change. To give the work to a different team, move \
             the task to the board of that team."
        );
        assert_eq!(
            answer["error"]["details"]["team_id"],
            platform.id.as_str(),
            "the refusal gives the team that the board has: {answer}"
        );
        assert_eq!(board_row(&mut stack.conn, &platform_board), unchanged);
    }

    // --- the team that the board has is accepted, and changes nothing. A
    // --- client can send back the board that it read.
    let mut board = stack.send(Method::GET, &uri, None).await.1;
    board["name"] = json!("Platform Work");
    let (status, answer) = stack.send(Method::PATCH, &uri, Some(board)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["name"], "Platform Work");
    assert_eq!(answer["team_id"], platform.id.as_str());
    assert_eq!(
        board_row(&mut stack.conn, &platform_board),
        ("Platform Work".to_string(), Some(platform_id), true)
    );
    // It is not a field to update.
    let (status, answer) = stack
        .send(Method::PATCH, &uri, Some(json!({ "team_id": platform.id })))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(error_code(&answer), "VALIDATION");
    // A value that is not a UUID is a bad request body.
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &uri,
            Some(json!({ "name": "Moved", "team_id": "platform" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(error_code(&answer), "VALIDATION");

    // --- the typed client: no team is no `team_id` in the body
    let request = UpdateBoardRequest {
        name: Some("Platform Delivery".into()),
        slug: None,
        team_id: None,
    };
    assert_eq!(
        serde_json::to_value(&request).expect("json").get("team_id"),
        None
    );
    let typed = stack
        .svc
        .update_board(&platform_board, &request)
        .await
        .expect("the typed update");
    assert_eq!(typed.team_id.as_deref(), Some(platform.id.as_str()));
    let err = stack
        .svc
        .update_board(
            &platform_board,
            &UpdateBoardRequest {
                name: Some("Moved".into()),
                slug: None,
                team_id: Some(Some(data.id.clone())),
            },
        )
        .await
        .expect_err("a different team");
    match &err {
        Error::Other { status, code, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "BOARD_TEAM_IS_FIXED");
        }
        other => panic!("expected 422 BOARD_TEAM_IS_FIXED, got {other}"),
    }
    assert!(err.to_string().contains("move the task"), "{err}");

    // --- a board of the organization: a null is the team that it has
    let strategy = stack.board_of_level("strategy").await;
    let uri = format!("/api/boards/{strategy}");
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &uri,
            Some(json!({ "name": "Strategy 2027", "team_id": null })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["team_id"], Value::Null);
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &uri,
            Some(json!({ "name": "Moved", "team_id": platform.id })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(error_code(&answer), "BOARD_TEAM_IS_FIXED");
    assert_eq!(
        board_row(&mut stack.conn, &strategy),
        ("Strategy 2027".to_string(), None, true)
    );

    stack.shutdown();
}
