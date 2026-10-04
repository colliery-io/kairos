//! COLLIERY-T-3108 — a member of a team writes ADRs on the ADR board of the
//! team with no grant, against the booted production router.
//!
//! - The team rule gives a member of the team `manage_adrs` on the ADR
//!   board of the team: `POST /api/adrs` and `PATCH /api/adrs/{code}` work
//!   with no grant, and so does the MCP tool `create_item`.
//! - A member of a different team does not get `manage_adrs` there (403).
//! - The ADR board of the organization has no team: `manage_adrs` there
//!   still needs a grant (403 with no grant, 201 with one).
//! - The MCP tools `whoami` and `my_boards` show the capability.
//!
//! The rule is computed when the server reads it
//! (`kairos_db::abac::check_capability`). Nothing is stored. The db-layer
//! half is `kairos-db/tests/team_adr_boards.rs`.
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
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    user_token, with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_team_adr_writes_t3108_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];
/// The tenant resolves from the host, as for each MCP request.
const HOST_HEADER: &str = "acme.kairos.test";

const ADR_BOARD_TEAM_CAPS: &str = "manage_tasks, manage_documents, transition_items, manage_adrs";

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

/// Start an MCP session. Returns the id of the session.
async fn connect(router: &Router, token: &str) -> String {
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
                "clientInfo": {"name": "colliery-t3108-test", "version": "0.0.0"},
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "initialize: {body}");
    let session = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .expect("initialize returns Mcp-Session-Id")
        .to_string();
    let (status, _, body) = mcp_request(
        router,
        token,
        Some(&session),
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
    session: &str,
    tool: &str,
    arguments: Value,
) -> (bool, String) {
    let (status, _, body) = mcp_request(
        router,
        token,
        Some(session),
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

#[tokio::test]
async fn team_members_write_adrs_on_the_adr_board_of_their_team_against_live_stack() {
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

    // JIT-provision each user. svc is the org admin; alice and bob are
    // members with no grant.
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
    let (alice_id, bob_id) = (ids[1], ids[2]);

    let as_user = |token: &str, method: Method, uri: &str, body: Option<Value>| {
        let (router, token, uri) = (router.clone(), token.to_string(), uri.to_string());
        async move { request(&router, method, &uri, Some(&token), &TENANT, body).await }
    };

    // Two teams: alice is in skadi, bob is in crt.
    let team = async |name: &str, slug: &str, prefix: &str, member: Uuid| -> String {
        let (status, body) = as_user(
            &svc,
            Method::POST,
            "/api/teams",
            Some(json!({ "name": name, "slug": slug, "code_prefix": prefix })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let id = body["id"].as_str().expect("team id").to_string();
        let (status, body) = as_user(
            &svc,
            Method::POST,
            &format!("/api/teams/{id}/members"),
            Some(json!({ "user_id": member })),
        )
        .await;
        assert!(status.is_success(), "{status}: {body}");
        id
    };
    let skadi = team("Skadi", "skadi", "SKADI", alice_id).await;
    let _crt = team("Crt", "crt", "CRT", bob_id).await;

    // The ADR board of skadi.
    let (status, body) = as_user(
        &svc,
        Method::POST,
        "/api/boards",
        Some(json!({
            "name": "Skadi ADRs",
            "slug": "skadi-adrs",
            "board_level": "adr",
            "team_id": skadi,
            "code_prefix": "SKADI",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // --- AC1: a member of the team creates an ADR with no grant.
    let (status, body) = as_user(
        &alice,
        Method::POST,
        "/api/adrs",
        Some(json!({ "board_id": "skadi-adrs", "title": "Use Postgres" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["short_code"], "SKADI-A-0001", "{body}");

    // ...and edits an ADR that a different user created: the edit rule
    // passes on `manage_adrs`, not on "created it".
    let (status, body) = as_user(
        &svc,
        Method::POST,
        "/api/adrs",
        Some(json!({ "board_id": "skadi-adrs", "title": "Use Rust" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let svc_adr = body["short_code"].as_str().expect("code").to_string();
    let version = body["version"].as_i64().expect("version");
    let (status, body) = as_user(
        &alice,
        Method::PATCH,
        &format!("/api/adrs/{svc_adr}"),
        Some(json!({ "content": "Rust, edited by a team member.", "version": version })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // --- AC2: a member of a different team does not get `manage_adrs`.
    let (status, body) = as_user(
        &bob,
        Method::POST,
        "/api/adrs",
        Some(json!({ "board_id": "skadi-adrs", "title": "Use MySQL" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "FORBIDDEN");
    assert!(body.to_string().contains("manage_adrs"), "{body}");
    let (status, body) = as_user(
        &bob,
        Method::PATCH,
        &format!("/api/adrs/{svc_adr}"),
        Some(json!({ "content": "Not mine.", "version": version + 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // --- AC3: the ADR board of the organization has no team. A member
    // --- needs a grant there.
    let (status, body) = as_user(
        &alice,
        Method::POST,
        "/api/adrs",
        Some(json!({ "board_id": "adrs", "title": "Org decision" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains("manage_adrs"), "{body}");
    let (status, body) = as_user(&svc, Method::GET, "/api/boards/adrs", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["team_id"].is_null(), "{body}");
    let org_board = body["id"].as_str().expect("board id").to_string();
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &format!("/api/boards/{org_board}/members"),
        Some(json!({ "user_id": alice_id, "capabilities": ["manage_adrs"] })),
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let (status, body) = as_user(
        &alice,
        Method::POST,
        "/api/adrs",
        Some(json!({ "board_id": "adrs", "title": "Org decision" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "with a grant: {body}");

    // --- MCP: create_item writes an ADR on the board of the team.
    let session = connect(&router, &alice).await;
    let (is_error, text) = call(
        &router,
        &alice,
        &session,
        "create_item",
        json!({ "item_type": "adr", "title": "Use Tokio", "board": "skadi-adrs" }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(text.contains("SKADI-A-"), "{text}");

    // --- AC4: whoami and my_boards show the capability.
    let (is_error, text) = call(&router, &alice, &session, "whoami", json!({})).await;
    assert!(!is_error, "{text}");
    assert!(
        text.contains(&format!(
            "- skadi-adrs (Skadi ADRs): {ADR_BOARD_TEAM_CAPS} (team skadi, no grant)"
        )),
        "{text}"
    );
    assert!(
        text.contains("- skadi-delivery (")
            && text.contains(
                "manage_tasks, manage_documents, transition_items (team skadi, no grant)"
            ),
        "the delivery board of the team gives the delivery set: {text}"
    );
    let (is_error, text) = call(&router, &alice, &session, "my_boards", json!({})).await;
    assert!(!is_error, "{text}");
    let block = board_block(&text, "skadi-adrs");
    assert!(
        block.contains(&format!("  team capabilities: {ADR_BOARD_TEAM_CAPS}").as_str()),
        "{block:?}\n{text}"
    );
    // The organization ADR board shows no team capabilities.
    assert!(
        !board_block(&text, "adrs")
            .iter()
            .any(|line| line.contains("team capabilities")),
        "{text}"
    );

    // bob is not in skadi: no team capabilities on skadi-adrs.
    let session = connect(&router, &bob).await;
    let (_, text) = call(&router, &bob, &session, "whoami", json!({})).await;
    assert!(!text.contains("skadi-adrs"), "{text}");
    let (_, text) = call(&router, &bob, &session, "my_boards", json!({})).await;
    assert!(
        !board_block(&text, "skadi-adrs")
            .iter()
            .any(|line| line.contains("team capabilities")),
        "{text}"
    );

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
