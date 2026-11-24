//! The size of an initiative (KAIROS-T-0779, COLLIERY-I-0678): the board
//! setting `size_warning` (default 15), and the warning past it.
//!
//! 1. The initiative board reads 15 by default; PATCH sets it; a delivery
//!    board and a number out of 1..=1000 are a 422 `VALIDATION`. MCP
//!    `set_board_policy` sets it too.
//! 2. Past the setting, a new task of the initiative is created, and the
//!    MCP answer has the warning line; at or under it, no warning.
//! 3. REST: the create has the header `Kairos-Warning` past the setting.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_initiative_size_t0779_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types_org::CreateTeamRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_initiative_size_t0779_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

/// One JSON-RPC message of an MCP response: plain JSON or SSE `data:`.
fn rpc_message(body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return value;
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .find(|value| value.get("jsonrpc").is_some())
        .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"))
}

/// A minimal MCP session over HTTP.
struct Mcp {
    http: reqwest::Client,
    url: String,
    token: String,
    session: Option<String>,
    next_id: i64,
}

impl Mcp {
    async fn post(&mut self, body: Value) -> (reqwest::StatusCode, String) {
        let mut request = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("x-tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        let response = request.send().await.expect("MCP request");
        let status = response.status();
        if let Some(session) = response.headers().get("mcp-session-id") {
            self.session = Some(session.to_str().expect("session id").to_string());
        }
        (status, response.text().await.expect("MCP body"))
    }

    async fn connect(base_url: &str, token: &str) -> Mcp {
        let mut mcp = Mcp {
            http: reqwest::Client::new(),
            url: format!("{base_url}/mcp"),
            token: token.to_string(),
            session: None,
            next_id: 0,
        };
        let (status, body) = mcp
            .post(json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "kairos-t0779-size", "version": "0.0.0"},
                },
            }))
            .await;
        assert!(status.is_success(), "initialize: {body}");
        let (status, body) = mcp
            .post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        assert!(status.is_success(), "initialized: {body}");
        mcp
    }

    async fn call(&mut self, tool: &str, arguments: Value) -> (String, bool) {
        self.next_id += 1;
        let (status, body) = self
            .post(json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .await;
        assert!(status.is_success(), "{tool}: {body}");
        let result = rpc_message(&body)["result"].clone();
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} gave no text: {result}"))
            .to_string();
        (text, result["isError"].as_bool().unwrap_or(false))
    }
}

async fn rest(
    base_url: &str,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value, Vec<String>) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{base_url}{path}"))
        .bearer_auth(token)
        .header("x-tenant", "acme");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.expect("request");
    let status = response.status().as_u16();
    let warnings = response
        .headers()
        .get_all("kairos-warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    (
        status,
        response.json::<Value>().await.unwrap_or(Value::Null),
        warnings,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_initiative_past_its_size_warning_gets_the_question() {
    let _admin_conn = recreate_scratch_db(SCRATCH_DB);
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
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let base = server.base_url.clone();
    let admin = server.client(&svc_token, "acme");
    let _ = admin.whoami().await;
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user_id(&mut conn, "svc@kairos.test"),
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");
    admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    let initiatives = admin.get_board("initiatives").await.expect("board");
    let web = admin.get_board("web-delivery").await.expect("board");

    // 1. The setting.
    assert_eq!(initiatives.board.size_warning, Some(15));
    assert_eq!(web.board.size_warning, None);
    let path = format!("/api/boards/{}", initiatives.board.id);
    let (status, body, _) = rest(
        &base,
        &svc_token,
        reqwest::Method::PATCH,
        &path,
        Some(json!({"size_warning": 0})),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    let (status, body, _) = rest(
        &base,
        &svc_token,
        reqwest::Method::PATCH,
        &format!("/api/boards/{}", web.board.id),
        Some(json!({"size_warning": 5})),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    let (status, body, _) = rest(
        &base,
        &svc_token,
        reqwest::Method::PATCH,
        &path,
        Some(json!({"size_warning": 3})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["size_warning"], 3);
    let mut mcp = Mcp::connect(&base, &svc_token).await;
    let (text, error) = mcp
        .call(
            "set_board_policy",
            json!({"board": "initiatives", "size_warning": 2}),
        )
        .await;
    assert!(!error && text.contains("size_warning 2"), "{text}");

    // 2. MCP: the third task is past 2.
    let (text, error) = mcp
        .call(
            "create_item",
            json!({"item_type": "initiative", "board": "initiatives", "title": "Grows"}),
        )
        .await;
    assert!(!error, "{text}");
    let initiative = text
        .split_whitespace()
        .nth(2)
        .and_then(|code| code.strip_suffix(':'))
        .unwrap_or_else(|| panic!("no code in {text}"))
        .to_string();
    for (i, title) in ["One", "Two", "Three"].iter().enumerate() {
        let (text, error) = mcp
            .call(
                "create_item",
                json!({"item_type": "task", "board": "web-delivery", "parent": initiative, "title": title}),
            )
            .await;
        assert!(!error, "{text}");
        if i < 2 {
            assert!(!text.contains("too big"), "{text}");
        } else {
            assert!(
                text.contains(&format!(
                    "Warning: {initiative} has 3 tasks; its board asks at 2."
                )),
                "{text}"
            );
            assert!(text.contains("Is it too big?"), "{text}");
        }
    }

    // 3. REST: the header.
    let (status, body, warnings) = rest(
        &base,
        &svc_token,
        reqwest::Method::POST,
        "/api/tasks",
        Some(
            json!({"board_id": web.board.id, "title": "Four", "content": "", "parent": initiative}),
        ),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("has 4 tasks, over the size_warning 2")),
        "{warnings:?}"
    );
}
