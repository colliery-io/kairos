//! Integration test for COLLIERY-T-3101: a move to another board can
//! rename the item, and the links change one time, on each surface.
//!
//! - MCP `move_item` with `rename`: the task gets the next code of the
//!   target board, the text of the other items names the new code (a URL
//!   does not change), `get_item` with the old code finds the task, and
//!   `get_history` lists the rename with the old code, the new code and who
//!   did it.
//! - REST `POST /api/tasks/{code}/move`: with no `rename` the code does not
//!   change; with `rename` the response has the new code. An unknown field
//!   is still refused and named.
//! - A document: `move_item` with `rename` and `to_board`; a rename with no
//!   board, or to the prefix that the code has, is refused.
//! - `PATCH /api/boards/{id}` with `code_prefix` gets 422
//!   `CODE_PREFIX_IS_FIXED`.
//!
//! The database part is in `kairos-db/tests/code_rename.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_rename_on_move_t3101_test`.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    spawn_server, user_token, with_database,
};
use kairos_db::items::{self, CreateDocument, CreateTask};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole, TaskType, WorkClass};
use kairos_db::schema::{boards as boards_table, organization_members, organizations, users};
use kairos_db::{TenantPool, boards, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_rename_on_move_t3101_test";

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
                    "clientInfo": {"name": "colliery-t3101-test", "version": "0.0.0"},
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

fn create_task(
    conn: &mut PgConnection,
    board: Uuid,
    actor: Uuid,
    title: &str,
    content: &str,
) -> String {
    items::create_task(
        conn,
        CreateTask {
            board_id: board,
            column_id: None,
            title,
            content,
            task_type: TaskType::Task,
            work_class: WorkClass::Planned,
            repository_id: None,
        },
        actor,
    )
    .expect("creating a task")
    .short_code
}

fn board_of_level(conn: &mut PgConnection, level: &str) -> (Uuid, String) {
    boards_table::table
        .filter(boards_table::board_level.eq(level))
        .filter(boards_table::deleted_at.is_null())
        .select((boards_table::id, boards_table::slug))
        .first(conn)
        .unwrap_or_else(|e| panic!("a board of level {level}: {e}"))
}

#[tokio::test]
async fn a_move_can_rename_the_item_and_the_links_change_one_time() {
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
    let (svc_id, svc_name): (Uuid, String) = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select((users::id, users::display_name))
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
    let skadi = boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Skadi",
        "skadi",
        kairos_db::CodePrefix::Given("SKADI"),
        Some(skadi_team),
        None,
    )
    .expect("creating skadi")
    .id;
    let crt_team = common::seed_team(&mut conn, "Crt", "crt");
    boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Crt",
        "crt",
        kairos_db::CodePrefix::Given("CRT"),
        Some(crt_team),
        None,
    )
    .expect("creating crt");
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
    .bind::<BigInt, _>(99)
    .execute(&mut conn)
    .expect("setting the sequence");
    let moved = create_task(
        &mut conn,
        colliery,
        svc_id,
        "Find the downloads",
        "The worker finds them.",
    );
    assert_eq!(moved, "COLLIERY-T-0100");
    let other = create_task(
        &mut conn,
        colliery,
        svc_id,
        "After the downloads",
        "Blocked by COLLIERY-T-0100. The notes are at https://example.com/COLLIERY-T-0100.md.",
    );
    let kept = create_task(&mut conn, colliery, svc_id, "Keep my code", "");

    let mut mcp = McpSession::open(&server.base_url, &svc_token).await;
    let tenant = [("X-Tenant", "acme")];

    // =======================================================================
    // Scenario: A move with rename gives a new code and updates the
    // references (MCP)
    // Scenario: A code in a URL does not change
    // =======================================================================
    let text = mcp
        .call_ok(
            "move_item",
            json!({"short_code": moved, "to_board": "skadi", "rename": true}),
        )
        .await;
    assert!(
        text.contains("Renamed COLLIERY-T-0100 -> SKADI-T-0001."),
        "the answer names the new code:\n{text}"
    );
    assert!(
        text.contains(&other),
        "the answer names the item that changed:\n{text}"
    );

    let text = mcp.call_ok("get_item", json!({"short_code": other})).await;
    assert!(
        text.contains(
            "Blocked by SKADI-T-0001. The notes are at https://example.com/COLLIERY-T-0100.md."
        ),
        "the other task names the new code, and the URL does not change:\n{text}"
    );

    // COLLIERY-T-0100 is retired, and get_item for it finds the moved task.
    let text = mcp
        .call_ok("get_item", json!({"short_code": "COLLIERY-T-0100"}))
        .await;
    assert!(
        text.starts_with("# SKADI-T-0001 — Find the downloads\n"),
        "{text}"
    );
    assert!(text.contains("- board: skadi"), "{text}");

    // =======================================================================
    // Scenario: The history records the rename
    // =======================================================================
    let text = mcp
        .call_ok("get_history", json!({"short_code": "SKADI-T-0001"}))
        .await;
    let line = text
        .lines()
        .skip_while(|line| *line != "## Renames")
        .nth(1)
        .unwrap_or_else(|| panic!("no renames section:\n{text}"));
    assert!(
        line.starts_with("- COLLIERY-T-0100 -> SKADI-T-0001 — ")
            && line.ends_with(&format!(" by {svc_name}")),
        "the history has the old code, the new code, and who did it: {line:?}\n{text}"
    );
    // An item with no rename has no section.
    let text = mcp
        .call_ok("get_history", json!({"short_code": other}))
        .await;
    assert!(!text.contains("## Renames"), "{text}");

    // An unknown argument of move_item is refused and named.
    let (text, is_error) = mcp
        .call(
            "move_item",
            json!({"short_code": kept, "to_board": "skadi", "renamed": true}),
        )
        .await;
    assert!(is_error, "an unknown argument is refused: {text}");
    assert!(text.contains("renamed"), "the refusal names it: {text}");

    // =======================================================================
    // Scenario: A move with no rename keeps the code (REST)
    // =======================================================================
    let (status, body) = request(
        &router,
        Method::POST,
        &format!("/api/tasks/{kept}/move"),
        Some(&svc_token),
        &tenant,
        Some(json!({"board": "skadi"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["short_code"],
        kept.as_str(),
        "its code does not change"
    );
    assert_eq!(body["board_id"], skadi.to_string());

    // REST with rename, back to the board whose prefix the code has: the
    // rename would use up a code and change nothing, so it is refused, and
    // the task does not move.
    let (status, body) = request(
        &router,
        Method::POST,
        &format!("/api/tasks/{kept}/move"),
        Some(&svc_token),
        &tenant,
        Some(json!({"board": "colliery-io-delivery", "rename": true})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "RENAME_NOT_NEEDED", "{body}");
    let (status, body) = request(
        &router,
        Method::GET,
        &format!("/api/tasks/{kept}"),
        Some(&svc_token),
        &tenant,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["board_id"],
        skadi.to_string(),
        "the refused move left it on skadi"
    );

    // REST with rename: the response has the new code.
    let (status, body) = request(
        &router,
        Method::POST,
        &format!("/api/tasks/{kept}/move"),
        Some(&svc_token),
        &tenant,
        Some(json!({"board": "crt", "rename": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let renamed = body["short_code"].as_str().unwrap_or_default().to_string();
    assert_eq!(renamed, "CRT-T-0001", "{body}");
    // The client mirror.
    let task = svc
        .move_task_with(&renamed, "skadi", true)
        .await
        .expect("the typed move");
    assert_eq!(task.short_code, "SKADI-T-0002");

    // An unknown field of the body is refused and named.
    let (status, body) = request(
        &router,
        Method::POST,
        "/api/tasks/SKADI-T-0002/move",
        Some(&svc_token),
        &tenant,
        Some(json!({"board": "colliery-io-delivery", "renames": true})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["details"]["field"], "renames", "{body}");

    // =======================================================================
    // A document: a rename on a move to a different owner board
    // =======================================================================
    let (initiative_board, _) = board_of_level(&mut conn, "initiative");
    let (_, strategy_slug) = board_of_level(&mut conn, "strategy");
    let document = items::create_document(
        &mut conn,
        CreateDocument {
            board_id: initiative_board,
            title: "The design",
            content: Some("A note."),
            template_id: None,
        },
        svc_id,
    )
    .expect("creating a document");
    assert!(
        document.short_code.starts_with("ACME-D-"),
        "{}",
        document.short_code
    );
    create_task(
        &mut conn,
        colliery,
        svc_id,
        "Read the design",
        &format!("See {}.", document.short_code),
    );

    // No board: refused, and nothing changes.
    let (text, is_error) = mcp
        .call(
            "move_item",
            json!({"short_code": document.short_code, "rename": true}),
        )
        .await;
    assert!(is_error, "{text}");
    assert!(text.contains("to_board"), "{text}");

    // A board with the prefix that the code has: RENAME_NOT_NEEDED, and the
    // owner board does not change either.
    let (text, is_error) = mcp
        .call(
            "move_item",
            json!({"short_code": document.short_code, "to_board": strategy_slug, "rename": true}),
        )
        .await;
    assert!(is_error, "{text}");
    assert!(
        text.contains("RENAME_NOT_NEEDED") || text.contains("prefix ACME"),
        "{text}"
    );
    let text = mcp
        .call_ok("get_item", json!({"short_code": document.short_code}))
        .await;
    assert!(
        !text.contains(&strategy_slug),
        "the move was undone too:\n{text}"
    );

    let text = mcp
        .call_ok(
            "move_item",
            json!({
                "short_code": document.short_code,
                "to_board": "colliery-io-delivery",
                "rename": true,
            }),
        )
        .await;
    assert!(
        text.contains(&format!(
            "Renamed {} -> COLLIERY-D-0001.",
            document.short_code
        )),
        "{text}"
    );
    let found = mcp.call_ok("search", json!({"q": "Read the design"})).await;
    assert!(found.contains("Read the design"), "{found}");
    let (status, body) = request(
        &router,
        Method::GET,
        &format!("/api/documents/{}", document.short_code),
        Some(&svc_token),
        &tenant,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["short_code"], "COLLIERY-D-0001");

    // =======================================================================
    // A board prefix does not change
    // =======================================================================
    let (status, body) = request(
        &router,
        Method::PATCH,
        &format!("/api/boards/{skadi}"),
        Some(&svc_token),
        &tenant,
        Some(json!({"code_prefix": "SKD"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "CODE_PREFIX_IS_FIXED", "{body}");
    assert_eq!(body["error"]["details"]["field"], "code_prefix");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("does not change"), "{message}");
    // With a name too: still refused, and the name does not change.
    let (status, body) = request(
        &router,
        Method::PATCH,
        &format!("/api/boards/{skadi}"),
        Some(&svc_token),
        &tenant,
        Some(json!({"name": "Skadi 2", "code_prefix": "SKADI"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let name: String = boards_table::table
        .filter(boards_table::id.eq(skadi))
        .select(boards_table::name)
        .first(&mut conn)
        .expect("the board");
    assert_eq!(name, "Skadi");

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
