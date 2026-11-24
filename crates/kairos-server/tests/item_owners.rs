//! KAIROS-T-0411 (KAIROS-A-0025 decisions 2 to 7) — a strategy or an
//! initiative has one owner, by claim, against the booted production
//! router:
//!
//! - a new item has no owner;
//! - a member of the board (any grant on it) claims an item with no owner;
//!   a person with no grant is refused; a second claim is refused with the
//!   name of the owner;
//! - the owner hands it off, only to a member of the board
//!   (`NOT_BOARD_MEMBER` else); a member who is not the owner cannot hand
//!   it off or release it; the owner releases it (KAIROS-A-0025 amendments
//!   3 and 4); an org admin clears it and sets it, also only to a member of
//!   the board;
//! - the owner stays in each column, Completed too, and in the archive;
//! - the owner gives no right: an owner with no grant cannot edit;
//! - an owner who leaves the organization (`DELETE /api/members`): the
//!   item has no owner;
//! - REST (the item, the board items, `/owner`) and MCP (`get_item`,
//!   `board_items`, `claim_item`, `hand_off_item`, `release_item`) show
//!   and change it; MCP refuses an unknown argument and names it.
//!
//! The db-layer half is `kairos-db/tests/item_owners.rs`.
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

const SCRATCH_DB: &str = "kairos_item_owners_t0411_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];
/// The tenant resolves from the host, as for each MCP request.
const HOST_HEADER: &str = "acme.kairos.test";

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
                "clientInfo": {"name": "kairos-t0411-test", "version": "0.0.0"},
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

/// The owner of an item by REST: the display name, or `None`.
async fn rest_owner(router: &Router, token: &str, code: &str) -> Option<String> {
    let (status, body) = request(
        router,
        Method::GET,
        &format!("/api/initiatives/{code}"),
        Some(token),
        &TENANT,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["owner"]["display_name"].as_str().map(str::to_string)
}

#[tokio::test]
async fn a_strategy_or_an_initiative_has_one_owner_by_claim_against_live_stack() {
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
    let carol = user_token(&http, "carol").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));

    // svc is the org admin; alice, bob and carol are members with no grant.
    let mut ids = Vec::new();
    for (token, email, role) in [
        (&svc, "svc@kairos.test", OrgRole::Admin),
        (&alice, "alice@kairos.test", OrgRole::Member),
        (&bob, "bob@kairos.test", OrgRole::Member),
        (&carol, "carol@kairos.test", OrgRole::Member),
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
    let (alice_id, bob_id, carol_id) = (ids[1], ids[2], ids[3]);

    let as_user = |token: &str, method: Method, uri: &str, body: Option<Value>| {
        let (router, token, uri) = (router.clone(), token.to_string(), uri.to_string());
        async move { request(&router, method, &uri, Some(&token), &TENANT, body).await }
    };

    // svc creates an initiative and a strategy.
    let (status, body) = as_user(
        &svc,
        Method::POST,
        "/api/initiatives",
        Some(json!({ "board_id": "initiatives", "title": "Shared work" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let code = body["short_code"].as_str().expect("code").to_string();
    assert!(
        body.get("owner").is_none(),
        "a new item has no owner: {body}"
    );
    let (status, body) = as_user(
        &svc,
        Method::POST,
        "/api/strategies",
        Some(json!({ "board_id": "strategy", "title": "Direction" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let strategy = body["short_code"].as_str().expect("code").to_string();
    let owner_uri = format!("/api/initiatives/{code}/owner");
    let (status, body) = as_user(&bob, Method::GET, &owner_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["owner"], Value::Null, "{body}");

    // --- A person with no grant on the board cannot claim.
    let claim_uri = format!("{owner_uri}/claim");
    let (status, body) = as_user(&alice, Method::POST, &claim_uri, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains("member of its board"), "{body}");

    // --- Any grant makes a member: alice and bob.
    let (status, body) = as_user(&svc, Method::GET, "/api/boards/initiatives", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let board = body["id"].as_str().expect("board id").to_string();
    for (user, capability) in [(alice_id, "configure_boards"), (bob_id, "transition_items")] {
        let (status, body) = as_user(
            &svc,
            Method::POST,
            &format!("/api/boards/{board}/members"),
            Some(json!({ "user_id": user, "capabilities": [capability] })),
        )
        .await;
        assert!(status.is_success(), "{status}: {body}");
    }

    // alice claims; bob is refused with the name of the owner.
    let (status, body) = as_user(&alice, Method::POST, &claim_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["owner"]["user_id"],
        json!(alice_id.to_string()),
        "{body}"
    );
    let (status, body) = as_user(&bob, Method::POST, &claim_uri, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let alice_name = rest_owner(&router, &bob, &code).await.expect("an owner");
    assert!(body.to_string().contains(&alice_name), "{body}");

    // --- Nobody else changes it: bob cannot hand off or release.
    let hand_off_uri = format!("{owner_uri}/hand-off");
    let (status, body) = as_user(
        &bob,
        Method::POST,
        &hand_off_uri,
        Some(json!({ "to": bob_id.to_string() })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = as_user(&bob, Method::DELETE, &owner_uri, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        rest_owner(&router, &bob, &code).await,
        Some(alice_name.clone())
    );
    // An unknown field of the hand-off is refused and named.
    let (status, body) = as_user(
        &alice,
        Method::POST,
        &hand_off_uri,
        Some(json!({ "to": carol_id.to_string(), "reason": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("reason"), "{body}");

    // --- The owner stays in each column, Completed too.
    for name in [
        "Design",
        "Decompose",
        "Ready",
        "Active",
        "Monitoring",
        "Completed",
    ] {
        let to = column_id(&mut conn, "initiatives", name);
        let (status, body) = as_user(
            &bob,
            Method::POST,
            &format!("/api/initiatives/{code}/transition"),
            Some(json!({ "to_column_id": to })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");
        assert_eq!(body["owner"]["display_name"], json!(alice_name), "{name}");
    }
    // The board items show the owner on the card.
    let (status, body) = as_user(
        &bob,
        Method::GET,
        &format!("/api/boards/{board}/items"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let card = body["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .flat_map(|c| c["initiatives"].as_array().cloned().unwrap_or_default())
        .find(|i| i["short_code"] == json!(code))
        .expect("the card");
    assert_eq!(card["owner"]["display_name"], json!(alice_name), "{card}");

    // --- A hand-off to carol, who has no grant on the board, is refused.
    let (status, body) = as_user(
        &alice,
        Method::POST,
        &hand_off_uri,
        Some(json!({ "to": carol_id.to_string() })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "NOT_BOARD_MEMBER", "{body}");
    assert_eq!(
        rest_owner(&router, &bob, &code).await,
        Some(alice_name.clone())
    );
    // carol gets a grant; the owner hands it off to her.
    let carol_member = format!("/api/boards/{board}/members/{carol_id}");
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &format!("/api/boards/{board}/members"),
        Some(json!({ "user_id": carol_id, "capabilities": ["configure_boards"] })),
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let (status, body) = as_user(
        &alice,
        Method::POST,
        &hand_off_uri,
        Some(json!({ "to": carol_id.to_string() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["owner"]["user_id"],
        json!(carol_id.to_string()),
        "{body}"
    );
    // The owner gives no right: carol loses her grant, stays the owner, and
    // cannot edit it.
    let (status, body) = as_user(&svc, Method::DELETE, &carol_member, None).await;
    assert!(status.is_success(), "{status}: {body}");
    assert!(rest_owner(&router, &bob, &code).await.is_some());
    let (status, body) =
        as_user(&svc, Method::GET, &format!("/api/initiatives/{code}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let version = body["version"].as_i64().expect("version");
    let (status, body) = as_user(
        &carol,
        Method::PATCH,
        &format!("/api/initiatives/{code}"),
        Some(json!({ "content": "The owner edits.", "version": version })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    // carol is told.
    let told: i64 = diesel::sql_query(format!(
        "SELECT count(*) AS n FROM org_acme.notifications WHERE user_id = '{carol_id}' \
         AND kind = 'hand_off'"
    ))
    .get_result::<Count>(&mut conn)
    .expect("notifications")
    .n;
    assert_eq!(told, 1);

    // --- An org admin clears it, and sets it, only to a member of the
    // board.
    let (status, body) = as_user(&svc, Method::DELETE, &owner_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(rest_owner(&router, &bob, &code).await, None);
    let (status, body) = as_user(&svc, Method::DELETE, &owner_uri, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "NO_OWNER", "{body}");
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &hand_off_uri,
        Some(json!({ "to": carol_id.to_string() })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "NOT_BOARD_MEMBER", "{body}");
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &hand_off_uri,
        Some(json!({ "to": bob_id.to_string() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let bob_name = rest_owner(&router, &alice, &code).await.expect("bob");

    // --- MCP: get_item and board_items show it; the tools change it.
    let session = connect(&router, &bob).await;
    let (is_error, text) = call(
        &router,
        &bob,
        session.as_deref(),
        "get_item",
        json!({ "short_code": code }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(
        text.lines()
            .any(|l| l.starts_with(&format!("- owner: {bob_name} since "))),
        "{text}"
    );
    let (is_error, text) = call(
        &router,
        &bob,
        session.as_deref(),
        "board_items",
        json!({ "board": "initiatives" }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(
        text.lines()
            .any(|l| l.starts_with(&format!("- {code} "))
                && l.contains(&format!("[owner: {bob_name}]"))),
        "{text}"
    );
    // A claim of an owned item is refused.
    let (is_error, text) = call(
        &router,
        &alice,
        connect(&router, &alice).await.as_deref(),
        "claim_item",
        json!({ "short_code": code }),
    )
    .await;
    assert!(is_error, "{text}");
    assert!(text.contains(&bob_name), "{text}");
    // The owner hands off to alice.
    let (is_error, text) = call(
        &router,
        &bob,
        session.as_deref(),
        "hand_off_item",
        json!({ "short_code": code, "to": "alice@kairos.test" }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(
        text.contains(&format!("- owner: {alice_name} since ")),
        "{text}"
    );
    // A hand-off to a person with no grant on the board is refused.
    let alice_session = connect(&router, &alice).await;
    let (is_error, text) = call(
        &router,
        &alice,
        alice_session.as_deref(),
        "hand_off_item",
        json!({ "short_code": code, "to": "carol@kairos.test" }),
    )
    .await;
    assert!(is_error, "{text}");
    assert!(text.contains("NOT_BOARD_MEMBER"), "{text}");
    // release_item: a member who is not the owner cannot; the owner can.
    let (is_error, text) = call(
        &router,
        &bob,
        session.as_deref(),
        "release_item",
        json!({ "short_code": code }),
    )
    .await;
    assert!(is_error, "{text}");
    assert!(text.contains("Only the owner"), "{text}");
    let (is_error, text) = call(
        &router,
        &alice,
        alice_session.as_deref(),
        "release_item",
        json!({ "short_code": code }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(text.contains(&format!("Released {code}")), "{text}");
    let released: i64 = diesel::sql_query(format!(
        "SELECT count(*) AS n FROM org_acme.activity_log WHERE action = 'release' \
         AND details = 'release:{alice_id} reason:release'"
    ))
    .get_result::<Count>(&mut conn)
    .expect("activity")
    .n;
    assert_eq!(released, 1);
    // An org admin can release the owner of a different person.
    let (is_error, text) = call(
        &router,
        &bob,
        session.as_deref(),
        "claim_item",
        json!({ "short_code": code }),
    )
    .await;
    assert!(!is_error, "{text}");
    let svc_session = connect(&router, &svc).await;
    let (is_error, text) = call(
        &router,
        &svc,
        svc_session.as_deref(),
        "release_item",
        json!({ "short_code": code }),
    )
    .await;
    assert!(!is_error, "{text}");
    // A member claims by MCP.
    let (is_error, text) = call(
        &router,
        &alice,
        alice_session.as_deref(),
        "claim_item",
        json!({ "short_code": code }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(
        text.contains(&format!("- owner: {alice_name} since ")),
        "{text}"
    );
    // A strategy: alice has no grant on the strategy board.
    let (is_error, text) = call(
        &router,
        &alice,
        alice_session.as_deref(),
        "claim_item",
        json!({ "short_code": strategy }),
    )
    .await;
    assert!(is_error, "{text}");
    let (is_error, text) = call(
        &router,
        &alice,
        alice_session.as_deref(),
        "get_item",
        json!({ "short_code": strategy }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(text.contains("- owner: none."), "{text}");
    // An unknown argument is refused, and named.
    let (status, _, body) = mcp_request(
        &router,
        &alice,
        alice_session.as_deref(),
        json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": {"name": "claim_item",
                       "arguments": {"short_code": strategy, "owner": "me"}},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains(r#"the argument \"owner\""#) && body.contains("does not accept"),
        "{body}"
    );

    // --- The archive keeps the owner.
    let (status, body) = as_user(
        &svc,
        Method::DELETE,
        &format!("/api/initiatives/{code}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    assert_eq!(
        rest_owner(&router, &bob, &code).await,
        Some(alice_name.clone())
    );
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &format!("/api/initiatives/{code}/restore"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");

    // --- The owner leaves the organization: the item has no owner.
    let (status, body) = as_user(
        &svc,
        Method::DELETE,
        &format!("/api/members/{alice_id}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    assert_eq!(rest_owner(&router, &bob, &code).await, None);
    let left: i64 = diesel::sql_query(format!(
        "SELECT count(*) AS n FROM org_acme.activity_log WHERE action = 'release' \
         AND details = 'release:{alice_id} reason:left'"
    ))
    .get_result::<Count>(&mut conn)
    .expect("activity")
    .n;
    assert_eq!(left, 1);

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}
