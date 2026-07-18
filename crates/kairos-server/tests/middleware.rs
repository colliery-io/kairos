//! Integration test for the auth + tenant middleware stack (KAIROS-T-0017,
//! contracts per KAIROS-A-0010 / KAIROS-A-0005 §2 / KAIROS-A-0013).
//!
//! Runs against the LIVE compose stack (`angreal services up`): real
//! Postgres and the real Dex issuer at `http://localhost:5558/dex` —
//! tokens are obtained via the password grant with the seeded test users
//! (`.angreal/dex/config.yaml`), never forged. For isolation the test
//! drops and recreates a dedicated scratch database
//! (`kairos_middleware_t0017_test`) on the same server; the shared
//! `kairos` database is never touched.
//!
//! The production router is exercised in-process via
//! `tower::ServiceExt::oneshot` on `kairos_server::app::router`, built
//! from `AppConfig` variants (subdomain + X-Tenant resolution, and
//! KAIROS_SINGLE_TENANT mode) — config is a struct, so no process-env
//! mutation is involved.
//!
//! Audience note (recorded per T-0017): Dex sets `aud` to the requesting
//! OAuth client id, so the server under test is configured with
//! `OIDC_AUDIENCE=kairos-cli` and user tokens are minted through the
//! `kairos-cli` public client. The wrong-audience case uses a REAL token
//! from the second seeded client (`kairos-svc`), whose `aud` is
//! `kairos-svc` — signature and issuer are valid, only `aud` differs.
//!
//! Shared helpers (scratch DB lifecycle, Dex tokens, in-process requests)
//! live in `tests/common/mod.rs` (KAIROS-T-0018 refactor).

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use serde_json::Value;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, dex_token, drop_scratch_db, error_code, recreate_scratch_db,
    request, with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::config::AppConfig;
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database (shared-services discipline: the
/// compose stack is shared with concurrent work, so this test owns its
/// own database and nothing else).
const SCRATCH_DB: &str = "kairos_middleware_t0017_test";

/// One in-process `GET /api/whoami` against the production router.
async fn call(
    router: &Router,
    token: Option<&str>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    request(router, Method::GET, "/api/whoami", token, headers, None).await
}

#[tokio::test]
async fn middleware_stack_against_live_dex() {
    // --- scratch database setup ------------------------------------------
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

    // --- live tokens ------------------------------------------------------
    let http = reqwest::Client::new();
    let alice_token = common::user_token(&http, "alice").await;
    let bob_token = common::user_token(&http, "bob").await;
    // REAL token, valid signature/issuer, but aud = "kairos-svc".
    let wrong_aud_token = dex_token(
        &http,
        "kairos-svc",
        Some("kairos-svc-secret"),
        "svc@kairos.test",
        "svc-password",
    )
    .await;

    // --- the production router, built from a config struct ---------------
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let config = base_config(&scratch_url);
    let router = app::router(app::state_with(config.clone(), pool.clone(), auth.clone()));

    // --- 1. valid token, resolvable tenant, but NO membership yet:
    //        403 MEMBERSHIP_REQUIRED with a "request access" message,
    //        AND the user row was JIT-created (A-0010: no auto-grant) -----
    let (status, body) = call(&router, Some(&alice_token), &[("x-tenant", "acme")]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("request access"),
        "{body}"
    );

    let alice: User = users::table
        .filter(users::email.eq("alice@kairos.test"))
        .select(User::as_select())
        .first(&mut conn)
        .expect("JIT provisioning created alice in public.users");
    assert_eq!(alice.display_name, "alice");
    assert!(!alice.external_id.is_empty(), "sub mapped to external_id");

    // --- 2. grant membership (explicit admin action), add a team ---------
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice.id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting alice membership");
    sql_query(
        "INSERT INTO org_acme.teams (name, slug, team_type) \
         VALUES ('Platform', 'platform', 'platform')",
    )
    .execute(&mut conn)
    .expect("creating team");
    sql_query(format!(
        "INSERT INTO org_acme.team_members (team_id, user_id) \
         SELECT t.id, '{}'::uuid FROM org_acme.teams t WHERE t.slug = 'platform'",
        alice.id
    ))
    .execute(&mut conn)
    .expect("adding alice to team");

    // --- 3. member via X-Tenant fallback: 200 with full identity ---------
    let (status, body) = call(&router, Some(&alice_token), &[("x-tenant", "acme")]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["id"], alice.id.to_string());
    assert_eq!(body["user"]["external_id"], alice.external_id);
    assert_eq!(body["user"]["email"], "alice@kairos.test");
    assert_eq!(body["user"]["display_name"], "alice");
    assert_eq!(body["organization"]["id"], org_id.to_string());
    assert_eq!(body["organization"]["slug"], "acme");
    assert_eq!(body["organization"]["role"], "admin");
    assert_eq!(body["teams"].as_array().expect("teams").len(), 1);
    assert_eq!(body["teams"][0]["slug"], "platform");
    assert_eq!(body["teams"][0]["name"], "Platform");

    // JIT is an upsert, not an insert: still exactly one alice row.
    let alice_rows: i64 = users::table
        .filter(users::external_id.eq(&alice.external_id))
        .count()
        .get_result(&mut conn)
        .expect("counting alice rows");
    assert_eq!(alice_rows, 1);

    // --- 4. member via Host subdomain against KAIROS_BASE_DOMAIN ---------
    let (status, body) = call(
        &router,
        Some(&alice_token),
        &[("host", "acme.kairos.test:8080")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["organization"]["slug"], "acme");

    // --- 5. auth failures: missing / garbage / tampered / wrong-aud ------
    let (status, body) = call(&router, None, &[("x-tenant", "acme")]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "UNAUTHORIZED");

    let (status, body) = call(&router, Some("garbage.token.here"), &[("x-tenant", "acme")]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "UNAUTHORIZED");

    // Tampered: real token with one payload byte flipped → bad signature.
    let tampered = {
        let mut parts: Vec<String> = alice_token.split('.').map(str::to_string).collect();
        assert_eq!(parts.len(), 3, "JWT has three segments");
        let mut payload = parts[1].clone().into_bytes();
        payload[0] = if payload[0] == b'A' { b'B' } else { b'A' };
        parts[1] = String::from_utf8(payload).expect("base64url is ascii");
        parts.join(".")
    };
    let (status, body) = call(&router, Some(&tampered), &[("x-tenant", "acme")]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "UNAUTHORIZED");

    // Wrong audience: REAL Dex token from the kairos-svc client.
    let (status, body) = call(&router, Some(&wrong_aud_token), &[("x-tenant", "acme")]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "UNAUTHORIZED");
    // ...and its rejection happened BEFORE JIT provisioning: no svc user.
    let svc_rows: i64 = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .count()
        .get_result(&mut conn)
        .expect("counting svc rows");
    assert_eq!(svc_rows, 0, "wrong-aud token must not provision a user");

    // --- 6. authenticated non-member: 403 MEMBERSHIP_REQUIRED ------------
    let (status, body) = call(&router, Some(&bob_token), &[("x-tenant", "acme")]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");
    let bob_rows: i64 = users::table
        .filter(users::email.eq("bob@kairos.test"))
        .count()
        .get_result(&mut conn)
        .expect("counting bob rows");
    assert_eq!(bob_rows, 1, "bob was JIT-provisioned despite 403");

    // --- 7. unknown tenant slug: 404 TENANT_NOT_FOUND --------------------
    let (status, body) = call(&router, Some(&alice_token), &[("x-tenant", "ghost")]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "TENANT_NOT_FOUND");

    // No tenant resolvable at all (no matching Host, no X-Tenant).
    let (status, body) = call(&router, Some(&alice_token), &[("host", "localhost:8080")]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "TENANT_NOT_FOUND");

    // --- 8. KAIROS_SINGLE_TENANT mode: no Host/X-Tenant needed -----------
    let single_tenant_config = AppConfig {
        single_tenant: Some("acme".to_string()),
        base_domain: None,
        ..config
    };
    let single_router = app::router(app::state_with(single_tenant_config, pool, auth));
    let (status, body) = call(&single_router, Some(&alice_token), &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["organization"]["slug"], "acme");
    assert_eq!(body["user"]["email"], "alice@kairos.test");

    // --- teardown ---------------------------------------------------------
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
