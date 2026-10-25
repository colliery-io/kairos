//! KAIROS-T-0268 — an update whose values are equal to the stored values
//! writes nothing and makes no activity entry, against the booted
//! production router.
//!
//! The forms of the GUI send each field at each save. Before this task the
//! update of a team and of a delivery stream wrote the row and made an
//! activity entry also when no value changed, so the feed showed changes
//! that did not occur. The repository follows the rule since
//! COLLIERY-T-0267 (`repository_fields.rs`).
//!
//! For each update route the test shows:
//!
//! - a body with no field is refused (422 `VALIDATION`), where the body
//!   can have no field;
//! - a body with the stored values is a success: the row keeps its
//!   `updated_at` (or its version), and the activity log gets no entry;
//! - a body with a different value writes, and makes one activity entry
//!   with the action `update` (for a team page, a template and a metadata
//!   definition too: before this task they made none).
//!
//! The routes: teams, delivery streams, boards, columns, board members,
//! organization members, team pages (content and structure), templates
//! and metadata definitions.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_update_no_change_t0268_test`.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text, Timestamptz};
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
    admin_conn: common::ScratchConn,
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

    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        request(&self.router, method, uri, Some(&self.token), &TENANT, body).await
    }

    async fn ok(&self, method: Method, uri: &str, body: Option<Value>) -> Value {
        let (status, answer) = self.send(method.clone(), uri, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {answer}");
        answer
    }

    /// A PATCH with no field: a 422 `VALIDATION` that says so, and no
    /// activity entry.
    async fn refuses_no_field(&mut self, uri: &str) {
        let before = activity_count(&mut self.conn);
        let (status, answer) = self.send(Method::PATCH, uri, Some(json!({}))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}: {answer}");
        assert_eq!(answer["error"]["code"], "VALIDATION", "{uri}: {answer}");
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("no field to change")),
            "{uri}: {answer}"
        );
        assert_eq!(activity_count(&mut self.conn), before, "{uri}");
    }

    /// A PATCH with the stored values: a success, the row keeps its
    /// `updated_at` (`stamp`), and the activity log gets no entry. Returns
    /// the answer.
    async fn writes_nothing(
        &mut self,
        uri: &str,
        body: Value,
        stamp: impl Fn(&mut PgConnection) -> String,
    ) -> Value {
        let before = (activity_count(&mut self.conn), stamp(&mut self.conn));
        let answer = self.ok(Method::PATCH, uri, Some(body.clone())).await;
        let after = (activity_count(&mut self.conn), stamp(&mut self.conn));
        assert_eq!(after, before, "{uri} {body}: it wrote");
        answer
    }

    /// A PATCH with a different value: it writes, and makes ONE activity
    /// entry with `action` about `entity`.
    async fn writes_once(&mut self, uri: &str, body: Value, entity: &str, action: &str) -> Value {
        let before = activity_count(&mut self.conn);
        let updates = action_count(&mut self.conn, entity, action);
        let answer = self.ok(Method::PATCH, uri, Some(body.clone())).await;
        assert_eq!(activity_count(&mut self.conn), before + 1, "{uri} {body}");
        assert_eq!(
            action_count(&mut self.conn, entity, action),
            updates + 1,
            "{uri} {body}: no entry with the action {action} about {entity}"
        );
        answer
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct StampRow {
    #[diesel(sql_type = Timestamptz)]
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// The number of the entries of the activity log of the tenant.
fn activity_count(conn: &mut PgConnection) -> i64 {
    diesel::sql_query("SELECT count(*) AS count FROM activity_log")
        .get_result::<CountRow>(conn)
        .expect("count of activity entries")
        .count
}

/// The number of the entries with `action` about one entity.
fn action_count(conn: &mut PgConnection, entity: &str, action: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*) AS count FROM activity_log \
          WHERE entity_id = $1::uuid AND action::text = $2",
    )
    .bind::<Text, _>(entity)
    .bind::<Text, _>(action)
    .get_result::<CountRow>(conn)
    .expect("count of update entries")
    .count
}

/// A function that reads `updated_at` of one row of `table`.
fn stamp_of(table: &'static str, id: String) -> impl Fn(&mut PgConnection) -> String {
    move |conn| {
        diesel::sql_query(format!(
            "SELECT updated_at FROM {table} WHERE id = $1::uuid"
        ))
        .bind::<Text, _>(&id)
        .get_result::<StampRow>(conn)
        .unwrap_or_else(|e| panic!("updated_at of {table} {id}: {e}"))
        .updated_at
        .to_rfc3339()
    }
}

fn id_of(value: &Value) -> String {
    value["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id in {value}"))
        .to_string()
}

#[tokio::test]
async fn an_update_with_the_stored_values_writes_nothing_against_live_stack() {
    let mut stack = Stack::boot("kairos_update_no_change_t0268_test").await;

    // =====================================================================
    // A team
    // =====================================================================
    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
        )
        .await;
    let team_id = id_of(&team);
    let team_uri = format!("/api/teams/{team_id}");
    stack.refuses_no_field(&team_uri).await;
    for body in [
        json!({"name": "Payments"}),
        json!({"slug": "payments"}),
        json!({"name": "Payments", "slug": "payments", "team_type": team["team_type"]}),
    ] {
        stack
            .writes_nothing(&team_uri, body, stamp_of("teams", team_id.clone()))
            .await;
    }
    let answer = stack
        .writes_once(
            &team_uri,
            json!({"name": "Payments and billing", "slug": "payments"}),
            &team_id,
            "update",
        )
        .await;
    assert_eq!(answer["name"], "Payments and billing");

    // =====================================================================
    // A delivery stream
    // =====================================================================
    let stream = stack
        .ok(
            Method::POST,
            "/api/delivery-streams",
            Some(json!({"name": "Checkout", "slug": "checkout", "description": "Pay"})),
        )
        .await;
    let stream_id = id_of(&stream);
    let stream_uri = format!("/api/delivery-streams/{stream_id}");
    stack.refuses_no_field(&stream_uri).await;
    stack
        .writes_nothing(
            &stream_uri,
            json!({"name": "Checkout", "slug": "checkout", "description": "Pay"}),
            stamp_of("delivery_streams", stream_id.clone()),
        )
        .await;
    stack
        .writes_once(
            &stream_uri,
            json!({"description": "Pay now"}),
            &stream_id,
            "update",
        )
        .await;

    // =====================================================================
    // A board
    // =====================================================================
    let board_id = team["delivery_board_id"]
        .as_str()
        .expect("the delivery board of the team")
        .to_string();
    let board_uri = format!("/api/boards/{board_id}");
    let board = stack.ok(Method::GET, &board_uri, None).await;
    stack.refuses_no_field(&board_uri).await;
    stack
        .writes_nothing(
            &board_uri,
            json!({"name": board["name"], "slug": board["slug"], "team_id": team_id}),
            stamp_of("boards", board_id.clone()),
        )
        .await;
    stack
        .writes_once(
            &board_uri,
            json!({"name": "Payments work", "slug": board["slug"]}),
            &board_id,
            "board_config",
        )
        .await;

    // =====================================================================
    // A column
    // =====================================================================
    let columns = board["columns"].as_array().expect("columns").clone();
    let column = columns
        .iter()
        .find(|c| c["position"] == 1)
        .expect("the column at position 1")
        .clone();
    let column_id = id_of(&column);
    let column_uri = format!("/api/boards/{board_id}/columns/{column_id}");
    stack.refuses_no_field(&column_uri).await;
    let answer = stack
        .writes_nothing(
            &column_uri,
            json!({
                "name": column["name"],
                "position": column["position"],
                "is_done": column["is_done"],
                "claims": column["claims"],
            }),
            stamp_of("board_columns", column_id.clone()),
        )
        .await;
    assert_eq!(answer["name"], column["name"]);
    // The column order of the board stays.
    let after = stack.ok(Method::GET, &board_uri, None).await;
    assert_eq!(after["columns"], board["columns"]);
    stack
        .writes_once(
            &column_uri,
            json!({"name": "Ready", "position": column["position"]}),
            &board_id,
            "board_config",
        )
        .await;

    // =====================================================================
    // A member of a board
    // =====================================================================
    let bob = stack.bob_id.to_string();
    stack
        .ok(
            Method::POST,
            &format!("/api/boards/{board_id}/members"),
            Some(json!({"user_id": bob, "capabilities": ["manage_tasks", "transition_items"]})),
        )
        .await;
    let member_uri = format!("/api/boards/{board_id}/members/{bob}");
    let before = activity_count(&mut stack.conn);
    let answer = stack
        .ok(
            Method::PATCH,
            &member_uri,
            // The same set, in a different sequence.
            Some(json!({"capabilities": ["transition_items", "manage_tasks"]})),
        )
        .await;
    assert_eq!(activity_count(&mut stack.conn), before, "{answer}");
    stack
        .ok(
            Method::PATCH,
            &member_uri,
            Some(json!({"capabilities": ["manage_tasks"]})),
        )
        .await;
    assert_eq!(activity_count(&mut stack.conn), before + 1);

    // =====================================================================
    // A member of the organization
    // =====================================================================
    let org_member_uri = format!("/api/members/{bob}");
    let before = activity_count(&mut stack.conn);
    stack
        .ok(
            Method::PATCH,
            &org_member_uri,
            Some(json!({"role": "member"})),
        )
        .await;
    assert_eq!(activity_count(&mut stack.conn), before);
    stack
        .writes_once(&org_member_uri, json!({"role": "admin"}), &bob, "update")
        .await;

    // =====================================================================
    // A team page: the content, then the structure
    // =====================================================================
    let page = stack
        .ok(
            Method::POST,
            &format!("/api/teams/{team_id}/pages"),
            Some(json!({
                "kind": "page", "slug": "runbook", "title": "Runbook",
                "content": "Restart the worker.", "position": 3,
            })),
        )
        .await;
    let page_id = id_of(&page);
    let page_uri = format!("/api/teams/{team_id}/pages/{page_id}");
    stack.refuses_no_field(&page_uri).await;
    let version = page["version"].clone();
    let answer = stack
        .writes_nothing(
            &page_uri,
            json!({"title": "Runbook", "content": "Restart the worker.", "version": version}),
            stamp_of("team_pages", page_id.clone()),
        )
        .await;
    assert_eq!(answer["version"], version, "no new version");
    stack
        .writes_nothing(
            &page_uri,
            json!({"slug": "runbook", "position": 3}),
            stamp_of("team_pages", page_id.clone()),
        )
        .await;
    let answer = stack
        .writes_once(
            &page_uri,
            json!({"content": "Restart the worker, then the queue.", "version": version}),
            &page_id,
            "update",
        )
        .await;
    assert_ne!(answer["version"], version);
    stack
        .writes_once(&page_uri, json!({"slug": "run-book"}), &page_id, "update")
        .await;

    // =====================================================================
    // A metadata definition
    // =====================================================================
    let definition = stack
        .ok(
            Method::POST,
            "/api/metadata-definitions",
            Some(json!({
                "name": "Risk", "slug": "risk", "field_type": "enum",
                "enum_options": ["low", "high"], "entity_types": ["task", "initiative"],
            })),
        )
        .await;
    let definition_id = id_of(&definition);
    let definition_uri = format!("/api/metadata-definitions/{definition_id}");
    stack.refuses_no_field(&definition_uri).await;
    stack
        .writes_nothing(
            &definition_uri,
            json!({
                "name": "Risk", "slug": "risk", "enum_options": ["low", "high"],
                // A set: the sequence is not a value.
                "entity_types": ["task", "initiative"],
            }),
            stamp_of("metadata_definitions", definition_id.clone()),
        )
        .await;
    stack
        .writes_nothing(
            &definition_uri,
            json!({"entity_types": ["initiative", "task"]}),
            stamp_of("metadata_definitions", definition_id.clone()),
        )
        .await;
    // The options are a list: a different sequence is a change.
    let answer = stack
        .writes_once(
            &definition_uri,
            json!({"enum_options": ["high", "low"]}),
            &definition_id,
            "update",
        )
        .await;
    assert_eq!(answer["enum_options"], json!(["high", "low"]));

    // =====================================================================
    // A template
    // =====================================================================
    let template = stack
        .ok(
            Method::POST,
            "/api/templates",
            Some(json!({
                "name": "Postmortem", "slug": "postmortem", "content": "## What happened",
                "metadata": [{"definition_slug": "risk", "default_value": "low", "required": true}],
            })),
        )
        .await;
    let template_id = id_of(&template);
    let template_uri = format!("/api/templates/{template_id}");
    stack.refuses_no_field(&template_uri).await;
    stack
        .writes_nothing(
            &template_uri,
            // KAIROS-T-0696: REST accepts `definition`, the name on MCP.
            json!({
                "name": "Postmortem", "slug": "postmortem", "content": "## What happened",
                "metadata": [{"definition": "risk", "default_value": "low", "required": true}],
            }),
            stamp_of("templates", template_id.clone()),
        )
        .await;
    stack
        .writes_once(
            &template_uri,
            json!({
                "content": "## What happened",
                "metadata": [{"definition_slug": "risk", "default_value": "high", "required": true}],
            }),
            &template_id,
            "update",
        )
        .await;
    let answer = stack
        .writes_once(
            &template_uri,
            json!({"name": "Incident review"}),
            &template_id,
            "update",
        )
        .await;
    assert_eq!(answer["name"], "Incident review");

    stack.shutdown();
}
