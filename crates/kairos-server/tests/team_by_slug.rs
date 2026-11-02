//! KAIROS-T-0607 — the routes of a team take the slug or the id of the
//! team, against the booted production router.
//!
//! Before this task the routes took the id only: `GET /api/teams/payments`
//! was a 422 (`The value "payments" of id is not a UUID.`), so the CLI read
//! `/api/teams/by-slug/{slug}` before each of its team commands.
//!
//! - `GET /api/teams/{id}`, `/members`, `/pages`, `/tree-items`,
//!   `/archived-pages`, `/announcements`, `/links` and `/unlinked-refs`
//!   give the same answer for the slug and for the id,
//! - the writes take the slug too: a member, a page, an announcement and
//!   the team itself,
//! - an unknown slug and an unknown id are a 404 `NOT_FOUND`,
//! - a deleted team is a 404 by its slug and by its id,
//! - `/api/teams/by-slug/{slug}` stays.
//!
//! Runs against the LIVE compose stack (`angreal services up`). The test
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
    admin_conn: common::ScratchConn,
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

#[tokio::test]
async fn the_routes_of_a_team_take_a_slug_against_live_stack() {
    let stack = Stack::boot("kairos_team_by_slug_t0607_test").await;

    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
        )
        .await;
    let team_id = team["id"].as_str().expect("the team id").to_string();

    // --- the writes take the slug ------------------------------------------
    let member = stack
        .ok(
            Method::POST,
            "/api/teams/payments/members",
            Some(json!({"user_id": stack.bob_id})),
        )
        .await;
    assert_eq!(member["user_id"], stack.bob_id.to_string(), "{member}");
    let page = stack
        .ok(
            Method::POST,
            "/api/teams/payments/pages",
            Some(json!({"kind": "page", "slug": "runbook", "title": "Runbook", "content": "# Runbook"})),
        )
        .await;
    let page_id = page["id"].as_str().expect("the page id").to_string();
    stack
        .ok(
            Method::POST,
            "/api/teams/payments/announcements",
            Some(json!({"body": "Payments is live."})),
        )
        .await;
    let renamed = stack
        .ok(
            Method::PATCH,
            "/api/teams/payments",
            Some(json!({"name": "Payments Team"})),
        )
        .await;
    assert_eq!(renamed["name"], "Payments Team", "{renamed}");
    assert_eq!(renamed["id"], team_id, "{renamed}");

    // --- the slug and the id give the same answer --------------------------
    for tail in [
        "",
        "/members",
        "/pages",
        &format!("/pages/{page_id}"),
        &format!("/pages/{page_id}/history"),
        "/tree-items",
        "/archived-pages",
        "/announcements",
        "/links",
        "/unlinked-refs",
    ] {
        let by_id = stack
            .ok(Method::GET, &format!("/api/teams/{team_id}{tail}"), None)
            .await;
        let by_slug = stack
            .ok(Method::GET, &format!("/api/teams/payments{tail}"), None)
            .await;
        assert_eq!(by_id, by_slug, "GET /api/teams/{{ref}}{tail}");
    }
    // --- KAIROS-T-0629: the lists have no content; one page has it ------
    let tree = stack
        .ok(Method::GET, "/api/teams/payments/pages", None)
        .await;
    let rows = tree.as_array().expect("the tree");
    assert!(!rows.is_empty(), "{tree}");
    assert!(
        rows.iter().all(|row| row.get("content").is_none()),
        "a row of the tree has no content: {tree}"
    );
    let one = stack
        .ok(
            Method::GET,
            &format!("/api/teams/payments/pages/{page_id}"),
            None,
        )
        .await;
    assert_eq!(one["content"], "# Runbook", "{one}");
    let history = stack
        .ok(
            Method::GET,
            &format!("/api/teams/payments/pages/{page_id}/history"),
            None,
        )
        .await;
    assert_eq!(
        (history["total"].as_i64(), history["limit"].as_i64()),
        (Some(1), Some(50))
    );
    assert!(history["items"][0].get("content").is_none(), "{history}");
    let v1 = stack
        .ok(
            Method::GET,
            &format!("/api/teams/payments/pages/{page_id}/history?version=1"),
            None,
        )
        .await;
    assert_eq!(v1["content"], "# Runbook", "{v1}");
    let feed = stack
        .ok(
            Method::GET,
            "/api/teams/payments/announcements?limit=1&offset=0",
            None,
        )
        .await;
    assert_eq!(
        (feed["total"].as_i64(), feed["limit"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(feed["items"][0]["body"], "Payments is live.", "{feed}");
    for path in [
        format!("/api/teams/payments/pages/{page_id}/history?page=2"),
        "/api/teams/payments/announcements?page=2".to_string(),
    ] {
        let (status, answer) = stack.send(Method::GET, &path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {answer}");
        assert!(answer.to_string().contains("page"), "{path}: {answer}");
    }

    let by_slug_route = stack
        .ok(Method::GET, "/api/teams/by-slug/payments", None)
        .await;
    assert_eq!(by_slug_route["id"], team_id, "{by_slug_route}");

    // --- an unknown slug and an unknown id are a 404 -----------------------
    for reference in ["nobody", &Uuid::nil().to_string()] {
        for tail in ["", "/members", "/pages", "/announcements"] {
            let (status, answer) = stack
                .send(Method::GET, &format!("/api/teams/{reference}{tail}"), None)
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{reference}{tail}: {answer}");
            assert_eq!(
                error_code(&answer),
                "NOT_FOUND",
                "{reference}{tail}: {answer}"
            );
        }
        let (status, answer) = stack
            .send(
                Method::POST,
                &format!("/api/teams/{reference}/members"),
                Some(json!({"user_id": stack.bob_id})),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{reference}: {answer}");
    }

    // --- a member goes by the slug, then the page -----------------------------
    stack
        .ok(
            Method::DELETE,
            &format!("/api/teams/payments/members/{}", stack.bob_id),
            None,
        )
        .await;
    let members = stack
        .ok(Method::GET, "/api/teams/payments/members", None)
        .await;
    assert_eq!(members.as_array().map(Vec::len), Some(0), "{members}");
    stack
        .ok(
            Method::DELETE,
            &format!("/api/teams/payments/pages/{page_id}"),
            None,
        )
        .await;
    let archived = stack
        .ok(Method::GET, "/api/teams/payments/archived-pages", None)
        .await;
    assert_eq!(archived[0]["id"], page_id, "{archived}");

    // --- a deleted team ----------------------------------------------------
    stack.ok(Method::DELETE, "/api/teams/payments", None).await;
    for reference in ["payments", team_id.as_str()] {
        let (status, answer) = stack
            .send(Method::GET, &format!("/api/teams/{reference}"), None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{reference}: {answer}");
    }

    stack.shutdown();
}
