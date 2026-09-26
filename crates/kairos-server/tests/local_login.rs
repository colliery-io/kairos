//! Integration test for local password login (KAIROS-T-0203, KAIROS-I-0018):
//! `POST /api/login` mints a session bearer, `require_auth` accepts it as its
//! third branch, `POST /api/logout` revokes it, and — the part that is a security
//! property rather than a feature — every way to fail looks and costs the same.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`). No Dex: the
//! session path never validates a JWT, and one test here asserts that an OIDC
//! deployment with local auth OFF is unchanged.

mod common;

use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::{Duration, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, with_database,
};
use kairos_db::local_auth::{self as db_local_auth, NewLocalSession};
use kairos_db::models::{NewOrganizationMember, NewUser, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::local_auth::{hash_password, hash_session_token};
use kairos_server::middleware::auth::Authenticator;
use uuid::Uuid;

const PASSWORD: &str = "correct-horse-battery-staple";

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("non-JSON body: {e}"))
    };
    (status, body)
}

/// `POST /api/login`, with how long it took.
///
/// The duration is part of the contract here: the failure paths must cost roughly
/// the same, or the timing answers the question the message refuses to.
async fn login(router: &Router, email: &str, password: &str) -> (StatusCode, Value, StdDuration) {
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "email": email, "password": password }).to_string(),
        ))
        .expect("request");
    let started = Instant::now();
    let (status, body) = send(router, request).await;
    (status, body, started.elapsed())
}

async fn whoami(router: &Router, token: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/whoami")
        .header("authorization", format!("Bearer {token}"))
        .header("x-tenant", "acme")
        .body(Body::empty())
        .expect("request");
    send(router, request).await
}

async fn logout(router: &Router, token: &str) -> StatusCode {
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/logout")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    send(router, request).await.0
}

fn make_user(conn: &mut PgConnection, email: &str, org_id: Uuid, with_password: bool) -> User {
    let user: User = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: format!("oidc:{email}"),
            user_name: email.to_string(),
            email: email.to_string(),
            display_name: format!("Person {email}"),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user.id,
            role: OrgRole::Member,
        })
        .execute(conn)
        .expect("insert membership");
    if with_password {
        let hash = hash_password(PASSWORD).expect("hash");
        db_local_auth::set_password(conn, user.id, &hash).expect("set password");
    }
    user
}

/// Scratch DB + provisioned `acme`, and the org id to hang members off.
fn fixture(scratch_db: &str) -> (PgConnection, String, Uuid) {
    drop(recreate_scratch_db(scratch_db));
    let scratch_url = with_database(&common::admin_database_url(), scratch_db);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org id");
    (conn, scratch_url, org_id)
}

/// Drop the scratch database. A fresh admin connection, because the scratch one
/// has to be closed first or the DROP finds it in use.
fn teardown(conn: PgConnection, scratch_db: &str) {
    drop(conn);
    let mut admin = PgConnection::establish(&common::admin_database_url()).expect("admin");
    drop_scratch_db(&mut admin, scratch_db);
}

#[tokio::test]
async fn logging_in_with_a_password_works_and_every_failure_looks_the_same() {
    let scratch_db = "kairos_local_login_t0203_test";
    let (mut conn, scratch_url, org_id) = fixture(scratch_db);

    make_user(&mut conn, "local@example.test", org_id, true);
    make_user(&mut conn, "oidc-only@example.test", org_id, false);

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(&scratch_url);
    config.local_auth = true;
    // High, so the throttle does not fire during the failure-shape assertions
    // below. Its own behaviour on this endpoint is asserted separately.
    config.auth_max_failures = 100;
    let router = app::router(app::state_with(config, pool, auth));

    // --- the happy path ---------------------------------------------------
    let (status, body, good_elapsed) = login(&router, "local@example.test", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = body["token"].as_str().expect("a token").to_string();
    assert!(token.starts_with("kairos_ss_"), "token shape: {token}");
    assert!(!token.contains("acme"), "a session carries no tenant");
    assert_eq!(body["user"]["email"], "local@example.test");
    let expires_at =
        chrono::DateTime::parse_from_rfc3339(body["expires_at"].as_str().expect("expires_at"))
            .expect("RFC 3339");
    let fortnight = Utc::now() + Duration::days(14);
    assert!(
        (expires_at.with_timezone(&Utc) - fortnight)
            .num_minutes()
            .abs()
            < 5,
        "the default lifetime should be a fortnight, got {expires_at}"
    );

    // The email is matched case-insensitively and trimmed, because a login form is
    // not a place to be pedantic about what someone typed.
    let (status, _, _) = login(&router, "  LOCAL@Example.TEST  ", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "email matching is lenient");

    // --- require_auth accepts the session --------------------------------
    let (status, body) = whoami(&router, &token).await;
    assert_eq!(status, StatusCode::OK, "session bearer accepted: {body}");
    assert_eq!(body["user"]["email"], "local@example.test");

    // --- every failure is the same 401 -----------------------------------
    let (wrong_status, wrong_body, wrong_elapsed) =
        login(&router, "local@example.test", "not-the-password").await;
    let (unknown_status, unknown_body, unknown_elapsed) =
        login(&router, "nobody@example.test", PASSWORD).await;
    let (oidc_status, oidc_body, oidc_elapsed) =
        login(&router, "oidc-only@example.test", PASSWORD).await;

    for (label, status) in [
        ("wrong password", wrong_status),
        ("unknown email", unknown_status),
        ("OIDC-only account", oidc_status),
    ] {
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{label}");
    }
    assert_eq!(error_code(&wrong_body), "UNAUTHORIZED");
    assert_eq!(
        wrong_body, unknown_body,
        "a wrong password and an unknown email must be indistinguishable"
    );
    assert_eq!(
        wrong_body, oidc_body,
        "an OIDC-only account must not be distinguishable from a nonexistent one — \
         this is the enumeration oracle that is easiest to ship by accident"
    );

    // ...and comparable in TIME. A loose bound on purpose: the defect being caught
    // is skipping the argon2 verification entirely for an absent user, which shows
    // up as an order of magnitude, not as a few percent.
    let floor = wrong_elapsed.mul_f64(0.25);
    assert!(
        unknown_elapsed >= floor,
        "an unknown email answered far too fast ({unknown_elapsed:?} vs {wrong_elapsed:?}); \
         the dummy verification is being skipped"
    );
    assert!(
        oidc_elapsed >= floor,
        "an OIDC-only account answered far too fast ({oidc_elapsed:?} vs {wrong_elapsed:?})"
    );
    // And the real thing is not suspiciously cheap either — if this is tiny, argon2
    // is not running at all and the assertions above prove nothing.
    assert!(
        good_elapsed >= StdDuration::from_millis(5),
        "a real verification should cost real work, took {good_elapsed:?}"
    );

    // --- logout revokes --------------------------------------------------
    assert_eq!(logout(&router, &token).await, StatusCode::NO_CONTENT);
    let (status, body) = whoami(&router, &token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "revoked: {body}");

    // Logging out twice, and logging out with something that is not a session, are
    // both 204: the caller has what they wanted either way, and saying otherwise
    // would be an oracle.
    assert_eq!(logout(&router, &token).await, StatusCode::NO_CONTENT);
    assert_eq!(
        logout(&router, "not-a-session").await,
        StatusCode::NO_CONTENT
    );

    // --- an expired session is a 401, not an error -----------------------
    let expired_token = format!("kairos_ss_{}", "e".repeat(64));
    let local_user_id: Uuid = users::table
        .filter(users::email.eq("local@example.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("user id");
    db_local_auth::create_session(
        &mut conn,
        NewLocalSession {
            user_id: local_user_id,
            token_hash: hash_session_token(&expired_token),
            expires_at: Utc::now() - Duration::hours(1),
        },
    )
    .expect("create expired session");
    let (status, _) = whoami(&router, &expired_token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "expired session");

    // A well-shaped session token that was never issued is the same 401.
    let (status, _) = whoami(&router, &format!("kairos_ss_{}", "f".repeat(64))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "unknown session");

    teardown(conn, scratch_db);
}

#[tokio::test]
async fn a_burst_of_password_guesses_is_refused() {
    let scratch_db = "kairos_local_login_throttle_t0203_test";
    let (mut conn, scratch_url, org_id) = fixture(scratch_db);
    make_user(&mut conn, "target@example.test", org_id, true);

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(&scratch_url);
    config.local_auth = true;
    config.auth_max_failures = 3;
    let router = app::router(app::state_with(config, pool, auth));

    // Throttled by IDENTITY here: `oneshot` supplies no socket peer, so the email
    // is the only subject — which is precisely the grain that matters for a
    // password endpoint, and the reason the throttle counts both.
    for attempt in 1..=3 {
        let (status, _, _) = login(&router, "target@example.test", "wrong").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {attempt}");
    }
    let (status, body, _) = login(&router, "target@example.test", "wrong").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(error_code(&body), "TOO_MANY_REQUESTS");

    // The CORRECT password is refused too while the lockout holds. That is the
    // point of a lockout, and it is also why the lockout is short.
    let (status, _, _) = login(&router, "target@example.test", PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    // A different account is untouched — one person being guessed at must not lock
    // out everybody.
    make_user(&mut conn, "other@example.test", org_id, true);
    let (status, body, _) = login(&router, "other@example.test", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    teardown(conn, scratch_db);
}

#[tokio::test]
async fn with_local_auth_off_there_is_no_login_endpoint() {
    let scratch_db = "kairos_local_login_off_t0203_test";
    let (mut conn, scratch_url, org_id) = fixture(scratch_db);
    make_user(&mut conn, "local@example.test", org_id, true);

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let config = base_config(&scratch_url);
    assert!(!config.local_auth, "off is the default");
    let router = app::router(app::state_with(config, pool, auth));

    // NOT ROUTED — 404, not 401. A deployment that never wanted local accounts has
    // no password endpoint to attack, which is a stronger statement than one that
    // exists and declines.
    let (status, _, _) = login(&router, "local@example.test", PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "/api/login must not exist when KAIROS_LOCAL_AUTH is off"
    );
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/logout")
        .body(Body::empty())
        .expect("request");
    assert_eq!(send(&router, request).await.0, StatusCode::NOT_FOUND);

    // And a session bearer is refused rather than looked up: with local auth off
    // there are no sessions, so the database is not asked.
    let token = format!("kairos_ss_{}", "a".repeat(64));
    let (status, body) = whoami(&router, &token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "UNAUTHORIZED");

    // Nothing that worked before local auth existed has changed: an unknown bearer
    // shape still reaches the OIDC path and still 401s there.
    let (status, _) = whoami(&router, "not.a.jwt").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    teardown(conn, scratch_db);
}

#[tokio::test]
async fn a_service_account_cannot_log_in_with_a_password() {
    // Service accounts authenticate with API keys. Even if something writes a
    // password hash onto one, it must not become a human login — a session carries
    // a person's identity and a service account is not one.
    let scratch_db = "kairos_local_login_svc_t0203_test";
    let (mut conn, scratch_url, org_id) = fixture(scratch_db);

    let svc: User = diesel::insert_into(users::table)
        .values(kairos_db::models::NewServiceAccountUser::new(
            "svc:ci".to_string(),
            "ci@svc.acme.kairos".to_string(),
            "ci",
        ))
        .returning(User::as_returning())
        .get_result(&mut conn)
        .expect("insert service account");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: svc.id,
            role: OrgRole::Member,
        })
        .execute(&mut conn)
        .expect("membership");
    let hash = hash_password(PASSWORD).expect("hash");
    db_local_auth::set_password(&mut conn, svc.id, &hash).expect("set password");

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(&scratch_url);
    config.local_auth = true;
    let router = app::router(app::state_with(config, pool, auth));

    let (status, body, _) = login(&router, "ci@svc.acme.kairos", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "UNAUTHORIZED");

    teardown(conn, scratch_db);
}

#[tokio::test]
async fn a_deployment_with_no_issuer_still_lets_people_in() {
    // KAIROS-T-0208, and KAIROS-I-0018's exit criterion "a ten-person team can run
    // Kairos with no IdP at all". Before this, the server refused to start without
    // an issuer it would never use.
    let scratch_db = "kairos_no_issuer_t0208_test";
    let (mut conn, scratch_url, org_id) = fixture(scratch_db);
    make_user(&mut conn, "solo@example.test", org_id, true);

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    // What `build_state` constructs when the config names no issuer.
    let auth = Arc::new(Authenticator::disabled());
    let mut config = base_config(&scratch_url);
    config.local_auth = true;
    config.oidc_issuer_url = None;
    config.oidc_audience = None;
    let router = app::router(app::state_with(config, pool, auth));

    // A password is enough to get in and to be recognised.
    let (status, body, _) = login(&router, "solo@example.test", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = body["token"].as_str().expect("token").to_string();
    let (status, body) = whoami(&router, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["email"], "solo@example.test");

    // A JWT-shaped bearer is refused, and the message NAMES the misconfiguration
    // instead of hiding behind "invalid token". This is the one failure on the auth
    // path that should say what is wrong: there is no secret to protect, and a
    // caller told only "invalid" would hunt through their own token for a long time.
    let jwt_shaped = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ4In0.sig";
    let (status, body) = whoami(&router, jwt_shaped).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let message = body["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("no OIDC issuer"),
        "the 401 should name the missing issuer, got: {message}"
    );
    assert!(
        message.contains("/api/login") || message.contains("password"),
        "and should point at what DOES work, got: {message}"
    );

    // /api/config tells the SPA there is nothing to redirect to, without trying to
    // discover it. A 502 IDP_UNREACHABLE here would be a lie — nothing is
    // unreachable — and would stop the GUI rendering the login page it CAN offer.
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/config")
        .body(Body::empty())
        .expect("request");
    let (status, body) = send(&router, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["issuer"].is_null(), "{body}");
    assert!(body["authorization_endpoint"].is_null(), "{body}");
    assert_eq!(body["local_auth"], true, "{body}");

    // The MCP protected-resource metadata lists NO authorization server, rather
    // than one called "null" that a client would try to fetch from.
    let request = Request::builder()
        .method(Method::GET)
        .uri("/.well-known/oauth-protected-resource")
        .body(Body::empty())
        .expect("request");
    let (status, body) = send(&router, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["authorization_servers"],
        json!([]),
        "an empty list, not [null] — a client reads this to find out where to get a \
         token, and would try to fetch a discovery document from the string \
         \"null\": {body}"
    );

    teardown(conn, scratch_db);
}
