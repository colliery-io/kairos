//! COLLIERY-T-0265 — a change to a team, a delivery stream or a
//! membership makes an entry of the activity log with the action `update`,
//! against the booted production router.
//!
//! Before this ticket such a change made an entry with the action
//! `create`: the set of the actions had no action for a change. A filter of
//! the feed by `create` gave the changes too, and a filter for the changes
//! did not exist.
//!
//! - the update of a team, of a delivery stream and of the role of a
//!   member makes one entry with the action `update`,
//! - the create and the delete keep their actions,
//! - `GET /api/activity?action=update` gives the changes and no create.
//!
//! The changes that SCIM makes are in `scim.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The booted router on a scratch database, with `svc` as the org admin
/// and `bob` as a member.
struct Stack {
    scratch_db: &'static str,
    admin_conn: PgConnection,
    /// Pinned to the tenant schema.
    conn: PgConnection,
    pool: TenantPool,
    router: Router,
    token: String,
    bob_id: Uuid,
}

impl Stack {
    async fn boot(scratch_db: &'static str) -> Self {
        let admin_conn = recreate_scratch_db(scratch_db);
        let scratch_url = with_database(&common::admin_database_url(), scratch_db);
        let mut conn =
            PgConnection::establish(&scratch_url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("acme org row");

        let http = reqwest::Client::new();
        let token = user_token(&http, "svc").await;
        let bob_token = user_token(&http, "bob").await;
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(
            Authenticator::discover(ISSUER, AUDIENCE)
                .await
                .expect("OIDC discovery against live Dex"),
        );
        let router = app::router(app::state_with(
            base_config(&scratch_url),
            pool.clone(),
            auth,
        ));

        // JIT-provision the two users, then give each a membership.
        let mut ids = Vec::new();
        for (user, user_token, role) in [
            ("svc", &token, OrgRole::Admin),
            ("bob", &bob_token, OrgRole::Member),
        ] {
            let _ = request(
                &router,
                Method::GET,
                "/api/whoami",
                Some(user_token),
                &TENANT,
                None,
            )
            .await;
            let user_id: Uuid = users::table
                .filter(users::email.eq(format!("{user}@kairos.test")))
                .select(users::id)
                .first(&mut conn)
                .expect("user provisioned");
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id,
                    role,
                })
                .execute(&mut conn)
                .expect("granting membership");
            ids.push(user_id);
        }
        diesel::sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pinning search_path");

        Stack {
            scratch_db,
            admin_conn,
            conn,
            pool,
            router,
            token,
            bob_id: ids[1],
        }
    }

    /// One request as the org admin.
    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        request(&self.router, method, uri, Some(&self.token), &TENANT, body).await
    }

    /// One request that must pass. Returns the body.
    async fn ok(&self, method: Method, uri: &str, body: Option<Value>) -> Value {
        let (status, answer) = self.send(method.clone(), uri, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {answer}");
        answer
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

#[derive(QueryableByName)]
struct ActionRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    action: String,
}

/// The actions of the entries about one entity whose details start with
/// `details`, oldest first.
fn actions(conn: &mut PgConnection, entity: &str, details: &str) -> Vec<String> {
    diesel::sql_query(
        "SELECT action FROM activity_log \
          WHERE entity_id = $1::uuid AND details LIKE $2 ORDER BY occurred_at, id",
    )
    .bind::<diesel::sql_types::Text, _>(entity)
    .bind::<diesel::sql_types::Text, _>(format!("{details}%"))
    .load::<ActionRow>(conn)
    .unwrap_or_else(|e| panic!("the entries of {entity}: {e}"))
    .into_iter()
    .map(|row| row.action)
    .collect()
}

#[tokio::test]
async fn a_change_has_the_action_update_against_live_stack() {
    let mut stack = Stack::boot("kairos_update_action_t0265_test").await;

    // --- a team --------------------------------------------------------------
    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
        )
        .await;
    let team_id = team["id"].as_str().expect("team id").to_string();
    stack
        .ok(
            Method::PATCH,
            &format!("/api/teams/{team_id}"),
            Some(json!({"name": "Payments and billing"})),
        )
        .await;
    assert_eq!(
        actions(&mut stack.conn, &team_id, "team:payments"),
        ["create"],
        "the create of a team keeps its action"
    );
    assert_eq!(
        actions(&mut stack.conn, &team_id, "team_settings:payments"),
        ["update"],
        "the update of a team"
    );

    // --- a delivery stream ---------------------------------------------------
    let stream = stack
        .ok(
            Method::POST,
            "/api/delivery-streams",
            Some(json!({"name": "Checkout", "slug": "checkout"})),
        )
        .await;
    let stream_id = stream["id"].as_str().expect("stream id").to_string();
    stack
        .ok(
            Method::PATCH,
            &format!("/api/delivery-streams/{stream_id}"),
            Some(json!({"description": "The path to a payment."})),
        )
        .await;
    assert_eq!(
        actions(
            &mut stack.conn,
            &stream_id,
            "delivery_stream_settings:checkout"
        ),
        ["update"],
        "the update of a delivery stream"
    );
    assert_eq!(
        actions(&mut stack.conn, &stream_id, "").len(),
        2,
        "one entry for the create and one for the update"
    );
    assert_eq!(
        actions(&mut stack.conn, &stream_id, "")[0],
        "create",
        "the create of a delivery stream keeps its action"
    );

    // --- the role of a member ------------------------------------------------
    let bob = stack.bob_id.to_string();
    stack
        .ok(
            Method::PATCH,
            &format!("/api/members/{bob}"),
            Some(json!({"role": "admin"})),
        )
        .await;
    assert_eq!(
        actions(&mut stack.conn, &bob, "membership_role:member->admin"),
        ["update"],
        "the change of a role"
    );

    // --- the feed ------------------------------------------------------------
    let feed = stack
        .ok(Method::GET, "/api/activity?action=update", None)
        .await;
    assert_eq!(feed["total"], 3, "{feed}");
    let details: Vec<&str> = feed["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|entry| {
            assert_eq!(entry["action"], "update", "{entry}");
            entry["details"].as_str().expect("details")
        })
        .collect();
    assert!(details.contains(&"team_settings:payments"), "{details:?}");
    assert!(
        details.contains(&"delivery_stream_settings:checkout"),
        "{details:?}"
    );
    // No change is in the list of the creates.
    let creates = stack
        .ok(Method::GET, "/api/activity?action=create&limit=200", None)
        .await;
    for entry in creates["items"].as_array().expect("items") {
        let details = entry["details"].as_str().expect("details");
        assert!(
            !details.starts_with("team_settings:")
                && !details.starts_with("delivery_stream_settings:")
                && !details.starts_with("membership_role:"),
            "a change is in the list of the creates: {entry}"
        );
    }

    stack.shutdown();
}
