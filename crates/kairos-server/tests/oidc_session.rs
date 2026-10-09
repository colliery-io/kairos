//! KAIROS-T-0364 — a reload of the GUI keeps an OIDC session.
//!
//! An issuer that gives no refresh token (Google: no `offline_access`) left
//! the GUI nothing to restore a session from after a reload, so the person
//! was sent to sign in again. The GUI now opens a Kairos session from its
//! OIDC login, as a password login does (KAIROS-T-0327): the session is an
//! HttpOnly cookie, and a reload restores it.
//!
//! On a deployment with an issuer and NO local accounts:
//!
//! - `POST /api/session` with an OIDC bearer opens a session, sets the
//!   cookie, and returns the session bearer;
//! - the cookie alone authenticates a GET, as the same person;
//! - `POST /api/session` refuses a request with no OIDC bearer: the cookie
//!   alone, a session bearer;
//! - `POST /api/logout` with the cookie ends the session and clears it.
//!
//! Runs against the LIVE compose stack (`angreal services up`): Postgres and
//! Dex. Owns the scratch database `kairos_oidc_session_t0364_test`.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_oidc_session_t0364_test";
const HOST: &str = "kairos.test";

/// One request against the router: method, URI, an optional bearer, an
/// optional session cookie, extra headers. Returns the status, the
/// `Set-Cookie` values and the JSON body.
async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    cookie: Option<&str>,
    headers: &[(&str, &str)],
) -> (StatusCode, Vec<String>, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", HOST)
        .header("x-tenant", "acme");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(token) = cookie {
        builder = builder.header("cookie", format!("kairos_session={token}"));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(Body::empty()).expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let cookies = response
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

#[tokio::test]
async fn an_oidc_login_opens_a_session_that_a_reload_keeps_against_live_stack() {
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

    let http = reqwest::Client::new();
    let oidc = user_token(&http, "alice").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    // An issuer and no local accounts: the shape of a Google deployment.
    let config = base_config(&scratch_url);
    assert!(!config.local_auth);
    let ttl = config.session_ttl_secs;
    let router = app::router(app::state_with(config, pool, auth));

    // The first request provisions alice (JIT); then she joins acme.
    let _ = send(&router, Method::GET, "/api/whoami", Some(&oidc), None, &[]).await;
    let alice: Uuid = users::table
        .filter(users::email.eq("alice@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("alice is provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("membership");

    // --- the OIDC login opens a session ----------------------------------
    let (status, cookies, body) = send(
        &router,
        Method::POST,
        "/api/session",
        Some(&oidc),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = body["token"]
        .as_str()
        .expect("the session bearer")
        .to_string();
    assert!(token.starts_with("kairos_ss_"), "{token}");
    assert_eq!(body["user"]["email"], "alice@kairos.test");
    assert_eq!(
        cookies,
        [format!(
            "kairos_session={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={ttl}"
        )]
    );

    // --- a reload: the cookie alone is alice -----------------------------
    let (status, _, body) =
        send(&router, Method::GET, "/api/whoami", None, Some(&token), &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["id"], alice.to_string());

    // --- only an OIDC bearer opens a session -----------------------------
    let same_origin = [("origin", "https://kairos.test")];
    let (status, cookies, body) = send(
        &router,
        Method::POST,
        "/api/session",
        None,
        Some(&token),
        &same_origin,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the cookie alone: {body}");
    assert_eq!(error_code(&body), "FORBIDDEN");
    assert!(cookies.is_empty());
    let (status, _, body) = send(
        &router,
        Method::POST,
        "/api/session",
        Some(&token),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a session bearer: {body}");
    let (status, _, _) = send(&router, Method::POST, "/api/session", None, None, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no credential");

    // --- logout with the cookie ends the session and clears it -----------
    let (status, cookies, _) = send(
        &router,
        Method::POST,
        "/api/logout",
        None,
        Some(&token),
        &same_origin,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        cookies,
        ["kairos_session=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0"]
    );
    let (status, _, _) = send(&router, Method::GET, "/api/whoami", None, Some(&token), &[]).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an ended session is refused"
    );

    drop(conn);
    let mut admin = PgConnection::establish(&common::admin_database_url()).expect("admin");
    drop_scratch_db(&mut admin, SCRATCH_DB);
}
