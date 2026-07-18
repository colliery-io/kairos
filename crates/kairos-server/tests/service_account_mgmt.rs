//! Integration test for the service-account + API-key management API
//! (KAIROS-A-0017 / KAIROS-T-0059), end to end with the auth branch
//! (KAIROS-T-0058): an org admin creates a service account, mints a key, that
//! key authenticates `/api`, then revoke + delete take effect. Non-admins are
//! refused.
//!
//! Runs against the LIVE compose stack (`angreal services up`): real Postgres
//! and real Dex tokens (alice = admin, bob = non-member).

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};

use common::{AUDIENCE, ISSUER, base_config, recreate_scratch_db, request, with_database};
use kairos_db::models::{NewOrganizationMember, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use uuid::Uuid;

const SCRATCH_DB: &str = "kairos_sa_mgmt_t0059_test";

async fn get(
    router: &Router,
    uri: &str,
    token: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    request(router, Method::GET, uri, Some(token), headers, None).await
}

async fn post(
    router: &Router,
    uri: &str,
    token: &str,
    headers: &[(&str, &str)],
    body: Value,
) -> (StatusCode, Value) {
    request(router, Method::POST, uri, Some(token), headers, Some(body)).await
}

#[tokio::test]
async fn service_account_management_end_to_end() {
    // --- scratch DB + tenant ---------------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org id");

    // --- real Dex tokens --------------------------------------------------
    let http = reqwest::Client::new();
    let alice = common::user_token(&http, "alice").await;
    let bob = common::user_token(&http, "bob").await;

    // --- router (real OIDC, needed to validate the human tokens) ---------
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("discover live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));
    let acme = &[("x-tenant", "acme")][..];

    // Alice authenticates once (JIT-creating her user), then is made org admin.
    let (status, _) = get(&router, "/api/whoami", &alice, acme).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no membership yet");
    let alice_row: User = users::table
        .filter(users::email.eq("alice@kairos.test"))
        .select(User::as_select())
        .first(&mut conn)
        .expect("alice JIT row");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice_row.id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("make alice admin");

    // --- non-admin is refused --------------------------------------------
    let (status, body) = post(
        &router,
        "/api/service-accounts",
        &bob,
        acme,
        json!({"name": "hacker"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "non-member bob refused: {body}"
    );

    // --- admin creates a service account ---------------------------------
    let (status, body) = post(
        &router,
        "/api/service-accounts",
        &alice,
        acme,
        json!({"name": "ci-deploy"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["name"], "ci-deploy");
    let sa_id = body["id"].as_str().expect("sa id").to_string();

    // It shows up in the listing.
    let (status, body) = get(&router, "/api/service-accounts", &alice, acme).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);

    // --- mint a key: raw secret returned once ----------------------------
    let (status, body) = post(
        &router,
        &format!("/api/service-accounts/{sa_id}/keys"),
        &alice,
        acme,
        json!({"name": "gha-main"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let raw_key = body["key"].as_str().expect("raw key").to_string();
    assert!(raw_key.starts_with("kairos_sk_acme_"));
    let key_id = body["id"].as_str().expect("key id").to_string();

    // Listing keys shows a prefix, NEVER the secret or hash.
    let (status, body) = get(
        &router,
        &format!("/api/service-accounts/{sa_id}/keys"),
        &alice,
        acme,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let listed = body["items"][0].clone();
    assert!(
        listed["prefix"]
            .as_str()
            .unwrap()
            .starts_with("kairos_sk_acme_")
    );
    assert!(listed.get("key").is_none() && listed.get("token_hash").is_none());

    // --- the minted key AUTHENTICATES /api (T-0058 integration) ----------
    // No tenant header: the key carries its own tenant.
    let (status, body) = get(&router, "/api/whoami", &raw_key, &[]).await;
    assert_eq!(status, StatusCode::OK, "minted key authenticates: {body}");

    // --- revoke the key → it stops working -------------------------------
    let (status, _) = request(
        &router,
        Method::DELETE,
        &format!("/api/service-accounts/{sa_id}/keys/{key_id}"),
        Some(&alice),
        acme,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "revoke ok");
    let (status, _) = get(&router, "/api/whoami", &raw_key, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "revoked key rejected");

    // Double-revoke → 409.
    let (status, body) = request(
        &router,
        Method::DELETE,
        &format!("/api/service-accounts/{sa_id}/keys/{key_id}"),
        Some(&alice),
        acme,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "already revoked: {body}");

    // --- delete the service account → gone -------------------------------
    let (status, _) = request(
        &router,
        Method::DELETE,
        &format!("/api/service-accounts/{sa_id}"),
        Some(&alice),
        acme,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete ok");
    let (status, body) = get(&router, "/api/service-accounts", &alice, acme).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0, "service account removed");

    // Deleting an unknown id → 404.
    let (status, _) = request(
        &router,
        Method::DELETE,
        &format!("/api/service-accounts/{}", Uuid::new_v4()),
        Some(&alice),
        acme,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown sa → 404");

    // --- teardown ---------------------------------------------------------
    drop(conn);
    common::drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
