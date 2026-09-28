//! COLLIERY-T-0260 — a slug that a caller sends for a team or for a
//! delivery stream has the form of a slug, against the booted production
//! router.
//!
//! Before this ticket REST did no check of the form: a team could get the
//! slug `Road Map`, and its page `/teams/Road Map` and its delivery board
//! `Road Map-delivery` came from that slug. The rule is the rule of a
//! board slug (COLLIERY-T-0258), and one function has it
//! (`kairos_core::slug::is_valid_slug`).
//!
//! - the create and the update of a team and of a delivery stream refuse
//!   a slug that does not have the form: 422 `VALIDATION`,
//!   `details.field` = `slug`, and the message gives the rule,
//! - a refused request writes nothing,
//! - a team or a delivery stream with a slug from before the rule can be
//!   read, updated in other fields, and deleted,
//! - the slug that the server makes for the delivery board of a team gets
//!   no check.
//!
//! The tests of the board are in `board_slug.rs`.
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

/// The refusal of a slug that does not have the form of a slug: 422
/// `VALIDATION`, the message gives the kind, the slug and the rule, and
/// the details name the field.
fn assert_slug_form(status: StatusCode, answer: &Value, kind: &str, slug: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(error_code(answer), "VALIDATION", "{answer}");
    assert_eq!(
        answer["error"]["message"],
        format!(
            "The {kind} slug {slug:?} is not correct. A {kind} slug must match \
             ^[a-z][a-z0-9_-]{{1,62}}$, and it cannot have the form of a UUID. Send a \
             different slug."
        ),
        "{answer}"
    );
    assert_eq!(answer["error"]["details"], json!({"field": "slug"}));
}

/// The slugs that do not have the form.
fn bad_slugs() -> Vec<String> {
    let mut slugs: Vec<String> = [
        "Road Map",
        "payments!",
        "p",
        "9lives",
        "-payments",
        "_payments",
        "pay/ments",
        "pay.ments",
        "",
        "abcdef12-0000-7000-8000-000000000003",
    ]
    .iter()
    .map(|slug| slug.to_string())
    .collect();
    slugs.push("a".repeat(64));
    slugs
}

#[tokio::test]
async fn a_sent_team_slug_has_the_form_of_a_slug_against_live_stack() {
    let mut stack = Stack::boot("kairos_team_slug_form_t0260_test").await;

    // --- create -------------------------------------------------------------
    let before = (
        count(&mut stack.conn, "teams"),
        count(&mut stack.conn, "boards"),
        count(&mut stack.conn, "team_pages"),
        count(&mut stack.conn, "activity_log"),
    );
    for slug in bad_slugs() {
        let (status, answer) = stack
            .send(
                Method::POST,
                "/api/teams",
                Some(json!({"name": "Payments", "slug": slug})),
            )
            .await;
        // THE DEFECT: before COLLIERY-T-0260 each of these creates gave 201.
        assert_slug_form(status, &answer, "team", &slug);
    }
    assert_eq!(
        (
            count(&mut stack.conn, "teams"),
            count(&mut stack.conn, "boards"),
            count(&mut stack.conn, "team_pages"),
            count(&mut stack.conn, "activity_log"),
        ),
        before,
        "a refused create wrote nothing"
    );
    for slug in ["payments", "pay_ments-2", "ab"] {
        let team = stack
            .ok(
                Method::POST,
                "/api/teams",
                Some(json!({"name": "Payments", "slug": slug})),
            )
            .await;
        assert_eq!(team["slug"], slug, "{team}");
    }

    // --- the slug that the server makes for the delivery board ----------------
    // A team slug of 63 characters has the form. The slug of the delivery
    // board has 72 characters, and the server makes it, so it gets no
    // check.
    let long = "t".repeat(63);
    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Long", "slug": long})),
        )
        .await;
    let board_id = team["delivery_board_id"].as_str().expect("board id");
    let board = stack
        .ok(Method::GET, &format!("/api/boards/{board_id}"), None)
        .await;
    assert_eq!(board["slug"], format!("{long}-delivery"), "{board}");

    // --- update -------------------------------------------------------------
    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Platform", "slug": "platform"})),
        )
        .await;
    let team_id = team["id"].as_str().expect("team id").to_string();
    let team_uri = format!("/api/teams/{team_id}");
    let before = count(&mut stack.conn, "activity_log");
    for slug in bad_slugs() {
        let (status, answer) = stack
            .send(
                Method::PATCH,
                &team_uri,
                Some(json!({"name": "Renamed", "slug": slug})),
            )
            .await;
        // THE DEFECT: before COLLIERY-T-0260 each of these updates gave 200.
        assert_slug_form(status, &answer, "team", &slug);
    }
    let team = stack.ok(Method::GET, &team_uri, None).await;
    assert_eq!(team["slug"], "platform", "no change of the slug: {team}");
    assert_eq!(team["name"], "Platform", "no change of the name: {team}");
    assert_eq!(count(&mut stack.conn, "activity_log"), before);
    let team = stack
        .ok(
            Method::PATCH,
            &team_uri,
            Some(json!({"slug": "platform-2"})),
        )
        .await;
    assert_eq!(team["slug"], "platform-2", "{team}");

    // --- a team with a slug from before the rule stays as it is ---------------
    diesel::sql_query(format!(
        "UPDATE teams SET slug = 'Old Platform' WHERE id = '{team_id}'"
    ))
    .execute(&mut stack.conn)
    .expect("a slug from before the rule");
    let team = stack.ok(Method::GET, &team_uri, None).await;
    assert_eq!(team["slug"], "Old Platform", "{team}");
    let team = stack
        .ok(Method::GET, "/api/teams/by-slug/Old%20Platform", None)
        .await;
    assert_eq!(team["id"], team_id.as_str(), "{team}");
    // An update of a different field passes, with the slug and with no slug.
    let team = stack
        .ok(
            Method::PATCH,
            &team_uri,
            Some(json!({"name": "Platform 2"})),
        )
        .await;
    assert_eq!(team["name"], "Platform 2", "{team}");
    let team = stack
        .ok(
            Method::PATCH,
            &team_uri,
            Some(json!({"name": "Platform 3", "slug": "Old Platform"})),
        )
        .await;
    assert_eq!(team["name"], "Platform 3", "{team}");
    assert_eq!(team["slug"], "Old Platform", "{team}");
    // A different slug that does not have the form is a refusal.
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &team_uri,
            Some(json!({"slug": "Older Platform"})),
        )
        .await;
    assert_slug_form(status, &answer, "team", "Older Platform");
    // The delete passes.
    let deleted = stack.ok(Method::DELETE, &team_uri, None).await;
    assert_eq!(deleted["deleted"], true, "{deleted}");

    stack.shutdown();
}

#[tokio::test]
async fn a_sent_stream_slug_has_the_form_of_a_slug_against_live_stack() {
    let mut stack = Stack::boot("kairos_stream_slug_form_t0260_test").await;

    // --- create -------------------------------------------------------------
    let before = (
        count(&mut stack.conn, "delivery_streams"),
        count(&mut stack.conn, "activity_log"),
    );
    for slug in bad_slugs() {
        let (status, answer) = stack
            .send(
                Method::POST,
                "/api/delivery-streams",
                Some(json!({"name": "Checkout", "slug": slug})),
            )
            .await;
        // THE DEFECT: before COLLIERY-T-0260 each of these creates gave 201.
        assert_slug_form(status, &answer, "delivery stream", &slug);
    }
    assert_eq!(
        (
            count(&mut stack.conn, "delivery_streams"),
            count(&mut stack.conn, "activity_log"),
        ),
        before,
        "a refused create wrote nothing"
    );
    for slug in ["checkout", "check_out-2", "ab", &"a".repeat(63)] {
        let stream = stack
            .ok(
                Method::POST,
                "/api/delivery-streams",
                Some(json!({"name": "Checkout", "slug": slug})),
            )
            .await;
        assert_eq!(stream["slug"], slug, "{stream}");
    }

    // --- update -------------------------------------------------------------
    let stream = stack
        .ok(
            Method::POST,
            "/api/delivery-streams",
            Some(json!({"name": "Growth", "slug": "growth"})),
        )
        .await;
    let stream_id = stream["id"].as_str().expect("stream id").to_string();
    let stream_uri = format!("/api/delivery-streams/{stream_id}");
    let before = count(&mut stack.conn, "activity_log");
    for slug in bad_slugs() {
        let (status, answer) = stack
            .send(
                Method::PATCH,
                &stream_uri,
                Some(json!({"name": "Renamed", "slug": slug})),
            )
            .await;
        // THE DEFECT: before COLLIERY-T-0260 each of these updates gave 200.
        assert_slug_form(status, &answer, "delivery stream", &slug);
    }
    let stream = stack.ok(Method::GET, &stream_uri, None).await;
    assert_eq!(stream["slug"], "growth", "no change of the slug: {stream}");
    assert_eq!(stream["name"], "Growth", "no change of the name: {stream}");
    assert_eq!(count(&mut stack.conn, "activity_log"), before);
    let stream = stack
        .ok(
            Method::PATCH,
            &stream_uri,
            Some(json!({"slug": "growth-2"})),
        )
        .await;
    assert_eq!(stream["slug"], "growth-2", "{stream}");

    // --- a stream with a slug from before the rule stays as it is -------------
    diesel::sql_query(format!(
        "UPDATE delivery_streams SET slug = 'Old Growth' WHERE id = '{stream_id}'"
    ))
    .execute(&mut stack.conn)
    .expect("a slug from before the rule");
    let stream = stack.ok(Method::GET, &stream_uri, None).await;
    assert_eq!(stream["slug"], "Old Growth", "{stream}");
    // An update of a different field passes, with the slug and with no slug.
    let stream = stack
        .ok(
            Method::PATCH,
            &stream_uri,
            Some(json!({"description": "The growth of the product."})),
        )
        .await;
    assert_eq!(stream["slug"], "Old Growth", "{stream}");
    let stream = stack
        .ok(
            Method::PATCH,
            &stream_uri,
            Some(json!({"name": "Growth 2", "slug": "Old Growth"})),
        )
        .await;
    assert_eq!(stream["name"], "Growth 2", "{stream}");
    assert_eq!(stream["slug"], "Old Growth", "{stream}");
    // A different slug that does not have the form is a refusal.
    let (status, answer) = stack
        .send(
            Method::PATCH,
            &stream_uri,
            Some(json!({"slug": "Older Growth"})),
        )
        .await;
    assert_slug_form(status, &answer, "delivery stream", "Older Growth");
    // The delete passes.
    let deleted = stack.ok(Method::DELETE, &stream_uri, None).await;
    assert_eq!(deleted["deleted"], true, "{deleted}");

    stack.shutdown();
}
