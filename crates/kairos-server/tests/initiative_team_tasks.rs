//! A team change of an initiative names its open tasks that stay on other
//! boards (KAIROS-T-0777, COLLIERY-I-0678).
//!
//! 1. MCP `set_team` lists the open tasks off the team and names the bulk
//!    move with the board of the team. A completed task is not listed.
//! 2. `GET /api/initiatives/{code}/teams/{team}/open-tasks` gives the same
//!    tasks and the board of the team (the web asks with it).
//! 3. After `POST /api/tasks/move` with them, the list is empty.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_initiative_team_tasks_t0777_test`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_org::CreateTeamRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_initiative_team_tasks_t0777_test";

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
                    "clientInfo": {"name": "kairos-t0777-team", "version": "0.0.0"},
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
) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{base_url}{path}"))
        .bearer_auth(token)
        .header("x-tenant", "acme");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.expect("request");
    let status = response.status().as_u16();
    (
        status,
        response.json::<Value>().await.unwrap_or(Value::Null),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_team_change_names_the_open_tasks_on_other_boards() {
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
    for (name, slug, prefix) in [("Web", "web", "WEB"), ("Ops", "ops", "OPS")] {
        admin
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                code_prefix: prefix.into(),
                team_type: None,
            })
            .await
            .expect("team");
    }
    let web = admin.get_board("web-delivery").await.expect("board");
    let columns: HashMap<String, String> = web
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let mut mcp = Mcp::connect(&base, &svc_token).await;
    let (text, error) = mcp
        .call(
            "create_item",
            json!({"item_type": "initiative", "board": "initiatives", "title": "Moves teams"}),
        )
        .await;
    assert!(!error, "{text}");
    let initiative = text
        .split_whitespace()
        .nth(2)
        .and_then(|code| code.strip_suffix(':'))
        .unwrap_or_else(|| panic!("no code in {text}"))
        .to_string();
    let new_task = |title: &str| CreateTaskRequest {
        board_id: Some(web.board.id.clone()),
        column_id: None,
        title: title.into(),
        content: "one".into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
        from_team: None,
        parent: Some(initiative.clone()),
    };
    let mut open = Vec::new();
    for title in ["One", "Two"] {
        open.push(
            admin
                .create_task(&new_task(title))
                .await
                .expect("task")
                .short_code,
        );
    }
    let done = admin
        .create_task(&new_task("Done"))
        .await
        .expect("task")
        .short_code;
    admin
        .transition_task(&done, &columns["Todo"])
        .await
        .expect("todo");
    let (text, error) = mcp
        .call(
            "cancel_item",
            json!({"short_code": done, "reason": "Not needed."}),
        )
        .await;
    assert!(!error, "{text}");

    // 1. MCP set_team names the open tasks and the bulk move.
    let (text, error) = mcp
        .call("set_team", json!({"short_code": initiative, "team": "ops"}))
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("2 open task(s)"), "{text}");
    for code in &open {
        assert!(text.contains(code.as_str()), "{text}");
    }
    assert!(!text.contains(done.as_str()), "{text}");
    assert!(
        text.contains("move_item with short_codes and to_board ops-delivery"),
        "{text}"
    );

    // 2. The REST list for the web.
    let path = format!("/api/initiatives/{initiative}/teams/ops/open-tasks");
    let (status, body) = rest(&base, &svc_token, reqwest::Method::GET, &path, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["board"], "ops-delivery");
    let listed: Vec<&str> = body["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|t| t["short_code"].as_str().unwrap())
        .collect();
    assert_eq!(listed, open.iter().map(String::as_str).collect::<Vec<_>>());

    // 3. Move them; the list is empty.
    let (status, body) = rest(
        &base,
        &svc_token,
        reqwest::Method::POST,
        "/api/tasks/move",
        Some(json!({"short_codes": open, "board": "ops-delivery"})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["moved"].as_array().map(Vec::len), Some(2), "{body}");
    let (status, body) = rest(&base, &svc_token, reqwest::Method::GET, &path, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["tasks"].as_array().map(Vec::len), Some(0), "{body}");
}
