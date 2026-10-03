//! COLLIERY-T-0265 — the READ routes of a board take the slug or the id of
//! the board, against the booted production router.
//!
//! Before this ticket the routes took the id only:
//! `GET /api/boards/colliery-io-delivery` was a 422 (`The value
//! "colliery-io-delivery" of id is not a UUID.`). The board view of the GUI
//! has the slug of the board in its URL, so it read the full list of the
//! boards at each read to get the id.
//!
//! - `GET /api/boards/{id}`, `/items`, `/columns`, `/transitions` and
//!   `/members` give the same answer for the slug and for the id,
//! - an unknown slug and an unknown id are a 404 `NOT_FOUND`,
//! - a deleted board is a 404 by its slug and by its id,
//! - a board with a slug from before the rule of a slug can be read by
//!   that slug,
//! - the query parameters of the routes are as they were: a page of the
//!   items by slug, and the refusal of an unknown parameter,
//! - the WRITE routes take the id only.
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

#[tokio::test]
async fn the_read_routes_of_a_board_take_a_slug_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_by_slug_t0265_test").await;

    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
        )
        .await;
    let board_id = team["delivery_board_id"]
        .as_str()
        .expect("the delivery board")
        .to_string();
    for n in 0..5 {
        stack
            .ok(
                Method::POST,
                "/api/tasks",
                Some(json!({"board_id": board_id, "title": format!("Task {n}")})),
            )
            .await;
    }
    stack
        .ok(
            Method::POST,
            &format!("/api/boards/{board_id}/members"),
            Some(json!({"user_id": stack.bob_id, "capabilities": ["manage_tasks"]})),
        )
        .await;

    // --- the slug and the id give the same answer ----------------------------
    for tail in [
        "",
        "?include_removed_columns=true",
        "/items",
        "/items?limit=2&offset=1",
        "/items?include_deleted=true",
        "/columns",
        "/transitions",
        "/members",
    ] {
        let by_id = stack
            .ok(Method::GET, &format!("/api/boards/{board_id}{tail}"), None)
            .await;
        let by_slug = stack
            .ok(
                Method::GET,
                &format!("/api/boards/payments-delivery{tail}"),
                None,
            )
            .await;
        assert_eq!(by_slug, by_id, "GET /api/boards/<board>{tail}");
    }
    let detail = stack
        .ok(Method::GET, "/api/boards/payments-delivery", None)
        .await;
    assert_eq!(detail["id"], board_id.as_str(), "{detail}");
    assert_eq!(detail["slug"], "payments-delivery", "{detail}");
    assert!(!detail["columns"].as_array().expect("columns").is_empty());
    let page = stack
        .ok(
            Method::GET,
            "/api/boards/payments-delivery/items?limit=2&offset=1",
            None,
        )
        .await;
    assert_eq!(page["total"], 5, "{page}");
    assert_eq!(page["limit"], 2, "{page}");
    assert_eq!(page["offset"], 1, "{page}");
    let members = stack
        .ok(Method::GET, "/api/boards/payments-delivery/members", None)
        .await;
    assert_eq!(members.as_array().expect("members").len(), 1, "{members}");

    // --- an unknown board ----------------------------------------------------
    let unknown_id = Uuid::new_v4();
    for tail in ["", "/items", "/columns", "/transitions", "/members"] {
        let (status, answer) = stack
            .send(
                Method::GET,
                &format!("/api/boards/no-such-board{tail}"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{tail}: {answer}");
        assert_eq!(error_code(&answer), "NOT_FOUND", "{tail}: {answer}");
        assert_eq!(
            answer["error"]["message"], "No live board has the slug or the id \"no-such-board\".",
            "{tail}"
        );
        let (status, answer) = stack
            .send(
                Method::GET,
                &format!("/api/boards/{unknown_id}{tail}"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{tail}: {answer}");
        assert_eq!(
            answer["error"]["message"],
            format!("No live board has the id {unknown_id}."),
            "{tail}"
        );
    }
    // A value that cannot be a slug is a reference that no board has.
    let (status, answer) = stack
        .send(Method::GET, "/api/boards/Road%20Map", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");

    // --- an unknown parameter is refused, as before --------------------------
    let (status, answer) = stack
        .send(
            Method::GET,
            "/api/boards/payments-delivery/items?colour=red",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["error"]["details"]["parameter"], "colour");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("colour")),
        "{answer}"
    );

    // --- a slug from before the rule -----------------------------------------
    diesel::sql_query("UPDATE boards SET slug = 'Road Map' WHERE slug = 'initiatives'")
        .execute(&mut stack.conn)
        .expect("an old slug");
    let old = stack.ok(Method::GET, "/api/boards/Road%20Map", None).await;
    assert_eq!(old["slug"], "Road Map", "{old}");
    assert_eq!(old["board_level"], "initiative", "{old}");

    // --- the write routes take the id only -----------------------------------
    let (status, answer) = stack
        .send(
            Method::PATCH,
            "/api/boards/payments-delivery",
            Some(json!({"name": "Payments work"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(
        answer["error"]["message"],
        "The value \"payments-delivery\" of id is not a UUID.",
    );
    let (status, answer) = stack
        .send(Method::DELETE, "/api/boards/payments-delivery", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    let unchanged = stack
        .ok(Method::GET, "/api/boards/payments-delivery", None)
        .await;
    assert_eq!(unchanged["name"], "Payments Delivery", "{unchanged}");

    // --- a deleted board -----------------------------------------------------
    let web = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Web", "slug": "web", "code_prefix": "WEB"})),
        )
        .await;
    let web_board = web["delivery_board_id"]
        .as_str()
        .expect("board")
        .to_string();
    stack
        .ok(Method::GET, "/api/boards/web-delivery/items", None)
        .await;
    stack
        .ok(
            Method::DELETE,
            &format!("/api/teams/{}", web["id"].as_str().expect("team id")),
            None,
        )
        .await;
    for reference in ["web-delivery", web_board.as_str()] {
        let (status, answer) = stack
            .send(Method::GET, &format!("/api/boards/{reference}"), None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{reference}: {answer}");
    }

    stack.shutdown();
}
