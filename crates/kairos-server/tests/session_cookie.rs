//! KAIROS-T-0327 — the session cookie of a password login (the 2026-10-06
//! amendment of KAIROS-A-0015), against the booted production router:
//!
//! - `POST /api/login` sets the cookie with each attribute, and keeps the
//!   bearer in the body;
//! - the cookie alone authenticates a GET;
//! - a change with the cookie needs the Kairos origin: a foreign `Origin`
//!   and a request with no origin information are refused; the Kairos
//!   origin passes; a bearer needs no origin;
//! - a WebSocket handshake with the cookie needs the Kairos origin too;
//! - `POST /api/logout` with the cookie ends the session and clears the
//!   cookie, and the cookie no longer works;
//! - a deployment with local auth off still takes the cookie of a session
//!   (an OIDC login of the GUI opens one too, KAIROS-T-0364).
//!
//! Runs against the LIVE compose Postgres (`angreal services up`). No Dex.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, with_database,
};
use kairos_db::local_auth as db_local_auth;
use kairos_db::models::{NewOrganizationMember, NewUser, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::local_auth::hash_password;
use kairos_server::middleware::auth::Authenticator;
use uuid::Uuid;

const PASSWORD: &str = "correct-horse-battery-staple";
const SCRATCH_DB: &str = "kairos_session_cookie_t0327_test";
const HOST: &str = "kairos.test";

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Vec<String>, Value) {
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let cookies: Vec<String> = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().expect("ascii").to_string())
        .collect();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, cookies, body)
}

/// A request with the session cookie and the given extra headers.
fn with_cookie(
    method: Method,
    uri: &str,
    cookie: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", HOST)
        .header("x-tenant", "acme")
        .header("cookie", format!("theme=dark; kairos_session={cookie}"));
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("request")
}

fn fixture() -> (PgConnection, String) {
    drop(recreate_scratch_db(SCRATCH_DB));
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org id");
    let user: User = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: "local:ada".into(),
            user_name: "ada@example.test".into(),
            email: "ada@example.test".into(),
            display_name: "Ada".into(),
        })
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("insert user");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user.id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");
    let hash = hash_password(PASSWORD).expect("hash");
    db_local_auth::set_password(&mut conn, user.id, &hash).expect("password");
    (conn, scratch_url)
}

#[tokio::test]
async fn the_password_session_is_an_http_only_cookie_against_live_stack() {
    let (conn, scratch_url) = fixture();
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(&scratch_url);
    config.local_auth = true;
    let ttl = config.session_ttl_secs;
    let router = app::router(app::state_with(config, pool, auth));

    // --- login sets the cookie, and keeps the bearer in the body ---------
    let (status, cookies, body) = send(
        &router,
        Request::builder()
            .method(Method::POST)
            .uri("/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "email": "ada@example.test", "password": PASSWORD }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = body["token"]
        .as_str()
        .expect("the bearer stays")
        .to_string();
    assert_eq!(
        cookies,
        [format!(
            "kairos_session={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={ttl}"
        )]
    );

    // --- the cookie alone authenticates a GET ----------------------------
    let (status, _, body) = send(
        &router,
        with_cookie(Method::GET, "/api/whoami", &token, &[], None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["email"], "ada@example.test");

    // --- a change with the cookie needs the Kairos origin ----------------
    let create = |headers: &[(&str, &str)]| {
        with_cookie(
            Method::POST,
            "/api/initiatives",
            &token,
            headers,
            Some(json!({ "board_id": "initiatives", "title": "From the cookie" })),
        )
    };
    let (status, _, body) = send(&router, create(&[("origin", "https://evil.kairos.test")])).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "FORBIDDEN");
    assert!(
        body.to_string()
            .contains("does not come from the Kairos page"),
        "{body}"
    );
    let (status, _, body) = send(&router, create(&[("origin", "null")])).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, _, body) = send(&router, create(&[])).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "no origin information: {body}"
    );
    let (status, _, body) = send(&router, create(&[("sec-fetch-site", "same-site")])).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, _, body) = send(&router, create(&[("origin", "https://kairos.test")])).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _, body) = send(&router, create(&[("sec-fetch-site", "same-origin")])).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // A bearer needs no origin, also with a foreign cookie beside it.
    let (status, _, body) = send(
        &router,
        Request::builder()
            .method(Method::POST)
            .uri("/api/initiatives")
            .header("host", HOST)
            .header("x-tenant", "acme")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "board_id": "initiatives", "title": "From the bearer" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // --- a WebSocket handshake with the cookie needs the origin ----------
    let ws = |origin: &str| {
        with_cookie(
            Method::GET,
            "/ws/events",
            &token,
            &[
                ("origin", origin),
                ("connection", "upgrade"),
                ("upgrade", "websocket"),
                ("sec-websocket-version", "13"),
                ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ],
            None,
        )
    };
    let (status, _, body) = send(&router, ws("https://evil.kairos.test")).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a cross-origin handshake: {body}"
    );
    let (status, _, _) = send(&router, ws("https://kairos.test")).await;
    assert_ne!(
        status,
        StatusCode::FORBIDDEN,
        "the Kairos origin passes the auth"
    );
    assert_ne!(
        status,
        StatusCode::UNAUTHORIZED,
        "the cookie authenticates the handshake"
    );

    // --- logout with the cookie ends the session and clears it -----------
    let (status, _, _) = send(
        &router,
        with_cookie(
            Method::POST,
            "/api/logout",
            &token,
            &[("origin", "https://evil.kairos.test")],
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a cross-origin logout");
    let (status, cookies, _) = send(
        &router,
        with_cookie(
            Method::POST,
            "/api/logout",
            &token,
            &[("origin", "https://kairos.test")],
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        cookies,
        ["kairos_session=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0"]
    );
    let (status, _, _) = send(
        &router,
        with_cookie(Method::GET, "/api/whoami", &token, &[], None),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an ended session is refused"
    );

    // --- local auth off: the cookie of a session is still a credential ---
    // KAIROS-T-0364: an OIDC login of the GUI opens a session on a deployment
    // with no local accounts, so the session branch does not depend on them.
    let (status, _, body) = send(
        &router,
        Request::builder()
            .method(Method::POST)
            .uri("/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "email": "ada@example.test", "password": PASSWORD }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let live = body["token"].as_str().expect("bearer").to_string();
    let pool = TenantPool::new(&scratch_url, 2).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let off = app::router(app::state_with(base_config(&scratch_url), pool, auth));
    let (status, _, body) = send(
        &off,
        with_cookie(Method::GET, "/api/whoami", &live, &[], None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, _) = send(
        &off,
        with_cookie(Method::GET, "/api/whoami", &token, &[], None),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the ended session stays ended"
    );

    drop(conn);
    let mut admin = PgConnection::establish(&common::admin_database_url()).expect("admin");
    drop_scratch_db(&mut admin, SCRATCH_DB);
}
