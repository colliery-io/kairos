//! COLLIERY-T-0249 — a write with a field that the route does not know is
//! refused, over REST against the booted production router.
//!
//! The rule is for each write route of the API. For a set of routes that
//! represents them, this test sends one unknown field in a body that is
//! correct in all other respects, and it expects:
//!
//! - 422 `VALIDATION` in the KAIROS-S-0005 envelope,
//! - a message and a `details.field` that name the field,
//! - no write.
//!
//! Then it sends the same body with no unknown field, and the write
//! passes: the rule refuses the unknown field and nothing more.
//!
//! The last test is about each other body that a route cannot read: it
//! gets the envelope too.
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

    /// One request with a body that is not made from a JSON value.
    async fn send_text(
        &self,
        method: Method,
        uri: &str,
        content_type: Option<&str>,
        body: &str,
    ) -> (StatusCode, String) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {}", self.token))
            .header("x-tenant", "acme");
        if let Some(content_type) = content_type {
            builder = builder.header("content-type", content_type);
        }
        let request = builder.body(Body::from(body.to_string())).expect("request");
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
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    /// The refusal of COLLIERY-T-0249: 422 `VALIDATION`, and the message
    /// and the details name `field`.
    async fn refused(&self, method: Method, uri: &str, body: Value, field: &str) -> Value {
        // The raw answer first: without the rule, axum answers a body
        // fault in plain text, and the JSON helper cannot read that.
        let (status, text) = self
            .send_text(
                method.clone(),
                uri,
                Some("application/json"),
                &body.to_string(),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{method} {uri} with the unknown field {field:?}: {text}"
        );
        let answer: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{method} {uri}: the refusal is not JSON ({e}): {text}"));
        assert_eq!(error_code(&answer), "VALIDATION", "{method} {uri}");
        let message = answer["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.starts_with(&format!(
                "The body has the field {field:?}. This route does not accept that field."
            )),
            "{method} {uri}: the message names the field: {message}"
        );
        assert_eq!(
            answer["error"]["details"]["field"], field,
            "{method} {uri}: {answer}"
        );
        assert!(
            answer["error"]["details"]["allowed"].is_array(),
            "{method} {uri}: {answer}"
        );
        answer
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

/// `body` with one more field.
fn with_field(body: &Value, field: &str, value: Value) -> Value {
    let mut body = body.clone();
    body.as_object_mut()
        .expect("the body is an object")
        .insert(field.to_string(), value);
    body
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// The number of rows of a table of the tenant, live or not.
fn rows(conn: &mut PgConnection, table: &str) -> i64 {
    diesel::sql_query(format!("SELECT count(*) AS count FROM {table}"))
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("counting {table}: {e}"))
        .count
}

/// The number of rows of the activity log. Each write of the API adds one,
/// so a count that does not change shows that nothing was written.
fn activity(conn: &mut PgConnection) -> i64 {
    rows(conn, "activity_log")
}

#[tokio::test]
async fn a_write_with_an_unknown_field_is_refused_against_live_stack() {
    let mut stack = Stack::boot("kairos_unknown_fields_t0249_test").await;

    // =======================================================================
    // The cast: a team with its board, 2 tasks, a repository, a definition
    // =======================================================================
    let team_body = json!({"name": "Platform", "slug": "platform"});

    // --- team create --------------------------------------------------------
    let before = (rows(&mut stack.conn, "teams"), activity(&mut stack.conn));
    stack
        .refused(
            Method::POST,
            "/api/teams",
            with_field(&team_body, "description", json!("The platform team")),
            "description",
        )
        .await;
    assert_eq!(
        (rows(&mut stack.conn, "teams"), activity(&mut stack.conn)),
        before,
        "no team was written"
    );
    let team = stack.ok(Method::POST, "/api/teams", Some(team_body)).await;
    let team_id = team["id"].as_str().expect("team id").to_string();
    let board_id = team["delivery_board_id"]
        .as_str()
        .expect("delivery board")
        .to_string();

    // --- board create -------------------------------------------------------
    let board_body = json!({
        "name": "Second Strategy",
        "slug": "second-strategy",
        "board_level": "strategy",
    });
    let before = (rows(&mut stack.conn, "boards"), activity(&mut stack.conn));
    stack
        .refused(
            Method::POST,
            "/api/boards",
            with_field(&board_body, "id", json!(Uuid::new_v4().to_string())),
            "id",
        )
        .await;
    assert_eq!(
        (rows(&mut stack.conn, "boards"), activity(&mut stack.conn)),
        before,
        "no board was written"
    );
    let second = stack
        .ok(Method::POST, "/api/boards", Some(board_body))
        .await;
    let second_id = second["id"].as_str().expect("board id").to_string();

    // --- board update: the 3 fields of the finding --------------------------
    let board_uri = format!("/api/boards/{second_id}");
    let update = json!({"name": "Renamed"});
    let before = activity(&mut stack.conn);
    let answer = stack
        .refused(
            Method::PATCH,
            &board_uri,
            with_field(&update, "board_level", json!("delivery")),
            "board_level",
        )
        .await;
    assert_eq!(
        answer["error"]["message"],
        "The body has the field \"board_level\". This route does not accept that field. The \
         fields of the body are: name, slug, team_id."
    );
    assert_eq!(
        answer["error"]["details"],
        json!({"field": "board_level", "allowed": ["name", "slug", "team_id"]})
    );
    stack
        .refused(
            Method::PATCH,
            &board_uri,
            with_field(&update, "id", json!(Uuid::new_v4().to_string())),
            "id",
        )
        .await;
    stack
        .refused(
            Method::PATCH,
            &board_uri,
            with_field(&update, "deleted_at", json!("2026-01-01T00:00:00Z")),
            "deleted_at",
        )
        .await;
    let board = stack.ok(Method::GET, &board_uri, None).await;
    assert_eq!(board["name"], "Second Strategy", "no rename: {board}");
    assert_eq!(board["board_level"], "strategy", "{board}");
    assert_eq!(activity(&mut stack.conn), before, "no board was written");
    // A board that a client read and sends back is refused too: the body
    // of an update has the fields of the update only. The refusal names
    // the first unknown field of the body.
    stack
        .refused(Method::PATCH, &board_uri, board.clone(), "board_level")
        .await;
    let board = stack.ok(Method::PATCH, &board_uri, Some(update)).await;
    assert_eq!(board["name"], "Renamed", "{board}");

    // --- task create --------------------------------------------------------
    let task_body = json!({"board_id": board_id, "title": "First task"});
    let before = (rows(&mut stack.conn, "tasks"), activity(&mut stack.conn));
    stack
        .refused(
            Method::POST,
            "/api/tasks",
            with_field(&task_body, "priority", json!("high")),
            "priority",
        )
        .await;
    assert_eq!(
        (rows(&mut stack.conn, "tasks"), activity(&mut stack.conn)),
        before,
        "no task was written"
    );
    let first = stack.ok(Method::POST, "/api/tasks", Some(task_body)).await;
    let first_code = first["short_code"].as_str().expect("code").to_string();
    let other = stack
        .ok(
            Method::POST,
            "/api/tasks",
            Some(json!({"board_id": board_id, "title": "Second task"})),
        )
        .await;
    let other_code = other["short_code"].as_str().expect("code").to_string();
    // The alias of `repository` stays a field of the route.
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/tasks",
            Some(json!({"board_id": board_id, "title": "t", "repository_id": "no-such"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(
        answer["error"]["details"]["field"] != "repository_id",
        "the refusal is about the repository, and not about the field: {answer}"
    );

    // --- task update (the edit of the content) ------------------------------
    let task_uri = format!("/api/tasks/{first_code}");
    let edit = json!({"content": "New content", "version": first["version"]});
    let before = activity(&mut stack.conn);
    stack
        .refused(
            Method::PATCH,
            &task_uri,
            with_field(&edit, "status", json!("done")),
            "status",
        )
        .await;
    let task = stack.ok(Method::GET, &task_uri, None).await;
    assert_eq!(task["content"], first["content"], "no edit: {task}");
    assert_eq!(task["version"], first["version"], "no new version: {task}");
    assert_eq!(activity(&mut stack.conn), before, "no task was written");
    let task = stack.ok(Method::PATCH, &task_uri, Some(edit)).await;
    assert_eq!(task["content"], "New content", "{task}");

    // --- task transition ----------------------------------------------------
    let columns = stack
        .ok(Method::GET, &format!("/api/boards/{board_id}"), None)
        .await;
    let to_column = columns["columns"][1]["id"].as_str().expect("column");
    let transition = json!({"to_column_id": to_column});
    stack
        .refused(
            Method::POST,
            &format!("{task_uri}/transition"),
            with_field(&transition, "column", json!("Todo")),
            "column",
        )
        .await;
    let task = stack.ok(Method::GET, &task_uri, None).await;
    assert_eq!(task["column_id"], first["column_id"], "no move: {task}");

    // --- metadata definition create, then item metadata set -----------------
    let definition = json!({"name": "Severity", "slug": "severity", "field_type": "string"});
    let before = rows(&mut stack.conn, "metadata_definitions");
    stack
        .refused(
            Method::POST,
            "/api/metadata-definitions",
            with_field(&definition, "is_system_default", json!(true)),
            "is_system_default",
        )
        .await;
    assert_eq!(rows(&mut stack.conn, "metadata_definitions"), before);
    stack
        .ok(Method::POST, "/api/metadata-definitions", Some(definition))
        .await;

    let metadata_uri = format!("/api/tasks/{first_code}/metadata");
    let values = json!({"values": {"severity": "sev1"}});
    let before = (
        rows(&mut stack.conn, "item_metadata"),
        activity(&mut stack.conn),
    );
    stack
        .refused(
            Method::PATCH,
            &metadata_uri,
            with_field(&values, "replace", json!(true)),
            "replace",
        )
        .await;
    assert_eq!(
        (
            rows(&mut stack.conn, "item_metadata"),
            activity(&mut stack.conn)
        ),
        before,
        "no value was written"
    );
    let set = stack.ok(Method::PATCH, &metadata_uri, Some(values)).await;
    assert_eq!(set["values"][0]["value"], "sev1", "{set}");

    // --- relationship create ------------------------------------------------
    let edge = json!({
        "source_short_code": first_code,
        "target_short_code": other_code,
        "relationship": "blocks",
    });
    let before = (
        rows(&mut stack.conn, "item_relationships"),
        activity(&mut stack.conn),
    );
    stack
        .refused(
            Method::POST,
            "/api/relationships",
            with_field(&edge, "note", json!("why")),
            "note",
        )
        .await;
    assert_eq!(
        (
            rows(&mut stack.conn, "item_relationships"),
            activity(&mut stack.conn)
        ),
        before,
        "no edge was written"
    );
    stack
        .ok(Method::POST, "/api/relationships", Some(edge))
        .await;

    // --- repository create and update ---------------------------------------
    let repository = json!({
        "slug": "payments-api",
        "forge": "github",
        "repo_full_name": "acme/payments-api",
        "repo_url": "https://github.com/acme/payments-api",
        "team": team_id,
    });
    let before = (
        rows(&mut stack.conn, "repositories"),
        activity(&mut stack.conn),
    );
    stack
        .refused(
            Method::POST,
            "/api/repositories",
            with_field(&repository, "team_id", json!(team_id)),
            "team_id",
        )
        .await;
    assert_eq!(
        (
            rows(&mut stack.conn, "repositories"),
            activity(&mut stack.conn)
        ),
        before,
        "no repository was written"
    );
    stack
        .ok(Method::POST, "/api/repositories", Some(repository))
        .await;

    let repository_uri = "/api/repositories/payments-api";
    let change = json!({"description": "Read the README first."});
    let before = activity(&mut stack.conn);
    stack
        .refused(
            Method::PATCH,
            repository_uri,
            with_field(&change, "forge", json!("gitlab")),
            "forge",
        )
        .await;
    let read = stack.ok(Method::GET, repository_uri, None).await;
    assert_eq!(read["description"], "", "no change: {read}");
    assert_eq!(read["forge"], "github", "{read}");
    assert_eq!(activity(&mut stack.conn), before, "nothing was written");
    let changed = stack.ok(Method::PATCH, repository_uri, Some(change)).await;
    assert_eq!(changed["description"], "Read the README first.");

    // --- a field in an object of the body: the entry of a template ----------
    let template = json!({
        "name": "Incident",
        "slug": "incident",
        "metadata": [{"definition_slug": "severity", "required": true}],
    });
    let mut nested = template.clone();
    nested["metadata"][0]["default"] = json!("sev1");
    let before = rows(&mut stack.conn, "templates");
    stack
        .refused(Method::POST, "/api/templates", nested, "default")
        .await;
    assert_eq!(rows(&mut stack.conn, "templates"), before);
    stack
        .ok(Method::POST, "/api/templates", Some(template))
        .await;

    // --- local account create -----------------------------------------------
    let account = json!({
        "email": "newhire@example.test",
        "password": "a-perfectly-fine-password",
    });
    let users_with_email = |conn: &mut PgConnection| -> i64 {
        users::table
            .filter(users::email.eq("newhire@example.test"))
            .count()
            .get_result(conn)
            .expect("counting users")
    };
    stack
        .refused(
            Method::POST,
            "/api/local-accounts",
            with_field(&account, "is_admin", json!(true)),
            "is_admin",
        )
        .await;
    assert_eq!(users_with_email(&mut stack.conn), 0, "no account");
    stack
        .ok(Method::POST, "/api/local-accounts", Some(account))
        .await;
    assert_eq!(users_with_email(&mut stack.conn), 1);

    // --- the routes with the types of the server: tokens and keys -----------
    let before = activity(&mut stack.conn);
    stack
        .refused(
            Method::POST,
            "/api/service-accounts",
            json!({"name": "ci", "role": "admin"}),
            "role",
        )
        .await;
    stack
        .refused(
            Method::POST,
            "/api/scim-tokens",
            json!({"name": "okta", "expires_at": "2030-01-01T00:00:00Z"}),
            "expires_at",
        )
        .await;
    assert_eq!(activity(&mut stack.conn), before, "nothing was written");

    stack.shutdown();
}

/// Each body that a write route cannot read gets the envelope, with the
/// status that axum gave before.
#[tokio::test]
async fn a_body_that_the_route_cannot_read_gets_the_envelope_against_live_stack() {
    let stack = Stack::boot("kairos_unknown_fields_t0249_envelope_test").await;
    let json_type = Some("application/json");

    let envelope = |text: &str| -> Value {
        let answer: Value = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("the refusal is not JSON ({e}): {text}"));
        assert_eq!(error_code(&answer), "VALIDATION", "{answer}");
        assert!(answer["error"]["message"].is_string(), "{answer}");
        assert!(answer["error"]["details"].is_object(), "{answer}");
        answer
    };

    // --- a required field is absent: 422
    let (status, text) = stack
        .send_text(Method::POST, "/api/teams", json_type, r#"{"slug": "web"}"#)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{text}");
    let answer = envelope(&text);
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("missing field `name`"),
        "{answer}"
    );

    // --- a field has the wrong type: 422
    let (status, text) = stack
        .send_text(
            Method::POST,
            "/api/teams",
            json_type,
            r#"{"name": 7, "slug": "web"}"#,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{text}");
    envelope(&text);

    // --- the body is not JSON: 400
    let (status, text) = stack
        .send_text(Method::POST, "/api/teams", json_type, r#"{"name": "Web","#)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    envelope(&text);

    // --- the body has no JSON content type: 415
    let (status, text) = stack
        .send_text(
            Method::POST,
            "/api/teams",
            Some("text/plain"),
            r#"{"name": "Web", "slug": "web"}"#,
        )
        .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{text}");
    envelope(&text);

    // --- the search keeps its status: 400, and the message names the field
    let (status, text) = stack
        .send_text(
            Method::POST,
            "/api/search",
            json_type,
            r#"{"query": "auth"}"#,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    let answer = envelope(&text);
    assert_eq!(answer["error"]["details"]["field"], "query", "{answer}");

    // --- the login is a write route with no bearer
    let (status, text) = stack
        .send_text(
            Method::POST,
            "/api/login",
            json_type,
            r#"{"email": "a@example.test", "password": "p", "remember": true}"#,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{text}");
    let answer = envelope(&text);
    assert_eq!(answer["error"]["details"]["field"], "remember", "{answer}");

    // --- nothing of the above made a team
    let teams = stack.ok(Method::GET, "/api/teams", None).await;
    assert_eq!(teams["total"], 0, "{teams}");

    stack.shutdown();
}
