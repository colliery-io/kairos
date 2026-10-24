//! The reopen of a done task, and the refusal of a board move of a done
//! task (KAIROS-T-0564), on the REST and MCP surfaces.
//!
//! 1. A board move of a task in a done column is a 422 `TASK_DONE` that
//!    names `reopen_item`, over REST and MCP. The task stays.
//! 2. A person with no `transition_items` gets a 403 for a reopen.
//! 3. A completed task reopens into the first column that is not done
//!    (Backlog): the transition graph has no edge out of Completed. The
//!    activity has a `transition` row, and the creator of a dependant gets
//!    `blocked_again` (the admin reopens: Kairos does not tell the actor). A second reopen is a 422 `TASK_NOT_DONE`. After the
//!    reopen, the board move works.
//! 4. MCP `reopen_item` of a cancelled task removes the mark. The `cancel`
//!    row stays, and the dependant gets no `blocked_again`.
//! 5. `parent_gate`: a reopen into a claims column under an initiative in
//!    Discovery is a 422 `PARENT_NOT_ACTIVE`, and the task stays done.
//! 6. A board with only done columns: 422 `NO_OPEN_COLUMN`.
//! 7. `reopen_item` of an item that is not a task is `VALIDATION`.
//! 8. KAIROS-T-0675: a transition out of Completed, which has no
//!    transition, names `reopen_item`. A refused move of an initiative from
//!    Discovery to Active names the path, in the text and in `path`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_reopen_t0564_test`.

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
use kairos_client::types_meta::{ActivityQuery, CreateRelationshipRequest};
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest, UpdateColumnRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_task_reopen_t0564_test";

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
                    "clientInfo": {"name": "kairos-t0564-reopen", "version": "0.0.0"},
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

    async fn ok(&mut self, tool: &str, arguments: Value) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(!error, "{tool}: {text}");
        text
    }

    async fn refused(&mut self, tool: &str, arguments: Value, code: &str) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(
            error && text.contains(code),
            "{tool}: expected {code}, got {text}"
        );
        text
    }

    /// Create an item; its short code.
    async fn create(&mut self, arguments: Value) -> String {
        let text = self.ok("create_item", arguments).await;
        text.split_whitespace()
            .nth(2)
            .and_then(|code| code.strip_suffix(':'))
            .unwrap_or_else(|| panic!("no code in {text}"))
            .to_string()
    }
}

/// The `blocked_again` notifications about the item `item_id`.
fn blocked_again(conn: &mut PgConnection, item_id: &str) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    diesel::sql_query(
        "SELECT count(*) AS n FROM org_acme.notifications \
         WHERE kind = 'blocked_again' AND item_id = $1",
    )
    .bind::<diesel::sql_types::Uuid, _>(item_id.parse::<Uuid>().expect("uuid"))
    .get_result::<Count>(conn)
    .expect("count")
    .n
}

fn refused_with(result: Result<impl std::fmt::Debug, Error>, code: &str, status: u16) -> String {
    let err = result.expect_err("refused");
    assert_eq!(err.code(), Some(code), "{err:?}");
    assert_eq!(err.status(), Some(status), "{err:?}");
    err.to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_done_task_is_reopened_and_not_moved() {
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
    let carol = server.client(&carol_token, "acme");
    for client in [&admin, &alice, &carol] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("carol@kairos.test", OrgRole::Member),
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
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    // alice is in the teams Web and Ops: `transition_items` and
    // `manage_tasks` on the two boards. carol is a member of the
    // organization only.
    for (name, slug, prefix) in [
        ("Web", "web", "WEB"),
        ("Ops", "ops", "OPS"),
        ("Lab", "lab", "LAB"),
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
        admin
            .add_team_member(
                &team.id,
                &AddTeamMemberRequest {
                    user_id: alice_id.to_string(),
                },
            )
            .await
            .expect("team member");
    }
    let web = admin.get_board("web-delivery").await.expect("board");
    let columns: HashMap<String, String> = web
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let to = |name: &str| columns[name].clone();
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
    let blocks = |source: &str, target: &str| CreateRelationshipRequest {
        source_short_code: source.into(),
        target_short_code: target.into(),
        relationship: "blocks".into(),
    };
    let mut mcp = Mcp::connect(&server.base_url, &alice_token).await;
    // The admin reopens where a notification counts: Kairos does not tell
    // the actor.
    let mut admin_mcp = Mcp::connect(&server.base_url, &svc_token).await;

    // --- A completed task, and a task that waits for it.
    let done = alice
        .create_task(&new_task("Done work"))
        .await
        .expect("task");
    let waits = alice
        .create_task(&new_task("Waits for it"))
        .await
        .expect("task");
    alice
        .create_relationship(&blocks(&done.short_code, &waits.short_code))
        .await
        .expect("blocks");
    for column in ["Todo", "Active", "Completed"] {
        alice
            .transition_task(&done.short_code, &to(column))
            .await
            .expect("walk");
    }

    // 1. A board move of a done task is refused, and the task stays.
    let text = refused_with(
        alice.move_task(&done.short_code, "ops-delivery").await,
        "TASK_DONE",
        422,
    );
    assert!(text.contains("reopen_item"), "{text}");
    let got = alice.get_task(&done.short_code).await.expect("get");
    assert_eq!(got.board_id, web.board.id, "the task stays");
    assert_eq!(got.column_id, to("Completed"));
    mcp.refused(
        "move_item",
        json!({"short_code": done.short_code, "to_board": "ops-delivery"}),
        "TASK_DONE",
    )
    .await;
    // 8. KAIROS-T-0675: the description names the tool that lists the
    //    columns and the moves.
    mcp.next_id += 1;
    let id = mcp.next_id;
    let (_, body) = mcp
        .post(json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}))
        .await;
    let tools = rpc_message(&body)["result"]["tools"].clone();
    let description = tools
        .as_array()
        .expect("tools")
        .iter()
        .find(|tool| tool["name"] == "transition_item")
        .and_then(|tool| tool["description"].as_str())
        .expect("transition_item description")
        .to_string();
    for part in [
        "A move goes one column at a time",
        "call `my_boards` first",
        "`reopen_item`",
    ] {
        assert!(description.contains(part), "{part}: {description}");
    }
    assert!(!description.contains("board_flow"), "{description}");
    // 8. KAIROS-T-0675: Completed has no transition out; the refusal
    //    names the reopen.
    let text = mcp
        .refused(
            "transition_item",
            json!({"short_code": done.short_code, "to_column": "Todo"}),
            "INVALID_TRANSITION",
        )
        .await;
    assert!(
        text.contains(
            "No transition starts at Completed. Completed is a done column. To move the task \
             out of it, call reopen_item."
        ),
        "{text}"
    );

    // 2. carol has no `transition_items` on the board.
    let err = carol
        .reopen_task(&done.short_code)
        .await
        .expect_err("carol cannot reopen");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // 3. The reopen: Completed has no edge out; the task goes to Backlog.
    assert_eq!(blocked_again(&mut conn, &waits.id), 0);
    let reopened = admin.reopen_task(&done.short_code).await.expect("reopen");
    assert_eq!(reopened.column_id, to("Backlog"));
    assert_eq!(reopened.board_id, web.board.id);
    assert_eq!(reopened.cancellation, None);
    assert_eq!(reopened.claim, None);
    let transitions = alice
        .activity(&ActivityQuery {
            entity_id: Some(done.id.clone()),
            action: Some("transition".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert!(
        transitions
            .iter()
            .any(|row| row.details == "column:Completed->Backlog"),
        "{transitions:?}"
    );
    // The blocker blocks again: alice created the dependant.
    assert_eq!(blocked_again(&mut conn, &waits.id), 1);
    refused_with(
        alice.reopen_task(&done.short_code).await,
        "TASK_NOT_DONE",
        422,
    );
    mcp.refused(
        "reopen_item",
        json!({"short_code": done.short_code}),
        "TASK_NOT_DONE",
    )
    .await;
    // After the reopen, the board move works.
    let moved = alice
        .move_task(&done.short_code, "ops-delivery")
        .await
        .expect("move after reopen");
    assert_ne!(moved.board_id, web.board.id);

    // 4. MCP reopen of a cancelled task removes the mark.
    let cancelled = alice
        .create_task(&new_task("Will not do"))
        .await
        .expect("task");
    let waits_too = alice
        .create_task(&new_task("Waits too"))
        .await
        .expect("task");
    alice
        .create_relationship(&blocks(&cancelled.short_code, &waits_too.short_code))
        .await
        .expect("blocks");
    alice
        .cancel_task(&cancelled.short_code, "No need.")
        .await
        .expect("cancel");
    let text = admin_mcp
        .ok("reopen_item", json!({"short_code": cancelled.short_code}))
        .await;
    assert!(
        text.contains(&format!(
            "Reopened {}: Completed -> Backlog.",
            cancelled.short_code
        )) && text.contains("The cancel mark is removed"),
        "{text}"
    );
    let got = alice.get_task(&cancelled.short_code).await.expect("get");
    assert_eq!(got.cancellation, None, "the mark is gone");
    assert_eq!(got.column_id, to("Backlog"));
    let cancels = alice
        .activity(&ActivityQuery {
            entity_id: Some(cancelled.id.clone()),
            action: Some("cancel".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert_eq!(cancels.len(), 1, "the history keeps the cancel");
    // A cancelled blocker never stopped blocking.
    assert_eq!(blocked_again(&mut conn, &waits_too.id), 0);

    // 5. parent_gate: the first open column holds claims, and the
    //    initiative is in Discovery.
    let initiative = admin_mcp
        .create(
            json!({"item_type": "initiative", "board": "initiatives", "title": "Discovery work"}),
        )
        .await;
    // 8. KAIROS-T-0675: the path from Discovery to Active.
    let text = admin_mcp
        .refused(
            "transition_item",
            json!({"short_code": initiative, "to_column": "Active"}),
            "INVALID_TRANSITION",
        )
        .await;
    assert!(
        text.contains(
            "The path to Active is Discovery → Design → Ready → Decompose → Active: call \
             transition_item for each step."
        ),
        "{text}"
    );
    let err = admin
        .transition_initiative(&initiative, &{
            let board = admin.get_board("initiatives").await.expect("board");
            board
                .columns
                .iter()
                .find(|c| c.name == "Active")
                .expect("Active")
                .id
                .clone()
        })
        .await
        .expect_err("refused");
    let Error::InvalidTransition { details, .. } = &err else {
        panic!("expected InvalidTransition, got {err:?}");
    };
    let path: Vec<&str> = details["path"]
        .as_array()
        .expect("path")
        .iter()
        .map(|c| c["name"].as_str().expect("name"))
        .collect();
    assert_eq!(
        path,
        ["Discovery", "Design", "Ready", "Decompose", "Active"],
        "{details}"
    );
    let gated = admin_mcp
        .create(json!({"item_type": "task", "board": "web-delivery", "title": "Gated", "parent": initiative}))
        .await;
    alice.cancel_task(&gated, "Not now.").await.expect("cancel");
    admin
        .update_column(
            &web.board.id,
            &to("Backlog"),
            &UpdateColumnRequest {
                name: None,
                position: None,
                is_done: None,
                claims: Some(true),
                keeps_claims: None,
                wip_limit: None,
            },
        )
        .await
        .expect("claims on Backlog");
    let text = refused_with(alice.reopen_task(&gated).await, "PARENT_NOT_ACTIVE", 422);
    assert!(text.contains(&initiative), "{text}");
    let got = alice.get_task(&gated).await.expect("get");
    assert_eq!(got.column_id, to("Completed"), "the task stays done");
    assert!(got.cancellation.is_some(), "the mark stays");
    admin_mcp
        .refused(
            "reopen_item",
            json!({"short_code": gated}),
            "PARENT_NOT_ACTIVE",
        )
        .await;

    // 6. A board with only done columns.
    let lab = admin.get_board("lab-delivery").await.expect("board");
    let stuck = alice
        .create_task(&CreateTaskRequest {
            board_id: Some(lab.board.id.clone()),
            ..new_task("Stuck")
        })
        .await
        .expect("task");
    alice
        .cancel_task(&stuck.short_code, "Gone.")
        .await
        .expect("cancel");
    diesel::sql_query("UPDATE org_acme.board_columns SET is_done = true WHERE board_id = $1")
        .bind::<diesel::sql_types::Uuid, _>(lab.board.id.parse::<Uuid>().expect("uuid"))
        .execute(&mut conn)
        .expect("all columns done");
    refused_with(
        alice.reopen_task(&stuck.short_code).await,
        "NO_OPEN_COLUMN",
        422,
    );

    // 7. Only a task.
    admin_mcp
        .refused(
            "reopen_item",
            json!({"short_code": initiative}),
            "VALIDATION",
        )
        .await;
}
