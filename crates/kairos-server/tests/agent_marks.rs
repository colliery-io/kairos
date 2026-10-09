//! History and activity mark a change made by an agent (KAIROS-T-0359,
//! KAIROS-A-0024 decision 3).
//!
//! 1. alice makes a task with her own token: version 1 and the `create`
//!    entry have no agent mark.
//! 2. An agent edits and moves the task with the agent key of alice: the
//!    version and the `transition` entry name alice and have the mark.
//! 3. alice edits and moves the task with her own token: no mark. The
//!    server has ONE connection in its blocking pool, so this request gets
//!    the connection of the request of step 2: the key does not carry over.
//! 4. A service account edits the task with its key: no mark.
//! 5. The agent edits the task over MCP: the version has the mark, and the
//!    MCP `get_history` text says "(agent)" on that version only.
//! 6. A write on a connection that never set the agent key (a background
//!    job, SCIM) gets no mark.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_agent_marks_t0359_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{CreateTaskRequest, UpdateContentRequest};
use kairos_client::types_meta::{ActivityEntry, ActivityQuery, HistoryVersion};
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_service_accounts::{CreateApiKeyRequest, CreateServiceAccountRequest};
use kairos_client::{EntityKind, KairosClient};
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{activity_log, organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::blocking::BlockingTenantPool;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_agent_marks_t0359_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

fn key_request(name: &str) -> CreateApiKeyRequest {
    CreateApiKeyRequest {
        name: name.into(),
        expires_at: None,
    }
}

/// The version `version` of the history.
fn version(history: &[HistoryVersion], version: i32) -> &HistoryVersion {
    history
        .iter()
        .find(|row| row.version == version)
        .unwrap_or_else(|| panic!("no version {version} in {history:?}"))
}

/// The entries of the task with the action `action`, oldest first.
fn entries<'a>(activity: &'a [ActivityEntry], action: &str) -> Vec<&'a ActivityEntry> {
    let mut found: Vec<&ActivityEntry> = activity
        .iter()
        .filter(|entry| entry.action == action)
        .collect();
    found.sort_by(|a, b| a.occurred_at.cmp(&b.occurred_at));
    found
}

/// Move the task one column on: the first transition of the board from
/// its column.
async fn move_on(client: &KairosClient, board: &str, short_code: &str) {
    let task = client.get_task(short_code).await.expect("task");
    let detail = client.get_board(board).await.expect("board");
    let to = detail
        .transitions
        .iter()
        .find(|t| t.from_column_id == task.column_id)
        .unwrap_or_else(|| panic!("no transition from {}", task.column_id))
        .to_column_id
        .clone();
    client
        .transition_task(short_code, &to)
        .await
        .expect("transition");
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
                    "clientInfo": {"name": "kairos-t0359-test", "version": "0.0.0"},
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

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
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
        assert!(
            !result["isError"].as_bool().unwrap_or(false),
            "{tool}: {text}"
        );
        text
    }
}

#[tokio::test]
async fn history_and_activity_mark_a_change_made_by_an_agent() {
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
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let mut state = app::state_with(base_config(&scratch_url), pool, auth);
    // ONE connection: each request gets the connection of the request
    // before it (step 3).
    state.blocking = BlockingTenantPool::new(&scratch_url, 1);
    let server = spawn_server(app::router(state)).await;
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    for client in [&admin, &alice] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
    ] {
        let user_id = user_id(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let alice_id = user_id(&mut conn, "alice@kairos.test").to_string();
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    admin
        .add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: alice_id.clone(),
            },
        )
        .await
        .expect("team member");
    let board = team.delivery_board_id.clone().expect("delivery board");
    let created = alice
        .create_agent_key(&key_request("laptop"))
        .await
        .expect("alice makes an agent key");
    let agent = server.client(&created.key, "acme");

    // 1. alice makes the task with her own token.
    let task = alice
        .create_task(&CreateTaskRequest {
            board_id: Some(board.clone()),
            column_id: None,
            title: "Marked".into(),
            content: "one".into(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
        })
        .await
        .expect("alice makes a task");

    // 2. The agent edits and moves it.
    let edited = agent
        .update_task(
            &task.short_code,
            &UpdateContentRequest {
                title: None,
                content: "two".into(),
                version: task.version,
            },
        )
        .await
        .expect("the agent edits the task");
    move_on(&agent, &board, &task.short_code).await;

    // 3. alice edits and moves it with her own token, on the same
    //    connection.
    let edited = alice
        .update_task(
            &task.short_code,
            &UpdateContentRequest {
                title: None,
                content: "three".into(),
                version: edited.version,
            },
        )
        .await
        .expect("alice edits the task");
    move_on(&alice, &board, &task.short_code).await;

    // 4. A service account edits it with its key.
    let sa = admin
        .create_service_account(&CreateServiceAccountRequest { name: "ci".into() })
        .await
        .expect("service account");
    admin
        .add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: sa.id.clone(),
            },
        )
        .await
        .expect("the service account joins the team");
    let sa_key = admin
        .create_api_key(&sa.id, &key_request("ci-main"))
        .await
        .expect("service-account key");
    let machine = server.client(&sa_key.key, "acme");
    let edited = machine
        .update_task(
            &task.short_code,
            &UpdateContentRequest {
                title: None,
                content: "four".into(),
                version: edited.version,
            },
        )
        .await
        .expect("the service account edits the task");

    // 5. The agent edits it over MCP.
    let mut mcp = Mcp::connect(&server.base_url, &created.key).await;
    mcp.call_ok(
        "update_item",
        json!({
            "short_code": task.short_code,
            "content": "five",
            "version": edited.version,
        }),
    )
    .await;

    let history = alice
        .history(EntityKind::Task, &task.short_code, None, None)
        .await
        .expect("history")
        .items;
    assert_eq!(history.len(), 5, "{history:?}");
    let key = Some(created.id.clone());
    for (number, mark) in [
        (1, None),
        (2, key.clone()),
        (3, None),
        (4, None),
        (5, key.clone()),
    ] {
        let row = version(&history, number);
        assert_eq!(row.agent_key_id, mark, "the mark of version {number}");
        if number != 4 {
            assert_eq!(row.edited_by, alice_id, "version {number} names alice");
        }
    }
    let snapshot = alice
        .history_snapshot(EntityKind::Task, &task.short_code, 2)
        .await
        .expect("snapshot");
    assert_eq!(snapshot.agent_key_id, key);

    let activity = alice
        .activity(&ActivityQuery {
            entity_id: Some(task.id.clone()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    let creates = entries(&activity, "create");
    assert_eq!(creates.len(), 1, "{activity:?}");
    assert_eq!(creates[0].agent_key_id, None);
    let transitions = entries(&activity, "transition");
    assert_eq!(transitions.len(), 2, "{activity:?}");
    assert_eq!(transitions[0].actor_id, alice_id);
    assert_eq!(transitions[0].agent_key_id, key, "the agent moved it");
    assert_eq!(transitions[1].actor_id, alice_id);
    assert_eq!(transitions[1].agent_key_id, None, "alice moved it");

    let text = mcp
        .call_ok("get_history", json!({"short_code": task.short_code}))
        .await;
    let marked: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("(agent)"))
        .collect();
    assert_eq!(marked.len(), 2, "{text}");
    assert!(
        marked.iter().any(|line| line.starts_with("- v5 ")),
        "{text}"
    );
    assert!(
        marked.iter().any(|line| line.starts_with("- v2 ")),
        "{text}"
    );

    // 6. A write on a connection that never set the agent key.
    let mut job = PgConnection::establish(&scratch_url).expect("connecting as a background job");
    diesel::sql_query("SET search_path TO \"org_acme\", public")
        .execute(&mut job)
        .expect("search_path");
    let mark: Option<Uuid> = diesel::insert_into(activity_log::table)
        .values(NewActivityLogEntry {
            actor_id: Uuid::nil(),
            action: ActivityAction::Update,
            entity_id: None,
            entity_type: None,
            details: "background".into(),
        })
        .returning(activity_log::agent_key_id)
        .get_result(&mut job)
        .expect("a background write");
    assert_eq!(mark, None, "a background write has no mark");
    // The same connection after a request scope cleared the setting.
    kairos_db::agent_mark::set(&mut job, Some(Uuid::nil())).expect("set");
    assert_eq!(
        kairos_db::agent_mark::current(&mut job).expect("current"),
        Some(Uuid::nil())
    );
    kairos_db::agent_mark::set(&mut job, None).expect("clear");
    assert_eq!(
        kairos_db::agent_mark::current(&mut job).expect("current"),
        None
    );
}
