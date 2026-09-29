//! Integration test for the `/mcp` MCP endpoint (KAIROS-T-0026, contracts
//! per KAIROS-A-0011 / KAIROS-S-0006): a real MCP session over streamable
//! HTTP against the production router, authenticated with a REAL Dex token
//! (never forged), exercising the frozen S-0006 tool surface end to end.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer. For isolation the test owns the
//! uniquely named scratch databases (`kairos_mcp_t0026_test`,
//! `kairos_mcp_t0266_test`); the shared
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
//! - tools/list is EXACTLY the 23-tool inventory: 14 from S-0006, the two
//!   repository tools of KAIROS-T-0107, `move_item` (KAIROS-I-0012),
//!   `restore_item` (KAIROS-A-0020), `related_work` (KAIROS-T-0191),
//!   `propose_edge` (KAIROS-T-0192), `set_repository` (COLLIERY-T-0220),
//!   and `add_repository` and `update_repository` (COLLIERY-T-0266);
//!   the reference page and the how-to give the same count, and the
//!   reference page has one section for each tool;
//! - `set_repository` (COLLIERY-T-0220): set, clear, the board and the team
//!   do not change, the refusals, and agreement with the REST route;
//! - `add_repository` and `update_repository` (COLLIERY-T-0266), in a test
//!   of their own: a member of the owner team, an organization admin, each
//!   refusal, the activity rows, and agreement with the REST routes;
//! - golden path: whoami → my_boards → create_item(initiative) →
//!   create_item(task, parent) → get_item → edit_item → transition_item
//!   (invalid first: INVALID_TRANSITION enumerating allowed targets,
//!   REQ-1.4; then valid) → board_items → search → get_history →
//!   update_item stale-version conflict carrying the current state
//!   (REQ-1.5) → delete_item (confirm required; cascade listed) →
//!   board_items reflects the delete;
//! - ABAC parity (REQ-1.1): the link rule and the edit rule
//!   (COLLIERY-T-0228). A member who can edit one end writes an edge of
//!   each relationship type; a member who can edit neither end is
//!   FORBIDDEN. The creator of a request edits it and cannot move it;
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

/// The scratch database of the test of `add_repository` and
/// `update_repository` (COLLIERY-T-0266). The two tests of this file run at
/// the same time, so each has a database of its own.
const REPOSITORY_TOOLS_DB: &str = "kairos_mcp_t0266_test";

/// The tenant slug.
const TENANT: &str = "acme";

/// How many tools `tools/list` returns, and the word the reference pages
/// use for that number (COLLIERY-T-0220, COLLIERY-T-0266).
const TOOL_COUNT: usize = 23;
const TOOL_COUNT_WORD: &str = "twenty-three";

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
    // the capability path, and link_items(informs) proves that no
    // relationship type needs the admin role (COLLIERY-T-0228).
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
    // COLLIERY-T-0230: a delivery board always has a team, so the team is
    // made first. Alice joins it in the repositories section below: until
    // then she has her explicit grants and nothing from the team.
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
    let delivery = kairos_db::boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        Some(platform.id),
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
    // KAIROS-T-0178: needed to create an ADR over MCP and prove
    // `decision_date` persists. An explicit narrow grant, in keeping with the
    // rest of this fixture — alice stays a plain member with no admin bypass,
    // which is what makes the ABAC assertions further down mean anything.
    let adr_board = board_id_of(&mut conn, BoardLevel::Adr);
    abac::grant_capability(&mut conn, adr_board, alice, "manage_adrs", alice)
        .expect("grant manage_adrs");
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
    // COLLIERY-T-0267: the instructions name the repository tools.
    assert_eq!(
        init["result"]["instructions"],
        "Kairos work-item tools. Start with `whoami` (identity, teams, board capabilities) \
         and `my_boards` (boards + columns). Items are identified by short code (e.g. \
         ACME-T-0012) everywhere. Use `search` to find items, `get_item` for full content, \
         and the write tools (create/update/edit/transition/link/set_metadata/delete) to \
         work them; writes require board capabilities and edits use optimistic versioning. \
         Repositories: `list_repositories` and `get_repository` read the directory, \
         `add_repository` and `update_repository` write it, and `set_repository` links a \
         task to one."
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
        // KAIROS-I-0012: move a task to another delivery board.
        "move_item",
        "link_items",
        "unlink_items",
        "set_metadata",
        "delete_item",
        // KAIROS-A-0020 / T-0160: put archived work back.
        "restore_item",
        // KAIROS-T-0107 (A-0019): the repository directory.
        "list_repositories",
        "get_repository",
        // KAIROS-T-0191 (A-0021 rules 5-7): related work, as proposals.
        "related_work",
        // KAIROS-T-0192 (A-0021 rule 6): an agent proposes an edge; a human
        // confirms it. There is deliberately no confirm/reject tool here —
        // deciding is not an agent's to do.
        "propose_edge",
        // COLLIERY-T-0220 (COLLIERY-A-0023): set or clear the repository of
        // a task. A tool of its own, not an argument of `update_item`.
        "set_repository",
        // COLLIERY-T-0266: an agent adds a repository to the directory, and
        // changes its description, its default branch and its URL. No tool
        // deletes a repository, and no tool changes its owner or its slug.
        "add_repository",
        "update_repository",
    ];
    expected.sort_unstable();
    assert_eq!(names, expected, "tools/list is exactly the S-0006 surface");
    // The count as a number. The list above is the real gate; this line is
    // what the reference pages quote, so it fails with them.
    assert_eq!(names.len(), TOOL_COUNT, "{names:?}");
    assert!(names.iter().any(|name| name == "set_repository"));

    // --- COLLIERY-T-0220: the reference pages agree with tools/list ---------
    // The reference page is hand-written. Until this task it gave a count
    // that was two tools behind, and no test read it.
    {
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/src");
        let read = |rel: &str| {
            std::fs::read_to_string(docs.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
        };
        let reference = read("reference/mcp-tools.md");
        let mut sections: Vec<String> = reference
            .lines()
            .filter_map(|line| line.strip_prefix("### `")?.strip_suffix('`'))
            .map(str::to_string)
            .collect();
        sections.sort();
        assert_eq!(
            sections, names,
            "reference/mcp-tools.md has one section for each tool of tools/list"
        );
        assert!(
            reference.contains(&format!("exactly\n{TOOL_COUNT_WORD} tools"))
                && reference.contains(&format!("returns these {TOOL_COUNT_WORD} and no others")),
            "reference/mcp-tools.md gives the count {TOOL_COUNT_WORD}"
        );
        for stale in ["eighteen", "nineteen", "twenty tools", "twenty-one"] {
            assert!(
                !reference.contains(stale),
                "reference/mcp-tools.md: {stale:?}"
            );
        }
        let section = reference
            .split("### `set_repository`")
            .nth(1)
            .expect("the set_repository section")
            .split("\n### ")
            .next()
            .expect("the section body");
        for needed in [
            "| `short_code` | string | yes |",
            "| `repository` | string | no |",
            // COLLIERY-T-0228: the section said "Requires `manage_tasks`".
            // The edit rule has three ways in, and the page gives them.
            "The edit rule applies",
            "`manage_tasks`",
            "Refuses:",
            "`FORBIDDEN`",
            "`VALIDATION`",
            "`NOT_FOUND`",
        ] {
            assert!(
                section.contains(needed),
                "set_repository section lacks {needed:?}: {section}"
            );
        }
        let how_to = read("how-to/connect-over-mcp.md");
        assert_eq!(
            how_to.matches(TOOL_COUNT_WORD).count(),
            3,
            "how-to/connect-over-mcp.md gives the count in three places"
        );
        for stale in ["eighteen", "twenty-one"] {
            assert!(
                !how_to.contains(stale),
                "how-to/connect-over-mcp.md: {stale:?}"
            );
        }
    }

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

    // --- KAIROS-T-0178: a bucket initiative and a dated ADR, over MCP --------
    //
    // Both fields were hardcoded `None` in the tool, so an initiative created
    // over MCP could never be a bucket at all â while `get_item` happily
    // reported `bucket:` on read. Asserted through get_item rather than the
    // create response, because the question is whether the value was PERSISTED.
    let text = session
        .call_ok(
            "create_item",
            json!({
                "item_type": "initiative",
                "title": "Keeping the flux capacitor serviced",
                "bucket_type": "tech_debt",
            }),
        )
        .await;
    let bucket_code = extract_code(&text, "ACME-I-");
    let text = session
        .call_ok("get_item", json!({"short_code": bucket_code}))
        .await;
    assert!(
        text.contains("- bucket: tech_debt"),
        "bucket_type must reach the row, not just the request: {text}"
    );

    let text = session
        .call_ok(
            "create_item",
            json!({
                "item_type": "adr",
                "title": "Use a DeLorean",
                "decision_maker": "alice",
                "decision_date": "1985-10-26",
            }),
        )
        .await;
    let adr_code = extract_code(&text, "ACME-A-");
    let text = session
        .call_ok("get_item", json!({"short_code": adr_code}))
        .await;
    assert!(text.contains("- decision_date: 1985-10-26"), "{text}");
    assert!(text.contains("- decision_maker: alice"), "{text}");

    // Wrong item_type is refused by name, as the other per-type fields are.
    let err = session
        .call_err(
            "create_item",
            json!({
                "item_type": "task",
                "title": "not an initiative",
                "bucket_type": "tech_debt",
            }),
        )
        .await;
    assert!(
        err.contains("bucket_type") && err.contains("initiatives"),
        "the refusal must name the field and the type it belongs to: {err}"
    );

    // And a malformed date is a typed refusal, not a silently dropped field.
    let err = session
        .call_err(
            "create_item",
            json!({
                "item_type": "adr",
                "title": "badly dated",
                "decision_date": "26-10-1985",
            }),
        )
        .await;
    assert!(err.contains("YYYY-MM-DD"), "{err}");

    // --- KAIROS-T-0096: set_metadata respects entity-type scoping ------------
    //
    // The KAIROS-T-0078 scoping guard was enforced on the REST path only, so an
    // agent could stamp a documents-only definition onto a task through MCP
    // while the GUI refused the identical write with a 422 and never offered the
    // field. Agents are a first-class writer (KAIROS-A-0011), so MCP is not a
    // side door with relaxed rules.
    //
    // `document_type` is scoped to `document` in the system defaults; `priority`
    // is unscoped.
    let err = session
        .call_err(
            "set_metadata",
            json!({
                "short_code": task_code,
                "values": {"document_type": "prd"},
            }),
        )
        .await;
    assert!(
        err.contains("document_type") && err.contains("does not apply to"),
        "the refusal must name the definition and the entity type: {err}"
    );

    // An in-scope definition on the same item still works, so the guard is
    // scoping and not simply refusing metadata over MCP.
    let text = session
        .call_ok(
            "set_metadata",
            json!({
                "short_code": task_code,
                "values": {"priority": "high"},
            }),
        )
        .await;
    assert!(text.contains("priority"), "{text}");

    // And a CLEAR of an out-of-scope definition stays allowed, on purpose: it is
    // how an item sheds a value some earlier version let it acquire, and
    // refusing the cleanup would strand exactly the rows the guard prevents.
    let text = session
        .call_ok(
            "set_metadata",
            json!({
                "short_code": task_code,
                "values": {"document_type": null},
            }),
        )
        .await;
    assert!(
        !text.to_lowercase().contains("does not apply"),
        "clearing an out-of-scope value must be allowed: {text}"
    );

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
    assert!(text.contains("The text of search is not in"), "{text}");

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
    assert!(text.contains("to version 2"), "{text}");
    assert!(text.contains("(1 replacement)"), "{text}");

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
        text.contains(&format!(": {first_column} -> {valid_target}.")),
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

    // --- ABAC parity: the link rule (COLLIERY-T-0228). alice is a member,
    // and she can edit the items here. Until then `informs` was refused
    // with FORBIDDEN, because it needed the admin role, and the type rule
    // was never reached. The refusal of THIS edge is the type rule now:
    // `informs` runs from a document or an ADR.
    let text = session
        .call_err(
            "link_items",
            json!({"source": initiative_code, "target": task_code, "relationship": "informs"}),
        )
        .await;
    assert!(
        text.contains("RELATIONSHIP_RULE") && !text.contains("FORBIDDEN"),
        "{text}"
    );
    // An `informs` edge that the type rule allows: she writes it, and
    // removes it. The negative case, a member who can edit neither end, is
    // below, where bob has a session.
    let text = session
        .call_ok(
            "link_items",
            json!({"source": adr_code, "target": initiative_code, "relationship": "informs"}),
        )
        .await;
    assert!(text.contains("Linked"), "{text}");
    let text = session
        .call_ok(
            "unlink_items",
            json!({"source": adr_code, "target": initiative_code, "relationship": "informs"}),
        )
        .await;
    assert!(text.contains("Unlinked"), "{text}");
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
        text.contains(&format!("Archived {initiative_code}")),
        "{text}"
    );
    assert!(text.contains("Cascade archived 1 descendant"), "{text}");
    assert!(text.contains(&task_code), "cascade names the task: {text}");

    // --- board_items reflects the cascade delete ----------------------------
    let text = session
        .call_ok("board_items", json!({"board": "platform-delivery"}))
        .await;
    assert!(!text.contains(&task_code), "{text}");

    // ...but the archived item is still READABLE, and says so loudly
    // (KAIROS-A-0020, KAIROS-T-0155). An agent that cannot tell retired
    // work from live work will try to act on it and be refused with no
    // idea why, so the banner comes before anything else in the render.
    let text = session
        .call_ok("get_item", json!({"short_code": task_code}))
        .await;
    assert!(
        text.contains("ARCHIVED"),
        "the banner is not optional: {text}"
    );
    assert!(
        text.contains(&task_code),
        "archived work is readable by short code: {text}"
    );

    // Its history reads too — the audit answer the whole ADR exists for.
    let text = session
        .call_ok("get_history", json!({"short_code": task_code}))
        .await;
    assert!(text.contains("ARCHIVED"), "{text}");
    assert!(text.contains("v1"), "the versions are intact: {text}");

    // …and every write to it is still refused: archived work is read-only
    // by construction, because the write tools keep resolving LiveOnly.
    let text = session
        .call_err(
            "transition_item",
            json!({"short_code": task_code, "to_column": "Todo"}),
        )
        .await;
    assert!(
        text.contains("NOT_FOUND"),
        "archived work is read-only: {text}"
    );

    // An unknown code is still a plain NOT_FOUND — the 404 now means
    // "no such thing" rather than "put away".
    let text = session
        .call_err("get_item", json!({"short_code": "ACME-T-9999"}))
        .await;
    assert!(text.contains("NOT_FOUND"), "{text}");

    // --- NFR-1.3: every mutation above landed in activity_log ---------------
    // 4 item creates — the initiative, its task, and (KAIROS-T-0178) the bucket
    // initiative and the dated ADR. The board create wrote none: it is
    // system-provisioned. Plus 1 transition and 1 delete, all written by the
    // SAME services the API handlers call.
    assert_eq!(
        activity_count(&mut conn, alice, "create"),
        4,
        "item creates"
    );
    assert_eq!(activity_count(&mut conn, alice, "transition"), 1);
    assert_eq!(activity_count(&mut conn, alice, "delete"), 1);
    // The parent edge from create_item(parent) plus the informs edge and
    // the blocks edge above are relationship_add rows; the two unlinks are
    // removes. COLLIERY-T-0228 added the informs pair: a member writes it.
    assert_eq!(activity_count(&mut conn, alice, "relationship_add"), 3);
    assert_eq!(activity_count(&mut conn, alice, "relationship_remove"), 2);

    // --- repositories (KAIROS-T-0107, A-0019) --------------------------------
    // Put alice on the team of the delivery board, register a repo under
    // it, and exercise the directory tools plus the `repository` filters.
    // The board has had its team since it was made (COLLIERY-T-0230).
    // Cross-team filing over MCP is covered by tests/file_backlog.rs.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
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

    // create_item with `repository` and no `board`: the board is the single
    // delivery board of the tenant, the same default a task with no
    // repository gets (COLLIERY-T-0217). The repository is the link and has
    // no say. It is the same board as before only because platform's board
    // is the one delivery board here; with two, see the move section below.
    // The filters then find the task and only it.
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
    // COLLIERY-T-0216: the board decides the team. Over MCP there is no way
    // to SEND a team, and until this task a task made here simply had none
    // unless a repository supplied one.
    {
        use kairos_db::schema::tasks;
        sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");
        let team: Option<Uuid> = tasks::table
            .filter(tasks::short_code.eq(&unbound_code))
            .select(tasks::team_id)
            .first(&mut conn)
            .expect("the task exists");
        assert_eq!(
            team,
            Some(platform.id),
            "a task with no repository takes the team of its board"
        );
    }
    // A board and a repository: the board is the one that was named. Until
    // COLLIERY-T-0217 the two had to agree, and a board that was not the
    // delivery board of the repository's owner was a VALIDATION error. The
    // case where they differ needs a second delivery board: see the move
    // section below.
    let text = session
        .call_ok(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Board and repository",
                "repository": "payments-api",
                "board": "platform-delivery",
            }),
        )
        .await;
    assert!(text.contains("board platform-delivery"), "{text}");
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
        text.contains("- owner's delivery board: platform-delivery (Platform Delivery)"),
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
    // #5: bob is an org member with no grant on platform's board. He may
    // send a request to it (file_backlog) and is told THAT rule when he tries
    // to move what he filed — not the bare capability name.
    //
    // COLLIERY-T-0218 (COLLIERY-A-0023): the request needs no repository, its
    // work class is `support`, and `planned` is refused. The explanation is
    // given only for a request the caller filed that is still in the entry
    // column. Until then every refused write by a member was told that the
    // item "sits in the Backlog", whatever column it was in.
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
        .call_ok("get_item", json!({"short_code": filed}))
        .await;
    assert!(
        text.contains("column: Backlog"),
        "the request is in the entry column: {text}"
    );
    assert!(
        text.contains("(task) · lane: support"),
        "the request is in the support lane, though its type is task: {text}"
    );
    // No repository: the request is created all the same. The board is
    // named, because the tenant has one delivery board here and the default
    // would do, but a request names the team it is for.
    let text = bob_session
        .call_ok(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Request with no repository",
                "board": "platform-delivery",
            }),
        )
        .await;
    let no_repository = extract_code(&text, "ACME-T-");
    let text = bob_session
        .call_ok("get_item", json!({"short_code": no_repository}))
        .await;
    assert!(
        text.contains("column: Backlog") && text.contains("(task) · lane: support"),
        "a request with no repository: entry column, support lane: {text}"
    );
    // `planned` is refused, and nothing is created.
    let text = bob_session
        .call_err(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Pushed into the planned lane",
                "board": "platform-delivery",
                "work_class": "planned",
            }),
        )
        .await;
    assert!(text.contains("FORBIDDEN"), "{text}");
    assert!(
        text.contains("support lane") && text.contains("manage_tasks"),
        "the refusal says which lane a request uses: {text}"
    );
    {
        use kairos_db::schema::tasks;
        let made: i64 = tasks::table
            .filter(tasks::title.eq("Pushed into the planned lane"))
            .count()
            .get_result(&mut conn)
            .expect("counting tasks");
        assert_eq!(made, 0, "a refused request leaves no task");
    }
    // An explicit `support` is the same as none.
    bob_session
        .call_ok(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Request that names its lane",
                "board": "platform-delivery",
                "work_class": "support",
            }),
        )
        .await;
    let text = bob_session
        .call_err(
            "transition_item",
            json!({"short_code": filed, "to_column": "Todo"}),
        )
        .await;
    assert!(text.contains("FORBIDDEN"), "{text}");
    assert!(
        text.contains("is a request in the entry column of the board of the team platform")
            && text.contains("file_backlog")
            && text.contains("transition_items"),
        "the person who filed a request gets the request rule: {text}"
    );
    // COLLIERY-T-0228: the explanation says what he CAN do, and it is true.
    assert_eq!(
        text.lines().next().unwrap_or_default(),
        format!(
            "FORBIDDEN: {filed} is a request in the entry column of the board of the team platform. \
             That team moves it. You created it (file_backlog), so you can edit it, link it and \
             archive it. To move it, you need \"transition_items\" on that board."
        ),
        "{text}"
    );
    // He created the request, so he can edit it (the edit rule,
    // COLLIERY-T-0228). Until then this call was refused with the request
    // rule.
    let text = bob_session
        .call_ok(
            "update_item",
            json!({"short_code": filed, "content": "edited by the filer", "version": 1}),
        )
        .await;
    assert!(text.contains("to version 2"), "{text}");
    let text = session
        .call_ok("get_item", json!({"short_code": filed}))
        .await;
    assert!(
        text.contains("edited by the filer") && text.contains("column: Backlog"),
        "the edit is stored, and the request did not move: {text}"
    );
    // The link rule, negative: bob can edit neither end of these edges. He
    // created none of the items and holds no grant on their boards. Each
    // relationship type is refused, and the refusal names the capability.
    let edges_before: i64 = {
        use kairos_db::schema::item_relationships;
        sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");
        item_relationships::table
            .count()
            .get_result(&mut conn)
            .expect("counting edges")
    };
    for (relationship, source, target, source_capability, target_capability) in [
        (
            "parent",
            &bucket_code,
            &unbound_code,
            "manage_initiatives",
            "manage_tasks",
        ),
        (
            "blocks",
            &bucket_code,
            &unbound_code,
            "manage_initiatives",
            "manage_tasks",
        ),
        (
            "supports",
            &bucket_code,
            &adr_code,
            "manage_initiatives",
            "manage_adrs",
        ),
        (
            "informs",
            &adr_code,
            &bucket_code,
            "manage_adrs",
            "manage_initiatives",
        ),
        (
            "supersedes",
            &adr_code,
            &adr_code,
            "manage_adrs",
            "manage_adrs",
        ),
    ] {
        let text = bob_session
            .call_err(
                "link_items",
                json!({"source": source, "target": target, "relationship": relationship}),
            )
            .await;
        assert!(
            text.starts_with("FORBIDDEN: ")
                && text.contains(source_capability)
                && text.contains(target_capability),
            "{relationship} by a member who can edit neither end: {text}"
        );
        assert!(!text.contains("admin role"), "{text}");
    }
    {
        use kairos_db::schema::item_relationships;
        let edges_after: i64 = item_relationships::table
            .count()
            .get_result(&mut conn)
            .expect("counting edges");
        assert_eq!(edges_before, edges_after, "a refused link writes no edge");
    }
    // A card in the entry column that bob did NOT file: the plain refusal.
    let text = bob_session
        .call_err(
            "update_item",
            json!({"short_code": unbound_code, "content": "not his", "version": 1}),
        )
        .await;
    assert!(
        text.contains("FORBIDDEN") && text.contains("manage_tasks"),
        "{text}"
    );
    assert!(
        !text.contains("file_backlog") && !text.contains("entry column"),
        "he did not file it, so the request rule is not his to be told: {text}"
    );
    // The team moves his request to Active. It is not in the entry column
    // now, so a refused MOVE names the missing capability and no more.
    // Before COLLIERY-T-0218 this refusal said that the card "sits in
    // platform's Backlog".
    session
        .call_ok(
            "transition_item",
            json!({"short_code": filed, "to_column": "Todo"}),
        )
        .await;
    session
        .call_ok(
            "transition_item",
            json!({"short_code": filed, "to_column": "Active"}),
        )
        .await;
    //
    // COLLIERY-T-0228: `update_item` and `delete_item` were in this list of
    // refusals. They are edits, he created the request, and so they are
    // allowed, below. `move_item` takes their place: it is a move, and
    // creation grants no movement.
    for (tool, arguments, capability) in [
        (
            "transition_item",
            json!({"short_code": filed, "to_column": "Completed"}),
            "transition_items",
        ),
        (
            "move_item",
            json!({"short_code": filed, "to_board": "initiatives"}),
            "manage_tasks",
        ),
    ] {
        let text = bob_session.call_err(tool, arguments).await;
        assert!(
            text.contains("FORBIDDEN") && text.contains(capability),
            "{tool}: {text}"
        );
        assert!(
            !text.contains("Backlog")
                && !text.contains("entry column")
                && !text.contains("file_backlog"),
            "{tool} on a card in Active does not name the Backlog: {text}"
        );
    }
    let text = session
        .call_ok("get_item", json!({"short_code": filed}))
        .await;
    assert!(
        text.contains("column: Active"),
        "no refusal moved it: {text}"
    );
    // The edits of its creator, wherever the team moved the card.
    let text = bob_session
        .call_ok(
            "update_item",
            json!({"short_code": filed, "content": "edited again by the filer", "version": 2}),
        )
        .await;
    assert!(text.contains("to version 3"), "{text}");
    let text = bob_session
        .call_ok("delete_item", json!({"short_code": filed, "confirm": true}))
        .await;
    assert!(text.contains(&format!("Archived {filed}")), "{text}");
    let text = bob_session
        .call_ok("restore_item", json!({"short_code": filed}))
        .await;
    assert!(text.contains(&format!("Restored {filed}")), "{text}");
    let text = session
        .call_ok("get_item", json!({"short_code": filed}))
        .await;
    assert!(
        text.contains("column: Active") && text.contains("edited again by the filer"),
        "the request is back where the team put it: {text}"
    );

    // --- KAIROS-T-0128: move_item (the I-0012 board move) -------------------
    // A second delivery board to move to, with alice able to manage both.
    // COLLIERY-T-0230: a delivery board always has a team.
    let web_team = common::seed_team(&mut conn, "Web", "web");
    let web_board = kairos_db::boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Web Delivery",
        "web-delivery",
        Some(web_team),
        None,
    )
    .expect("creating the web delivery board");
    abac::grant_capability(&mut conn, web_board.id, alice, "manage_tasks", alice)
        .expect("granting manage_tasks on web");
    // --- COLLIERY-T-0249: a write tool refuses an argument that it does
    // --- not know, names it, and writes nothing. `column` is the argument
    // --- that `create_item` has not (KAIROS-T-0178).
    let tasks_before: i64 = {
        use kairos_db::schema::tasks::dsl;
        dsl::tasks.count().get_result(&mut conn).expect("counting")
    };
    let text = session
        .call_err(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Placed by hand",
                "board": "platform-delivery",
                "column": "In Progress",
            }),
        )
        .await;
    // COLLIERY-T-0256: the refusal has the form of each other tool error,
    // `CODE: message` and a line of details.
    let (first_line, details) = text.split_once("\ndetails: ").expect("a line of details");
    assert!(
        first_line.starts_with(
            "VALIDATION: The call has the argument \"column\". This tool does not accept \
             that argument. The arguments of this tool are: item_type, title, board, "
        ),
        "{text}"
    );
    let details: Value = serde_json::from_str(details).expect("the details are JSON");
    assert_eq!(details["argument"], "column", "{text}");
    assert_eq!(details["allowed"][0], "item_type", "{text}");
    let tasks_after: i64 = {
        use kairos_db::schema::tasks::dsl;
        dsl::tasks.count().get_result(&mut conn).expect("counting")
    };
    assert_eq!(tasks_after, tasks_before, "no task was written");
    // The schema of the tool says it, so a client can know before the call.
    // COLLIERY-T-0256: the rule is for each tool, the tools that read too.
    for tool in listed["tools"].as_array().expect("tools array") {
        let name = tool["name"].as_str().expect("tool name");
        assert_eq!(
            tool["inputSchema"]["additionalProperties"],
            json!(false),
            "{name}: additionalProperties is false for each tool"
        );
    }
    // A tool that reads refuses such an argument, in the same form.
    let text = session
        .call_err(
            "board_items",
            json!({"board": "platform-delivery", "no_such_argument": true}),
        )
        .await;
    assert_eq!(
        text,
        "VALIDATION: The call has the argument \"no_such_argument\". This tool does not \
         accept that argument. The arguments of this tool are: board, column, repository, \
         include_deleted, limit, offset.\n\
         details: {\"allowed\":[\"board\",\"column\",\"repository\",\"include_deleted\",\
         \"limit\",\"offset\"],\"argument\":\"no_such_argument\"}"
    );
    let full = session
        .call_ok(
            "board_items",
            json!({"board": "platform-delivery", "include_deleted": true}),
        )
        .await;

    // --- board_items: a part of a board says that it is a part ---------------
    // COLLIERY-T-0261. The cards of a result are the lines `- CODE [...`.
    let cards = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with("- "))
            .map(|line| line.to_string())
            .collect()
    };
    let all = cards(&full);
    assert!(all.len() >= 3, "the board has 3 cards or more: {full}");
    assert!(
        !full.contains("This result shows"),
        "the full board has no note: {full}"
    );
    let total = all.len();
    let mut read: Vec<String> = Vec::new();
    let mut offset = 0;
    loop {
        let part = session
            .call_ok(
                "board_items",
                json!({
                    "board": "platform-delivery", "include_deleted": true,
                    "limit": 2, "offset": offset,
                }),
            )
            .await;
        let shown = cards(&part);
        let next = offset + shown.len();
        let note = if next < total {
            format!(
                "The board has {total} items that agree with the filters. This result shows \
                 {} (limit 2, offset {offset}). To read the next part, call the tool with \
                 offset {next}.",
                shown.len()
            )
        } else {
            format!(
                "The board has {total} items that agree with the filters. This result shows \
                 {} (limit 2, offset {offset}). To read the first part, call the tool with \
                 offset 0.",
                shown.len()
            )
        };
        // THE DEFECT: before COLLIERY-T-0261 the tool had no `limit`, and
        // the result had each card of the board.
        assert!(part.lines().any(|line| line == note), "{note}\n{part}");
        assert!(part.contains(" in this result)"), "{part}");
        assert!(shown.len() <= 2, "{part}");
        read.extend(shown);
        if next >= total {
            break;
        }
        offset = next;
    }
    assert_eq!(
        read, all,
        "the parts give the board, in the order of the board"
    );
    // With no filter, the note has the words of the board.
    let live_total = cards(
        &session
            .call_ok("board_items", json!({"board": "platform-delivery"}))
            .await,
    )
    .len();
    let part = session
        .call_ok(
            "board_items",
            json!({"board": "platform-delivery", "limit": 1}),
        )
        .await;
    let note = format!(
        "The board has {live_total} items. This result shows 1 (limit 1, offset 0). To read \
         the next part, call the tool with offset 1."
    );
    assert!(part.lines().any(|line| line == note), "{note}\n{part}");
    // An argument in an object of the call.
    let text = session
        .call_err(
            "search",
            json!({"q": "cache", "filter": {"board": "platform-delivery"}}),
        )
        .await;
    assert!(
        text.starts_with(
            "VALIDATION: The call has the argument \"board\". This tool does not accept \
             that argument. The arguments of this tool are: entity_type, board_id, "
        ),
        "{text}"
    );
    // `whoami` has no arguments.
    let text = session.call_err("whoami", json!({"verbose": true})).await;
    assert!(
        text.starts_with(
            "VALIDATION: The call has the argument \"verbose\". This tool does not accept \
             that argument. This tool has no arguments."
        ),
        "{text}"
    );
    session.call_ok("whoami", json!({})).await;
    // An argument that the tool must have, and the call has not.
    let text = session.call_err("get_item", json!({})).await;
    assert!(
        text.starts_with(
            "VALIDATION: The call does not have the argument \"short_code\". This tool \
             must have that argument. The arguments of this tool are: short_code."
        ),
        "{text}"
    );

    let text = session
        .call_ok(
            "create_item",
            json!({"item_type": "task", "title": "Moves house", "board": "platform-delivery"}),
        )
        .await;
    let movable = extract_code(&text, "ACME-T-");
    let text = session
        .call_ok(
            "move_item",
            json!({"short_code": movable, "to_board": "web-delivery"}),
        )
        .await;
    assert!(
        text.contains(&format!(
            "Moved {movable}: platform-delivery -> web-delivery /"
        )),
        "{text}"
    );
    let text = session
        .call_ok("get_item", json!({"short_code": movable}))
        .await;
    assert!(text.contains("board: web-delivery"), "{text}");
    // COLLIERY-T-0219: the open tasks of the repository, before web links
    // a task to it.
    let open_tasks = |text: &str| -> i64 {
        text.lines()
            .find_map(|line| line.strip_prefix("- open tasks (all boards): "))
            .unwrap_or_else(|| panic!("no open-task line: {text}"))
            .parse()
            .expect("a count")
    };
    let text = session
        .call_ok("get_repository", json!({"repository": "payments-api"}))
        .await;
    let open_before = open_tasks(&text);
    // COLLIERY-T-0217 (COLLIERY-A-0023): a task on web's board links to
    // platform's repository. The board and the owner of the repository
    // differ, which was a VALIDATION error before.
    let text = session
        .call_ok(
            "create_item",
            json!({
                "item_type": "task",
                "title": "Web's work in platform's code",
                "board": "web-delivery",
                "repository": "payments-api",
            }),
        )
        .await;
    let linked = extract_code(&text, "ACME-T-");
    assert!(text.contains("board web-delivery"), "{text}");
    let text = session
        .call_ok("get_item", json!({"short_code": linked}))
        .await;
    assert!(text.contains("board: web-delivery"), "{text}");
    assert!(text.contains("payments-api"), "{text}");
    // A task with a repository moves like any other, and keeps the
    // repository. Before, the move was refused unless the target was the
    // board of the repository's owner: to platform's board was allowed, and
    // back to web's was not.
    let text = session
        .call_ok(
            "move_item",
            json!({"short_code": linked, "to_board": "platform-delivery"}),
        )
        .await;
    assert!(
        text.contains(": web-delivery -> platform-delivery /"),
        "{text}"
    );
    let text = session
        .call_ok(
            "move_item",
            json!({"short_code": linked, "to_board": "web-delivery"}),
        )
        .await;
    assert!(
        text.contains(&format!(
            "Moved {linked}: platform-delivery -> web-delivery /"
        )),
        "the move does not look at the repository: {text}"
    );
    let text = session
        .call_ok("get_item", json!({"short_code": linked}))
        .await;
    assert!(text.contains("board: web-delivery"), "{text}");
    assert!(text.contains("payments-api"), "it keeps the link: {text}");
    // COLLIERY-T-0219: platform's repository has a linked task on web's
    // board. That is normal work (COLLIERY-A-0023): the repository counts it
    // as open and says nothing about stale tasks. Before, it printed a
    // `STALE` line that told the reader to rebind or move the task.
    let text = session
        .call_ok("get_repository", json!({"repository": "payments-api"}))
        .await;
    assert!(text.contains("- owner team: platform"), "{text}");
    assert_eq!(open_tasks(&text), open_before + 1, "{text}");
    assert!(!text.to_lowercase().contains("stale"), "{text}");
    let text = session.call_ok("list_repositories", json!({})).await;
    assert!(!text.to_lowercase().contains("stale"), "{text}");
    assert!(
        text.contains(&format!("open tasks: {}", open_before + 1)),
        "{text}"
    );
    // With two delivery boards there is no default, and a repository does
    // not supply one: the caller must name the board.
    let text = session
        .call_err(
            "create_item",
            json!({
                "item_type": "task",
                "title": "No board, two to choose from",
                "repository": "payments-api",
            }),
        )
        .await;
    assert!(
        text.contains("VALIDATION") && text.contains("Send `board`"),
        "{text}"
    );
    // Only tasks: an initiative is told to use transition_item instead.
    let text = session
        .call_ok(
            "create_item",
            json!({"item_type": "initiative", "title": "Stays put"}),
        )
        .await;
    let other = extract_code(&text, "ACME-I-");
    let text = session
        .call_err(
            "move_item",
            json!({"short_code": other, "to_board": "web-delivery"}),
        )
        .await;
    assert!(text.contains("is not a task"), "{text}");
    assert!(text.contains("transition_item"), "{text}");
    // Two-sided: bob may manage neither board. He did not file this task, so
    // the refusal names the missing capability and no more
    // (COLLIERY-T-0218). Until then it explained the Backlog rule to him,
    // about a task that was never a request of his.
    let text = bob_session
        .call_err(
            "move_item",
            json!({"short_code": movable, "to_board": "platform-delivery"}),
        )
        .await;
    assert!(text.contains("FORBIDDEN"), "{text}");
    assert!(text.contains("manage_tasks"), "{text}");
    assert!(
        !text.contains("file_backlog") && !text.contains("entry column"),
        "{text}"
    );

    // --- COLLIERY-T-0220: set_repository (COLLIERY-A-0023) -------------------
    // The repository of a task is a link. REST, the CLI and the GUI could
    // set it; MCP could not. A second team owns the repository here, so a
    // link that changed the board or the team of the task would show.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let billing: kairos_db::models::teams::Team =
        diesel::insert_into(kairos_db::schema::teams::table)
            .values(kairos_db::models::teams::NewTeam {
                name: "Billing".into(),
                slug: "billing".into(),
                team_type: kairos_db::models::enums::TeamType::StreamAligned,
            })
            .returning(kairos_db::models::teams::Team::as_returning())
            .get_result(&mut conn)
            .expect("team");
    let ledger = kairos_db::repositories::create(
        &mut conn,
        kairos_db::models::repositories::NewRepository {
            slug: "ledger".into(),
            forge: kairos_db::models::enums::Forge::Github,
            repo_full_name: "acme/ledger".into(),
            repo_url: "https://github.com/acme/ledger".into(),
            default_branch: "main".into(),
            team_id: billing.id,
            description: String::new(),
            created_by: alice,
            updated_by: alice,
        },
    )
    .expect("repo");
    /// What `set_repository` can and cannot change, read from the row.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Placement {
        board: Uuid,
        team: Option<Uuid>,
        column: Uuid,
        version: i32,
        repository: Option<Uuid>,
    }
    fn placement(conn: &mut PgConnection, short_code: &str) -> Placement {
        use kairos_db::schema::tasks;
        let (board, team, column, version, repository) = tasks::table
            .filter(tasks::short_code.eq(short_code))
            .select((
                tasks::board_id,
                tasks::team_id,
                tasks::column_id,
                tasks::version,
                tasks::repository_id,
            ))
            .first(conn)
            .unwrap_or_else(|e| panic!("task {short_code}: {e}"));
        Placement {
            board,
            team,
            column,
            version,
            repository,
        }
    }
    /// One REST call as alice; returns the status and the JSON body.
    async fn rest(
        router: &Router,
        token: &str,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, _, body) = raw_request(router, method, uri, Some(token), None, body).await;
        let json = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{uri}: {e}: {body}"));
        (status, json)
    }

    let text = session
        .call_ok(
            "create_item",
            json!({"item_type": "task", "title": "Found its codebase", "board": "platform-delivery"}),
        )
        .await;
    let found = extract_code(&text, "ACME-T-");
    let before = placement(&mut conn, &found);
    assert_eq!(before.board, delivery.id);
    assert_eq!(before.team, Some(platform.id));
    assert_eq!(before.repository, None);
    let text = session
        .call_ok("get_item", json!({"short_code": found}))
        .await;
    assert!(text.contains("· repository: (none)"), "{text}");

    // Set, by slug. The repository belongs to billing; the task is on the
    // board of platform.
    let text = session
        .call_ok(
            "set_repository",
            json!({"short_code": found, "repository": "ledger"}),
        )
        .await;
    assert_eq!(
        text,
        format!("Set the repository of {found}: ledger (owner: billing).")
    );
    // ...seen through get_item, through the row, and through REST.
    let text = session
        .call_ok("get_item", json!({"short_code": found}))
        .await;
    assert!(
        text.contains("· repository: ledger (owner: billing)"),
        "{text}"
    );
    assert!(text.contains("- board: platform-delivery"), "{text}");
    let after = placement(&mut conn, &found);
    assert_eq!(after.repository, Some(ledger.id));
    assert_eq!(
        after,
        Placement {
            repository: Some(ledger.id),
            ..before.clone()
        },
        "the link changes the repository and nothing else: not the board, \
         not the team, not the column, not the version"
    );
    let (status, task) = rest(
        &router,
        &alice_token,
        Method::GET,
        &format!("/api/tasks/{found}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{task}");
    assert_eq!(task["repository"]["slug"], "ledger", "{task}");
    assert_eq!(task["repository_id"], ledger.id.to_string(), "{task}");
    assert_eq!(task["board_id"], delivery.id.to_string(), "{task}");
    assert_eq!(task["team_id"], platform.id.to_string(), "{task}");

    // The value the task already has: a success, and no second audit row.
    let audit_rows = activity_count(&mut conn, alice, "repository");
    let text = session
        .call_ok(
            "set_repository",
            json!({"short_code": found, "repository": "ledger"}),
        )
        .await;
    assert_eq!(
        text,
        format!("Set the repository of {found}: ledger (owner: billing).")
    );
    assert_eq!(placement(&mut conn, &found), after);
    assert_eq!(activity_count(&mut conn, alice, "repository"), audit_rows);

    // By UUID, to a different repository. One audit row for the change.
    let text = session
        .call_ok(
            "set_repository",
            json!({"short_code": found, "repository": payments.id.to_string()}),
        )
        .await;
    assert_eq!(
        text,
        format!("Set the repository of {found}: payments-api (owner: platform).")
    );
    assert_eq!(
        activity_count(&mut conn, alice, "repository"),
        audit_rows + 1,
        "NFR-1.3: the tool writes the audit row that the REST route writes"
    );

    // Clear: an absent argument, null, and an empty string. Each starts from
    // a task that has a link.
    for (what, arguments) in [
        ("absent", json!({"short_code": found})),
        ("null", json!({"short_code": found, "repository": null})),
        (
            "empty string",
            json!({"short_code": found, "repository": ""}),
        ),
        (
            "blank string",
            json!({"short_code": found, "repository": "  "}),
        ),
    ] {
        session
            .call_ok(
                "set_repository",
                json!({"short_code": found, "repository": "ledger"}),
            )
            .await;
        assert_eq!(
            placement(&mut conn, &found).repository,
            Some(ledger.id),
            "{what}: the task has a link to clear"
        );
        let text = session.call_ok("set_repository", arguments).await;
        assert_eq!(
            text,
            format!("Cleared the repository of {found}: the task has no repository."),
            "{what}"
        );
        assert_eq!(
            placement(&mut conn, &found),
            before,
            "{what}: no repository, and the board and the team as they were"
        );
        let text = session
            .call_ok("get_item", json!({"short_code": found}))
            .await;
        assert!(text.contains("· repository: (none)"), "{what}: {text}");
        let (_, task) = rest(
            &router,
            &alice_token,
            Method::GET,
            &format!("/api/tasks/{found}"),
            None,
        )
        .await;
        assert!(task["repository"].is_null(), "{what}: {task}");
        assert!(task["repository_id"].is_null(), "{what}: {task}");
    }
    // To clear a task that has no repository is a success too.
    let text = session
        .call_ok("set_repository", json!({"short_code": found}))
        .await;
    assert!(text.contains("has no repository"), "{text}");

    // REST and MCP agree in the other direction: REST sets, get_item shows.
    let (status, task) = rest(
        &router,
        &alice_token,
        Method::PUT,
        &format!("/api/tasks/{found}/repository"),
        Some(json!({"repository": "ledger"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let text = session
        .call_ok("get_item", json!({"short_code": found}))
        .await;
    assert!(
        text.contains("· repository: ledger (owner: billing)"),
        "{text}"
    );
    let (status, task) = rest(
        &router,
        &alice_token,
        Method::PUT,
        &format!("/api/tasks/{found}/repository"),
        Some(json!({"repository": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let text = session
        .call_ok("get_item", json!({"short_code": found}))
        .await;
    assert!(text.contains("· repository: (none)"), "{text}");
    // The two give the same refusal for a repository that does not exist.
    let (status, refused) = rest(
        &router,
        &alice_token,
        Method::PUT,
        &format!("/api/tasks/{found}/repository"),
        Some(json!({"repository": "nope"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    let text = session
        .call_err(
            "set_repository",
            json!({"short_code": found, "repository": "nope"}),
        )
        .await;
    assert_eq!(
        text,
        "VALIDATION: No live repository has the slug \"nope\"."
    );
    assert_eq!(refused["error"]["code"], "VALIDATION", "{refused}");
    assert_eq!(
        refused["error"]["message"], "No live repository has the slug \"nope\".",
        "{refused}"
    );
    // An unknown UUID is the same kind of refusal.
    let text = session
        .call_err(
            "set_repository",
            json!({"short_code": found, "repository": Uuid::new_v4().to_string()}),
        )
        .await;
    assert!(
        text.starts_with("VALIDATION: The repository ") && text.ends_with("does not exist."),
        "{text}"
    );
    assert_eq!(
        placement(&mut conn, &found),
        before,
        "a refused call changes nothing"
    );

    // A short code that names an item that is not a task.
    let text = session
        .call_err(
            "set_repository",
            json!({"short_code": other, "repository": "ledger"}),
        )
        .await;
    assert_eq!(
        text,
        format!(
            "VALIDATION: The initiative {other} is not a task. \
             set_repository applies to tasks only."
        )
    );
    // A short code that names nothing, and one that names archived work:
    // NOT_FOUND, as from every write tool.
    let text = session
        .call_err(
            "set_repository",
            json!({"short_code": "ACME-T-9999", "repository": "ledger"}),
        )
        .await;
    assert_eq!(
        text,
        "NOT_FOUND: No live item has the short code \"ACME-T-9999\"."
    );
    let text = session
        .call_err(
            "set_repository",
            json!({"short_code": task_code, "repository": "ledger"}),
        )
        .await;
    assert!(text.starts_with("NOT_FOUND: No live item"), "{text}");

    // bob has no `manage_tasks` on the board. He did not file this task, so
    // the refusal names the capability and no more.
    let text = bob_session
        .call_err(
            "set_repository",
            json!({"short_code": found, "repository": "ledger"}),
        )
        .await;
    assert!(
        text.starts_with("FORBIDDEN: ") && text.contains("manage_tasks"),
        "{text}"
    );
    assert!(
        !text.contains("file_backlog") && !text.contains("entry column"),
        "{text}"
    );
    // The gate comes before the repository is resolved, as in the REST
    // route: he is refused, not told that the repository does not exist.
    let text = bob_session
        .call_err(
            "set_repository",
            json!({"short_code": found, "repository": "nope"}),
        )
        .await;
    assert!(text.starts_with("FORBIDDEN: "), "{text}");
    // He cannot clear a link either.
    let text = bob_session
        .call_err("set_repository", json!({"short_code": found}))
        .await;
    assert!(text.starts_with("FORBIDDEN: "), "{text}");
    assert_eq!(placement(&mut conn, &found), before);
    // The request that he created, still in the entry column. The
    // repository is an edit, so its creator sets it and clears it
    // (COLLIERY-T-0228). Until then this call was refused with the request
    // rule. The board, the team and the column of the request do not
    // change.
    let request_before = placement(&mut conn, &no_repository);
    let text = bob_session
        .call_ok(
            "set_repository",
            json!({"short_code": no_repository, "repository": "ledger"}),
        )
        .await;
    assert!(text.contains("ledger"), "{text}");
    let request_bound = placement(&mut conn, &no_repository);
    assert_eq!(request_bound.repository, Some(ledger.id));
    assert_eq!(request_bound.board, request_before.board);
    assert_eq!(request_bound.team, request_before.team);
    bob_session
        .call_ok("set_repository", json!({"short_code": no_repository}))
        .await;
    assert_eq!(placement(&mut conn, &no_repository), request_before);
    let (status, refused) = rest(
        &router,
        &bob_token,
        Method::PUT,
        &format!("/api/tasks/{found}/repository"),
        Some(json!({"repository": "ledger"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "REST agrees: {refused}");

    // --- teardown ------------------------------------------------------------
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}

// ---------------------------------------------------------------------------
// COLLIERY-T-0266: add_repository and update_repository
// ---------------------------------------------------------------------------

/// The row of a repository, as the tools can change it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryRow {
    forge: String,
    repo_full_name: String,
    repo_url: String,
    default_branch: String,
    description: String,
    team: Uuid,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// The live repository with `slug`, or `None`.
fn repository_row(conn: &mut PgConnection, slug: &str) -> Option<RepositoryRow> {
    kairos_db::repositories::load_by_slug(conn, slug)
        .ok()
        .map(|row| RepositoryRow {
            forge: row.forge.to_string(),
            repo_full_name: row.repo_full_name,
            repo_url: row.repo_url,
            default_branch: row.default_branch,
            description: row.description,
            team: row.team_id,
            updated_at: row.updated_at,
        })
}

/// The number of live repositories of the tenant.
fn repository_count(conn: &mut PgConnection) -> usize {
    kairos_db::repositories::list(conn, None)
        .expect("the directory")
        .len()
}

#[derive(QueryableByName)]
struct DetailsRow {
    #[diesel(sql_type = SqlText)]
    details: String,
}

/// The `details` of the `repository` rows of the activity log of one
/// actor, oldest first.
fn repository_activity(conn: &mut PgConnection, actor: Uuid) -> Vec<String> {
    sql_query(
        "SELECT details FROM org_acme.activity_log \
         WHERE actor_id = $1 AND action = 'repository' ORDER BY occurred_at ASC, id ASC",
    )
    .bind::<SqlUuid, _>(actor)
    .load::<DetailsRow>(conn)
    .expect("activity rows")
    .into_iter()
    .map(|row| row.details)
    .collect()
}

/// A team, with a delivery board when `board` is true, in the current
/// `search_path`.
fn team_with_board(
    conn: &mut PgConnection,
    name: &str,
    slug: &str,
    board: bool,
) -> kairos_db::models::teams::Team {
    let team: kairos_db::models::teams::Team = diesel::insert_into(kairos_db::schema::teams::table)
        .values(kairos_db::models::teams::NewTeam {
            name: name.into(),
            slug: slug.into(),
            team_type: kairos_db::models::enums::TeamType::StreamAligned,
        })
        .returning(kairos_db::models::teams::Team::as_returning())
        .get_result(conn)
        .expect("team");
    if board {
        kairos_db::boards::create_board(
            conn,
            BoardLevel::Delivery,
            &format!("{name} Delivery"),
            &format!("{slug}-delivery"),
            Some(team.id),
            None,
        )
        .expect("creating the delivery board");
    }
    team
}

/// Cast: `svc` is an organization admin and is in no team. `alice` is a
/// member of `platform` and of `guild`, a team with no delivery board. `bob`
/// is a member of `web`.
#[tokio::test]
async fn repository_tools_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(REPOSITORY_TOOLS_DB);
    let scratch_url = with_database(&common::admin_database_url(), REPOSITORY_TOOLS_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, TENANT, "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq(TENANT))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");

    let http = reqwest::Client::new();
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

    // A first authenticated call JIT-provisions the users row (403 until the
    // user is a member). Only then can the membership be granted.
    let mut cast = Vec::new();
    for (name, role) in [
        ("svc", OrgRole::Admin),
        ("alice", OrgRole::Member),
        ("bob", OrgRole::Member),
    ] {
        let token = user_token(&http, name).await;
        let (status, _, _) = raw_request(
            &router,
            Method::POST,
            "/mcp",
            Some(&token),
            None,
            Some(
                json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "t", "version": "0"}}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}");
        let id = user_id(&mut conn, &format!("{name}@kairos.test"));
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: id,
                role,
            })
            .execute(&mut conn)
            .unwrap_or_else(|e| panic!("granting {name} membership: {e}"));
        cast.push((token, id));
    }
    let [(svc_token, svc), (alice_token, alice), (bob_token, bob)] =
        <[(String, Uuid); 3]>::try_from(cast).expect("three users");

    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let platform = team_with_board(&mut conn, "Platform", "platform", true);
    let web = team_with_board(&mut conn, "Web", "web", true);
    let guild = team_with_board(&mut conn, "Guild", "guild", false);
    for (team, user) in [(&platform, alice), (&guild, alice), (&web, bob)] {
        diesel::insert_into(kairos_db::schema::team_members::table)
            .values(kairos_db::models::teams::NewTeamMember {
                team_id: team.id,
                user_id: user,
            })
            .execute(&mut conn)
            .expect("team member");
    }

    let (mut svc_session, _) = McpSession::connect(&router, &svc_token).await;
    let (mut alice_session, _) = McpSession::connect(&router, &alice_token).await;
    let (mut bob_session, _) = McpSession::connect(&router, &bob_token).await;

    // --- the schema: what the tools have, and what they do not have ----------
    let listed = alice_session.request("tools/list", json!({})).await;
    let tools = listed["tools"].as_array().expect("tools array").clone();
    let tool = |name: &str| {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no tool {name}"))
            .clone()
    };
    let arguments_of = |name: &str| {
        let mut names: Vec<String> = tool(name)["inputSchema"]["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        arguments_of("add_repository"),
        [
            "default_branch",
            "description",
            "forge",
            "repo_full_name",
            "repo_url",
            "slug",
            "team"
        ]
    );
    // No `slug` and no `team`: the tool changes neither.
    assert_eq!(
        arguments_of("update_repository"),
        ["default_branch", "description", "repo_url", "repository"]
    );
    for name in ["add_repository", "update_repository"] {
        let tool = tool(name);
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{name}");
        let description = tool["description"].as_str().expect("description");
        // The description says what no tool does, who does it, and where.
        for needed in [
            "o tool deletes a repository",
            "owner team",
            "slug",
            "A person does these",
            "Admin, Repositories",
            "`kairos repos update`",
            "`kairos repos delete`",
            "/api/repositories/{slug}",
            "`set_repository`",
        ] {
            assert!(description.contains(needed), "{name} lacks {needed:?}");
        }
    }
    // No tool deletes a repository.
    for t in &tools {
        let name = t["name"].as_str().expect("name");
        assert!(
            !(name.contains("repositor") && (name.contains("delete") || name.contains("remove"))),
            "{name}"
        );
    }
    assert_eq!(repository_count(&mut conn), 0);

    // --- add_repository: a member of the owner team --------------------------
    // The least that a call can have. The slug comes from the full name and
    // the default branch is `main`, as in POST /api/repositories.
    let text = alice_session
        .call_ok(
            "add_repository",
            json!({
                "forge": "github",
                "repo_full_name": "acme/fidius",
                "repo_url": "https://github.com/acme/fidius",
                "team": "platform",
            }),
        )
        .await;
    assert_eq!(
        text,
        "Added repository acme-fidius: github acme/fidius (owner: platform, default branch main)."
    );
    let row = repository_row(&mut conn, "acme-fidius").expect("the row");
    assert_eq!(row.forge, "github");
    assert_eq!(row.repo_full_name, "acme/fidius");
    assert_eq!(row.repo_url, "https://github.com/acme/fidius");
    assert_eq!(row.default_branch, "main");
    assert_eq!(row.description, "");
    assert_eq!(row.team, platform.id);
    // The activity log has the action, and alice is the actor.
    assert_eq!(
        repository_activity(&mut conn, alice),
        ["repository_created:acme-fidius"]
    );
    // Each argument, and the team by UUID.
    let text = alice_session
        .call_ok(
            "add_repository",
            json!({
                "slug": "portal",
                "forge": "gitlab",
                "repo_full_name": "acme/portal/web",
                "repo_url": "https://gitlab.com/acme/portal/web",
                "default_branch": "trunk",
                "team": platform.id.to_string(),
                "description": "Run `angreal test unit` before each pull request.",
            }),
        )
        .await;
    assert_eq!(
        text,
        "Added repository portal: gitlab acme/portal/web (owner: platform, default branch trunk)."
    );
    // The other tools and the REST route see it.
    let text = alice_session
        .call_ok("get_repository", json!({"repository": "portal"}))
        .await;
    assert!(text.contains("- default branch: trunk"), "{text}");
    assert!(
        text.contains("Run `angreal test unit` before each pull request."),
        "{text}"
    );
    let (status, _, body) = raw_request(
        &router,
        Method::GET,
        "/api/repositories/portal",
        Some(&bob_token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rest: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(rest["repo_full_name"], "acme/portal/web", "{rest}");
    assert_eq!(rest["team"]["slug"], "platform", "{rest}");

    // --- add_repository: an organization admin -------------------------------
    // svc is in no team.
    let text = svc_session
        .call_ok(
            "add_repository",
            json!({
                "slug": "site",
                "forge": "other",
                "repo_full_name": "acme/site",
                "repo_url": "https://git.acme.example/acme/site",
                "team": "web",
            }),
        )
        .await;
    assert_eq!(
        text,
        "Added repository site: other acme/site (owner: web, default branch main)."
    );
    assert_eq!(
        repository_activity(&mut conn, svc),
        ["repository_created:site"]
    );
    assert_eq!(repository_count(&mut conn), 3);

    // --- add_repository: the refusals ----------------------------------------
    let forbidden = "FORBIDDEN: This action requires the capability \"manage_tasks\" on the \
         delivery board of the team \"platform\", the owner team of the repository. You do not \
         have that capability. Each member of the team has it, and an organization admin has \
         each capability. Ask a member of the team \"platform\" or an organization admin to do \
         this.";
    // bob is a member of the organization and of `web`. He is not in
    // `platform`.
    let text = bob_session
        .call_err(
            "add_repository",
            json!({
                "forge": "github",
                "repo_full_name": "acme/not-for-bob",
                "repo_url": "https://github.com/acme/not-for-bob",
                "team": "platform",
            }),
        )
        .await;
    let (message, details) = text.split_once("\ndetails: ").expect("details");
    assert_eq!(message, forbidden);
    let details: Value = serde_json::from_str(details).expect("details are JSON");
    assert_eq!(details["required_capability"], "manage_tasks", "{details}");
    assert!(details["board_id"].is_string(), "{details}");
    // The REST route gives the same refusal: it is the same function.
    let (status, _, body) = raw_request(
        &router,
        Method::POST,
        "/api/repositories",
        Some(&bob_token),
        None,
        Some(json!({
            "forge": "github",
            "repo_full_name": "acme/not-for-bob",
            "repo_url": "https://github.com/acme/not-for-bob",
            "team": "platform",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let rest: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(
        format!("FORBIDDEN: {}", rest["error"]["message"].as_str().unwrap()),
        forbidden
    );
    // A team with no delivery board: membership gives no capability, so the
    // member is refused and the admin is not.
    let guild_repository = json!({
        "forge": "github",
        "repo_full_name": "acme/guild-notes",
        "repo_url": "https://github.com/acme/guild-notes",
        "team": "guild",
    });
    let text = alice_session
        .call_err("add_repository", guild_repository.clone())
        .await;
    assert!(
        text.starts_with(
            "FORBIDDEN: The team \"guild\", the owner team of the repository, does not have \
             one live delivery board. Thus this action requires the organization admin role. \
             Ask an organization admin to do this."
        ),
        "{text}"
    );
    assert_eq!(
        repository_count(&mut conn),
        3,
        "a refused call adds nothing"
    );
    assert_eq!(repository_activity(&mut conn, bob), Vec::<String>::new());
    svc_session
        .call_ok("add_repository", guild_repository)
        .await;

    // A slug that a repository has already: 409 from REST, CONFLICT here.
    let duplicate = json!({
        "slug": "portal",
        "forge": "github",
        "repo_full_name": "acme/second-portal",
        "repo_url": "https://github.com/acme/second-portal",
        "team": "platform",
    });
    let text = alice_session
        .call_err("add_repository", duplicate.clone())
        .await;
    assert_eq!(
        text,
        "CONFLICT: A repository has the slug \"portal\" already."
    );
    let (status, _, body) = raw_request(
        &router,
        Method::POST,
        "/api/repositories",
        Some(&alice_token),
        None,
        Some(duplicate),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let rest: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(
        rest["error"]["message"],
        "A repository has the slug \"portal\" already."
    );
    // The same repository of the forge, with a different slug.
    let text = alice_session
        .call_err(
            "add_repository",
            json!({
                "slug": "fidius-again",
                "forge": "github",
                "repo_full_name": "acme/fidius",
                "repo_url": "https://github.com/acme/fidius",
                "team": "platform",
            }),
        )
        .await;
    assert_eq!(
        text,
        "CONFLICT: The github repository \"acme/fidius\" is in the directory already."
    );
    // The form of the slug.
    for slug in ["Upper", "under_score", "x", "-first"] {
        let text = alice_session
            .call_err(
                "add_repository",
                json!({
                    "slug": slug,
                    "forge": "github",
                    "repo_full_name": "acme/form",
                    "repo_url": "https://github.com/acme/form",
                    "team": "platform",
                }),
            )
            .await;
        assert_eq!(
            text,
            format!(
                "VALIDATION: The repository slug {slug:?} is not correct. A repository slug \
                 must match ^[a-z0-9][a-z0-9-]{{1,62}}$."
            )
        );
    }
    let text = alice_session
        .call_err(
            "add_repository",
            json!({
                "forge": "bitbucket",
                "repo_full_name": "acme/form",
                "repo_url": "https://bitbucket.org/acme/form",
                "team": "platform",
            }),
        )
        .await;
    assert_eq!(
        text,
        "VALIDATION: The value \"bitbucket\" is not a value of forge. The values are: \
         github, gitlab, other."
    );
    let text = alice_session
        .call_err(
            "add_repository",
            json!({
                "forge": "github",
                "repo_full_name": "acme/form",
                "repo_url": "https://github.com/acme/form",
                "team": "nobody",
            }),
        )
        .await;
    assert_eq!(
        text,
        "VALIDATION: The team \"nobody\" is not in the organization. Send the id or the \
         slug of a team of the organization."
    );
    // An argument that the tool does not know. `owner` is a probable guess
    // for `team`.
    let text = alice_session
        .call_err(
            "add_repository",
            json!({
                "forge": "github",
                "repo_full_name": "acme/form",
                "repo_url": "https://github.com/acme/form",
                "team": "platform",
                "owner": "platform",
            }),
        )
        .await;
    assert!(
        text.starts_with(
            "VALIDATION: The call has the argument \"owner\". This tool does not accept \
             that argument. The arguments of this tool are: slug, forge, repo_full_name, \
             repo_url, default_branch, team, description.\ndetails: "
        ),
        "{text}"
    );
    // An argument that the tool must have.
    let text = alice_session
        .call_err(
            "add_repository",
            json!({
                "forge": "github",
                "repo_full_name": "acme/form",
                "repo_url": "https://github.com/acme/form",
            }),
        )
        .await;
    assert!(
        text.starts_with(
            "VALIDATION: The call does not have the argument \"team\". This tool must have \
             that argument."
        ),
        "{text}"
    );
    assert_eq!(
        repository_count(&mut conn),
        4,
        "a refused call adds nothing"
    );
    assert!(repository_row(&mut conn, "acme-form").is_none());
    assert_eq!(repository_activity(&mut conn, alice).len(), 2);

    // --- update_repository: a member of the owner team -----------------------
    let text = alice_session
        .call_ok(
            "update_repository",
            json!({
                "repository": "acme-fidius",
                "description": "Run `cargo test` before each pull request.",
                "default_branch": "trunk",
            }),
        )
        .await;
    assert_eq!(
        text,
        "Updated repository acme-fidius: description, default_branch."
    );
    let changed = repository_row(&mut conn, "acme-fidius").expect("the row");
    assert_eq!(
        changed,
        RepositoryRow {
            description: "Run `cargo test` before each pull request.".into(),
            default_branch: "trunk".into(),
            updated_at: changed.updated_at,
            ..row.clone()
        },
        "the two values change, and no other"
    );
    assert!(changed.updated_at > row.updated_at);
    assert_eq!(
        repository_activity(&mut conn, alice),
        [
            "repository_created:acme-fidius",
            "repository_created:portal",
            "repository_updated:acme-fidius"
        ]
    );
    // Only the values that are different are in the result. The default
    // branch of the call is the one that the repository has.
    let text = alice_session
        .call_ok(
            "update_repository",
            json!({
                "repository": "acme-fidius",
                "default_branch": "trunk",
                "repo_url": "https://github.com/acme/fidius-rs",
            }),
        )
        .await;
    assert_eq!(text, "Updated repository acme-fidius: repo_url.");
    // An empty string removes the description.
    let text = alice_session
        .call_ok(
            "update_repository",
            json!({"repository": "acme-fidius", "description": ""}),
        )
        .await;
    assert_eq!(text, "Updated repository acme-fidius: description.");
    let text = alice_session
        .call_ok("get_repository", json!({"repository": "acme-fidius"}))
        .await;
    assert!(text.contains("(no description yet)"), "{text}");
    assert!(
        text.contains("- url: https://github.com/acme/fidius-rs"),
        "{text}"
    );

    // --- update_repository: an organization admin, by UUID -------------------
    let fidius = kairos_db::repositories::load_by_slug(&mut conn, "acme-fidius").expect("row");
    let text = svc_session
        .call_ok(
            "update_repository",
            json!({"repository": fidius.id.to_string(), "description": "Read CONTRIBUTING.md."}),
        )
        .await;
    assert_eq!(text, "Updated repository acme-fidius: description.");
    assert_eq!(
        repository_activity(&mut conn, svc),
        [
            "repository_created:site",
            "repository_created:acme-guild-notes",
            "repository_updated:acme-fidius"
        ]
    );

    // --- update_repository: the refusals -------------------------------------
    let before = repository_row(&mut conn, "acme-fidius").expect("the row");
    let audit_rows = activity_count(&mut conn, alice, "repository");
    // bob is not in the owner team.
    let text = bob_session
        .call_err(
            "update_repository",
            json!({"repository": "acme-fidius", "description": "bob was here"}),
        )
        .await;
    let (message, _) = text.split_once("\ndetails: ").expect("details");
    assert_eq!(message, forbidden);
    // He is refused also when the call has the values of the repository: the
    // gate comes before the comparison.
    let text = bob_session
        .call_err(
            "update_repository",
            json!({"repository": "acme-fidius", "description": "Read CONTRIBUTING.md."}),
        )
        .await;
    assert!(text.starts_with("FORBIDDEN: "), "{text}");
    let (status, _, body) = raw_request(
        &router,
        Method::PATCH,
        "/api/repositories/acme-fidius",
        Some(&bob_token),
        None,
        Some(json!({"description": "bob was here"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "REST agrees: {body}");
    // He can change a repository of his team.
    let text = bob_session
        .call_ok(
            "update_repository",
            json!({"repository": "site", "default_branch": "release"}),
        )
        .await;
    assert_eq!(text, "Updated repository site: default_branch.");
    // A repository that does not exist.
    let text = alice_session
        .call_err(
            "update_repository",
            json!({"repository": "nope", "description": "x"}),
        )
        .await;
    assert_eq!(text, "NOT_FOUND: No live repository has the slug \"nope\".");
    let text = alice_session
        .call_err(
            "update_repository",
            json!({"repository": Uuid::new_v4().to_string(), "description": "x"}),
        )
        .await;
    assert!(
        text.starts_with("NOT_FOUND: No live repository has the id "),
        "{text}"
    );
    // Nothing to change: no argument, and null for each.
    for arguments in [
        json!({"repository": "acme-fidius"}),
        json!({"repository": "acme-fidius", "description": null, "default_branch": null, "repo_url": null}),
    ] {
        let text = alice_session.call_err("update_repository", arguments).await;
        assert_eq!(
            text,
            "VALIDATION: The call has nothing to change. Send one or more of these \
             arguments: description, default_branch, repo_url."
        );
    }
    // The values that the repository has: a success that writes nothing.
    let text = alice_session
        .call_ok(
            "update_repository",
            json!({
                "repository": "acme-fidius",
                "description": "Read CONTRIBUTING.md.",
                "default_branch": "trunk",
                "repo_url": "https://github.com/acme/fidius-rs",
            }),
        )
        .await;
    assert_eq!(
        text,
        "No change to repository acme-fidius: it has these values already."
    );
    // The slug and the owner team: the tool does not know the arguments.
    for (argument, value) in [("slug", "fidius"), ("team", "web")] {
        let text = alice_session
            .call_err(
                "update_repository",
                json!({"repository": "acme-fidius", "description": "x", argument: value}),
            )
            .await;
        assert!(
            text.starts_with(&format!(
                "VALIDATION: The call has the argument {argument:?}. This tool does not \
                 accept that argument. The arguments of this tool are: repository, \
                 description, default_branch, repo_url."
            )),
            "{text}"
        );
    }
    let text = alice_session
        .call_err("update_repository", json!({"description": "x"}))
        .await;
    assert!(
        text.starts_with(
            "VALIDATION: The call does not have the argument \"repository\". This tool must \
             have that argument."
        ),
        "{text}"
    );
    assert_eq!(
        repository_row(&mut conn, "acme-fidius").expect("the row"),
        before,
        "a refused call changes nothing, and `updated_at` is as it was"
    );
    assert_eq!(
        activity_count(&mut conn, alice, "repository"),
        audit_rows,
        "a refused call and a call with no change write no activity row"
    );
    assert_eq!(
        repository_row(&mut conn, "acme-fidius").unwrap().team,
        platform.id
    );

    // --- COLLIERY-T-0267: the form of the fields -----------------------------
    // Until COLLIERY-T-0267 the two tools accepted an empty value and a blank
    // value for the full name, the URL and the default branch. The rules are
    // those of REST (`tests/repository_fields.rs`), from the same functions.
    let count = repository_count(&mut conn);
    let audit_rows = activity_count(&mut conn, alice, "repository");
    let refused: &[(&str, &str, &str)] = &[
        ("repo_full_name", "", "github"),
        ("repo_full_name", "   ", "github"),
        ("repo_full_name", " acme/form", "github"),
        ("repo_full_name", "acme/form ", "github"),
        ("repo_full_name", "acme/form.git", "github"),
        ("repo_full_name", "form", "github"),
        ("repo_full_name", "acme/form/web", "github"),
        ("repo_full_name", "form", "gitlab"),
        ("repo_full_name", "acme//form", "other"),
        ("repo_url", "", "github"),
        ("repo_url", "  ", "github"),
        ("repo_url", " https://github.com/acme/form", "github"),
        ("repo_url", "github.com/acme/form", "github"),
        ("repo_url", "git@github.com:acme/form.git", "github"),
        ("repo_url", "https://", "github"),
        (
            "repo_url",
            "https://alice:s3cret@github.com/acme/form",
            "github",
        ),
        ("default_branch", "", "github"),
        ("default_branch", "  ", "github"),
        ("default_branch", "main ", "github"),
        ("default_branch", "-main", "github"),
        ("default_branch", "release..1", "github"),
        ("default_branch", "main/", "github"),
        ("default_branch", "main.lock", "github"),
    ];
    for (field, value, forge) in refused {
        let mut arguments = json!({
            "slug": "form",
            "forge": forge,
            "repo_full_name": "acme/form",
            "repo_url": "https://github.com/acme/form",
            "team": "platform",
        });
        arguments[*field] = json!(value);
        let text = alice_session.call_err("add_repository", arguments).await;
        let (message, details) = text.split_once("\ndetails: ").expect("details");
        assert!(
            message.starts_with(&format!("VALIDATION: The {field}")),
            "{field} = {value:?}: {text}"
        );
        assert_eq!(
            serde_json::from_str::<Value>(details).expect("JSON"),
            json!({ "field": field }),
            "{field} = {value:?}"
        );
        assert!(!text.contains("s3cret"), "{text}");
        if *field == "repo_full_name" {
            continue;
        }
        let text = alice_session
            .call_err(
                "update_repository",
                json!({"repository": "acme-fidius", *field: value}),
            )
            .await;
        let (update_message, details) = text.split_once("\ndetails: ").expect("details");
        assert_eq!(update_message, message, "the two tools give the same text");
        assert_eq!(
            serde_json::from_str::<Value>(details).expect("JSON"),
            json!({ "field": field })
        );
    }
    let text = alice_session
        .call_err(
            "add_repository",
            json!({
                "forge": "github",
                "repo_full_name": "acme/form",
                "repo_url": "https://alice:s3cret@github.com/acme/form",
                "team": "platform",
            }),
        )
        .await;
    assert_eq!(
        text,
        "VALIDATION: The repo_url has a user name or a password in it. Each member of the \
         organization can read the URL. Remove the user name and the password.\ndetails: \
         {\"field\":\"repo_url\"}"
    );
    // REST gives the same text.
    let (status, _, body) = raw_request(
        &router,
        Method::PATCH,
        "/api/repositories/acme-fidius",
        Some(&alice_token),
        None,
        Some(json!({"default_branch": ""})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let rest: Value = serde_json::from_str(&body).expect("JSON");
    let text = alice_session
        .call_err(
            "update_repository",
            json!({"repository": "acme-fidius", "default_branch": ""}),
        )
        .await;
    assert_eq!(
        text,
        format!(
            "VALIDATION: {}\ndetails: {{\"field\":\"default_branch\"}}",
            rest["error"]["message"].as_str().expect("message")
        )
    );
    assert_eq!(
        text,
        "VALIDATION: The default_branch is empty. Send a value, for example main.\ndetails: \
         {\"field\":\"default_branch\"}"
    );
    assert_eq!(repository_count(&mut conn), count);
    assert_eq!(
        repository_row(&mut conn, "acme-fidius").expect("the row"),
        before
    );
    assert_eq!(activity_count(&mut conn, alice, "repository"), audit_rows);

    // A repository from before the rule: the tools read it, and
    // `update_repository` changes a different field of it.
    sql_query(
        "INSERT INTO repositories \
            (slug, forge, repo_full_name, repo_url, default_branch, team_id, description, \
             created_by, updated_by) \
         VALUES ('old-one', 'github', 'Old One.git', '', ' ', $1, '', $2, $2)",
    )
    .bind::<SqlUuid, _>(platform.id)
    .bind::<SqlUuid, _>(svc)
    .execute(&mut conn)
    .expect("the old row");
    let text = alice_session
        .call_ok("get_repository", json!({"repository": "old-one"}))
        .await;
    assert!(text.contains("github Old One.git"), "{text}");
    let text = alice_session
        .call_ok("list_repositories", json!({"team": "platform"}))
        .await;
    assert!(text.contains("old-one"), "{text}");
    let text = alice_session
        .call_ok(
            "update_repository",
            json!({
                "repository": "old-one",
                "description": "Ask the platform team.",
                "repo_url": "",
                "default_branch": " ",
            }),
        )
        .await;
    assert_eq!(text, "Updated repository old-one: description.");
    let old = repository_row(&mut conn, "old-one").expect("the row");
    assert_eq!(old.repo_url, "");
    assert_eq!(old.default_branch, " ");
    assert_eq!(old.description, "Ask the platform team.");
    // The values of the row: no change, and nothing written.
    let text = alice_session
        .call_ok(
            "update_repository",
            json!({"repository": "old-one", "repo_url": "", "default_branch": " "}),
        )
        .await;
    assert_eq!(
        text,
        "No change to repository old-one: it has these values already."
    );
    assert_eq!(repository_row(&mut conn, "old-one").expect("the row"), old);
    // REST does the same with the same body: 200, and nothing written.
    let audit_rows = activity_count(&mut conn, alice, "repository");
    let (status, _, body) = raw_request(
        &router,
        Method::PATCH,
        "/api/repositories/old-one",
        Some(&alice_token),
        None,
        Some(json!({"repo_url": "", "default_branch": " "})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rest: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(rest["slug"], "old-one", "{rest}");
    assert_eq!(repository_row(&mut conn, "old-one").expect("the row"), old);
    assert_eq!(activity_count(&mut conn, alice, "repository"), audit_rows);

    // --- teardown ------------------------------------------------------------
    drop(conn);
    drop_scratch_db(&mut admin_conn, REPOSITORY_TOOLS_DB);
}
