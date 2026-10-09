//! A single-tenant deployment provisions its pinned tenant on the first login
//! of a deployment admin, who becomes its first admin. Anyone else still gets
//! 404 `TENANT_NOT_FOUND` until it exists, and then 403 `MEMBERSHIP_REQUIRED`
//! as before.
//!
//! Runs against the live compose stack (`angreal services up`) in its own
//! scratch database.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    user_token, with_database,
};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_single_tenant_first_login_test";

async fn whoami(router: &Router, token: &str) -> (StatusCode, Value) {
    request(router, Method::GET, "/api/whoami", Some(token), &[], None).await
}

fn sub_of(conn: &mut PgConnection, email: &str) -> String {
    users::table
        .filter(users::email.eq(email))
        .select(users::external_id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user row for {email}: {e}"))
}

#[tokio::test]
async fn the_first_deployment_admin_to_log_in_provisions_the_single_tenant() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");

    let http = reqwest::Client::new();
    let alice = user_token(&http, "alice").await;
    let bob = user_token(&http, "bob").await;

    let pool = TenantPool::new(&scratch_url, 8).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );

    // JIT-create both users so their subjects can go in the config. The admin
    // routes authenticate before refusing the empty admin list.
    let plain = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    for token in [&alice, &bob] {
        let (status, body) = request(
            &plain,
            Method::GET,
            "/api/admin/tenants",
            Some(token),
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    let alice_sub = sub_of(&mut conn, "alice@kairos.test");

    let mut config = base_config(&scratch_url);
    config.base_domain = None;
    config.single_tenant = Some("solo".to_string());
    config.single_tenant_name = Some("Solo Inc".to_string());
    config.deployment_admins = vec![alice_sub];
    let router = app::router(app::state_with(config, pool, auth));

    // bob is not a deployment admin: nothing is provisioned for him.
    let (status, body) = whoami(&router, &bob).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "TENANT_NOT_FOUND");
    let orgs: i64 = organizations::table
        .count()
        .get_result(&mut conn)
        .expect("counting orgs");
    assert_eq!(orgs, 0, "a non-admin must not provision the tenant");

    // alice's first login sends several requests at once, as the GUI does.
    // Every one succeeds, and she is the admin of the one tenant they made.
    let calls = (0..8).map(|_| {
        let router = router.clone();
        let alice = alice.clone();
        tokio::spawn(async move { whoami(&router, &alice).await })
    });
    for call in futures_util::future::join_all(calls).await {
        let (status, me) = call.expect("request task");
        assert_eq!(status, StatusCode::OK, "{me}");
        assert_eq!(me["organization"]["slug"], "solo", "{me}");
        assert_eq!(me["organization"]["role"], "admin", "{me}");
    }
    let (org_id, name): (Uuid, String) = organizations::table
        .filter(organizations::slug.eq("solo"))
        .select((organizations::id, organizations::name))
        .first(&mut conn)
        .expect("the solo org");
    assert_eq!(name, "Solo Inc");
    let members: i64 = organization_members::table
        .filter(organization_members::organization_id.eq(org_id))
        .count()
        .get_result(&mut conn)
        .expect("counting members");
    assert_eq!(members, 1, "alice and nobody else");

    // Once it exists, bob is an ordinary non-member.
    let (status, body) = whoami(&router, &bob).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
