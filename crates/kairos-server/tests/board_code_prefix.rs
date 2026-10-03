//! COLLIERY-T-3099 — each board has a short-code prefix, over REST against
//! the booted production router.
//!
//! - `POST /api/boards` and `POST /api/teams` need `code_prefix`. A body
//!   with no prefix, or with a prefix that does not match the rule, is a
//!   422 `VALIDATION` that names the field `code_prefix`.
//! - Boards can share a prefix when they hold different types. A prefix
//!   that a live board of the same level has is a 409 `CONFLICT` that names
//!   that board, and the server writes nothing.
//! - A task on the board gets the prefix of the board, and the board
//!   detail gives the prefix.
//!
//! The migration and the sequences are in
//! `kairos-db/tests/board_code_prefix.rs`.
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
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    user_token, with_database,
};
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
    token: String,
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

        // JIT-provision svc, then make it the org admin.
        let _ = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(&token),
            &TENANT,
            None,
        )
        .await;
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
            token,
        }
    }

    /// One request as the org admin.
    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        request(&self.router, method, uri, Some(&self.token), &TENANT, body).await
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// The number of rows that a query counts, in the tenant.
fn count(conn: &mut PgConnection, from_where: &str) -> i64 {
    diesel::sql_query(format!("SELECT count(*) AS count FROM {from_where}"))
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("counting {from_where}: {e}"))
        .count
}

/// A refusal that names the field `code_prefix`: 422 `VALIDATION`.
fn assert_names_code_prefix(status: StatusCode, answer: &Value) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(error_code(answer), "VALIDATION", "{answer}");
    assert_eq!(
        answer["error"]["details"]["field"], "code_prefix",
        "{answer}"
    );
    let message = answer["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("code_prefix"), "{message}");
}

#[tokio::test]
async fn a_board_needs_a_valid_prefix_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_prefix_rest_t3099").await;
    let before = count(&mut stack.conn, "boards");

    // When I create a board with no prefix, or with the prefix "sk-adi",
    // then the create is refused, and the error names the field
    // code_prefix.
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(json!({"name": "Roadmap", "slug": "roadmap", "board_level": "initiative"})),
        )
        .await;
    assert_names_code_prefix(status, &answer);
    for bad in ["sk-adi", "skadi", "S", "ABCDEFGHIJK", ""] {
        let (status, answer) = stack
            .send(
                Method::POST,
                "/api/boards",
                Some(json!({
                    "name": "Roadmap", "slug": "roadmap", "board_level": "initiative",
                    "code_prefix": bad,
                })),
            )
            .await;
        assert_names_code_prefix(status, &answer);
        assert!(
            answer["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("^[A-Z][A-Z0-9]{1,9}$"),
            "the refusal gives the rule: {answer}"
        );
    }

    // The same for a team, which creates a delivery board.
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Skadi", "slug": "skadi"})),
        )
        .await;
    assert_names_code_prefix(status, &answer);
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Skadi", "slug": "skadi", "code_prefix": "sk-adi"})),
        )
        .await;
    assert_names_code_prefix(status, &answer);
    assert_eq!(count(&mut stack.conn, "teams"), 0, "no team is written");
    assert_eq!(
        count(&mut stack.conn, "boards"),
        before,
        "no board is written"
    );

    // An unknown field is still refused and named.
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(json!({
                "name": "Roadmap", "slug": "roadmap", "board_level": "initiative",
                "code_prefix": "ROAD", "prefix": "ROAD",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"]["details"]["field"], "prefix", "{answer}");

    // A valid prefix: the board has it, and a task on it gets it.
    let (status, team) = stack
        .send(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Skadi", "slug": "skadi", "code_prefix": "SKADI"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    let board_id = team["delivery_board_id"]
        .as_str()
        .expect("the board")
        .to_string();
    let (status, board) = stack
        .send(Method::GET, &format!("/api/boards/{board_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{board}");
    assert_eq!(board["code_prefix"], "SKADI", "{board}");
    let (status, task) = stack
        .send(
            Method::POST,
            "/api/tasks",
            Some(json!({"board_id": board_id, "title": "First"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    assert_eq!(task["short_code"], "SKADI-T-0001", "{task}");

    stack.shutdown();
}

#[tokio::test]
async fn boards_share_a_prefix_only_for_different_types_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_prefix_share_rest_t3099").await;

    // Given the board "initiatives" with prefix "ACME" (provisioning).
    let (status, list) = stack.send(Method::GET, "/api/boards", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let initiatives = list["items"]
        .as_array()
        .expect("boards")
        .iter()
        .find(|b| b["slug"] == "initiatives")
        .expect("the initiatives board")
        .clone();
    assert_eq!(initiatives["code_prefix"], "ACME", "{initiatives}");

    // A delivery board can share ACME: it holds different types.
    let (status, core) = stack
        .send(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Core", "slug": "core", "code_prefix": "ACME"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{core}");
    let core_board = core["delivery_board_id"]
        .as_str()
        .expect("board")
        .to_string();

    // When I create a delivery board with prefix "ACME" for a new team, then
    // the create is refused, and the error names core-delivery, which has
    // ACME for tasks.
    let teams_before = count(&mut stack.conn, "teams");
    let boards_before = count(&mut stack.conn, "boards");
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Next", "slug": "next", "code_prefix": "ACME"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(error_code(&answer), "CONFLICT", "{answer}");
    let message = answer["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("core-delivery") && message.contains("tasks"),
        "the message names the board and the type: {message}"
    );
    assert_eq!(
        answer["error"]["details"]["code_prefix"], "ACME",
        "{answer}"
    );
    assert_eq!(
        answer["error"]["details"]["board"]["id"], core_board,
        "{answer}"
    );
    assert_eq!(
        answer["error"]["details"]["board"]["slug"], "core-delivery",
        "{answer}"
    );
    assert_eq!(count(&mut stack.conn, "teams"), teams_before, "no team");
    assert_eq!(count(&mut stack.conn, "boards"), boards_before, "no board");

    // The same over POST /api/boards, for a board of the organization: the
    // board "adrs" has ACME for ADRs.
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(json!({
                "name": "More decisions", "slug": "more-adrs", "board_level": "adr",
                "code_prefix": "ACME",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(
        answer["error"]["details"]["board"]["slug"], "adrs",
        "{answer}"
    );

    // A different prefix is accepted.
    let (status, board) = stack
        .send(
            Method::POST,
            "/api/boards",
            Some(json!({
                "name": "More decisions", "slug": "more-adrs", "board_level": "adr",
                "code_prefix": "MOREADR",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{board}");
    assert_eq!(board["code_prefix"], "MOREADR", "{board}");

    stack.shutdown();
}
