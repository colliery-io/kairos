//! KAIROS-T-0736 (COLLIERY-I-0664, decision 8): a person watches an item,
//! over REST and MCP. The acceptance criteria:
//!
//! 1. `watch_item` (MCP) and `PUT /api/items/{code}/watch` (REST) start a
//!    watch, with or without the children; an agent key watches for its
//!    person. `list_watches` and `GET /api/me/watches` list them.
//! 2. A note of a different person on a watched item makes a `watch`
//!    notification; a note of the watcher makes none.
//! 3. `unwatch_item` and `DELETE` end it; a second end changes nothing.
//! 4. A service account cannot watch (403); an unknown code is a 404; an
//!    unknown argument is refused.
//!
//! The kinds of change and the children are in
//! `kairos-db/tests/item_watches.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_item_watches_t0736_server_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_notifications::WatchRequest;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_service_accounts::{CreateApiKeyRequest, CreateServiceAccountRequest};
use kairos_client::{EntityKind, Error};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_item_watches_t0736_server_test";

fn user(conn: &mut PgConnection, email: &str) -> Uuid {
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
        no_expiry: false,
        scope: None,
    }
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
    /// The text blocks of the last answer (KAIROS-T-0549).
    last_blocks: Vec<String>,
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
            last_blocks: Vec::new(),
        };
        let (status, body) = mcp
            .post(json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "kairos-t0736-watches", "version": "0.0.0"},
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

    /// The text of a tool call, and whether it is an error.
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
        // KAIROS-T-0549: the answer is the first block; the notifications,
        // when the person has unread ones, are a block of their own. `text`
        // joins every text block, so the assertions on the whole stay.
        let blocks: Vec<&str> = result["content"]
            .as_array()
            .unwrap_or_else(|| panic!("{tool} gave no content: {result}"))
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect();
        assert!(!blocks.is_empty(), "{tool} gave no text: {result}");
        self.last_blocks = blocks.iter().map(|b| b.to_string()).collect();
        let text = blocks.join("\n\n");
        (text, result["isError"].as_bool().unwrap_or(false))
    }

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(!error, "{tool}: {text}");
        text
    }
}

#[tokio::test]
async fn a_person_watches_an_item_over_rest_and_mcp() {
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
        let user_id = user(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let bob_id = user(&mut conn, "bob@kairos.test");
    let web = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    // Both are members of the team: Alice writes a note too.
    for member in [bob_id, user(&mut conn, "alice@kairos.test")] {
        admin
            .add_team_member(
                &web.id,
                &AddTeamMemberRequest {
                    user_id: member.to_string(),
                },
            )
            .await
            .expect("member of web");
    }
    let board = web.delivery_board_id.clone().expect("board");
    let mut codes = Vec::new();
    for title in ["First", "Second"] {
        let task = bob
            .create_task(&CreateTaskRequest {
                board_id: Some(board.clone()),
                column_id: None,
                title: title.into(),
                content: String::new(),
                task_type: None,
                work_class: None,
                team_id: None,
                repository: None,
                from_team: None,
                parent: None,
            })
            .await
            .expect("task");
        codes.push(task.short_code);
    }
    let (first, second) = (codes[0].clone(), codes[1].clone());

    // --- 1. A watch over MCP with an agent key, and over REST -----------------
    let agent_key = alice
        .create_agent_key(&key_request("laptop"))
        .await
        .expect("agent key");
    let mut agent = Mcp::connect(&server.base_url, &agent_key.key).await;
    let text = agent
        .call_ok("watch_item", json!({"short_code": first}))
        .await;
    assert!(text.starts_with(&format!("You watch {first}.")), "{text}");
    // The watch is the person's.
    let watch = alice.watch_state(&first).await.expect("state");
    assert!(watch.watching && !watch.with_children, "{watch:?}");
    let watch = alice
        .watch_item(
            &second,
            &WatchRequest {
                with_children: Some(true),
            },
        )
        .await
        .expect("REST watch");
    assert!(watch.watching && watch.with_children, "{watch:?}");
    let list = alice.watches().await.expect("REST list");
    let listed: Vec<(&str, bool)> = list
        .items
        .iter()
        .map(|w| (w.short_code.as_str(), w.with_children))
        .collect();
    assert_eq!(
        listed,
        vec![(first.as_str(), false), (second.as_str(), true)]
    );
    let text = agent.call_ok("list_watches", json!({})).await;
    assert!(text.starts_with("# Your watches (2)"), "{text}");
    assert!(
        text.contains(&format!("- {second} (task) · with children — Second")),
        "{text}"
    );
    // Bob watches nothing.
    assert!(bob.watches().await.expect("bob").items.is_empty());
    assert!(!bob.watch_state(&first).await.expect("state").watching);
    // The kind is in the settings.
    assert!(alice.notification_settings().await.expect("settings").watch);

    // --- 2. A note of a different person tells the watcher --------------------
    bob.append_note(EntityKind::Task, &first, "Started.")
        .await
        .expect("note");
    let list = alice.notifications().await.expect("notifications");
    let watched: Vec<&str> = list
        .items
        .iter()
        .filter(|n| n.kind == "watch")
        .map(|n| n.text.as_str())
        .collect();
    assert_eq!(watched.len(), 1, "{:?}", list.items);
    assert!(
        watched[0].contains(&format!("added a note to {first}: First")),
        "{watched:?}"
    );
    let events = agent.call_ok("get_events", json!({})).await;
    assert!(events.contains("added a note to"), "{events}");
    // A note of the watcher makes none.
    alice
        .append_note(EntityKind::Task, &first, "Mine.")
        .await
        .expect("note");
    let count = alice
        .notifications()
        .await
        .expect("notifications")
        .items
        .iter()
        .filter(|n| n.kind == "watch")
        .count();
    assert_eq!(count, 1, "the actor is not told");

    // --- 3. The end of a watch --------------------------------------------------
    let text = agent
        .call_ok("unwatch_item", json!({"short_code": first}))
        .await;
    assert_eq!(
        text.lines().next(),
        Some(format!("You do not watch {first} now.").as_str())
    );
    let text = agent
        .call_ok("unwatch_item", json!({"short_code": first}))
        .await;
    assert!(
        text.starts_with(&format!("You did not watch {first}. Nothing changed.")),
        "{text}"
    );
    let watch = alice.unwatch_item(&second).await.expect("REST unwatch");
    assert!(!watch.watching && !watch.with_children, "{watch:?}");
    assert!(alice.watches().await.expect("list").items.is_empty());
    let text = agent.call_ok("list_watches", json!({})).await;
    assert!(text.starts_with("You watch no item."), "{text}");

    // --- 4. Refusals ------------------------------------------------------------
    let account = admin
        .create_service_account(&CreateServiceAccountRequest { name: "bot".into() })
        .await
        .expect("service account");
    let bot_key = admin
        .create_api_key(&account.id, &key_request("k"))
        .await
        .expect("key");
    let bot = server.client(&bot_key.key, "acme");
    let err = bot
        .watch_item(&first, &WatchRequest::default())
        .await
        .expect_err("a service account cannot watch");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    let mut bot_mcp = Mcp::connect(&server.base_url, &bot_key.key).await;
    let (text, error) = bot_mcp
        .call("watch_item", json!({"short_code": first}))
        .await;
    assert!(error && text.starts_with("FORBIDDEN"), "{text}");
    let err = alice
        .watch_item("WEB-T-9999", &WatchRequest::default())
        .await
        .expect_err("no such item");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let (text, error) = agent
        .call("watch_item", json!({"short_code": first, "children": true}))
        .await;
    assert!(
        error && text.contains("children"),
        "an unknown argument: {text}"
    );
}
