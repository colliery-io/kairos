//! KAIROS-T-0410 (KAIROS-A-0025 decisions 1 and 8) — a member of a
//! strategy or initiative board edits and moves every item on it, against
//! the booted production router.
//!
//! - KAIROS-T-0541: only the `member` grant makes a member. A person with
//!   only `configure_boards` or only `pages` on the initiative board does
//!   not edit (`PATCH`) or move (`transition`). A person with only
//!   `transition_items` moves and does not edit. Until KAIROS-T-0541 this
//!   test showed that `configure_boards` alone edits and moves: that was
//!   the bug.
//! - A person with the `member` grant edits and moves an initiative that a
//!   different person created.
//! - A person with no grant on the board is refused (403).
//! - Archive and restore still need `manage_initiatives` (403 without it).
//! - The removal of the member grant removes the rights.
//! - `whoami` (REST and MCP) and `my_boards` show the implied capabilities
//!   for the member grant only.
//!
//! The rule is computed when the server reads it
//! (`kairos_db::abac::check_capability`). Nothing is stored. The db-layer
//! half is `kairos-db/tests/abac.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). The test
//! owns a scratch database.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_board_member_rights_t0410_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];
/// The tenant resolves from the host, as for each MCP request.
const HOST_HEADER: &str = "acme.kairos.test";

const MEMBER_CAPS: &str = "manage_initiatives, transition_items";

/// One in-process request against `/mcp`: the status, the headers and the
/// body.
async fn mcp_request(
    router: &Router,
    token: &str,
    session: Option<&str>,
    body: Value,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("host", HOST_HEADER)
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    let request = builder.body(Body::from(body.to_string())).expect("request");
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

/// The JSON-RPC message of a response: plain JSON, or a `data:` line.
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

/// Start an MCP session. Returns the id of the session, when the server
/// issues one.
async fn connect(router: &Router, token: &str) -> Option<String> {
    let (status, headers, body) = mcp_request(
        router,
        token,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "kairos-t0410-test", "version": "0.0.0"},
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "initialize: {body}");
    let session = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let (status, _, body) = mcp_request(
        router,
        token,
        session.as_deref(),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "initialized: {body}");
    session
}

/// Call one tool. Returns `(is_error, text)`.
async fn call(
    router: &Router,
    token: &str,
    session: Option<&str>,
    tool: &str,
    arguments: Value,
) -> (bool, String) {
    let (status, _, body) = mcp_request(
        router,
        token,
        session,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tool}: {body}");
    let result = rpc_message(&body)["result"].clone();
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{tool}: no text in {result}"))
        .to_string();
    (result["isError"] == true, text)
}

/// The lines of `text` from the line that starts with `- {slug} ` up to the
/// next board line.
fn board_block<'a>(text: &'a str, slug: &str) -> Vec<&'a str> {
    let head = format!("- {slug} ");
    let mut lines = text.lines().skip_while(|line| !line.starts_with(&head));
    let mut block: Vec<&str> = lines.next().into_iter().collect();
    block.extend(lines.take_while(|line| line.starts_with("  ")));
    block
}

/// The id of the column `name` of the board `board_slug`.
fn column_id(conn: &mut PgConnection, board_slug: &str, name: &str) -> String {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        id: Uuid,
    }
    let row: Row = diesel::sql_query(
        "SELECT c.id FROM org_acme.board_columns c \
         JOIN org_acme.boards b ON b.id = c.board_id \
         WHERE b.slug = $1 AND c.name = $2",
    )
    .bind::<diesel::sql_types::Text, _>(board_slug)
    .bind::<diesel::sql_types::Text, _>(name)
    .get_result(conn)
    .expect("column");
    row.id.to_string()
}

#[tokio::test]
async fn a_member_of_an_initiative_board_edits_and_moves_each_initiative_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
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
    let svc = user_token(&http, "svc").await;
    let alice = user_token(&http, "alice").await;
    let bob = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));

    // svc is the org admin; alice and bob are members with no grant.
    let mut ids = Vec::new();
    for (token, email, role) in [
        (&svc, "svc@kairos.test", OrgRole::Admin),
        (&alice, "alice@kairos.test", OrgRole::Member),
        (&bob, "bob@kairos.test", OrgRole::Member),
    ] {
        let _ = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(token),
            &TENANT,
            None,
        )
        .await;
        let id: Uuid = users::table
            .filter(users::email.eq(email))
            .select(users::id)
            .first(&mut conn)
            .expect("user provisioned");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
        ids.push(id);
    }
    let alice_id = ids[1];

    let as_user = |token: &str, method: Method, uri: &str, body: Option<Value>| {
        let (router, token, uri) = (router.clone(), token.to_string(), uri.to_string());
        async move { request(&router, method, &uri, Some(&token), &TENANT, body).await }
    };

    // svc creates an initiative on the initiative board.
    let (status, body) = as_user(
        &svc,
        Method::POST,
        "/api/initiatives",
        Some(json!({ "board_id": "initiatives", "title": "Shared work" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let code = body["short_code"].as_str().expect("code").to_string();
    let version = body["version"].as_i64().expect("version");
    let design = column_id(&mut conn, "initiatives", "Design");
    let ready = column_id(&mut conn, "initiatives", "Ready");
    let decompose = column_id(&mut conn, "initiatives", "Decompose");

    // --- Before a grant: alice is not a member, and is refused.
    let (status, body) = as_user(
        &alice,
        Method::PATCH,
        &format!("/api/initiatives/{code}"),
        Some(json!({ "content": "Not a member yet.", "version": version })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains("manage_initiatives"), "{body}");

    // --- KAIROS-T-0541: a grant other than `member` makes no member.
    let (status, body) = as_user(&svc, Method::GET, "/api/boards/initiatives", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let board = body["id"].as_str().expect("board id").to_string();
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &format!("/api/boards/{board}/members"),
        Some(json!({ "user_id": alice_id, "capabilities": ["configure_boards"] })),
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    for (grant, moves) in [
        ("configure_boards", false),
        ("pages", false),
        ("transition_items", true),
    ] {
        let (status, body) = as_user(
            &svc,
            Method::PATCH,
            &format!("/api/boards/{board}/members/{alice_id}"),
            Some(json!({ "capabilities": [grant] })),
        )
        .await;
        assert!(status.is_success(), "{grant}: {status}: {body}");
        let (status, body) = as_user(
            &alice,
            Method::PATCH,
            &format!("/api/initiatives/{code}"),
            Some(json!({ "content": format!("Only {grant}."), "version": version })),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{grant} edits: {body}");
        assert!(body.to_string().contains("manage_initiatives"), "{body}");
        // REST whoami: no implied capability without the member grant.
        let (status, body) = as_user(&alice, Method::GET, "/api/whoami", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let entry = body["capabilities"]
            .as_array()
            .expect("capabilities")
            .iter()
            .find(|entry| entry["board_slug"] == "initiatives")
            .expect("the initiative board")
            .clone();
        assert_eq!(entry["implied"], json!([]), "{grant}: {entry}");
        let (status, body) = as_user(
            &alice,
            Method::POST,
            &format!("/api/initiatives/{code}/transition"),
            Some(json!({ "to_column_id": design })),
        )
        .await;
        if moves {
            assert_eq!(status, StatusCode::OK, "{grant} moves: {body}");
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{grant} moves: {body}");
            assert!(body.to_string().contains("transition_items"), "{body}");
        }
    }
    // MCP whoami and my_boards: no member line for `transition_items`.
    let session = connect(&router, &alice).await;
    let (_, text) = call(&router, &alice, session.as_deref(), "whoami", json!({})).await;
    assert!(!text.contains("(board member, no grant)"), "{text}");
    let (_, text) = call(&router, &alice, session.as_deref(), "my_boards", json!({})).await;
    assert!(
        !board_block(&text, "initiatives")
            .iter()
            .any(|line| line.contains("member capabilities")),
        "{text}"
    );

    // --- The member grant: alice is a member of the board.
    let (status, body) = as_user(
        &svc,
        Method::PATCH,
        &format!("/api/boards/{board}/members/{alice_id}"),
        Some(json!({ "capabilities": ["member"] })),
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let (status, body) =
        as_user(&svc, Method::GET, &format!("/api/initiatives/{code}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let version = body["version"].as_i64().expect("version");

    // She edits an initiative that svc created...
    let (status, body) = as_user(
        &alice,
        Method::PATCH,
        &format!("/api/initiatives/{code}"),
        Some(json!({ "content": "Edited by a board member.", "version": version })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // ...and moves it.
    let (status, body) = as_user(
        &alice,
        Method::POST,
        &format!("/api/initiatives/{code}/transition"),
        Some(json!({ "to_column_id": decompose })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) =
        as_user(&svc, Method::GET, &format!("/api/initiatives/{code}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let version = body["version"].as_i64().expect("version");

    // --- A person with no grant on the board is refused.
    let (status, body) = as_user(
        &bob,
        Method::PATCH,
        &format!("/api/initiatives/{code}"),
        Some(json!({ "content": "Not mine.", "version": version })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = as_user(
        &bob,
        Method::POST,
        &format!("/api/initiatives/{code}/transition"),
        Some(json!({ "to_column_id": ready })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains("transition_items"), "{body}");

    // --- Archive needs its own grant.
    let (status, body) = as_user(
        &alice,
        Method::DELETE,
        &format!("/api/initiatives/{code}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "archive: {body}");
    assert!(body.to_string().contains("manage_initiatives"), "{body}");
    // So does restore.
    let (status, body) = as_user(
        &svc,
        Method::DELETE,
        &format!("/api/initiatives/{code}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let (status, body) = as_user(
        &alice,
        Method::POST,
        &format!("/api/initiatives/{code}/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "restore: {body}");
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &format!("/api/initiatives/{code}/restore"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");

    // --- whoami and my_boards show the implied capabilities.
    let (status, body) = as_user(&alice, Method::GET, "/api/whoami", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let entry = body["capabilities"]
        .as_array()
        .expect("capabilities")
        .iter()
        .find(|entry| entry["board_slug"] == "initiatives")
        .expect("the initiative board")
        .clone();
    assert_eq!(entry["grants"], json!(["member"]), "{entry}");
    assert_eq!(
        entry["implied"],
        json!(["manage_initiatives", "transition_items"]),
        "{entry}"
    );
    let session = connect(&router, &alice).await;
    let (is_error, text) = call(&router, &alice, session.as_deref(), "whoami", json!({})).await;
    assert!(!is_error, "{text}");
    assert!(
        text.lines().any(|line| line.starts_with("- initiatives (")
            && line.ends_with(&format!("{MEMBER_CAPS} (board member, no grant)"))),
        "{text}"
    );
    let (is_error, text) = call(&router, &alice, session.as_deref(), "my_boards", json!({})).await;
    assert!(!is_error, "{text}");
    let block = board_block(&text, "initiatives");
    assert!(
        block.contains(&format!("  member capabilities: {MEMBER_CAPS}").as_str()),
        "{block:?}\n{text}"
    );
    // bob has no grant: no member line.
    let session = connect(&router, &bob).await;
    let (_, text) = call(&router, &bob, session.as_deref(), "my_boards", json!({})).await;
    assert!(
        !board_block(&text, "initiatives")
            .iter()
            .any(|line| line.contains("member capabilities")),
        "{text}"
    );

    // --- The removal of the member grant removes the rights, though alice
    // keeps `configure_boards` on the board.
    let (status, body) = as_user(
        &svc,
        Method::PATCH,
        &format!("/api/boards/{board}/members/{alice_id}"),
        Some(json!({ "capabilities": ["configure_boards"] })),
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let (status, body) =
        as_user(&svc, Method::GET, &format!("/api/initiatives/{code}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let version = body["version"].as_i64().expect("version");
    let (status, body) = as_user(
        &alice,
        Method::PATCH,
        &format!("/api/initiatives/{code}"),
        Some(json!({ "content": "No longer a member.", "version": version })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = as_user(
        &alice,
        Method::POST,
        &format!("/api/initiatives/{code}/transition"),
        Some(json!({ "to_column_id": ready })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
