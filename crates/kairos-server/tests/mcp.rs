//! Integration test for the `/mcp` MCP endpoint (KAIROS-T-0026, contracts
//! per KAIROS-A-0011 / KAIROS-S-0006): a real MCP session over streamable
//! HTTP against the production router, authenticated with a REAL Dex token
//! (never forged), exercising the frozen S-0006 tool surface end to end.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer. For isolation the test owns the
//! uniquely named scratch database (`kairos_mcp_t0026_test`); the shared
//! `kairos` database is never touched (shared-services discipline).
//!
//! Transport: the raw streamable-HTTP protocol — JSON-RPC over POST with
//! `Accept: application/json, text/event-stream`, the `Mcp-Session-Id`
//! header from `initialize`, and SSE-framed responses — driven in-process
//! via `tower::ServiceExt::oneshot`, so the exact production `/mcp`
//! mounting (auth → tenant middleware included) is what's under test.
//!
//! Coverage map (S-0006):
//! - initialize reports the server version (REQ-1.7); 401 pre-session
//!   without a token (with the RFC 9728 WWW-Authenticate challenge) and
//!   403 for an authenticated non-member — the SAME middleware as /api;
//! - tools/list is EXACTLY the 16-tool inventory (14 from S-0006 plus the
//!   two repository tools of KAIROS-T-0107);
//! - golden path: whoami → my_boards → create_item(initiative) →
//!   create_item(task, parent) → get_item → edit_item → transition_item
//!   (invalid first: INVALID_TRANSITION enumerating allowed targets,
//!   REQ-1.4; then valid) → board_items → search → get_history →
//!   update_item stale-version conflict carrying the current state
//!   (REQ-1.5) → delete_item (confirm required; cascade listed) →
//!   board_items reflects the delete;
//! - ABAC parity (REQ-1.1): a non-admin member is FORBIDDEN from the
//!   the org-admin-gated non-collaborative link_items (and MAY write the
//!   collaborative blocks edge, KAIROS-T-0111);
//! - NFR-1.3: the tool calls landed in `activity_log` exactly like API
//!   calls (asserted straight from the scratch database).

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text as SqlText, Uuid as SqlUuid};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, user_token, with_database,
};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{boards, organization_members, organizations, users};
use kairos_db::{TenantPool, abac, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_mcp_t0026_test";

/// The tenant slug.
const TENANT: &str = "acme";

/// Every request carries a real `Host` header and the tenant resolves from
/// its subdomain against the configured base domain — the S-0006 REQ-1.2
/// path (tenant from the connection host, no tenant parameter anywhere).
const HOST_HEADER: &str = "acme.kairos.test";

// ---------------------------------------------------------------------------
// Streamable-HTTP plumbing
// ---------------------------------------------------------------------------

/// One in-process request against `/mcp` (or any URI): returns status, the
/// response headers, and the raw body string (SSE or JSON).
async fn raw_request(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    session: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", HOST_HEADER)
        .header("accept", "application/json, text/event-stream");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, headers, String::from_utf8_lossy(&bytes).to_string())
}

/// Extract the JSON-RPC message from a streamable-HTTP response body:
/// either plain JSON or SSE framing (`data:` lines; priming events carry
/// no JSON-RPC payload and are skipped).
fn rpc_message(body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return value;
    }
    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data:")
            && let Ok(value) = serde_json::from_str::<Value>(data.trim())
            && value.get("jsonrpc").is_some()
        {
            return value;
        }
    }
    panic!("no JSON-RPC message in response body: {body:?}");
}

/// An MCP session over the production router: POSTs JSON-RPC to `/mcp`
/// with the bearer token and (after initialize) the session id.
struct McpSession<'a> {
    router: &'a Router,
    token: String,
    session_id: Option<String>,
    next_id: i64,
}

impl<'a> McpSession<'a> {
    /// Drive `initialize` + `notifications/initialized`; returns the
    /// InitializeResult.
    async fn connect(router: &'a Router, token: &str) -> (McpSession<'a>, Value) {
        let mut session = McpSession {
            router,
            token: token.to_string(),
            session_id: None,
            next_id: 0,
        };
        let (status, headers, body) = raw_request(
            router,
            Method::POST,
            "/mcp",
            Some(token),
            None,
            Some(json!({
                "jsonrpc": "2.0",
                "id": 0,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "kairos-t0026-test", "version": "0.0.0"},
                },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "initialize failed: {body}");
        let session_id = headers
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .expect("initialize returns Mcp-Session-Id")
            .to_string();
        session.session_id = Some(session_id);
        let init = rpc_message(&body);

        // The handshake completes with notifications/initialized (202).
        let (status, _, body) = raw_request(
            router,
            Method::POST,
            "/mcp",
            Some(&session.token),
            session.session_id.as_deref(),
            Some(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "initialized notify: {body}");
        (session, init)
    }

    /// One JSON-RPC request within the session; returns the `result`.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let (status, _, body) = raw_request(
            self.router,
            Method::POST,
            "/mcp",
            Some(&self.token),
            self.session_id.as_deref(),
            Some(json!({
                "jsonrpc": "2.0",
                "id": self.next_id,
                "method": method,
                "params": params,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{method} HTTP status: {body}");
        let message = rpc_message(&body);
        assert!(
            message.get("error").is_none(),
            "{method} returned a protocol error: {message}"
        );
        message["result"].clone()
    }

    /// Call a tool; returns `(is_error, text)` from the CallToolResult.
    async fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        let result = self
            .request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await;
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("tool {tool} returned no text content: {result}"))
            .to_string();
        (is_error, text)
    }

    /// Call a tool and require success, returning the text.
    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(!is_error, "{tool} unexpectedly errored: {text}");
        text
    }

    /// Call a tool and require a tool error, returning the text.
    async fn call_err(&mut self, tool: &str, arguments: Value) -> String {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(is_error, "{tool} unexpectedly succeeded: {text}");
        text
    }
}

/// The first short code with `prefix` in `text` (e.g. prefix `"ACME-I-"`).
fn extract_code(text: &str, prefix: &str) -> String {
    let start = text
        .find(prefix)
        .unwrap_or_else(|| panic!("no {prefix} short code in {text:?}"));
    text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

// ---------------------------------------------------------------------------
// Scratch-tenant scaffolding
// ---------------------------------------------------------------------------

/// `public.users.id` by email (JIT-provisioned by a first request).
fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

/// The tenant board of a level.
fn board_id_of(conn: &mut PgConnection, level: BoardLevel) -> Uuid {
    boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level} board: {e}"))
}

#[derive(QueryableByName)]
struct NameRow {
    #[diesel(sql_type = SqlText)]
    name: String,
}

/// Column names of a board in position order.
fn column_names(conn: &mut PgConnection, board: Uuid) -> Vec<String> {
    sql_query("SELECT name FROM org_acme.board_columns WHERE board_id = $1 ORDER BY position ASC")
        .bind::<SqlUuid, _>(board)
        .load::<NameRow>(conn)
        .expect("board columns")
        .into_iter()
        .map(|r| r.name)
        .collect()
}

/// Column names reachable from the FIRST column per `board_transitions`.
fn reachable_from_first(conn: &mut PgConnection, board: Uuid) -> Vec<String> {
    sql_query(
        "SELECT c_to.name FROM org_acme.board_transitions t \
         JOIN org_acme.board_columns c_from ON c_from.id = t.from_column_id \
         JOIN org_acme.board_columns c_to ON c_to.id = t.to_column_id \
         WHERE t.board_id = $1 AND c_from.position = 0 \
         ORDER BY c_to.position ASC",
    )
    .bind::<SqlUuid, _>(board)
    .load::<NameRow>(conn)
    .expect("board transitions")
    .into_iter()
    .map(|r| r.name)
    .collect()
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// `activity_log` rows in the scratch tenant for one action + actor.
fn activity_count(conn: &mut PgConnection, actor: Uuid, action: &str) -> i64 {
    sql_query(
        "SELECT COUNT(*) AS n FROM org_acme.activity_log \
         WHERE actor_id = $1 AND action = $2",
    )
    .bind::<SqlUuid, _>(actor)
    .bind::<SqlText, _>(action)
    .get_result::<CountRow>(conn)
    .expect("activity count")
    .n
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_endpoint_against_live_stack() {
    // --- scratch database + tenant -----------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, TENANT, "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq(TENANT))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    // --- router + live token ------------------------------------------------
    let http = reqwest::Client::new();
    let alice_token = user_token(&http, "alice").await;
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

    // --- RFC 9728 protected-resource metadata (unauthenticated) -------------
    let (status, _, body) = raw_request(
        &router,
        Method::GET,
        "/.well-known/oauth-protected-resource/mcp",
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let metadata: Value = serde_json::from_str(&body).expect("metadata is JSON");
    assert_eq!(metadata["authorization_servers"][0], ISSUER);
    assert!(
        metadata["resource"]
            .as_str()
            .expect("resource")
            .ends_with("/mcp"),
        "{metadata}"
    );

    // --- auth: no token → 401 pre-session, with the RFC 9728 challenge ------
    let (status, headers, body) = raw_request(
        &router,
        Method::POST,
        "/mcp",
        None,
        None,
        Some(
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "t", "version": "0"}}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let challenge = headers
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .expect("401 carries WWW-Authenticate");
    assert!(
        challenge.contains("resource_metadata=") && challenge.contains("/mcp"),
        "unexpected challenge: {challenge}"
    );

    // --- auth: authenticated NON-member → 403 (JIT-provisions the user) -----
    let (status, _, body) = raw_request(
        &router,
        Method::POST,
        "/mcp",
        Some(&alice_token),
        None,
        Some(
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "t", "version": "0"}}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("MEMBERSHIP_REQUIRED"), "{body}");

    // --- membership + boards + explicit capability grants (A-0006) ----------
    // alice is a plain MEMBER (no admin bypass): the write tools below prove
    // the capability path, and link_items(informs) proves the org-admin denial.
    let alice = user_id(&mut conn, "alice@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("granting alice membership");

    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let delivery = kairos_db::boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        None,
        None,
    )
    .expect("creating the delivery board");
    let initiative_board = board_id_of(&mut conn, BoardLevel::Initiative);
    abac::grant_capability(
        &mut conn,
        initiative_board,
        alice,
        "manage_initiatives",
        alice,
    )
    .expect("grant manage_initiatives");
    abac::grant_capability(&mut conn, delivery.id, alice, "manage_tasks", alice)
        .expect("grant manage_tasks");
    abac::grant_capability(&mut conn, delivery.id, alice, "transition_items", alice)
        .expect("grant transition_items");
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");

    let columns = column_names(&mut conn, delivery.id);
    let reachable = reachable_from_first(&mut conn, delivery.id);
    let first_column = columns.first().expect("delivery board has columns").clone();
    let valid_target = reachable
        .first()
        .expect("default boards allow a move out of the first column")
        .clone();
    let invalid_target = columns
        .iter()
        .find(|c| **c != first_column && !reachable.contains(c))
        .expect("default boards are not complete graphs")
        .clone();

    // --- initialize: session established, server version reported (REQ-1.7) -
    let (mut session, init) = McpSession::connect(&router, &alice_token).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "kairos");
    assert_eq!(
        init["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION"),
        "initialize reports the server version"
    );

    // --- tools/list: EXACTLY the S-0006 inventory ----------------------------
    let listed = session.request("tools/list", json!({})).await;
    let mut names: Vec<String> = listed["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().expect("tool name").to_string())
        .collect();
    names.sort();
    let mut expected = vec![
        "whoami",
        "my_boards",
        "board_items",
        "get_item",
        "get_history",
        "search",
        "create_item",
        "update_item",
        "edit_item",
        "transition_item",
        "link_items",
        "unlink_items",
        "set_metadata",
        "delete_item",
        // KAIROS-T-0107 (A-0019): the repository directory.
        "list_repositories",
        "get_repository",
    ];
    expected.sort_unstable();
    assert_eq!(names, expected, "tools/list is exactly the S-0006 surface");

    // --- whoami: identity, org, capability grants ----------------------------
    let text = session.call_ok("whoami", json!({})).await;
    assert!(text.contains("alice@kairos.test"), "{text}");
    assert!(text.contains("acme"), "{text}");
    assert!(text.contains("role: member"), "{text}");
    assert!(text.contains("manage_tasks"), "{text}");
    assert!(text.contains("platform-delivery"), "{text}");

    // --- my_boards: grouped by level; the user's delivery board shows columns
    let text = session.call_ok("my_boards", json!({})).await;
    assert!(text.contains("## delivery"), "{text}");
    assert!(text.contains("platform-delivery"), "{text}");
    assert!(text.contains("columns:"), "{text}");
    assert!(text.contains(&format!("{first_column} (0)")), "{text}");

    // --- create_item: initiative (board defaulted), then a child task -------
    let text = session
        .call_ok(
            "create_item",
            json!({"item_type": "initiative", "title": "Time circuits online"}),
        )
        .await;
    assert!(text.contains("Created initiative"), "{text}");
    let initiative_code = extract_code(&text, "ACME-I-");

    let text = session
        .call_ok(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Recalibrate the kondensator",
                "content": "The kondensator drifts under load.",
                "task_type": "bug",
                "parent": initiative_code,
            }),
        )
        .await;
    assert!(text.contains("Created task"), "{text}");
    assert!(text.contains("platform-delivery"), "{text}");
    assert!(
        text.contains(&format!("parent: {initiative_code}")),
        "{text}"
    );
    let task_code = extract_code(&text, "ACME-T-");

    // --- get_item: full content, placement, version, parent chain -----------
    let text = session
        .call_ok("get_item", json!({"short_code": task_code}))
        .await;
    assert!(text.contains(&task_code), "{text}");
    assert!(text.contains("type: task (bug)"), "{text}");
    assert!(text.contains("version: 1"), "{text}");
    assert!(text.contains("board: platform-delivery"), "{text}");
    assert!(text.contains("kondensator drifts"), "{text}");
    assert!(text.contains(&initiative_code), "parent chain: {text}");

    // --- edit_item: targeted server-side search/replace → version 2 ---------
    let text = session
        .call_err(
            "edit_item",
            json!({"short_code": task_code, "search": "no such text", "replace": "x"}),
        )
        .await;
    assert!(text.contains("VALIDATION"), "{text}");
    assert!(text.contains("not found"), "{text}");

    let text = session
        .call_ok(
            "edit_item",
            json!({
                "short_code": task_code,
                "search": "drifts under load",
                "replace": "drifts under load; recalibrate to 1.21 GW",
            }),
        )
        .await;
    assert!(text.contains("version 2"), "{text}");
    assert!(text.contains("1 replacement"), "{text}");

    // --- transition_item: invalid first (allowed targets enumerated) --------
    let text = session
        .call_err(
            "transition_item",
            json!({"short_code": task_code, "to_column": invalid_target}),
        )
        .await;
    assert!(text.contains("INVALID_TRANSITION"), "{text}");
    assert!(text.contains("allowed_targets"), "{text}");
    assert!(
        text.contains(&valid_target),
        "allowed targets name {valid_target}: {text}"
    );

    let text = session
        .call_ok(
            "transition_item",
            json!({"short_code": task_code, "to_column": valid_target}),
        )
        .await;
    assert!(
        text.contains(&format!("{first_column} -> {valid_target}")),
        "{text}"
    );

    // --- board_items: the task sits in the transitioned column --------------
    let text = session
        .call_ok("board_items", json!({"board": "platform-delivery"}))
        .await;
    assert!(text.contains(&task_code), "{text}");
    let column_section = text
        .split("## ")
        .find(|s| s.starts_with(&valid_target))
        .expect("target column section");
    assert!(column_section.contains(&task_code), "{text}");

    // --- search: full-text finds the task (compact listing) -----------------
    let text = session.call_ok("search", json!({"q": "kondensator"})).await;
    assert!(text.contains(&task_code), "{text}");
    assert!(text.contains("[bug]"), "{text}");
    assert!(
        !text.contains("drifts under load"),
        "search must stay compact (no content): {text}"
    );

    // --- get_history: v2 + v1 with the editor named -------------------------
    let text = session
        .call_ok("get_history", json!({"short_code": task_code}))
        .await;
    assert!(text.contains("current version 2"), "{text}");
    assert!(text.contains("- v2 —"), "{text}");
    assert!(text.contains("- v1 —"), "{text}");
    assert!(text.contains("by alice"), "{text}");

    // ...and one full snapshot by version.
    let text = session
        .call_ok(
            "get_history",
            json!({"short_code": task_code, "version": 1}),
        )
        .await;
    assert!(text.contains("v1"), "{text}");
    assert!(
        text.contains("The kondensator drifts under load."),
        "{text}"
    );

    // --- update_item: stale version → CONFLICT with current state (REQ-1.5) -
    let text = session
        .call_err(
            "update_item",
            json!({
                "short_code": task_code,
                "content": "clobbering write",
                "version": 1,
            }),
        )
        .await;
    assert!(text.contains("CONFLICT"), "{text}");
    assert!(text.contains("\"version\":2"), "current version: {text}");
    assert!(
        text.contains("recalibrate to 1.21 GW"),
        "current content for reconciliation: {text}"
    );

    // --- ABAC parity: non-collaborative link_items is org-admin-gated; alice
    // is a member. The collaborative `blocks` (KAIROS-T-0111) she may write
    // since she manages both boards.
    let text = session
        .call_err(
            "link_items",
            json!({"source": initiative_code, "target": task_code, "relationship": "informs"}),
        )
        .await;
    assert!(text.contains("FORBIDDEN"), "{text}");
    let text = session
        .call_ok(
            "link_items",
            json!({"source": initiative_code, "target": task_code, "relationship": "blocks"}),
        )
        .await;
    assert!(text.contains("Linked"), "{text}");
    let text = session
        .call_ok(
            "unlink_items",
            json!({"source": initiative_code, "target": task_code, "relationship": "blocks"}),
        )
        .await;
    assert!(text.contains("Unlinked"), "{text}");

    // --- delete_item: confirm required; cascade listed -----------------------
    let text = session
        .call_err(
            "delete_item",
            json!({"short_code": initiative_code, "confirm": false}),
        )
        .await;
    assert!(text.contains("VALIDATION"), "{text}");
    assert!(text.contains("confirm=true"), "{text}");

    let text = session
        .call_ok(
            "delete_item",
            json!({"short_code": initiative_code, "confirm": true}),
        )
        .await;
    assert!(
        text.contains(&format!("Deleted {initiative_code}")),
        "{text}"
    );
    assert!(text.contains("Cascade deleted 1"), "{text}");
    assert!(text.contains(&task_code), "cascade names the task: {text}");

    // --- board_items reflects the cascade delete ----------------------------
    let text = session
        .call_ok("board_items", json!({"board": "platform-delivery"}))
        .await;
    assert!(!text.contains(&task_code), "{text}");

    // ...and the deleted item is now NOT_FOUND (REQ-1.1 code parity).
    let text = session
        .call_err("get_item", json!({"short_code": task_code}))
        .await;
    assert!(text.contains("NOT_FOUND"), "{text}");

    // --- NFR-1.3: every mutation above landed in activity_log ---------------
    // 3 creates (board create wrote none: system-provisioned), 1 transition,
    // 1 delete — written by the SAME services the API handlers call.
    assert_eq!(
        activity_count(&mut conn, alice, "create"),
        2,
        "item creates"
    );
    assert_eq!(activity_count(&mut conn, alice, "transition"), 1);
    assert_eq!(activity_count(&mut conn, alice, "delete"), 1);
    // The parent edge from create_item(parent) plus the blocks edge above
    // (KAIROS-T-0111) are relationship_add rows; the unlink one remove.
    assert_eq!(activity_count(&mut conn, alice, "relationship_add"), 2);
    assert_eq!(activity_count(&mut conn, alice, "relationship_remove"), 1);

    // --- repositories (KAIROS-T-0107, A-0019) --------------------------------
    // Give the delivery board an owning team, register a repo under it, and
    // exercise the directory tools plus the `repository` filters. Cross-team
    // filing over MCP is covered by tests/file_backlog.rs.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let platform: kairos_db::models::teams::Team =
        diesel::insert_into(kairos_db::schema::teams::table)
            .values(kairos_db::models::teams::NewTeam {
                name: "Platform".into(),
                slug: "platform".into(),
                team_type: kairos_db::models::enums::TeamType::Platform,
            })
            .returning(kairos_db::models::teams::Team::as_returning())
            .get_result(&mut conn)
            .expect("team");
    diesel::update(boards::table.filter(boards::id.eq(delivery.id)))
        .set(boards::team_id.eq(Some(platform.id)))
        .execute(&mut conn)
        .expect("owning the delivery board");
    diesel::insert_into(kairos_db::schema::team_members::table)
        .values(kairos_db::models::teams::NewTeamMember {
            team_id: platform.id,
            user_id: alice,
        })
        .execute(&mut conn)
        .expect("alice → platform");
    let payments = kairos_db::repositories::create(
        &mut conn,
        kairos_db::models::repositories::NewRepository {
            slug: "payments-api".into(),
            forge: kairos_db::models::enums::Forge::Github,
            repo_full_name: "acme/payments-api".into(),
            repo_url: "https://github.com/acme/payments-api".into(),
            default_branch: "main".into(),
            team_id: platform.id,
            description: "Run `cargo test` before every PR.".into(),
            created_by: alice,
            updated_by: alice,
        },
    )
    .expect("repo");
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");

    let text = session.call_ok("list_repositories", json!({})).await;
    assert!(text.contains("payments-api"), "{text}");
    assert!(text.contains("owner: platform"), "{text}");
    assert!(
        text.contains(&delivery.id.to_string()),
        "board named: {text}"
    );
    let text = session
        .call_ok("list_repositories", json!({"team": "platform"}))
        .await;
    assert!(text.contains("payments-api"), "{text}");
    let text = session
        .call_err("list_repositories", json!({"team": "nope"}))
        .await;
    assert!(text.contains("VALIDATION"), "{text}");

    let text = session
        .call_ok("get_repository", json!({"repository": "payments-api"}))
        .await;
    assert!(text.contains("## How to work here"), "{text}");
    assert!(text.contains("Run `cargo test` before every PR."), "{text}");
    assert!(text.contains("(nothing open)"), "{text}");
    let text = session
        .call_ok(
            "get_repository",
            json!({"repository": payments.id.to_string()}),
        )
        .await;
    assert!(text.contains("acme/payments-api"), "by UUID: {text}");
    let text = session
        .call_err("get_repository", json!({"repository": "nope"}))
        .await;
    assert!(text.contains("NOT_FOUND"), "{text}");

    // create_item with `repository` and no `board` routes to the owner's
    // board and binds; the filters then find it and only it.
    let text = session
        .call_ok(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Bound over MCP",
                "repository": "payments-api",
            }),
        )
        .await;
    let bound_code = extract_code(&text, "ACME-T-");
    assert!(text.contains("board platform-delivery"), "{text}");
    let text = session
        .call_ok(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Unbound over MCP",
                "board": "platform-delivery",
            }),
        )
        .await;
    let unbound_code = extract_code(&text, "ACME-T-");
    // board + repository must agree (KAIROS-T-0112 covers the disagreement):
    // the initiative board is not platform's delivery board.
    let text = session
        .call_err(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Disagreeing board",
                "repository": "payments-api",
                "board": initiative_board.to_string(),
            }),
        )
        .await;
    assert!(
        text.contains("VALIDATION") && text.contains("delivery board"),
        "{text}"
    );
    let text = session
        .call_ok(
            "board_items",
            json!({"board": "platform-delivery", "repository": "payments-api"}),
        )
        .await;
    assert!(text.contains(&bound_code), "{text}");
    assert!(
        !text.contains(&unbound_code),
        "the filter narrows tasks: {text}"
    );
    let text = session
        .call_ok("search", json!({"filter": {"repository": "payments-api"}}))
        .await;
    assert!(text.contains(&bound_code), "{text}");
    assert!(!text.contains(&unbound_code), "{text}");
    let text = session
        .call_err("search", json!({"filter": {"repository": "nope"}}))
        .await;
    assert!(text.contains("VALIDATION"), "{text}");

    // whoami now lists my teams' repositories and the implicit capability.
    let text = session.call_ok("whoami", json!({})).await;
    assert!(text.contains("## My teams' repositories"), "{text}");
    assert!(
        text.contains("payments-api — acme/payments-api (owner: platform)"),
        "{text}"
    );
    assert!(text.contains("file_backlog"), "{text}");

    // --- KAIROS-T-0123 (UAT findings #3, #4, #5) ---------------------------
    // #4: the delivery board is printed as a slug, the way board_items takes it.
    let text = session
        .call_ok("get_repository", json!({"repository": "payments-api"}))
        .await;
    assert!(
        text.contains("- delivery board: platform-delivery (Platform Delivery)"),
        "{text}"
    );
    // #3: a PR linked to the task shows up on get_item under ## Development.
    {
        sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");
        use kairos_db::models::enums::{Forge, LinkKind, LinkState};
        use kairos_db::models::forge::{NewForgeConnection, NewItemLink};
        let connection = kairos_db::forge::create_connection(
            &mut conn,
            NewForgeConnection {
                forge: Forge::Github,
                repository_id: payments.id,
                created_by: alice,
            },
        )
        .expect("webhook connection");
        let task_id: Uuid = kairos_db::schema::tasks::table
            .filter(kairos_db::schema::tasks::short_code.eq(&bound_code))
            .select(kairos_db::schema::tasks::id)
            .first(&mut conn)
            .expect("bound task exists");
        kairos_db::forge::upsert_link(
            &mut conn,
            NewItemLink {
                item_id: task_id,
                connection_id: connection.id,
                kind: LinkKind::PullRequest,
                external_id: "77".into(),
                title: format!("Export endpoint for {bound_code}"),
                url: "https://github.com/acme/payments-api/pull/77".into(),
                state: LinkState::Merged,
                author: "carol".into(),
                forge_updated_at: chrono::Utc::now(),
            },
        )
        .expect("link");
    }
    let text = session
        .call_ok("get_item", json!({"short_code": bound_code}))
        .await;
    assert!(text.contains("## Development"), "{text}");
    assert!(
        text.contains("- pull_request 77 [merged] Export endpoint for"),
        "{text}"
    );
    // #5: bob is an org member with no grant on platform's board. He may file
    // into its Backlog (file_backlog) and is told THAT rule when he tries to
    // move what he filed — not the bare capability name.
    // A first authenticated call JIT-provisions bob's users row (403 until
    // he is a member); only then can membership be granted.
    let bob_token = user_token(&http, "bob").await;
    let (status, _, _) = raw_request(
        &router,
        Method::POST,
        "/mcp",
        Some(&bob_token),
        None,
        Some(
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "t", "version": "0"}}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let bob = user_id(&mut conn, "bob@kairos.test");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: bob,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("granting bob membership");
    let (mut bob_session, _) = McpSession::connect(&router, &bob_token).await;
    let text = bob_session
        .call_ok(
            "create_item",
            json!({"item_type": "task", "title": "Filed by bob", "repository": "payments-api"}),
        )
        .await;
    let filed = extract_code(&text, "ACME-T-");
    let text = bob_session
        .call_err(
            "transition_item",
            json!({"short_code": filed, "to_column": "Todo"}),
        )
        .await;
    assert!(text.contains("FORBIDDEN"), "{text}");
    assert!(
        text.contains("platform's Backlog") && text.contains("file_backlog"),
        "cross-team filer gets the Backlog-only explanation: {text}"
    );
    let text = bob_session
        .call_err(
            "update_item",
            json!({"short_code": filed, "content": "edited by the filer", "version": 1}),
        )
        .await;
    assert!(text.contains("file_backlog"), "{text}");

    // --- teardown ------------------------------------------------------------
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
