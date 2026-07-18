//! Integration test for service-account API-key authentication
//! (KAIROS-A-0017 / KAIROS-T-0058): a `kairos_sk_<slug>_<secret>` bearer is
//! accepted by `require_auth`, resolves to the service-account principal, pins
//! its own tenant, and is subject to the normal membership/ABAC stack.
//!
//! Runs against the LIVE compose Postgres (`angreal services up`). No Dex is
//! needed — the API-key path never validates a JWT — so the router is built
//! with a static-key (empty) [`Authenticator`].

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::Value;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    with_database,
};
use kairos_db::api_keys::{self, NewApiKey};
use kairos_db::models::{NewOrganizationMember, NewServiceAccountUser, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::config::ApiBearer;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::service_accounts::auth::{generate_key, hash_key};
use uuid::Uuid;

const SCRATCH_DB: &str = "kairos_api_key_auth_t0058_test";

async fn whoami(router: &Router, key: &str, headers: &[(&str, &str)]) -> (StatusCode, Value) {
    request(router, Method::GET, "/api/whoami", Some(key), headers, None).await
}

/// Create a service-account `public.users` row (+ optional org membership) and
/// return its id.
fn make_service_account(
    conn: &mut PgConnection,
    org_id: Uuid,
    name: &str,
    with_membership: bool,
) -> Uuid {
    let sa: User = diesel::insert_into(users::table)
        .values(NewServiceAccountUser::new(
            format!("svc:{name}"),
            format!("{name}@svc.acme.kairos"),
            name,
        ))
        .returning(User::as_returning())
        .get_result(conn)
        .expect("insert service account");
    assert!(sa.is_service_account());
    if with_membership {
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: sa.id,
                role: OrgRole::Member,
            })
            .execute(conn)
            .expect("insert membership");
    }
    sa.id
}

/// Mint a key for `user_id`, storing its hash, and return the raw key.
fn mint(conn: &mut PgConnection, user_id: Uuid, name: &str) -> String {
    let raw = generate_key("acme");
    api_keys::create_key(
        conn,
        NewApiKey {
            user_id,
            name: name.to_string(),
            token_hash: hash_key(&raw),
            prefix: name.to_string(),
            created_by: user_id,
            expires_at: None,
        },
    )
    .expect("create key");
    raw
}

#[tokio::test]
async fn api_key_authentication_end_to_end() {
    // --- scratch DB + tenant ---------------------------------------------
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connect scratch");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
    sql_query("SET search_path TO \"org_acme\", public")
        .execute(&mut conn)
        .expect("pin search_path");

    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org id");

    // A service account that IS a member, with several keys.
    let ci = make_service_account(&mut conn, org_id, "svc-ci", true);
    let good_key = mint(&mut conn, ci, "ci-good");

    let revoked_key = mint(&mut conn, ci, "ci-revoked");
    let revoked_id = api_keys::find_by_hash(&mut conn, &hash_key(&revoked_key))
        .expect("q")
        .expect("row")
        .id;
    api_keys::revoke_key(&mut conn, revoked_id).expect("revoke");

    let expired_key = generate_key("acme");
    api_keys::create_key(
        &mut conn,
        NewApiKey {
            user_id: ci,
            name: "ci-expired".to_string(),
            token_hash: hash_key(&expired_key),
            prefix: "ci-expired".to_string(),
            created_by: ci,
            expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
        },
    )
    .expect("create expired key");

    // A second service account that is NOT a member of the org.
    let outsider = make_service_account(&mut conn, org_id, "svc-outsider", false);
    let outsider_key = mint(&mut conn, outsider, "outsider");

    // --- the production router (no Dex needed for the key path) ----------
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(Authenticator::with_static_keys(ISSUER, AUDIENCE, []));
    let mut config = base_config(&scratch_url);
    config.api_bearer = ApiBearer::AccessToken;
    let router = app::router(app::state_with(config, pool, auth));

    // 1. Valid key, NO tenant header at all → tenant resolved FROM THE KEY.
    let (status, body) = whoami(&router, &good_key, &[]).await;
    assert_eq!(status, StatusCode::OK, "key pins its own tenant: {body}");
    assert_eq!(body["user"]["email"], "svc-ci@svc.acme.kairos");

    // 2. The key wins over a mismatched X-Tenant header (still acme).
    let (status, body) = whoami(&router, &good_key, &[("x-tenant", "globex")]).await;
    assert_eq!(status, StatusCode::OK, "key tenant beats header: {body}");
    assert_eq!(body["user"]["email"], "svc-ci@svc.acme.kairos");

    // 3. A valid key whose service account is NOT a member → 403 (the key does
    //    NOT bypass membership; the normal tenant stack still applies).
    let (status, body) = whoami(&router, &outsider_key, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");

    // 4. Revoked key → uniform 401.
    let (status, _) = whoami(&router, &revoked_key, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "revoked key rejected");

    // 5. Expired key → uniform 401.
    let (status, _) = whoami(&router, &expired_key, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "expired key rejected");

    // 6. Well-formed key for an unknown tenant → uniform 401.
    let (status, _) = whoami(&router, &generate_key("nosuchorg"), &[]).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "unknown-tenant key rejected"
    );

    // 7. Malformed kairos_sk_ bearer (bad secret) → 401, never OIDC fallthrough.
    let (status, _) = whoami(&router, "kairos_sk_acme_short", &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "malformed key rejected");

    // --- teardown ---------------------------------------------------------
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
