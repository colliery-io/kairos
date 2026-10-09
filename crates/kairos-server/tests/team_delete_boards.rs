//! COLLIERY-T-0250 — the delete of a team with 2 delivery boards, over REST
//! and SCIM against the booted production router, and the operator report
//! as the real binary:
//!
//! - `DELETE /api/teams/{id}` applies the empty rule to EACH live delivery
//!   board of the team (422 `BOARD_NOT_EMPTY`, naming the board), and it
//!   removes each board with the team.
//! - `DELETE /scim/v2/Groups/{id}` does the same (400 `mutability`).
//! - `kairos-server check-delivery-boards` lists the team, and it writes
//!   nothing.
//!
//! The API does not give a team a second delivery board
//! (COLLIERY-T-0240), so each test writes the second board directly, as
//! old data has it.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, TestServer, base_config, drop_scratch_db, error_code, recreate_scratch_db,
    request, spawn_server, user_token, with_database,
};
use kairos_client::KairosClient;
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_org::{CreateTeamRequest, Team};
use kairos_db::models::boards::{NewBoard, NewBoardColumn};
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
    scratch_url: String,
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
            scratch_url,
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
                code_prefix: kairos_core::short_code::prefix_from_slug(slug),
                team_type: None,
            })
            .await
            .unwrap_or_else(|e| panic!("team {slug}: {e}"))
    }

    /// A live task on a board.
    async fn task_on(&self, board: Uuid, title: &str) -> String {
        self.svc
            .create_task(&CreateTaskRequest {
                board_id: Some(board.to_string()),
                column_id: None,
                title: title.into(),
                content: String::new(),
                task_type: None,
                work_class: None,
                team_id: None,
                repository: None,
            })
            .await
            .unwrap_or_else(|e| panic!("task {title}: {e}"))
            .short_code
    }

    /// A SCIM token of the tenant.
    async fn scim_token(&self) -> String {
        let (status, body) = self
            .send(
                Method::POST,
                "/api/scim-tokens",
                Some(json!({"name": "t0250"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["token"].as_str().expect("token").to_string()
    }

    fn shutdown(mut self) {
        drop(self.server);
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

/// One SCIM request: the bearer only, and the SCIM media type.
async fn scim(
    router: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/scim+json")
            .body(Body::from(json.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("non-JSON body: {e}"))
    };
    (status, body)
}

/// A delivery board row with one column, written with no rule, as old
/// data has it.
fn old_delivery_board(conn: &mut PgConnection, name: &str, slug: &str, team: Uuid) -> Uuid {
    use kairos_db::schema::{board_columns, boards};
    let board: Uuid = diesel::insert_into(boards::table)
        .values(NewBoard {
            name: name.into(),
            slug: slug.into(),
            board_level: BoardLevel::Delivery,
            team_id: Some(team),
            // A prefix of its own (COLLIERY-T-3099).
            code_prefix: format!("B{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
                .to_uppercase(),
        })
        .returning(boards::id)
        .get_result(conn)
        .expect("inserting board");
    diesel::insert_into(board_columns::table)
        .values(NewBoardColumn {
            board_id: board,
            name: "Todo".into(),
            position: 0,
            is_done: false,
            claims: false,
        })
        .execute(conn)
        .expect("inserting column");
    board
}

fn board_is_live(conn: &mut PgConnection, board: Uuid) -> bool {
    use kairos_db::schema::boards::dsl;
    let deleted: Option<chrono::DateTime<chrono::Utc>> = dsl::boards
        .filter(dsl::id.eq(board))
        .select(dsl::deleted_at)
        .first(conn)
        .expect("board exists");
    deleted.is_none()
}

fn team_is_live(conn: &mut PgConnection, team: Uuid) -> bool {
    use kairos_db::schema::teams::dsl;
    let deleted: Option<chrono::DateTime<chrono::Utc>> = dsl::teams
        .filter(dsl::id.eq(team))
        .select(dsl::deleted_at)
        .first(conn)
        .expect("team exists");
    deleted.is_none()
}

fn message(body: &Value) -> &str {
    body["error"]["message"].as_str().unwrap_or_default()
}

/// The delete of a team with 2 empty delivery boards removes the two.
#[tokio::test]
async fn team_delete_removes_each_delivery_board_against_live_stack() {
    let mut stack = Stack::boot("kairos_team_delete_t0250_empty_test").await;

    let platform = stack.team("Platform", "platform").await;
    let platform_id: Uuid = platform.id.parse().expect("uuid");
    let first: Uuid = platform
        .delivery_board_id
        .as_deref()
        .expect("delivery board")
        .parse()
        .expect("uuid");
    let second = old_delivery_board(
        &mut stack.conn,
        "Platform Extra",
        "platform-extra",
        platform_id,
    );

    let (status, body) = stack
        .send(Method::DELETE, &format!("/api/teams/{platform_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], true, "{body}");

    assert!(!team_is_live(&mut stack.conn, platform_id), "the team");
    assert!(!board_is_live(&mut stack.conn, first), "the first board");
    assert!(!board_is_live(&mut stack.conn, second), "the second board");

    stack.shutdown();
}

/// A live item on the second board refuses the delete, and nothing is
/// deleted. The refusal names the board and the count.
#[tokio::test]
async fn team_delete_checks_each_delivery_board_against_live_stack() {
    let mut stack = Stack::boot("kairos_team_delete_t0250_item_test").await;

    let platform = stack.team("Platform", "platform").await;
    let platform_id: Uuid = platform.id.parse().expect("uuid");
    let first: Uuid = platform
        .delivery_board_id
        .as_deref()
        .expect("delivery board")
        .parse()
        .expect("uuid");
    let second = old_delivery_board(
        &mut stack.conn,
        "Platform Extra",
        "platform-extra",
        platform_id,
    );
    let task = stack.task_on(second, "Work on the second board").await;

    let uri = format!("/api/teams/{platform_id}");
    let (status, body) = stack.send(Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "BOARD_NOT_EMPTY");
    assert_eq!(
        message(&body),
        format!(
            "The delivery board \"Platform Extra\" of the team \"Platform\" has 1 live card: \
             [{task}]. Move each card to a different board (POST /api/tasks/{{code}}/move) or \
             delete it. Then delete the team."
        )
    );
    let details = &body["error"]["details"];
    assert_eq!(details["board_id"], second.to_string(), "{body}");
    assert_eq!(details["board"]["name"], "Platform Extra", "{body}");
    assert_eq!(details["board"]["slug"], "platform-extra", "{body}");
    assert_eq!(details["item_count"], 1, "{body}");
    assert_eq!(details["items"], json!([task]), "{body}");

    // --- nothing is deleted
    assert!(team_is_live(&mut stack.conn, platform_id), "the team");
    assert!(board_is_live(&mut stack.conn, first), "the first board");
    assert!(board_is_live(&mut stack.conn, second), "the second board");

    // --- with the item archived, the delete removes the team and the two
    // --- boards
    stack.svc.delete_task(&task).await.expect("archiving");
    let (status, body) = stack.send(Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!team_is_live(&mut stack.conn, platform_id), "the team");
    assert!(!board_is_live(&mut stack.conn, first), "the first board");
    assert!(!board_is_live(&mut stack.conn, second), "the second board");

    stack.shutdown();
}

/// SCIM `DELETE /scim/v2/Groups/{id}` has the same rule.
#[tokio::test]
async fn scim_group_delete_handles_each_delivery_board_against_live_stack() {
    let mut stack = Stack::boot("kairos_team_delete_t0250_scim_test").await;
    let token = stack.scim_token().await;

    let (status, body) = scim(
        &stack.router,
        Method::POST,
        "/scim/v2/Groups",
        &token,
        Some(json!({"displayName": "kairos-team-platform"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let group = body["id"].as_str().expect("group id").to_string();
    let platform_id: Uuid = group.parse().expect("the id of a team group is the team");
    let first: Uuid = {
        use kairos_db::schema::boards::dsl;
        dsl::boards
            .filter(dsl::team_id.eq(platform_id))
            .select(dsl::id)
            .first(&mut stack.conn)
            .expect("the delivery board of the team")
    };
    // The first board is empty, and the item is on the second: the check
    // of the first board alone does not see it.
    let second = old_delivery_board(
        &mut stack.conn,
        "Platform Extra",
        "platform-extra",
        platform_id,
    );
    let task = stack.task_on(second, "Work on the second board").await;

    // --- a live item on the second board refuses the delete
    let uri = format!("/scim/v2/Groups/{group}");
    let (status, body) = scim(&stack.router, Method::DELETE, &uri, &token, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["scimType"], "mutability", "{body}");
    assert_eq!(
        body["detail"],
        "The delivery board \"Platform Extra\" of the team \"platform\" has 1 live item. Move \
         or delete each item. Then delete the group.",
        "{body}"
    );
    assert!(team_is_live(&mut stack.conn, platform_id), "the team");
    assert!(board_is_live(&mut stack.conn, first), "the first board");
    assert!(board_is_live(&mut stack.conn, second), "the second board");

    // --- with the item archived, the delete removes the team and the two
    // --- boards
    stack.svc.delete_task(&task).await.expect("archiving");
    let (status, body) = scim(&stack.router, Method::DELETE, &uri, &token, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(!team_is_live(&mut stack.conn, platform_id), "the team");
    assert!(!board_is_live(&mut stack.conn, first), "the first board");
    assert!(!board_is_live(&mut stack.conn, second), "the second board");

    stack.shutdown();
}

#[derive(QueryableByName)]
struct Snapshot {
    #[diesel(sql_type = diesel::sql_types::Text)]
    state: String,
}

/// The rows of the teams, the boards and the migrations, as one string.
fn snapshot(conn: &mut PgConnection) -> String {
    diesel::sql_query(
        "SELECT coalesce((SELECT string_agg(t::text, ';' ORDER BY t.id) FROM teams t), '') \
             || '|' || \
             coalesce((SELECT string_agg(b::text, ';' ORDER BY b.id) FROM boards b), '') \
             || '|' || \
             (SELECT count(*) FROM activity_log)::text AS state",
    )
    .get_result::<Snapshot>(conn)
    .expect("snapshot")
    .state
}

/// `kairos-server check-delivery-boards` lists the team with 2 live
/// delivery boards, and only that team. It writes nothing.
#[tokio::test]
async fn the_report_lists_a_team_with_two_delivery_boards_against_live_stack() {
    let mut stack = Stack::boot("kairos_team_delete_t0250_report_test").await;

    let platform = stack.team("Platform", "platform").await;
    let platform_id: Uuid = platform.id.parse().expect("uuid");
    let web = stack.team("Web", "web").await;
    let second = old_delivery_board(
        &mut stack.conn,
        "Platform Extra",
        "platform-extra",
        platform_id,
    );
    let before = snapshot(&mut stack.conn);

    let run = |url: String| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_kairos-server"))
            .arg("check-delivery-boards")
            .env("DATABASE_URL", url)
            .output()
            .expect("run kairos-server check-delivery-boards");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            output.status.success(),
            "stdout: {stdout}\nstderr: {stderr}"
        );
        stdout
    };

    let stdout = run(stack.scratch_url.clone());
    assert!(
        stdout.contains(&format!(
            "acme: team platform ({platform_id}) has 2 live delivery boards:"
        )),
        "{stdout}"
    );
    assert!(stdout.contains("platform-delivery"), "{stdout}");
    assert!(
        stdout.contains(&format!("platform-extra ({second})")),
        "{stdout}"
    );
    assert!(!stdout.contains(&web.id), "a team with one board: {stdout}");
    assert!(
        stdout.contains("1 team(s) with 2 or more live delivery boards in 1 tenant(s)"),
        "{stdout}"
    );
    // It applies no migration, so it prints no line about one.
    assert!(!stdout.contains("migration"), "{stdout}");
    assert_eq!(snapshot(&mut stack.conn), before, "the report only reads");

    // --- after the delete of the team, the report is empty
    stack
        .svc
        .delete_team(&platform.id)
        .await
        .expect("the delete of the team");
    let stdout = run(stack.scratch_url.clone());
    assert!(
        stdout.contains("0 team(s) with 2 or more live delivery boards in 1 tenant(s)"),
        "{stdout}"
    );

    stack.shutdown();
}
