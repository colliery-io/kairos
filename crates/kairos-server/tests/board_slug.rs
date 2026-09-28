//! COLLIERY-T-0255 — two live boards cannot have the same slug, over REST
//! and SCIM against the booted production router.
//!
//! A board is addressed by its slug in the GUI (`/boards/{slug}`), in MCP
//! (`board`) and in the CLI. Before this ticket the table `boards` had no
//! unique index on `slug` and no check: two live boards could have one
//! slug, and a reference by slug gave one of them.
//!
//! The rule is that of the slug of a team (KAIROS-T-0184): unique among
//! the LIVE boards. The refusal is 409 `CONFLICT`. It names the slug and
//! the board that has it.
//!
//! - the create and the update of a board refuse the slug of a live board,
//! - a deleted board does not keep its slug,
//! - a team whose delivery board cannot get its slug is not created, over
//!   REST and over SCIM,
//! - a reference by slug gives the live board.
//!
//! COLLIERY-T-0258 — a slug that a caller sends has the form of a board
//! slug. The refusal is 422 `VALIDATION`, and it gives the rule.
//!
//! The migration is in `kairos-db/tests/board_slug_migration.rs`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Each test
//! owns a scratch database.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
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

/// The booted router on a scratch database, with `svc` as the org admin.
struct Stack {
    scratch_db: &'static str,
    admin_conn: PgConnection,
    /// Pinned to the tenant schema.
    conn: PgConnection,
    pool: TenantPool,
    router: Router,
    token: String,
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
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(
            Authenticator::discover(ISSUER, AUDIENCE)
                .await
                .expect("OIDC discovery against live Dex"),
        );
        // The routes of the local accounts are there only with local auth.
        let mut config = base_config(&scratch_url);
        config.local_auth = true;
        let router = app::router(app::state_with(config, pool.clone(), auth));

        // JIT-provision svc, then make it the org admin.
        let _ = request(
            &router,
            Method::GET,
            "/api/whoami",
            Some(&token),
            &TENANT,
            None,
        )
        .await;
        let svc_id: Uuid = users::table
            .filter(users::email.eq("svc@kairos.test"))
            .select(users::id)
            .first(&mut conn)
            .expect("svc provisioned");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: svc_id,
                role: OrgRole::Admin,
            })
            .execute(&mut conn)
            .expect("granting membership");
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

    /// One SCIM request: the bearer is the SCIM token, and no tenant
    /// header (the token gives the tenant).
    async fn scim(&self, token: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/scim+json")
            .body(Body::from(body.to_string()))
            .expect("request");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    /// One board create. Returns the status and the answer.
    async fn create_board(&self, name: &str, slug: &str) -> (StatusCode, Value) {
        self.send(
            Method::POST,
            "/api/boards",
            Some(json!({"name": name, "slug": slug, "board_level": "initiative"})),
        )
        .await
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// The number of rows that a query counts, in the tenant.
fn count(conn: &mut PgConnection, from_where: &str) -> i64 {
    diesel::sql_query(format!("SELECT count(*) AS count FROM {from_where}"))
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("counting {from_where}: {e}"))
        .count
}

/// The number of LIVE boards with a slug.
fn live_boards(conn: &mut PgConnection, slug: &str) -> i64 {
    count(
        conn,
        &format!("boards WHERE slug = '{slug}' AND deleted_at IS NULL"),
    )
}

/// The refusal of a slug that a live board has: 409 `CONFLICT`, and the
/// message and the details name the slug and the board.
fn assert_slug_taken(status: StatusCode, answer: &Value, slug: &str, holder: &Value) {
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(error_code(answer), "CONFLICT", "{answer}");
    let message = answer["error"]["message"].as_str().unwrap_or_default();
    let name = holder["name"].as_str().expect("the name of the holder");
    assert!(
        message.contains(&format!("{slug:?}")) && message.contains(&format!("{name:?}")),
        "the message names the slug and the board: {message}"
    );
    assert_eq!(answer["error"]["details"]["slug"], slug, "{answer}");
    assert_eq!(
        answer["error"]["details"]["board"],
        json!({"id": holder["id"], "name": holder["name"]}),
        "{answer}"
    );
}

#[tokio::test]
async fn a_live_board_has_its_slug_alone_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_slug_t0255_test").await;

    // --- create -------------------------------------------------------------
    let (status, roadmap) = stack.create_board("Roadmap", "roadmap").await;
    assert_eq!(status, StatusCode::CREATED, "{roadmap}");
    let roadmap_id = roadmap["id"].as_str().expect("board id").to_string();

    let before = (
        count(&mut stack.conn, "boards"),
        count(&mut stack.conn, "board_columns"),
        count(&mut stack.conn, "activity_log"),
    );
    let (status, answer) = stack.create_board("Second Roadmap", "roadmap").await;
    // THE DEFECT: before COLLIERY-T-0255 this create gave 201, and the
    // tenant had two live boards with the slug `roadmap`.
    assert_slug_taken(status, &answer, "roadmap", &roadmap);
    assert_eq!(
        answer["error"]["message"],
        "The live board \"Roadmap\" has the slug \"roadmap\". Two live boards cannot have \
         the same slug. Send a different slug."
    );
    assert_eq!(live_boards(&mut stack.conn, "roadmap"), 1);
    assert_eq!(
        (
            count(&mut stack.conn, "boards"),
            count(&mut stack.conn, "board_columns"),
            count(&mut stack.conn, "activity_log"),
        ),
        before,
        "the refused create wrote nothing"
    );

    // --- update -------------------------------------------------------------
    let (status, plans) = stack.create_board("Plans", "plans").await;
    assert_eq!(status, StatusCode::CREATED, "{plans}");
    let plans_uri = format!("/api/boards/{}", plans["id"].as_str().expect("board id"));
    let before = count(&mut stack.conn, "activity_log");
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &plans_uri,
            Some(json!({"name": "Renamed", "slug": "roadmap"})),
        )
        .await;
    assert_slug_taken(status, &answer, "roadmap", &roadmap);
    let board = stack.ok(Method::GET, &plans_uri, None).await;
    assert_eq!(board["slug"], "plans", "no change of the slug: {board}");
    assert_eq!(board["name"], "Plans", "no change of the name: {board}");
    assert_eq!(count(&mut stack.conn, "activity_log"), before);
    // A board can keep its slug in an update: it is not in conflict with
    // itself.
    let board = stack
        .ok(
            Method::PATCH,
            &format!("/api/boards/{roadmap_id}"),
            Some(json!({"name": "The Roadmap", "slug": "roadmap"})),
        )
        .await;
    assert_eq!(board["name"], "The Roadmap", "{board}");

    // --- a deleted board does not keep its slug ------------------------------
    stack
        .ok(Method::DELETE, &format!("/api/boards/{roadmap_id}"), None)
        .await;
    assert_eq!(live_boards(&mut stack.conn, "roadmap"), 0);
    // The update that was refused passes now.
    let board = stack
        .ok(Method::PATCH, &plans_uri, Some(json!({"slug": "roadmap"})))
        .await;
    assert_eq!(board["slug"], "roadmap", "{board}");
    stack
        .ok(Method::PATCH, &plans_uri, Some(json!({"slug": "plans"})))
        .await;
    // And a new board gets the slug.
    let (status, again) = stack.create_board("Roadmap again", "roadmap").await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_eq!(
        count(&mut stack.conn, "boards WHERE slug = 'roadmap'"),
        2,
        "the deleted board and the live board"
    );
    assert_eq!(live_boards(&mut stack.conn, "roadmap"), 1);

    // --- a reference by slug gives the live board ----------------------------
    // Two boards have the slug `roadmap`: one deleted, one live. A create
    // by slug goes to the live board.
    let initiative = stack
        .ok(
            Method::POST,
            "/api/initiatives",
            Some(json!({"board_id": "roadmap", "title": "By slug"})),
        )
        .await;
    assert_eq!(initiative["board_id"], again["id"], "{initiative}");
    // The deleted board has no reference by slug. Its id reads it no more.
    let (status, answer) = stack
        .send(Method::GET, &format!("/api/boards/{roadmap_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");

    stack.shutdown();
}

#[tokio::test]
async fn a_team_is_not_created_without_its_board_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_slug_team_t0255_test").await;

    // A board of the organization has the slug that the delivery board of
    // the team `payments` gets.
    let (status, holder) = stack
        .create_board("Payments Delivery Plan", "payments-delivery")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{holder}");

    // --- REST ---------------------------------------------------------------
    let before = (
        count(&mut stack.conn, "teams"),
        count(&mut stack.conn, "boards"),
        count(&mut stack.conn, "team_pages"),
        count(&mut stack.conn, "activity_log"),
    );
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments"})),
        )
        .await;
    assert_slug_taken(status, &answer, "payments-delivery", &holder);
    assert_eq!(
        answer["error"]["message"],
        "The delivery board of the team gets the slug \"payments-delivery\". The live board \
         \"Payments Delivery Plan\" has that slug. Send a different slug for the team, or \
         change the slug of that board."
    );
    assert_eq!(
        (
            count(&mut stack.conn, "teams"),
            count(&mut stack.conn, "boards"),
            count(&mut stack.conn, "team_pages"),
            count(&mut stack.conn, "activity_log"),
        ),
        before,
        "no team, no board, no page: the create is one transaction"
    );
    let (status, answer) = stack
        .send(Method::GET, "/api/teams/by-slug/payments", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");

    // --- SCIM ---------------------------------------------------------------
    let token = stack
        .ok(
            Method::POST,
            "/api/scim-tokens",
            Some(json!({"name": "issuer"})),
        )
        .await;
    let token = token["token"].as_str().expect("the SCIM token").to_string();
    let before = (
        count(&mut stack.conn, "teams"),
        count(&mut stack.conn, "boards"),
        count(&mut stack.conn, "activity_log"),
    );
    let (status, answer) = stack
        .scim(
            &token,
            "/scim/v2/Groups",
            json!({"displayName": "kairos-team-payments"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(answer["scimType"], "uniqueness", "{answer}");
    assert_eq!(
        answer["detail"],
        "The delivery board of the team gets the slug \"payments-delivery\". The live board \
         \"Payments Delivery Plan\" has that slug. Change the slug of that board, or use a \
         different name for the group."
    );
    assert_eq!(
        (
            count(&mut stack.conn, "teams"),
            count(&mut stack.conn, "boards"),
            count(&mut stack.conn, "activity_log"),
        ),
        before,
        "no team and no board: the create is one transaction"
    );

    // --- with the slug free, the two creates pass ----------------------------
    stack
        .ok(
            Method::PATCH,
            &format!("/api/boards/{}", holder["id"].as_str().expect("id")),
            Some(json!({"slug": "payments-plan"})),
        )
        .await;
    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments"})),
        )
        .await;
    assert!(team["delivery_board_id"].is_string(), "{team}");
    assert_eq!(live_boards(&mut stack.conn, "payments-delivery"), 1);

    stack.shutdown();
}

/// The refusal of a slug that does not have the form of a board slug
/// (COLLIERY-T-0258): 422 `VALIDATION`, the message gives the slug and the
/// rule, and the details name the field.
fn assert_slug_form(status: StatusCode, answer: &Value, slug: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(error_code(answer), "VALIDATION", "{answer}");
    assert_eq!(
        answer["error"]["message"],
        format!(
            "The board slug {slug:?} is not correct. A board slug must match \
             ^[a-z][a-z0-9_-]{{1,62}}$, and it cannot have the form of a UUID. Send a \
             different slug."
        ),
        "{answer}"
    );
    assert_eq!(answer["error"]["details"], json!({"field": "slug"}));
}

/// COLLIERY-T-0258: a slug that a caller sends has the form of a board
/// slug. The slug that the server makes for a team, and the slug that a
/// board has from before the rule, are not subject to the check.
#[tokio::test]
async fn a_sent_slug_has_the_form_of_a_board_slug_against_live_stack() {
    let mut stack = Stack::boot("kairos_board_slug_form_t0258_test").await;

    // --- create -------------------------------------------------------------
    let before = (
        count(&mut stack.conn, "boards"),
        count(&mut stack.conn, "activity_log"),
    );
    for slug in [
        "Road Map",
        "roadmap!",
        "r",
        "9lives",
        "-roadmap",
        "road/map",
        "",
        "abcdef12-0000-7000-8000-000000000003",
    ] {
        let (status, answer) = stack.create_board("Roadmap", slug).await;
        // THE DEFECT: before COLLIERY-T-0258 each of these creates gave 201.
        assert_slug_form(status, &answer, slug);
    }
    let long = "a".repeat(64);
    let (status, answer) = stack.create_board("Roadmap", &long).await;
    assert_slug_form(status, &answer, &long);
    assert_eq!(
        (
            count(&mut stack.conn, "boards"),
            count(&mut stack.conn, "activity_log"),
        ),
        before,
        "a refused create wrote nothing"
    );
    for slug in ["roadmap", "road_map-2", "ab", &"a".repeat(63)] {
        let (status, answer) = stack.create_board("Roadmap", slug).await;
        assert_eq!(status, StatusCode::CREATED, "{slug}: {answer}");
    }

    // --- update -------------------------------------------------------------
    let (status, plans) = stack.create_board("Plans", "plans").await;
    assert_eq!(status, StatusCode::CREATED, "{plans}");
    let plans_id = plans["id"].as_str().expect("board id").to_string();
    let plans_uri = format!("/api/boards/{plans_id}");
    let before = count(&mut stack.conn, "activity_log");
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &plans_uri,
            Some(json!({"name": "Renamed", "slug": "The Plans"})),
        )
        .await;
    assert_slug_form(status, &answer, "The Plans");
    let board = stack.ok(Method::GET, &plans_uri, None).await;
    assert_eq!(board["slug"], "plans", "no change of the slug: {board}");
    assert_eq!(board["name"], "Plans", "no change of the name: {board}");
    assert_eq!(count(&mut stack.conn, "activity_log"), before);

    // --- a board with a slug from before the rule stays as it is --------------
    diesel::sql_query(format!(
        "UPDATE boards SET slug = 'Old Plans' WHERE id = '{plans_id}'"
    ))
    .execute(&mut stack.conn)
    .expect("a slug from before the rule");
    let board = stack.ok(Method::GET, &plans_uri, None).await;
    assert_eq!(board["slug"], "Old Plans", "{board}");
    // An update of a different field passes, with the slug and with no slug.
    let board = stack
        .ok(Method::PATCH, &plans_uri, Some(json!({"name": "Plans 2"})))
        .await;
    assert_eq!(board["name"], "Plans 2", "{board}");
    let board = stack
        .ok(
            Method::PATCH,
            &plans_uri,
            Some(json!({"name": "Plans 3", "slug": "Old Plans"})),
        )
        .await;
    assert_eq!(board["name"], "Plans 3", "{board}");
    assert_eq!(board["slug"], "Old Plans", "{board}");
    // A different slug that does not have the form is a refusal.
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &plans_uri,
            Some(json!({"slug": "Older Plans"})),
        )
        .await;
    assert_slug_form(status, &answer, "Older Plans");
    // The board can get a slug that has the form, and the delete passes.
    let (status, old) = stack.create_board("Old", "old").await;
    assert_eq!(status, StatusCode::CREATED, "{old}");
    let old_id = old["id"].as_str().expect("board id").to_string();
    diesel::sql_query(format!(
        "UPDATE boards SET slug = 'Old Board' WHERE id = '{old_id}'"
    ))
    .execute(&mut stack.conn)
    .expect("a slug from before the rule");
    stack
        .ok(Method::DELETE, &format!("/api/boards/{old_id}"), None)
        .await;
    let board = stack
        .ok(Method::PATCH, &plans_uri, Some(json!({"slug": "plans"})))
        .await;
    assert_eq!(board["slug"], "plans", "{board}");

    // --- the slug that the server makes for a team ----------------------------
    // REST has no rule for the form of a team slug, and a team slug of 63
    // characters is correct for SCIM. The slug of the delivery board is
    // longer than 63 characters, and the server makes the board.
    let team_slug = "t".repeat(63);
    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Long", "slug": team_slug})),
        )
        .await;
    let board_id = team["delivery_board_id"].as_str().expect("board id");
    let board_uri = format!("/api/boards/{board_id}");
    let board = stack.ok(Method::GET, &board_uri, None).await;
    assert_eq!(board["slug"], format!("{team_slug}-delivery"), "{board}");
    let board = stack
        .ok(
            Method::PATCH,
            &board_uri,
            Some(json!({"name": "Long board"})),
        )
        .await;
    assert_eq!(board["name"], "Long board", "{board}");

    stack.shutdown();
}
