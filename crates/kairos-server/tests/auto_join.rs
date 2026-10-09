//! `KAIROS_AUTO_JOIN_DOMAINS`: on a single-tenant deployment, a person whose
//! issuer verifies an email at a listed domain becomes a `member` on their
//! first request, with no admin step. Someone an admin removed does not join
//! again, and an email at any other domain still gets 403
//! `MEMBERSHIP_REQUIRED`.
//!
//! Runs against the live compose stack (`angreal services up`) in its own
//! scratch database. Dex asserts `email_verified` for its static users.

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
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    user_token, with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::config::AppConfig;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_auto_join_test";

async fn api(router: &Router, method: Method, uri: &str, token: &str) -> (StatusCode, Value) {
    request(router, method, uri, Some(token), &[], None).await
}

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user row for {email}: {e}"))
}

fn is_member(conn: &mut PgConnection, org_id: Uuid, user: Uuid) -> bool {
    organization_members::table
        .filter(organization_members::organization_id.eq(org_id))
        .filter(organization_members::user_id.eq(user))
        .count()
        .get_result::<i64>(conn)
        .expect("counting memberships")
        == 1
}

fn config(scratch_url: &str, domains: &[&str]) -> AppConfig {
    let mut config = base_config(scratch_url);
    config.base_domain = None;
    config.single_tenant = Some("acme".to_string());
    config.auto_join_domains = domains.iter().map(|d| d.to_string()).collect();
    config
}

#[tokio::test]
async fn a_verified_email_at_a_listed_domain_joins_on_its_first_request() {
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
    let alice = user_token(&http, "alice").await;
    let bob = user_token(&http, "bob").await;
    let carol = user_token(&http, "carol").await;
    let svc = user_token(&http, "svc").await;

    let pool = TenantPool::new(&scratch_url, 8).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );

    // --- 1. a domain that is not listed: nothing changes -------------------
    let other = app::router(app::state_with(
        config(&scratch_url, &["other.example"]),
        pool.clone(),
        auth.clone(),
    ));
    let (status, body) = api(&other, Method::GET, "/api/whoami", &carol).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");
    let carol_id = user_id(&mut conn, "carol@kairos.test");
    assert!(!is_member(&mut conn, org_id, carol_id));

    let router = app::router(app::state_with(
        config(&scratch_url, &["kairos.test"]),
        pool,
        auth,
    ));

    // svc is the org admin who removes people below.
    let (status, body) = api(&other, Method::GET, "/api/whoami", &svc).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user_id(&mut conn, "svc@kairos.test"),
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("seeding svc as admin");

    // --- 2. a listed domain: the first request joins, as member ------------
    let (status, me) = api(&router, Method::GET, "/api/whoami", &alice).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["organization"]["role"], "member", "{me}");
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    assert!(is_member(&mut conn, org_id, alice_id));

    // The join is in the activity log, as the person's own action.
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("search_path");
    let logged: i64 = kairos_db::schema::activity_log::table
        .filter(kairos_db::schema::activity_log::entity_id.eq(alice_id))
        .filter(kairos_db::schema::activity_log::actor_id.eq(alice_id))
        .filter(kairos_db::schema::activity_log::details.like("%auto_join%"))
        .count()
        .get_result(&mut conn)
        .expect("counting the join");
    assert_eq!(logged, 1);
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("search_path");

    // --- 3. a first login sends several requests at once -------------------
    let calls = (0..8).map(|_| {
        let router = router.clone();
        let bob = bob.clone();
        tokio::spawn(async move { api(&router, Method::GET, "/api/whoami", &bob).await })
    });
    for call in futures_util::future::join_all(calls).await {
        let (status, me) = call.expect("request task");
        assert_eq!(status, StatusCode::OK, "{me}");
        assert_eq!(me["organization"]["role"], "member", "{me}");
    }

    // --- 4. removed by an admin: does not join again -----------------------
    let (status, body) = api(
        &router,
        Method::DELETE,
        &format!("/api/members/{alice_id}"),
        &svc,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = api(&router, Method::GET, "/api/whoami", &alice).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED");
    assert!(!is_member(&mut conn, org_id, alice_id));

    // An admin can still add her back by hand.
    let (status, body) = request(
        &router,
        Method::POST,
        "/api/members",
        Some(&svc),
        &[],
        Some(serde_json::json!({ "email": "alice@kairos.test" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, me) = api(&router, Method::GET, "/api/whoami", &alice).await;
    assert_eq!(status, StatusCode::OK, "{me}");

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
