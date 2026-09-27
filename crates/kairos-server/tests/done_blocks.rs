//! Integration test for COLLIERY-T-0214: completed work does not block, and
//! is not blocked.
//!
//! The defect: a card in Completed showed "blocked by 1" when its blocker
//! was in Completed too. `blocks_summary` excluded archived work, with the
//! reasoning that a count on a card is a claim about work that can still
//! move, and did not apply the same reasoning to work in a terminal column.
//!
//! The rule is one rule, so this test reads it from every surface that
//! shows blocking and checks that they agree about the SAME edges:
//!
//! - REST `GET /api/boards/{id}/items` (`blocks_summary`, what the card and
//!   the portfolio read of blocked chains show),
//! - REST `GET /api/tasks/{code}/relationships` (the list, `done` marker),
//! - MCP `board_items` (the queue of an agent),
//! - MCP `get_item` (the relationship lines).
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_done_blocks_t0214_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::EntityKind;
use kairos_client::types_meta::RelatedItem;
use kairos_db::items::{self, CreateTask};
use kairos_db::models::{
    BoardLevel, NewOrganizationMember, OrgRole, RelationshipType, TaskType, WorkClass,
};
use kairos_db::schema::{board_columns, organization_members, organizations, tasks, users};
use kairos_db::{TenantPool, boards, graph, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_done_blocks_t0214_test";

/// Minimal MCP session over HTTP (initialize → initialized → tools/call);
/// the same shape as the one in tests/create_refusal.rs.
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
                    "clientInfo": {"name": "colliery-t0214-test", "version": "0.0.0"},
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

    /// Call a tool that must succeed; returns its text.
    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
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
        assert!(
            !result["isError"].as_bool().unwrap_or(false),
            "{tool} unexpectedly errored: {text}"
        );
        text
    }
}

fn column_id(conn: &mut PgConnection, board: Uuid, name: &str) -> Uuid {
    board_columns::table
        .filter(board_columns::board_id.eq(board))
        .filter(board_columns::name.eq(name))
        .filter(board_columns::deleted_at.is_null())
        .select(board_columns::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("column {name:?} not found: {e}"))
}

/// The one line of a text listing that names `code` as its subject (the
/// line starts with `- {code} `).
fn line_of<'a>(text: &'a str, code: &str) -> &'a str {
    text.lines()
        .find(|line| line.starts_with(&format!("- {code} ")))
        .unwrap_or_else(|| panic!("no line for {code} in:\n{text}"))
}

/// The one line of a `get_item` render that starts with `- {label}: `.
fn labelled<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.lines()
        .find(|line| line.starts_with(&format!("- {label}: ")))
}

/// `(short_code, done, archived)` of each neighbour in the `blocks` group
/// of one direction.
fn blocks_group(
    groups: &[kairos_client::types_meta::RelationshipGroup],
) -> Vec<(String, bool, bool)> {
    groups
        .iter()
        .filter(|group| group.relationship == "blocks")
        .flat_map(|group| group.items.iter())
        .map(|item: &RelatedItem| {
            (
                item.short_code.clone(),
                item.done,
                item.archived_at.is_some(),
            )
        })
        .collect()
}

#[tokio::test]
async fn completed_work_does_not_block_and_is_not_blocked() {
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
    let server = spawn_server(router).await;
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

    // --- the board and its cards --------------------------------------------
    // Written through the same services the handlers call; the READS below
    // go through the wire.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    // COLLIERY-T-0230: a delivery board always has a team.
    let platform_delivery_team = common::seed_team(
        &mut conn,
        "Platform Delivery Team",
        "platform-delivery-team",
    );
    let delivery = boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        Some(platform_delivery_team),
        None,
    )
    .expect("creating the delivery board")
    .id;
    let todo = column_id(&mut conn, delivery, "Todo");
    let active = column_id(&mut conn, delivery, "Active");
    let completed = column_id(&mut conn, delivery, "Completed");
    let task = |conn: &mut PgConnection, title: &str, column: Uuid| {
        items::create_task(
            conn,
            CreateTask {
                board_id: delivery,
                column_id: Some(column),
                title,
                content: "",
                task_type: TaskType::Task,
                work_class: WorkClass::Planned,
                repository_id: None,
            },
            svc_id,
        )
        .expect("task")
    };
    let link = |conn: &mut PgConnection, from: Uuid, to: Uuid| {
        graph::link_items(conn, from, to, RelationshipType::Blocks, svc_id).expect("linking");
    };

    // waiting (Todo) is blocked by one open, one completed and one archived
    // task. finished (Completed) is blocked by the completed task and by an
    // open one.
    let open_blocker = task(&mut conn, "Open blocker", active);
    let done_blocker = task(&mut conn, "Completed blocker", completed);
    let archived_blocker = task(&mut conn, "Archived blocker", active);
    let waiting = task(&mut conn, "Waiting", todo);
    let finished = task(&mut conn, "Finished", completed);
    let open_blocker_of_finished = task(&mut conn, "Open, blocks finished work", active);
    link(&mut conn, open_blocker.id, waiting.id);
    link(&mut conn, done_blocker.id, waiting.id);
    link(&mut conn, archived_blocker.id, waiting.id);
    link(&mut conn, done_blocker.id, finished.id);
    link(&mut conn, open_blocker_of_finished.id, finished.id);
    diesel::update(tasks::table.find(archived_blocker.id))
        .set(tasks::deleted_at.eq(diesel::dsl::now))
        .execute(&mut conn)
        .expect("archiving a blocker");

    let mut mcp = McpSession::open(&server.base_url, &svc_token).await;

    // =======================================================================
    // REST board items: the counts on the cards
    // =======================================================================
    let board = svc
        .board_items(&delivery.to_string())
        .await
        .expect("board items");
    let counts = |code: &str| {
        board
            .blocks_summary
            .get(code)
            .map(|c| (c.blocked_by, c.blocks))
    };
    assert_eq!(
        counts(&waiting.short_code),
        Some((1, 0)),
        "three blockers, one of them open: the card says 1: {:?}",
        board.blocks_summary
    );
    assert_eq!(counts(&open_blocker.short_code), Some((0, 1)));
    assert_eq!(
        counts(&done_blocker.short_code),
        None,
        "a card in a terminal column shows no `blocks` count"
    );
    assert_eq!(
        counts(&finished.short_code),
        None,
        "a card in a terminal column shows no `blocked by` count, even \
         though one of its blockers is open"
    );
    assert_eq!(
        counts(&open_blocker_of_finished.short_code),
        None,
        "an item that blocks only completed work is in nobody's way"
    );

    // =======================================================================
    // REST relationships: the edge stays, the done end is marked
    // =======================================================================
    let rels = svc
        .relationships(EntityKind::Task, &waiting.short_code)
        .await
        .expect("relationships of the waiting task");
    assert_eq!(
        blocks_group(&rels.incoming),
        vec![
            (open_blocker.short_code.clone(), false, false),
            (done_blocker.short_code.clone(), true, false),
            (archived_blocker.short_code.clone(), false, true),
        ],
        "every edge is still listed; the completed blocker is marked done \
         and the archived one archived: {rels:?}"
    );
    let rels = svc
        .relationships(EntityKind::Task, &open_blocker_of_finished.short_code)
        .await
        .expect("relationships of the blocker of finished work");
    assert_eq!(
        blocks_group(&rels.outgoing),
        vec![(finished.short_code.clone(), true, false)],
        "{rels:?}"
    );

    // =======================================================================
    // MCP board_items: the queue of an agent
    // =======================================================================
    let text = mcp
        .call_ok("board_items", json!({"board": "platform-delivery"}))
        .await;
    let line = line_of(&text, &waiting.short_code);
    assert!(
        line.contains("[blocked by 1]"),
        "the open blocker, and only it, is counted: {line:?}\n{text}"
    );
    let line = line_of(&text, &open_blocker.short_code);
    assert!(line.contains("[blocks 1]"), "{line:?}");
    assert!(!line.contains("blocked by"), "{line:?}");
    for code in [
        &done_blocker.short_code,
        &finished.short_code,
        &open_blocker_of_finished.short_code,
    ] {
        let line = line_of(&text, code);
        assert!(
            !line.contains("blocked by") && !line.contains("[blocks"),
            "no open edge touches {code}, so its line says nothing about \
             blocking: {line:?}"
        );
    }

    // =======================================================================
    // MCP get_item: a completed blocker reads as resolved, not as open
    // =======================================================================
    let text = mcp
        .call_ok("get_item", json!({"short_code": waiting.short_code}))
        .await;
    let line = labelled(&text, "blocked by").unwrap_or_else(|| panic!("no blockers in:\n{text}"));
    assert_eq!(
        line,
        format!(
            "- blocked by: {} — Open blocker; {} — Completed blocker [done]; \
             {} — Archived blocker [archived]",
            open_blocker.short_code, done_blocker.short_code, archived_blocker.short_code
        ),
        "{text}"
    );
    // Read from the blocker: the completed item it blocks is marked too.
    let text = mcp
        .call_ok(
            "get_item",
            json!({"short_code": open_blocker_of_finished.short_code}),
        )
        .await;
    assert_eq!(
        labelled(&text, "blocks"),
        Some(format!("- blocks: {} — Finished [done]", finished.short_code).as_str()),
        "{text}"
    );
    // Read from the completed item: nothing blocks it, whatever its
    // blockers are doing, and the label says so.
    let text = mcp
        .call_ok("get_item", json!({"short_code": finished.short_code}))
        .await;
    assert_eq!(labelled(&text, "blocked by"), None, "{text}");
    assert_eq!(
        labelled(&text, "blocked by (resolved: this item is done)"),
        Some(
            format!(
                "- blocked by (resolved: this item is done): {} — Completed blocker [done]; \
                 {} — Open, blocks finished work",
                done_blocker.short_code, open_blocker_of_finished.short_code
            )
            .as_str()
        ),
        "{text}"
    );

    // =======================================================================
    // The blocker is reopened: every surface counts it again
    // =======================================================================
    // The default delivery graph has no way out of Completed, so the board
    // gains one. Nothing about the edge is stored; the column decides.
    boards::add_transition(&mut conn, delivery, completed, active, svc_id)
        .expect("the board permits Completed -> Active");
    let text = mcp
        .call_ok(
            "transition_item",
            json!({"short_code": done_blocker.short_code, "to_column": "Active"}),
        )
        .await;
    assert!(text.contains("Active"), "{text}");

    let board = svc
        .board_items(&delivery.to_string())
        .await
        .expect("board items after the reopen");
    let counts = |code: &str| {
        board
            .blocks_summary
            .get(code)
            .map(|c| (c.blocked_by, c.blocks))
    };
    assert_eq!(counts(&waiting.short_code), Some((2, 0)));
    assert_eq!(
        counts(&done_blocker.short_code),
        Some((0, 1)),
        "it blocks the waiting task again, and still not the finished one"
    );
    assert_eq!(counts(&finished.short_code), None);

    let text = mcp
        .call_ok("board_items", json!({"board": "platform-delivery"}))
        .await;
    assert!(
        line_of(&text, &waiting.short_code).contains("[blocked by 2]"),
        "{text}"
    );
    let text = mcp
        .call_ok("get_item", json!({"short_code": waiting.short_code}))
        .await;
    assert_eq!(
        labelled(&text, "blocked by"),
        Some(
            format!(
                "- blocked by: {} — Open blocker; {} — Completed blocker; \
                 {} — Archived blocker [archived]",
                open_blocker.short_code, done_blocker.short_code, archived_blocker.short_code
            )
            .as_str()
        ),
        "the marker is gone with the column: {text}"
    );

    drop(mcp);
    drop(conn);
    drop(server);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
