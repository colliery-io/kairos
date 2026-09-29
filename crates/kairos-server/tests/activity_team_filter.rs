//! COLLIERY-T-0265 — `GET /api/activity` has the filter `team`, against
//! the booted production router.
//!
//! Before this ticket the route had no filter by team. The activity page
//! read one page of the feed and removed the entries of the other teams
//! from that page. Thus a page of 25 entries could show 0 entries of the
//! team, although the team had entries on the next pages, and the count
//! below the table was the count of all teams.
//!
//! An entry passes the filter when its ACTOR is a member of the team at
//! the time of the request. That is the meaning that the filter of the
//! page had (KAIROS-T-0069).
//!
//! - the filter applies before the page, and `total` is the count after
//!   the filter,
//! - the slug and the id of the team give the same answer,
//! - the filter combines with the other filters,
//! - a team with no member gives an empty page,
//! - an unknown team is a 422 `VALIDATION` with `details.field` = `team`.
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
    AUDIENCE, ISSUER, base_config, drop_scratch_db, error_code, recreate_scratch_db, request,
    user_token, with_database,
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
    bob_token: String,
    svc_id: Uuid,
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
            bob_token,
            svc_id: ids[0],
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

/// The number of entries that `bob` makes: more than one page of the
/// activity page (25 entries).
const BOB_TASKS: usize = 3;
/// The number of entries that `svc` makes after the entries of `bob`.
const SVC_TASKS: usize = 30;

#[tokio::test]
async fn the_server_filters_the_activity_by_team_against_live_stack() {
    let stack = Stack::boot("kairos_activity_team_t0265_test").await;

    // `bob` is the one member of `payments`. `svc` is the one member of
    // `web`. `empty` has no member.
    let mut teams = Vec::new();
    for (name, slug, member) in [
        ("Payments", "payments", Some(stack.bob_id)),
        ("Web", "web", Some(stack.svc_id)),
        ("Empty", "empty", None),
    ] {
        let team = stack
            .ok(
                Method::POST,
                "/api/teams",
                Some(json!({"name": name, "slug": slug})),
            )
            .await;
        let id = team["id"].as_str().expect("team id").to_string();
        if let Some(user_id) = member {
            stack
                .ok(
                    Method::POST,
                    &format!("/api/teams/{id}/members"),
                    Some(json!({"user_id": user_id})),
                )
                .await;
        }
        teams.push(id);
    }
    let payments = teams[0].clone();

    // The entries of `bob` are the oldest. Then `svc` makes more entries
    // than one page has, so the first page of the feed has no entry of
    // `bob`.
    for n in 0..BOB_TASKS {
        let (status, answer) = request(
            &stack.router,
            Method::POST,
            "/api/tasks",
            Some(&stack.bob_token),
            &TENANT,
            Some(json!({"board_id": "payments-delivery", "title": format!("Bob {n}")})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    for n in 0..SVC_TASKS {
        stack
            .ok(
                Method::POST,
                "/api/tasks",
                Some(json!({"board_id": "web-delivery", "title": format!("Svc {n}")})),
            )
            .await;
    }

    // --- the defect: the first page has no entry of the team -----------------
    let first_page = stack.ok(Method::GET, "/api/activity?limit=25", None).await;
    let bob = stack.bob_id.to_string();
    assert!(
        first_page["items"]
            .as_array()
            .expect("items")
            .iter()
            .all(|entry| entry["actor_id"] != bob.as_str()),
        "the first page of the feed has no entry of bob"
    );
    assert!(first_page["total"].as_i64().expect("total") > 25);

    // --- the filter applies before the page ----------------------------------
    let by_slug = stack
        .ok(Method::GET, "/api/activity?team=payments&limit=25", None)
        .await;
    assert_eq!(by_slug["total"], BOB_TASKS, "{by_slug}");
    let items = by_slug["items"].as_array().expect("items");
    assert_eq!(items.len(), BOB_TASKS, "{by_slug}");
    for entry in items {
        assert_eq!(entry["actor_id"], bob.as_str(), "{entry}");
        assert_eq!(entry["action"], "create", "{entry}");
    }
    let by_id = stack
        .ok(
            Method::GET,
            &format!("/api/activity?team={payments}&limit=25"),
            None,
        )
        .await;
    assert_eq!(by_id, by_slug, "the id and the slug of the team");

    // The pages of the filter: `total` stays, and each entry is on one page.
    let mut seen = Vec::new();
    for offset in 0..BOB_TASKS {
        let page = stack
            .ok(
                Method::GET,
                &format!("/api/activity?team=payments&limit=1&offset={offset}"),
                None,
            )
            .await;
        assert_eq!(page["total"], BOB_TASKS, "{page}");
        let items = page["items"].as_array().expect("items");
        assert_eq!(items.len(), 1, "{page}");
        seen.push(items[0]["id"].as_str().expect("id").to_string());
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), BOB_TASKS);

    // --- the other team, and the team with no member -------------------------
    let web = stack
        .ok(Method::GET, "/api/activity?team=web&limit=200", None)
        .await;
    let all = stack.ok(Method::GET, "/api/activity?limit=1", None).await;
    assert_eq!(
        web["total"].as_i64().expect("total"),
        all["total"].as_i64().expect("total") - BOB_TASKS as i64,
        "each entry that is not of bob is of svc"
    );
    let empty = stack
        .ok(Method::GET, "/api/activity?team=empty", None)
        .await;
    assert_eq!(empty["total"], 0, "{empty}");
    assert_eq!(empty["items"], json!([]), "{empty}");

    // --- the filter combines with the other filters --------------------------
    let creates = stack
        .ok(
            Method::GET,
            "/api/activity?team=payments&action=create",
            None,
        )
        .await;
    assert_eq!(creates["total"], BOB_TASKS, "{creates}");
    let deletes = stack
        .ok(
            Method::GET,
            "/api/activity?team=payments&action=delete",
            None,
        )
        .await;
    assert_eq!(deletes["total"], 0, "{deletes}");
    let other_actor = stack
        .ok(
            Method::GET,
            &format!("/api/activity?team=payments&actor_id={}", stack.svc_id),
            None,
        )
        .await;
    assert_eq!(other_actor["total"], 0, "{other_actor}");

    // --- the membership at the time of the request ---------------------------
    stack
        .ok(
            Method::DELETE,
            &format!("/api/teams/{payments}/members/{bob}"),
            None,
        )
        .await;
    let after = stack
        .ok(Method::GET, "/api/activity?team=payments", None)
        .await;
    assert_eq!(after["total"], 0, "bob is not a member now: {after}");

    // --- an unknown team -----------------------------------------------------
    for reference in ["no-such-team".to_string(), Uuid::new_v4().to_string()] {
        let (status, answer) = stack
            .send(
                Method::GET,
                &format!("/api/activity?team={reference}"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
        assert_eq!(error_code(&answer), "VALIDATION", "{answer}");
        assert_eq!(
            answer["error"]["message"],
            format!(
                "The team {reference:?} is not in the organization. Send the id or the slug \
                 of a team of the organization."
            ),
        );
        assert_eq!(answer["error"]["details"], json!({"field": "team"}));
    }

    stack.shutdown();
}
