//! A first login sends several requests at once, and each one JIT-creates the
//! user. Every request must succeed and leave one row: before the fix a
//! concurrent insert of the same subject could trip the `user_name` unique
//! index, which `ON CONFLICT (external_id)` does not arbitrate, and that
//! request failed with a 500.
//!
//! Runs against the live compose stack (`angreal services up`) in its own
//! scratch database.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    user_token, with_database,
};
use kairos_db::schema::users;
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_jit_race_test";

/// Requests per simulated first login, and how many first logins to run.
const CONCURRENT: usize = 16;
const ROUNDS: usize = 20;

#[tokio::test]
async fn concurrent_first_logins_create_one_user_and_all_succeed() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");

    let http = reqwest::Client::new();
    let token = user_token(&http, "alice").await;

    let pool = TenantPool::new(&scratch_url, CONCURRENT as u32)
        .await
        .expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));

    for round in 0..ROUNDS {
        // alice has no membership, so removing her row makes the next request
        // her first login again.
        diesel::delete(users::table.filter(users::email.eq("alice@kairos.test")))
            .execute(&mut conn)
            .expect("removing alice");

        let calls = (0..CONCURRENT).map(|_| {
            let router = router.clone();
            let token = token.clone();
            tokio::spawn(async move {
                request(
                    &router,
                    Method::GET,
                    "/api/whoami",
                    Some(&token),
                    &[("x-tenant", "acme")],
                    None,
                )
                .await
            })
        });
        for call in futures_util::future::join_all(calls).await {
            let (status, body) = call.expect("request task");
            // Authenticated and provisioned, but not a member of acme.
            assert_eq!(status, StatusCode::FORBIDDEN, "round {round}: {body}");
            assert_eq!(error_code(&body), "MEMBERSHIP_REQUIRED", "round {round}");
        }

        let rows: i64 = users::table
            .filter(users::email.eq("alice@kairos.test"))
            .count()
            .get_result(&mut conn)
            .expect("counting alice");
        assert_eq!(rows, 1, "round {round}: one row per subject");
    }

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
