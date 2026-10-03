//! Integration test for COLLIERY-T-3100: a retired short code still finds
//! its item and names the current code, on each surface.
//!
//! The fixture: a task whose code changed from `COLLIERY-T-2430` to
//! `SKADI-T-0577` (the rename of COLLIERY-T-3101 does not exist yet, so the
//! code is changed directly and then retired with `retire_code`).
//!
//! - MCP `get_item` with the retired code gives the item, and the answer
//!   says that its code is now `SKADI-T-0577`.
//! - REST `GET /api/tasks/{code}` and the history give the item with its
//!   current code.
//! - REST and MCP search find the item by the retired code, with its
//!   current code.
//! - A write with the retired code is refused, and the refusal names the
//!   current code (REST 404 `details.current_code`; MCP tool error).
//!
//! The database part (the sequence, the trigger) is in
//! `kairos-db/tests/retired_codes.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_retired_codes_t3100_test`.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    spawn_server, user_token, with_database,
};
use kairos_db::items::{self, CreateTask};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole, TaskType, WorkClass};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, boards, provision_tenant, retired_codes, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_retired_codes_t3100_test";

/// Minimal MCP session over HTTP (initialize → initialized → tools/call);
/// the same shape as the one in tests/done_blocks.rs.
struct McpSession {
    http: reqwest::Client,
    url: String,
    token: String,
    session_id: String,
    next_id: u64,
}

impl McpSession {
    async fn open(base_url: &str, token: &str) -> Self {
        let http = reqwest::Client::new();
        let url = format!("{base_url}/mcp");
        let response = http
            .post(&url)
            .bearer_auth(token)
            .header("X-Tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "colliery-t3100-test", "version": "0.0.0"},
                },
            }))
            .send()
            .await
            .expect("initialize");
        assert_eq!(response.status(), 200, "initialize failed");
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .expect("session id")
            .to_string();
        let session = Self {
            http,
            url,
            token: token.to_string(),
            session_id,
            next_id: 0,
        };
        let response = session
            .http
            .post(&session.url)
            .bearer_auth(&session.token)
            .header("X-Tenant", "acme")
            .header("mcp-session-id", &session.session_id)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .send()
            .await
            .expect("initialized");
        assert_eq!(response.status(), 202, "initialized notify");
        session
    }

    /// Call a tool; returns its text and whether it is a tool error.
    async fn call(&mut self, tool: &str, arguments: Value) -> (String, bool) {
        self.next_id += 1;
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("X-Tenant", "acme")
            .header("mcp-session-id", &self.session_id)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .send()
            .await
            .expect("tools/call");
        assert_eq!(response.status(), 200, "tools/call HTTP status");
        let body = response.text().await.expect("body");
        let message: Value = serde_json::from_str::<Value>(&body)
            .ok()
            .filter(|v| v.get("jsonrpc").is_some())
            .or_else(|| {
                body.lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
                    .find(|v| v.get("jsonrpc").is_some())
            })
            .unwrap_or_else(|| panic!("no JSON-RPC message in response body: {body:?}"));
        let result = &message["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content: {message}"))
            .to_string();
        (text, result["isError"].as_bool().unwrap_or(false))
    }

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (text, is_error) = self.call(tool, arguments).await;
        assert!(!is_error, "{tool} unexpectedly errored: {text}");
        text
    }
}

#[tokio::test]
async fn a_retired_code_finds_its_item_and_names_the_current_code() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let acme: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("org row");

    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
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
    let _ = svc.whoami().await;
    let svc_id: Uuid = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("svc provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: acme,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting membership");

    // --- the fixture ------------------------------------------------------
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let colliery_team = common::seed_team(&mut conn, "Colliery", "colliery-io");
    let colliery = boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Colliery Delivery",
        "colliery-io-delivery",
        kairos_db::CodePrefix::Given("COLLIERY"),
        Some(colliery_team),
        None,
    )
    .expect("creating colliery-io-delivery")
    .id;
    let skadi_team = common::seed_team(&mut conn, "Skadi", "skadi");
    boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Skadi",
        "skadi",
        kairos_db::CodePrefix::Given("SKADI"),
        Some(skadi_team),
        None,
    )
    .expect("creating skadi");
    items::ensure_code_sequence(
        &mut conn,
        "COLLIERY",
        kairos_core::short_code::ItemType::Task,
    )
    .expect("the sequence row");
    sql_query(
        "UPDATE short_code_sequences SET last_number = $1 \
          WHERE code_prefix = 'COLLIERY' AND item_type = 'T'",
    )
    .bind::<BigInt, _>(2429)
    .execute(&mut conn)
    .expect("setting the sequence");
    let task = items::create_task(
        &mut conn,
        CreateTask {
            board_id: colliery,
            column_id: None,
            title: "Find the downloads",
            content: "The worker finds the downloads.",
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        svc_id,
    )
    .expect("creating the task");
    assert_eq!(task.short_code, "COLLIERY-T-2430");
    sql_query("UPDATE tasks SET short_code = $2 WHERE id = $1")
        .bind::<SqlUuid, _>(task.id)
        .bind::<Text, _>("SKADI-T-0577")
        .execute(&mut conn)
        .expect("changing the code");
    retired_codes::retire_code(&mut conn, "COLLIERY-T-2430", task.id, "moved to skadi")
        .expect("retiring the code");

    let mut mcp = McpSession::open(&server.base_url, &svc_token).await;
    let tenant = [("X-Tenant", "acme")];

    // =======================================================================
    // Scenario: get_item follows a retired code
    // =======================================================================
    let text = mcp
        .call_ok("get_item", json!({"short_code": "COLLIERY-T-2430"}))
        .await;
    assert!(
        text.starts_with("# SKADI-T-0577 — Find the downloads\n"),
        "get_item gives the item with its current code:\n{text}"
    );
    assert!(
        text.contains(
            "The code COLLIERY-T-2430 is retired. The current code of this item is SKADI-T-0577."
        ),
        "the answer says that the code is now SKADI-T-0577:\n{text}"
    );
    assert!(
        text.contains("- retired codes: COLLIERY-T-2430\n"),
        "{text}"
    );

    // With the current code: no notice, and the line of the retired codes.
    let text = mcp
        .call_ok("get_item", json!({"short_code": "SKADI-T-0577"}))
        .await;
    assert!(!text.contains("RETIRED CODE"), "{text}");
    assert!(
        text.contains("- retired codes: COLLIERY-T-2430\n"),
        "{text}"
    );

    // REST: the item, with its current code.
    let (status, body) = request(
        &router,
        Method::GET,
        "/api/tasks/COLLIERY-T-2430",
        Some(&svc_token),
        &tenant,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["short_code"], "SKADI-T-0577");
    assert_eq!(body["id"], task.id.to_string());

    // REST history (a read that includes archived work) follows too.
    let (status, body) = request(
        &router,
        Method::GET,
        "/api/tasks/COLLIERY-T-2430/history",
        Some(&svc_token),
        &tenant,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // An unknown code is still a 404 with no current code.
    let (status, body) = request(
        &router,
        Method::GET,
        "/api/tasks/COLLIERY-T-9999",
        Some(&svc_token),
        &tenant,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body["error"]["details"].get("current_code").is_none());

    // =======================================================================
    // A write with a retired code is refused, and names the current code
    // =======================================================================
    let (status, body) = request(
        &router,
        Method::PATCH,
        "/api/tasks/COLLIERY-T-2430",
        Some(&svc_token),
        &tenant,
        Some(json!({"content": "changed", "version": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "NOT_FOUND");
    assert_eq!(body["error"]["details"]["retired_code"], "COLLIERY-T-2430");
    assert_eq!(body["error"]["details"]["current_code"], "SKADI-T-0577");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("\"SKADI-T-0577\""), "{message}");

    let (text, is_error) = mcp
        .call(
            "edit_item",
            json!({"short_code": "COLLIERY-T-2430", "search": "worker", "replace": "agent"}),
        )
        .await;
    assert!(is_error, "a write with a retired code is refused: {text}");
    assert!(
        text.contains("retired") && text.contains("SKADI-T-0577"),
        "the refusal names the current code: {text}"
    );
    // Nothing changed.
    let text = mcp
        .call_ok("get_item", json!({"short_code": "SKADI-T-0577"}))
        .await;
    assert!(text.contains("The worker finds the downloads."), "{text}");

    // =======================================================================
    // Scenario: Search finds an item by a retired code
    // =======================================================================
    let text = mcp.call_ok("search", json!({"q": "COLLIERY-T-2430"})).await;
    assert!(
        text.contains(
            "The code COLLIERY-T-2430 is retired. The current code of the item is SKADI-T-0577."
        ),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.starts_with("- SKADI-T-0577 — Find the downloads")),
        "the result has the item with its current code:\n{text}"
    );

    let (status, body) = request(
        &router,
        Method::POST,
        "/api/search",
        Some(&svc_token),
        &tenant,
        Some(json!({"q": "COLLIERY-T-2430"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let codes: Vec<&str> = body["results"]["tasks"]
        .as_array()
        .unwrap_or_else(|| panic!("tasks in {body}"))
        .iter()
        .filter_map(|t| t["short_code"].as_str())
        .collect();
    assert_eq!(codes, vec!["SKADI-T-0577"], "{body}");

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
