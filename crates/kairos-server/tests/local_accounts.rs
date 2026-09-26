//! Integration test for local-account administration (KAIROS-T-0204): an org admin
//! creating an account, resetting a password, and inspecting and ending sessions —
//! plus the `kairos-server set-password` break-glass subcommand, exercised as the
//! real binary because the thing it has to prove is that it works with no login.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`).

mod common;

use std::sync::Arc;

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

const SCRATCH_DB: &str = "kairos_local_accounts_t0204_test";
const ADMIN_PASSWORD: &str = "admin-password-long-enough";

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

async fn as_user(
    router: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-tenant", "acme");
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string())),
        None => {
            builder = builder.header("content-type", "application/json");
            builder.body(Body::empty())
        }
    }
    .expect("request");
    send(router, request).await
}

/// Log in and return the session bearer.
async fn login(router: &Router, email: &str, password: &str) -> String {
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "email": email, "password": password }).to_string(),
        ))
        .expect("request");
    let (status, body) = send(router, request).await;
    assert_eq!(status, StatusCode::OK, "login failed: {body}");
    body["token"].as_str().expect("token").to_string()
}

fn member(conn: &mut PgConnection, email: &str, org_id: Uuid, role: OrgRole) -> User {
    let user: User = diesel::insert_into(users::table)
        .values(NewUser {
            external_id: format!("oidc:{email}"),
            user_name: email.to_string(),
            email: email.to_string(),
            display_name: email.to_string(),
        })
        .returning(User::as_returning())
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user.id,
            role,
        })
        .execute(conn)
        .expect("membership");
    let hash = hash_password(ADMIN_PASSWORD).expect("hash");
    db_local_auth::set_password(conn, user.id, &hash).expect("password");
    user
}

#[tokio::test]
async fn an_org_admin_manages_local_accounts() {
    let admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    provision_tenant(&mut conn, "globex", "Globex").expect("provision globex");
    let acme: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme id");
    let globex: Uuid = organizations::table
        .filter(organizations::slug.eq("globex"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("globex id");

    member(&mut conn, "boss@example.test", acme, OrgRole::Admin);
    member(&mut conn, "worker@example.test", acme, OrgRole::Member);
    let outsider = member(&mut conn, "outsider@example.test", globex, OrgRole::Admin);
    drop(admin_conn);

    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(&scratch_url);
    config.local_auth = true;
    config.auth_max_failures = 100;
    let router = app::router(app::state_with(config, pool, auth));

    let boss = login(&router, "boss@example.test", ADMIN_PASSWORD).await;
    let worker = login(&router, "worker@example.test", ADMIN_PASSWORD).await;

    // --- create -----------------------------------------------------------
    // The point of the endpoint: a person who has never logged in, and could not,
    // because there is nothing to log in to until they have an account.
    let (status, body) = as_user(
        &router,
        Method::POST,
        "/api/local-accounts",
        &boss,
        Some(json!({
            "email": "  NewHire@Example.TEST  ",
            "display_name": "New Hire",
            "password": "a-perfectly-fine-password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["email"], "newhire@example.test",
        "trimmed, lower-cased"
    );
    assert_eq!(body["created"], true);
    assert_eq!(body["membership_added"], true, "membership is the point");
    let new_hire_id = body["user_id"].as_str().expect("user_id").to_string();

    // And they can actually log in, which is the only assertion that matters.
    let new_hire = login(&router, "newhire@example.test", "a-perfectly-fine-password").await;
    let (status, body) = as_user(&router, Method::GET, "/api/whoami", &new_hire, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["email"], "newhire@example.test");

    // --- one person, one row ----------------------------------------------
    // `worker` already exists with an OIDC external_id. Adding a password must adopt
    // that row rather than fork the person.
    let (status, body) = as_user(
        &router,
        Method::POST,
        "/api/local-accounts",
        &boss,
        Some(json!({ "email": "worker@example.test", "password": "another-long-password" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["created"], false, "adopted, not created");
    assert_eq!(body["membership_added"], false, "already a member");
    let rows: i64 = users::table
        .filter(users::email.eq("worker@example.test"))
        .count()
        .get_result(&mut conn)
        .expect("count");
    assert_eq!(rows, 1, "exactly one row for one person");

    // That adoption set a password, so it revoked the worker's OWN session — the
    // KAIROS-T-0203 guarantee reaching an adopted row, which is exactly where it
    // would be easiest to miss. Log in again with the password just set.
    let (status, _) = as_user(&router, Method::GET, "/api/whoami", &worker, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "adopting a row is a password change, so its sessions end"
    );
    let worker = login(&router, "worker@example.test", "another-long-password").await;

    // --- a weak password is refused before any hashing --------------------
    let (status, body) = as_user(
        &router,
        Method::POST,
        "/api/local-accounts",
        &boss,
        Some(json!({ "email": "weak@example.test", "password": "short" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "WEAK_PASSWORD");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("12"),
        "the message should say the minimum: {body}"
    );

    // --- a reset ends every session that person held -----------------------
    // The mechanism is KAIROS-T-0203's; this proves it fires through the endpoint,
    // which is the difference between a guarantee and a comment.
    let (status, body) = as_user(&router, Method::GET, "/api/whoami", &new_hire, None).await;
    assert_eq!(status, StatusCode::OK, "still logged in: {body}");
    let (status, body) = as_user(
        &router,
        Method::PUT,
        &format!("/api/local-accounts/{new_hire_id}/password"),
        &boss,
        Some(json!({ "password": "a-replacement-password" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = as_user(&router, Method::GET, "/api/whoami", &new_hire, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a reset must not leave the old session working — that is the whole reason \
         someone asks for one"
    );
    let replaced = login(&router, "newhire@example.test", "a-replacement-password").await;

    // --- sessions listing --------------------------------------------------
    let (status, body) = as_user(
        &router,
        Method::GET,
        &format!("/api/local-accounts/{new_hire_id}/sessions"),
        &boss,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "the revoked one is retained: {body}");
    let active: Vec<&Value> = items.iter().filter(|s| s["active"] == true).collect();
    assert_eq!(active.len(), 1, "exactly one live session: {body}");
    // Never the token, and never its hash.
    let serialized = body.to_string();
    assert!(
        !serialized.contains("kairos_ss_"),
        "no token in the listing"
    );
    assert!(!serialized.contains("token_hash"), "no hash in the listing");

    // An expired session shows as inactive rather than being hidden.
    db_local_auth::create_session(
        &mut conn,
        NewLocalSession {
            user_id: Uuid::parse_str(&new_hire_id).expect("uuid"),
            token_hash: hash_session_token("kairos_ss_expired"),
            expires_at: Utc::now() - Duration::hours(1),
        },
    )
    .expect("expired session");
    let (_, body) = as_user(
        &router,
        Method::GET,
        &format!("/api/local-accounts/{new_hire_id}/sessions"),
        &boss,
        None,
    )
    .await;
    assert_eq!(body["items"].as_array().expect("items").len(), 3);
    assert_eq!(
        body["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter(|s| s["active"] == true)
            .count(),
        1,
        "still one live: {body}"
    );

    // --- revoking sessions without changing the password -------------------
    // A separate operation on purpose: "log this person out everywhere" and "they
    // forgot their password" are different incidents, and forcing the first to change
    // the password means telling someone a password they did not ask for.
    let (status, body) = as_user(
        &router,
        Method::DELETE,
        &format!("/api/local-accounts/{new_hire_id}/sessions"),
        &boss,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = as_user(&router, Method::GET, "/api/whoami", &replaced, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "logged out everywhere");
    // ...and the password still works, so they can simply log in again.
    let _ = login(&router, "newhire@example.test", "a-replacement-password").await;

    // --- org-admin only, on reads as well as writes ------------------------
    // A list of someone's live sessions is a security surface, not work content, so
    // A-0006's read-open rule does not reach it.
    for (method, uri, body) in [
        (
            Method::POST,
            "/api/local-accounts".to_string(),
            Some(json!({ "email": "sneaky@example.test", "password": "long-enough-password" })),
        ),
        (
            Method::PUT,
            format!("/api/local-accounts/{new_hire_id}/password"),
            Some(json!({ "password": "long-enough-password" })),
        ),
        (
            Method::GET,
            format!("/api/local-accounts/{new_hire_id}/sessions"),
            None,
        ),
        (
            Method::DELETE,
            format!("/api/local-accounts/{new_hire_id}/sessions"),
            None,
        ),
    ] {
        let (status, body) = as_user(&router, method.clone(), &uri, &worker, body).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} must be org-admin only: {body}"
        );
        assert_eq!(error_code(&body), "FORBIDDEN");
        assert_eq!(
            body["error"]["details"]["required_capability"], "manage_local_accounts",
            "the 403 names the pseudo-capability, as the other org-admin surfaces do"
        );
    }

    // --- and never across tenants -----------------------------------------
    // `public.users` is deployment-wide, so without the membership check an org admin
    // could reset the password of anybody in any other organization.
    let (status, body) = as_user(
        &router,
        Method::PUT,
        &format!("/api/local-accounts/{}/password", outsider.id),
        &boss,
        Some(json!({ "password": "long-enough-password" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // The same 404 as a nonexistent id: telling them apart would say whether an
    // address exists elsewhere in the deployment.
    let (nowhere_status, nowhere_body) = as_user(
        &router,
        Method::PUT,
        &format!("/api/local-accounts/{}/password", Uuid::new_v4()),
        &boss,
        Some(json!({ "password": "long-enough-password" })),
    )
    .await;
    assert_eq!(nowhere_status, status);
    assert_eq!(error_code(&nowhere_body), error_code(&body));

    let mut admin = PgConnection::establish(&common::admin_database_url()).expect("admin");
    drop(conn);
    drop_scratch_db(&mut admin, SCRATCH_DB);
}

#[tokio::test]
async fn the_break_glass_subcommand_needs_no_login() {
    // The case it exists for: the sole admin of a local-auth deployment has forgotten
    // their password and there is no reset email. So it is run as the real binary
    // against the database, with no server and no credential anywhere.
    let scratch_db = "kairos_break_glass_t0204_test";
    let admin_conn = recreate_scratch_db(scratch_db);
    let scratch_url = with_database(&common::admin_database_url(), scratch_db);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    let acme: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme id");
    let locked_out = member(&mut conn, "forgetful@example.test", acme, OrgRole::Admin);
    // They are logged in on one device, which the reset must end.
    db_local_auth::create_session(
        &mut conn,
        NewLocalSession {
            user_id: locked_out.id,
            token_hash: hash_session_token("kairos_ss_old_device"),
            expires_at: Utc::now() + Duration::days(7),
        },
    )
    .expect("session");
    drop(admin_conn);

    let exe = env!("CARGO_BIN_EXE_kairos-server");
    let output = std::process::Command::new(exe)
        .args([
            "set-password",
            "--email",
            "Forgetful@Example.TEST",
            "--password",
            "a-brand-new-password",
        ])
        .env("DATABASE_URL", &scratch_url)
        .output()
        .expect("run kairos-server set-password");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("1 existing session(s) revoked"),
        "it must say what it ended: {stdout}"
    );

    // The new password works...
    let stored = db_local_auth::find_user_by_email(&mut conn, "forgetful@example.test")
        .expect("query")
        .expect("row");
    assert!(
        kairos_server::local_auth::verify_password(
            "a-brand-new-password",
            stored.password_hash.as_deref().expect("hash")
        )
        .expect("verify")
    );
    // ...and the session it revoked is dead, so a break-glass reset does not leave
    // whoever had the old one logged in.
    assert!(
        !db_local_auth::find_session_by_hash(
            &mut conn,
            &hash_session_token("kairos_ss_old_device")
        )
        .expect("query")
        .expect("row")
        .is_valid_at(Utc::now())
    );

    // It refuses to CREATE an account: otherwise it would be a way to mint an admin
    // on any deployment whose database you can reach.
    let output = std::process::Command::new(exe)
        .args([
            "set-password",
            "--email",
            "nobody@example.test",
            "--password",
            "a-brand-new-password",
        ])
        .env("DATABASE_URL", &scratch_url)
        .output()
        .expect("run");
    assert!(!output.status.success(), "must refuse an unknown email");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("will not create one") && stderr.contains("KAIROS_BOOTSTRAP_ADMIN"),
        "and point at the bootstrap instead: {stderr}"
    );

    // A short password is refused here too — the floor is not only an API rule.
    let output = std::process::Command::new(exe)
        .args([
            "set-password",
            "--email",
            "forgetful@example.test",
            "--password",
            "short",
        ])
        .env("DATABASE_URL", &scratch_url)
        .output()
        .expect("run");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("at least 12"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut admin = PgConnection::establish(&common::admin_database_url()).expect("admin");
    drop(conn);
    drop_scratch_db(&mut admin, scratch_db);
}
