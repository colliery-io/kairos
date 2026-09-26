//! KAIROS-T-0039 — GUI serving + SPA auth support (`kairos_server::web`,
//! A-0015): the public `/api/config` discovery endpoint, the
//! `/api/auth/token` relay against the live Dex, and the SPA fallback in
//! its three modes (dist dir, reserved-prefix 404s, placeholder).
//!
//! Uses the shared compose stack (Postgres for the pool, Dex as the
//! issuer) with this binary's own scratch database, like every other
//! integration suite here.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use common::{AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, with_database};
use kairos_db::TenantPool;
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary.
const SCRATCH_DB: &str = "kairos_web_t0039_test";

/// One in-process request, returning status + content-type + raw body
/// (the shared `common::request` insists on JSON bodies; the SPA serves
/// HTML/CSS).
async fn raw_request(
    router: &Router,
    method: Method,
    uri: &str,
    form_body: Option<&str>,
) -> (StatusCode, String, String) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match form_body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        content_type,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test]
async fn web_surfaces_against_live_stack() {
    // --- scratch database (pool construction needs a real DB) ------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let config = base_config(&scratch_url);
    let router = app::router(app::state_with(config.clone(), pool.clone(), auth.clone()));

    // --- 1. /api/config is public and carries the SPA's login inputs -----
    let (status, content_type, body) = raw_request(&router, Method::GET, "/api/config", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        content_type.starts_with("application/json"),
        "{content_type}"
    );
    let config_json: Value = serde_json::from_str(&body).expect("config is JSON");
    assert_eq!(config_json["issuer"], ISSUER, "{config_json}");
    assert_eq!(config_json["client_id"], "kairos-web", "{config_json}");
    assert_eq!(
        config_json["authorization_endpoint"],
        format!("{ISSUER}/auth"),
        "discovery resolved against the live Dex: {config_json}"
    );
    // KAIROS-T-0205: an OIDC-only deployment reports local_auth FALSE, so the SPA
    // renders the provider button and no password form.
    //
    // This is the server half of "an OIDC-only login page is unchanged". The GUI's
    // branch is a pure function of these three fields — asserted over all three
    // combinations in `kairos_web::auth`'s tests — and the e2e suite proves the
    // provider button still works, so the assertion that cannot be made in a browser
    // without a second server is made here instead.
    assert_eq!(
        config_json["local_auth"], false,
        "local auth is off unless asked for: {config_json}"
    );

    // --- 2. the token relay refuses non-SPA grants outright --------------
    let (status, _, body) = raw_request(
        &router,
        Method::POST,
        "/api/auth/token",
        Some("grant_type=password&username=alice%40kairos.test&password=alice-password"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let envelope: Value = serde_json::from_str(&body).expect("S-0005 envelope");
    assert_eq!(envelope["error"]["code"], "VALIDATION", "{envelope}");

    // …and names the missing field for a half-formed grant.
    let (status, _, body) = raw_request(
        &router,
        Method::POST,
        "/api/auth/token",
        Some("grant_type=refresh_token"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("refresh_token is required"), "{body}");

    // --- 3. a well-formed grant is relayed to the live issuer: a bogus
    //        code comes back as Dex's own OAuth error, passed through ----
    let (status, content_type, body) = raw_request(
        &router,
        Method::POST,
        "/api/auth/token",
        Some(
            "grant_type=authorization_code&code=bogus-code\
             &redirect_uri=http%3A%2F%2Flocalhost%3A8080%2Fcallback\
             &code_verifier=not-the-verifier",
        ),
    )
    .await;
    assert!(
        status.is_client_error(),
        "issuer must reject the bogus code: {status} {body}"
    );
    assert!(
        content_type.starts_with("application/json"),
        "{content_type}"
    );
    let oauth_error: Value = serde_json::from_str(&body).expect("OAuth error JSON passthrough");
    assert!(
        oauth_error["error"].is_string(),
        "expected a standard OAuth error object, got: {oauth_error}"
    );

    // --- 4. reserved prefixes 404 with the S-0005 envelope, never HTML ---
    for reserved in [
        "/api/no-such-route",
        "/mcp/nope",
        "/scim/v2/nope",
        "/ws/nope",
    ] {
        let (status, content_type, body) = raw_request(&router, Method::GET, reserved, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{reserved}: {body}");
        assert!(
            content_type.starts_with("application/json"),
            "{reserved} must not fall back to the SPA: {content_type}"
        );
        let envelope: Value = serde_json::from_str(&body).expect("S-0005 envelope");
        assert_eq!(envelope["error"]["code"], "NOT_FOUND", "{envelope}");
    }

    // --- 5. no dist dir, no embed: `/` answers with the placeholder ------
    let (status, content_type, body) = raw_request(&router, Method::GET, "/", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(content_type.starts_with("text/html"), "{content_type}");
    assert!(
        body.contains("angreal web build"),
        "placeholder must say how to build the GUI: {body}"
    );

    // Non-GET methods never reach the SPA.
    let (status, _, _) = raw_request(&router, Method::POST, "/boards", Some("x=1")).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    // --- 6. KAIROS_WEB_DIST serving: assets, SPA fallback, 404s ----------
    let dist = std::env::temp_dir().join(format!("kairos_web_t0039_dist_{}", std::process::id()));
    std::fs::create_dir_all(&dist).expect("temp dist dir");
    std::fs::write(
        dist.join("index.html"),
        "<!doctype html><title>kairos-test-gui</title>",
    )
    .expect("index.html");
    std::fs::write(dist.join("app-abc123.css"), "body{}").expect("hashed asset");

    let mut dist_config = base_config(&scratch_url);
    dist_config.web_dist = Some(dist.clone());
    let dist_router = app::router(app::state_with(dist_config, pool, auth));

    // The entry point.
    let (status, content_type, body) = raw_request(&dist_router, Method::GET, "/", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(content_type.starts_with("text/html"), "{content_type}");
    assert!(body.contains("kairos-test-gui"), "{body}");

    // A hashed asset, with its own content type.
    let (status, content_type, body) =
        raw_request(&dist_router, Method::GET, "/app-abc123.css", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(content_type.starts_with("text/css"), "{content_type}");

    // A client-side route: index.html (the SPA router takes it from here).
    let (status, content_type, body) =
        raw_request(&dist_router, Method::GET, "/boards/acme", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(content_type.starts_with("text/html"), "{content_type}");
    assert!(body.contains("kairos-test-gui"), "{body}");

    // A missing asset-like path is a real 404, not HTML.
    let (status, _, _) = raw_request(&dist_router, Method::GET, "/missing.css", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Path traversal is refused (no file outside the dist dir is read).
    let (status, _, _) =
        raw_request(&dist_router, Method::GET, "/assets/../../Cargo.toml", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // API routes still win over the SPA on the dist-serving router too.
    let (status, content_type, _) =
        raw_request(&dist_router, Method::GET, "/api/config", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type.starts_with("application/json"),
        "{content_type}"
    );

    // --- teardown ---------------------------------------------------------
    let _ = std::fs::remove_dir_all(&dist);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
