//! KAIROS-T-0321 (COLLIERY-I-0602) — the teams of an initiative or a
//! strategy on REST and MCP, against the booted production router.
//!
//! - REST: `GET|POST /api/{entity_type}/{short_code}/teams` and
//!   `DELETE .../teams/{team}`: read, set and clear a team by hand; the
//!   edit rule (403); a task is refused (422 RELATIONSHIP_RULE); an
//!   unknown team (422) and an unknown field (422) are refused; a second
//!   set is ALREADY_LINKED; a clear of a team from tasks is 404 with the
//!   reason.
//! - `GET /api/boards/{id}/items`: `item_teams` for each strategy and
//!   initiative with a team; `team` and `no_team` filter the strategies and
//!   the initiatives before the page; the two together are refused.
//! - MCP: `get_item` has the `- teams:` line; `board_items` has the
//!   `[teams: …]` marker and the `team`/`no_team` filter; `set_team` and
//!   `clear_team` set and clear, and refuse a task.
//!
//! The db-layer half is `kairos-db/tests/item_teams.rs`. Runs against the
//! LIVE compose stack (`angreal services up`). The test owns a scratch
//! database.

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

const SCRATCH_DB: &str = "kairos_item_teams_api_t0321_test";
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
                "clientInfo": {"name": "kairos-t0321-test", "version": "0.0.0"},
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

#[tokio::test]
async fn the_teams_of_an_initiative_on_rest_and_mcp_against_live_stack() {
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
    let bob = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));

    // svc is the org admin; bob is a member with no grant.
    for (token, email, role) in [
        (&svc, "svc@kairos.test", OrgRole::Admin),
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
    }
    let as_user = |token: &str, method: Method, uri: &str, body: Option<Value>| {
        let (router, token, uri) = (router.clone(), token.to_string(), uri.to_string());
        async move { request(&router, method, &uri, Some(&token), &TENANT, body).await }
    };

    // Two teams with delivery boards.
    for (name, slug, prefix) in [("Skadi", "skadi", "SKADI"), ("Weir", "weir", "WEIR")] {
        let (status, body) = as_user(
            &svc,
            Method::POST,
            "/api/teams",
            Some(json!({ "name": name, "slug": slug, "code_prefix": prefix })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let create = async |path: &str, body: Value| -> String {
        let (status, body) = as_user(&svc, Method::POST, path, Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{path}: {body}");
        body["short_code"].as_str().expect("short code").to_string()
    };
    let i1 = create(
        "/api/initiatives",
        json!({ "board_id": "initiatives", "title": "From tasks" }),
    )
    .await;
    let i2 = create(
        "/api/initiatives",
        json!({ "board_id": "initiatives", "title": "No team" }),
    )
    .await;
    let task = create(
        "/api/tasks",
        json!({ "board_id": "skadi-delivery", "title": "A skadi task" }),
    )
    .await;
    let (status, body) = as_user(
        &svc,
        Method::POST,
        "/api/relationships",
        Some(
            json!({ "source_short_code": i1, "target_short_code": task, "relationship": "parent" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // --- REST read: a team from tasks, and no team.
    let (status, body) = as_user(
        &bob,
        Method::GET,
        &format!("/api/initiatives/{i1}/teams"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["teams"],
        json!([{ "slug": "skadi", "name": "Skadi", "from_tasks": true, "set_by_hand": false }])
    );
    let (status, body) = as_user(
        &bob,
        Method::GET,
        &format!("/api/initiatives/{i2}/teams"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["teams"], json!([]));

    // --- REST set: the edit rule, the subject, the team, unknown fields.
    let teams_of = |code: &str| format!("/api/initiatives/{code}/teams");
    let (status, body) = as_user(
        &bob,
        Method::POST,
        &teams_of(&i2),
        Some(json!({ "team": "weir" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains("manage_initiatives"), "{body}");
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &format!("/api/tasks/{task}/teams"),
        Some(json!({ "team": "weir" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "RELATIONSHIP_RULE");
    assert!(
        body.to_string()
            .contains("Only an initiative or a strategy"),
        "{body}"
    );
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &teams_of(&i2),
        Some(json!({ "team": "nope" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("nope"), "{body}");
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &teams_of(&i2),
        Some(json!({ "team": "weir", "colour": "red" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("colour"), "{body}");

    let (status, body) = as_user(
        &svc,
        Method::POST,
        &teams_of(&i2),
        Some(json!({ "team": "weir" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body,
        json!({ "slug": "weir", "name": "Weir", "from_tasks": false, "set_by_hand": true })
    );
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &teams_of(&i2),
        Some(json!({ "team": "weir" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "ALREADY_LINKED");

    // --- The board: the map and the filter.
    let (status, body) = as_user(&bob, Method::GET, "/api/boards/initiatives/items", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["item_teams"][&i1][0]["slug"], "skadi", "{body}");
    assert_eq!(body["item_teams"][&i2][0]["slug"], "weir", "{body}");
    let codes = |body: &Value| -> Vec<String> {
        body["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .flat_map(|c| c["initiatives"].as_array().cloned().unwrap_or_default())
            .map(|i| i["short_code"].as_str().expect("code").to_string())
            .collect()
    };
    let (_, body) = as_user(
        &bob,
        Method::GET,
        "/api/boards/initiatives/items?team=skadi",
        None,
    )
    .await;
    assert_eq!(codes(&body), vec![i1.clone()], "{body}");
    assert_eq!(body["total"], 1);
    let (_, body) = as_user(
        &bob,
        Method::GET,
        "/api/boards/initiatives/items?team=weir",
        None,
    )
    .await;
    assert_eq!(codes(&body), vec![i2.clone()], "{body}");
    let i3 = create(
        "/api/initiatives",
        json!({ "board_id": "initiatives", "title": "Really no team" }),
    )
    .await;
    let (_, body) = as_user(
        &bob,
        Method::GET,
        "/api/boards/initiatives/items?no_team=true",
        None,
    )
    .await;
    assert_eq!(codes(&body), vec![i3.clone()], "{body}");
    let (status, body) = as_user(
        &bob,
        Method::GET,
        "/api/boards/initiatives/items?team=weir&no_team=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = as_user(
        &bob,
        Method::GET,
        "/api/boards/initiatives/items?team=nope",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // --- REST clear: a team from tasks is not a hand-set link.
    let (status, body) = as_user(
        &svc,
        Method::DELETE,
        &format!("/api/initiatives/{i1}/teams/skadi"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.to_string().contains("from its tasks"), "{body}");
    let (status, body) = as_user(
        &svc,
        Method::DELETE,
        &format!("/api/initiatives/{i2}/teams/weir"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "short_code": i2, "team": "weir" }));
    let (status, _) = as_user(
        &svc,
        Method::DELETE,
        &format!("/api/initiatives/{i2}/teams/weir"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // --- MCP.
    let session = connect(&router, &svc).await;
    let (is_error, text) = call(
        &router,
        &svc,
        &session,
        "get_item",
        json!({ "short_code": i1 }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert!(text.contains("- teams: skadi (from tasks)"), "{text}");
    let (_, text) = call(
        &router,
        &svc,
        &session,
        "get_item",
        json!({ "short_code": i3 }),
    )
    .await;
    assert!(text.contains("- teams: none"), "{text}");

    let (is_error, text) = call(
        &router,
        &svc,
        &session,
        "set_team",
        json!({ "short_code": i3, "team": "weir" }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert_eq!(text, format!("Set the team weir on {i3} by hand."));
    let (_, text) = call(
        &router,
        &svc,
        &session,
        "get_item",
        json!({ "short_code": i3 }),
    )
    .await;
    assert!(text.contains("- teams: weir (set by hand)"), "{text}");

    let (_, text) = call(
        &router,
        &svc,
        &session,
        "board_items",
        json!({ "board": "initiatives", "team": "weir" }),
    )
    .await;
    assert!(
        text.contains(&format!("- {i3} [initiative] Really no team [teams: weir]")),
        "{text}"
    );
    assert!(!text.contains(&i1), "{text}");
    let (_, text) = call(
        &router,
        &svc,
        &session,
        "board_items",
        json!({ "board": "initiatives", "no_team": true }),
    )
    .await;
    assert!(text.contains(&i2), "{text}");
    assert!(!text.contains(&i3), "{text}");

    let (is_error, text) = call(
        &router,
        &svc,
        &session,
        "set_team",
        json!({ "short_code": task, "team": "weir" }),
    )
    .await;
    assert!(is_error, "{text}");
    assert!(text.contains("Only an initiative or a strategy"), "{text}");
    let (is_error, text) = call(
        &router,
        &svc,
        &session,
        "clear_team",
        json!({ "short_code": i3, "team": "weir" }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert_eq!(text, format!("Cleared the team weir set by hand on {i3}."));

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
