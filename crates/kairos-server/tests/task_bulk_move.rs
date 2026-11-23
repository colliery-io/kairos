//! Many tasks move to another delivery board in one call (KAIROS-T-0776,
//! COLLIERY-I-0678), over REST `POST /api/tasks/move` and MCP `move_item`
//! with `short_codes`.
//!
//! 1. Three tasks of Web: two move to Ops with the next codes of Ops (the
//!    default), the done one is refused with `TASK_DONE`; the old codes
//!    still find the moved tasks.
//! 2. A person with no `manage_tasks` on the target: each task is refused
//!    with a reason, and none moves.
//! 3. No code, or more than 200: 422 `VALIDATION`.
//! 4. MCP `move_item` with `short_codes` moves two tasks and lists them.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_bulk_move_t0776_test`.

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
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_task_bulk_move_t0776_test";

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
                    "clientInfo": {"name": "kairos-t0776-bulk", "version": "0.0.0"},
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

async fn bulk(base_url: &str, token: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("{base_url}/api/tasks/move"))
        .bearer_auth(token)
        .header("x-tenant", "acme")
        .json(&body)
        .send()
        .await
        .expect("bulk move");
    let status = response.status().as_u16();
    (
        status,
        response.json::<Value>().await.unwrap_or(Value::Null),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tasks_move_to_another_board_in_one_call() {
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
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
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
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    for client in [&admin, &alice, &bob] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
    ] {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: user_id(&mut conn, email),
                role,
            })
            .execute(&mut conn)
            .expect("membership");
    }
    // alice is in Web and Ops; bob is in Web only.
    for (name, slug, prefix, members) in [
        (
            "Web",
            "web",
            "WEB",
            vec!["alice@kairos.test", "bob@kairos.test"],
        ),
        ("Ops", "ops", "OPS", vec!["alice@kairos.test"]),
    ] {
        let team = admin
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                code_prefix: prefix.into(),
                team_type: None,
            })
            .await
            .expect("team");
        for email in members {
            admin
                .add_team_member(
                    &team.id,
                    &AddTeamMemberRequest {
                        user_id: user_id(&mut conn, email).to_string(),
                    },
                )
                .await
                .expect("team member");
        }
    }
    let web = admin.get_board("web-delivery").await.expect("board");
    let ops = admin.get_board("ops-delivery").await.expect("board");
    let columns: HashMap<String, String> = web
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
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
        parent: None,
    };
    let mut codes = Vec::new();
    for title in ["One", "Two", "Done"] {
        codes.push(
            alice
                .create_task(&new_task(title))
                .await
                .expect("task")
                .short_code,
        );
    }
    for column in ["Todo", "Active", "Completed"] {
        alice
            .transition_task(&codes[2], &columns[column])
            .await
            .expect("walk");
    }

    // 2. bob has no manage_tasks on Ops: each task is refused, none moves.
    let (status, body) = bulk(
        &base,
        &bob_token,
        json!({"short_codes": [codes[0], codes[1]], "board": "ops-delivery"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["moved"].as_array().map(Vec::len), Some(0), "{body}");
    assert_eq!(body["refused"].as_array().map(Vec::len), Some(2), "{body}");
    assert_eq!(
        bob.get_task(&codes[0]).await.expect("task").board_id,
        web.board.id
    );

    // 1. alice moves the three: two move with Ops codes, the done one stays.
    let (status, body) = bulk(
        &base,
        &alice_token,
        json!({"short_codes": codes, "board": "ops-delivery"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let moved = body["moved"].as_array().expect("moved");
    assert_eq!(moved.len(), 2, "{body}");
    for m in moved {
        let new_code = m["short_code"].as_str().expect("code");
        assert!(new_code.starts_with("OPS-T-"), "{body}");
        let task = alice.get_task(new_code).await.expect("task");
        assert_eq!(task.board_id, ops.board.id);
        let old = m["from"].as_str().expect("from");
        let by_old = alice.get_task(old).await.expect("the old code finds it");
        assert_eq!(by_old.short_code, new_code);
    }
    let refused = body["refused"].as_array().expect("refused");
    assert_eq!(refused.len(), 1, "{body}");
    assert_eq!(refused[0]["short_code"], codes[2].as_str());
    assert_eq!(refused[0]["code"], "TASK_DONE");

    // 3. No code; too many codes.
    let (status, body) = bulk(
        &base,
        &alice_token,
        json!({"short_codes": [], "board": "ops-delivery"}),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    let many: Vec<String> = (0..201).map(|i| format!("WEB-T-{i:04}")).collect();
    let (status, body) = bulk(
        &base,
        &alice_token,
        json!({"short_codes": many, "board": "ops-delivery"}),
    )
    .await;
    assert_eq!(status, 422, "{body}");

    // 4. MCP: two more tasks with short_codes.
    let mut more = Vec::new();
    for title in ["Three", "Four"] {
        more.push(
            alice
                .create_task(&new_task(title))
                .await
                .expect("task")
                .short_code,
        );
    }
    let mut mcp = Mcp::connect(&base, &alice_token).await;
    let (text, error) = mcp
        .call(
            "move_item",
            json!({"short_codes": more, "to_board": "ops-delivery"}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(
        text.contains("Moved 2 task(s) to ops-delivery; 0 not moved."),
        "{text}"
    );
    for code in &more {
        assert!(text.contains(code.as_str()), "{text}");
    }
    let (text, error) = mcp
        .call(
            "move_item",
            json!({"short_code": more[0], "short_codes": more, "to_board": "ops-delivery"}),
        )
        .await;
    assert!(error && text.contains("Send one of them"), "{text}");
}
