//! "Archive completed" (KAIROS-T-0363): one call archives each task in the
//! done columns of a board, on the REST and MCP surfaces.
//!
//! 1. The call archives the tasks of the done columns only: a finished task
//!    and a cancelled task go, a task in Active stays.
//! 2. The response gives the count and the codes, and each archived task
//!    has `archived_at`; the activity has one `delete` row for each.
//! 3. A person with no `manage_tasks` on the board gets a 403, and nothing
//!    changes.
//! 4. A restore brings back an archived task, cancel mark and all.
//! 5. MCP `archive_completed` needs confirm=true, and on empty done
//!    columns it archives nothing.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_archive_completed_t0363_server_test`.

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
use kairos_client::Error;
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_meta::ActivityQuery;
use kairos_client::types_org::{AddTeamMemberRequest, BoardItemsQuery, CreateTeamRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_archive_completed_t0363_server_test";

fn user(conn: &mut PgConnection, email: &str) -> (Uuid, String) {
    users::table
        .filter(users::email.eq(email))
        .select((users::id, users::display_name))
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
                    "clientInfo": {"name": "kairos-t0363-archive-completed", "version": "0.0.0"},
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
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} gave no text: {result}"))
            .to_string();
        (text, result["isError"].as_bool().unwrap_or(false))
    }

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(!error, "{tool}: {text}");
        text
    }
}

#[tokio::test]
async fn archive_completed_archives_the_done_columns() {
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
    let carol_token = user_token(&http, "carol").await;
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
    let carol = server.client(&carol_token, "acme");
    for client in [&admin, &alice, &bob, &carol] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
        ("carol@kairos.test", OrgRole::Member),
    ] {
        let (user_id, _) = user(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let (alice_id, alice_name) = user(&mut conn, "alice@kairos.test");
    let (bob_id, _) = user(&mut conn, "bob@kairos.test");
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    // alice and bob are in the team: the team gives them `transition_items`
    // and `manage_tasks` on its board. carol is a member of the
    // organization only.
    for user_id in [alice_id, bob_id] {
        admin
            .add_team_member(
                &team.id,
                &AddTeamMemberRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await
            .expect("team member");
    }
    let board = team.delivery_board_id.clone().expect("delivery board");
    let detail = admin.get_board(&board).await.expect("board");
    let columns: HashMap<String, String> = detail
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let done_column = detail
        .columns
        .iter()
        .find(|c| c.is_done)
        .expect("a done column")
        .clone();
    let to = |name: &str| columns[name].clone();
    let new_task = |title: &'static str| CreateTaskRequest {
        board_id: Some(board.clone()),
        column_id: None,
        title: title.into(),
        content: "one".into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
    };

    // --- the board: one finished, one cancelled, one in Active -------------
    let finished = alice
        .create_task(&new_task("Finished"))
        .await
        .expect("task");
    for column in ["Todo", "Active", "Completed"] {
        alice
            .transition_task(&finished.short_code, &to(column))
            .await
            .unwrap_or_else(|e| panic!("to {column}: {e}"));
    }
    let cancelled = alice
        .create_task(&new_task("Cancelled"))
        .await
        .expect("task");
    alice
        .cancel_task(&cancelled.short_code, "Not needed.")
        .await
        .expect("cancel");
    let active = alice
        .create_task(&new_task("Still going"))
        .await
        .expect("task");
    for column in ["Todo", "Active"] {
        alice
            .transition_task(&active.short_code, &to(column))
            .await
            .unwrap_or_else(|e| panic!("to {column}: {e}"));
    }
    let mut expected = vec![finished.short_code.clone(), cancelled.short_code.clone()];
    expected.sort();

    // --- 3. carol may not archive on the board -------------------------------
    let err = carol
        .archive_completed(&board)
        .await
        .expect_err("carol has no manage_tasks");
    assert!(
        matches!(&err, Error::Forbidden { capability: Some(c), .. } if c == "manage_tasks"),
        "{err}"
    );
    let got = carol.get_task(&finished.short_code).await.expect("get");
    assert_eq!(got.archived_at, None, "the refusal changed nothing");

    // --- 1 and 2. bob manages the board --------------------------------------
    let response = bob.archive_completed(&board).await.expect("bob archives");
    assert_eq!(response.count, 2, "{response:?}");
    assert_eq!(response.short_codes, expected);
    assert_eq!(response.board_id, board);
    for code in &expected {
        let got = carol.get_task(code).await.expect("get");
        assert!(got.archived_at.is_some(), "{code} is archived");
    }
    let got = carol.get_task(&active.short_code).await.expect("get");
    assert_eq!(got.archived_at, None, "a task in Active stays");
    let items = carol
        .board_items(&board, &BoardItemsQuery::default())
        .await
        .expect("board items");
    let live: Vec<&str> = items
        .columns
        .iter()
        .flat_map(|group| group.tasks.iter())
        .map(|t| t.short_code.as_str())
        .collect();
    assert_eq!(
        live,
        vec![active.short_code.as_str()],
        "the board shows the rest"
    );
    let deletes = carol
        .activity(&ActivityQuery {
            entity_id: Some(finished.id.clone()),
            action: Some("delete".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert_eq!(
        deletes.len(),
        1,
        "one archive row for each task: {deletes:?}"
    );

    // --- 4. a restore brings a task back -------------------------------------
    alice
        .restore_task(&cancelled.short_code)
        .await
        .expect("restore");
    let back = carol.get_task(&cancelled.short_code).await.expect("get");
    assert_eq!(back.archived_at, None);
    assert_eq!(
        back.cancellation.map(|m| m.reason),
        Some("Not needed.".to_string()),
        "the cancel mark stays"
    );

    // --- 5. MCP --------------------------------------------------------------
    let mut bob_mcp = Mcp::connect(&server.base_url, &bob_token).await;
    let (text, error) = bob_mcp
        .call(
            "archive_completed",
            json!({"board": board, "confirm": false}),
        )
        .await;
    assert!(error && text.contains("confirm=true"), "{text}");
    let text = bob_mcp
        .call_ok(
            "archive_completed",
            json!({"board": board, "confirm": true}),
        )
        .await;
    assert!(
        text.contains("Archived 1 completed task") && text.contains(&cancelled.short_code),
        "{text}"
    );
    let text = bob_mcp
        .call_ok(
            "archive_completed",
            json!({"board": board, "confirm": true}),
        )
        .await;
    assert!(text.contains("Kairos archived nothing"), "{text}");
    let mut carol_mcp = Mcp::connect(&server.base_url, &carol_token).await;
    let (text, error) = carol_mcp
        .call(
            "archive_completed",
            json!({"board": board, "confirm": true}),
        )
        .await;
    assert!(error && text.contains("FORBIDDEN"), "{text}");

    // --- a board that is not a delivery board has no tasks -------------------
    let boards = admin
        .list_boards(Default::default())
        .await
        .expect("boards")
        .items;
    let initiatives = boards
        .iter()
        .find(|b| b.board_level == "initiative")
        .expect("the tenant has an initiative board");
    let err = admin
        .archive_completed(&initiatives.id)
        .await
        .expect_err("not a delivery board");
    assert!(
        matches!(&err, Error::Other { status: 422, code, .. } if code == "NOT_DELIVERY_BOARD"),
        "{err}"
    );
    let _ = (alice_name, &done_column, Value::Null);
}
