//! COLLIERY-T-0256 — a request with an input that the route does not know
//! is refused, over REST against the booted production router.
//!
//! COLLIERY-T-0249 is the rule for the fields of a body
//! (`unknown_fields.rs`). This file is the rule for the two other inputs:
//!
//! - a query parameter that the route does not know: 400 `VALIDATION`, and
//!   the message and `details.parameter` name the parameter,
//! - a body on a route that accepts no body: 400 `VALIDATION`.
//!
//! The first two tests are about EACH route of the OpenAPI document. The
//! third test is about a set of routes that represents them, with data: a
//! known parameter passes, a request with no body passes, and a refused
//! write wrote nothing. The last test is about the routes that are not in
//! the rule.
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
use kairos_server::api::openapi::spec;
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The routes of the document that are not in the rule. Each has a reason.
const NOT_IN_THE_RULE: &[(&str, &str, &str)] = &[(
    "POST",
    "/api/auth/token",
    "the relay to the token endpoint of the issuer: OAuth gives its inputs",
)];

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

    /// One request with a body that is not made from a JSON value. The
    /// answer is text: without the rule, axum answers in plain text.
    async fn send_text(
        &self,
        method: Method,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, String) {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {}", self.token))
            .header("x-tenant", "acme");
        let request = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        }
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
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    /// The refusal of a query parameter: 400 `VALIDATION`, and the message
    /// and the details name `parameter`. Returns `details.allowed`.
    async fn parameter_refused(&self, method: Method, uri: &str, parameter: &str) -> Vec<String> {
        let (status, text) = self.send_text(method.clone(), uri, None).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{method} {uri} with the unknown parameter {parameter:?}: {text}"
        );
        let answer: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{method} {uri}: the refusal is not JSON ({e}): {text}"));
        assert_eq!(error_code(&answer), "VALIDATION", "{method} {uri}");
        let message = answer["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.starts_with(&format!(
                "The request has the query parameter {parameter:?}. This route does not \
                 accept that parameter."
            )),
            "{method} {uri}: the message names the parameter: {message}"
        );
        assert_eq!(
            answer["error"]["details"]["parameter"], parameter,
            "{method} {uri}: {answer}"
        );
        answer["error"]["details"]["allowed"]
            .as_array()
            .unwrap_or_else(|| panic!("{method} {uri}: no list of parameters: {answer}"))
            .iter()
            .map(|name| name.as_str().expect("a name").to_string())
            .collect()
    }

    /// The refusal of a body on a route that accepts none.
    async fn body_refused(&self, method: Method, uri: &str, body: &str) {
        let (status, text) = self.send_text(method.clone(), uri, Some(body)).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{method} {uri} with the body {body:?}: {text}"
        );
        let answer: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{method} {uri}: the refusal is not JSON ({e}): {text}"));
        assert_eq!(error_code(&answer), "VALIDATION", "{method} {uri}");
        assert_eq!(
            answer["error"]["message"],
            "The request has a body. This route does not accept a body. Send the request \
             with no body.",
            "{method} {uri}"
        );
    }

    fn shutdown(mut self) {
        drop(self.pool);
        drop(self.conn);
        drop_scratch_db(&mut self.admin_conn, self.scratch_db);
    }
}

/// One route of the document.
struct Route {
    method: Method,
    /// The path with a value in the place of each parameter.
    uri: String,
    /// The query parameters that the document gives, in order of name.
    query: Vec<String>,
    /// The document gives a body.
    body: bool,
}

/// Each route of the document that is in the rule.
fn routes_of_the_document() -> Vec<Route> {
    let spec = serde_json::to_value(spec()).expect("the document serializes");
    let mut routes = Vec::new();
    for (path, item) in spec["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            let upper = method.to_uppercase();
            if NOT_IN_THE_RULE
                .iter()
                .any(|(m, p, _)| *m == upper && p == path)
            {
                continue;
            }
            let Ok(method) = Method::from_bytes(upper.as_bytes()) else {
                continue;
            };
            let mut query: Vec<String> = operation["parameters"]
                .as_array()
                .map(|all| {
                    all.iter()
                        .filter(|p| p["in"] == "query")
                        .map(|p| p["name"].as_str().expect("a name").to_string())
                        .collect()
                })
                .unwrap_or_default();
            query.sort();
            routes.push(Route {
                method,
                uri: with_values(path),
                query,
                body: operation.get("requestBody").is_some(),
            });
        }
    }
    assert!(routes.len() > 100, "the document has the routes of the API");
    routes
}

/// `path` with a value in the place of each `{parameter}`: a family for
/// `entity_type`, and a UUID that no row has for each other.
fn with_values(path: &str) -> String {
    path.split('/')
        .map(|part| match part {
            "{entity_type}" => "tasks".to_string(),
            p if p.starts_with('{') => Uuid::nil().to_string(),
            p => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The number of handlers of this crate that read a query: the uses of
/// the extractor `ApiQuery` in `src/`.
fn handlers_with_a_query() -> usize {
    fn count(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
            .expect("readable src dir")
            .map(|entry| entry.expect("dir entry").path())
            .map(|path| {
                if path.is_dir() {
                    count(&path)
                } else {
                    std::fs::read_to_string(&path)
                        .expect("readable source file")
                        .matches("): ApiQuery<")
                        .count()
                }
            })
            .sum()
    }
    let found = count(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    assert!(found >= 20, "handlers with a query: {found}");
    found
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

/// Each route refuses a query parameter that it does not know. For a route
/// with query parameters, the list of the refusal comes from the type of
/// the query: it is the same as the list of the document.
#[tokio::test]
async fn each_route_refuses_an_unknown_parameter_against_live_stack() {
    let stack = Stack::boot("kairos_unknown_inputs_query_t0256_test").await;

    let (mut with_query, mut without) = (0, 0);
    for route in routes_of_the_document() {
        let uri = format!("{}?zz_unknown=1", route.uri);
        let mut allowed = stack
            .parameter_refused(route.method.clone(), &uri, "zz_unknown")
            .await;
        allowed.sort();
        assert_eq!(
            allowed, route.query,
            "{} {}: the parameters of the route, and those of the document",
            route.method, route.uri
        );
        if route.query.is_empty() {
            without += 1;
        } else {
            with_query += 1;
        }
    }
    // A handler that reads a query and does not give it to the document is
    // a route with no parameters for the middleware. So count the handlers.
    assert_eq!(
        with_query,
        handlers_with_a_query(),
        "each handler with `ApiQuery` gives its parameters to the document"
    );
    assert!(without >= 80, "routes with no query parameters: {without}");

    stack.shutdown();
}

/// Each route with no body refuses a body that is not empty. `{}` and
/// `null` are bodies.
#[tokio::test]
async fn each_route_with_no_body_refuses_a_body_against_live_stack() {
    let stack = Stack::boot("kairos_unknown_inputs_body_t0256_test").await;

    let mut count = 0;
    for route in routes_of_the_document() {
        if route.body {
            continue;
        }
        for body in ["{}", "null", r#"{"confirm": true}"#] {
            stack
                .body_refused(route.method.clone(), &route.uri, body)
                .await;
        }
        count += 1;
    }
    assert!(count >= 60, "routes with no body: {count}");

    stack.shutdown();
}

#[tokio::test]
async fn a_known_input_passes_and_a_refusal_writes_nothing_against_live_stack() {
    let mut stack = Stack::boot("kairos_unknown_inputs_t0256_test").await;

    // =======================================================================
    // The cast: a team with its board, 2 tasks with an edge, a repository
    // =======================================================================
    let team = stack
        .ok(
            Method::POST,
            "/api/teams",
            Some(json!({"name": "Platform", "slug": "platform", "code_prefix": "PLATFORM"})),
        )
        .await;
    let team_id = team["id"].as_str().expect("team id").to_string();
    let board_id = team["delivery_board_id"]
        .as_str()
        .expect("delivery board")
        .to_string();
    let first = stack
        .ok(
            Method::POST,
            "/api/tasks",
            Some(json!({"board_id": board_id, "title": "First task"})),
        )
        .await;
    let first_code = first["short_code"].as_str().expect("code").to_string();
    let second = stack
        .ok(
            Method::POST,
            "/api/tasks",
            Some(json!({"board_id": board_id, "title": "Second task"})),
        )
        .await;
    let second_code = second["short_code"].as_str().expect("code").to_string();
    let edge = stack
        .ok(
            Method::POST,
            "/api/relationships",
            Some(json!({
                "source_short_code": first_code,
                "target_short_code": second_code,
                "relationship": "blocks",
            })),
        )
        .await;
    let edge_id = edge["id"].as_str().expect("edge id").to_string();
    stack
        .ok(
            Method::POST,
            "/api/repositories",
            Some(json!({
                "team": team_id,
                "forge": "github",
                "repo_full_name": "acme/payments",
                "repo_url": "https://github.com/acme/payments",
                "slug": "payments",
            })),
        )
        .await;

    // --- list tasks ---------------------------------------------------------
    // `page` is not a parameter: the server does not give page 1 for it.
    let allowed = stack
        .parameter_refused(Method::GET, "/api/tasks?limit=1&page=2", "page")
        .await;
    assert_eq!(allowed, ["limit", "offset", "include_deleted"]);
    let page = stack
        .ok(
            Method::GET,
            "/api/tasks?limit=1&offset=1&include_deleted=true",
            None,
        )
        .await;
    assert_eq!(page["items"].as_array().map(Vec::len), Some(1), "{page}");
    // A value that the route cannot read has the envelope too.
    let (status, answer) = stack.send(Method::GET, "/api/tasks?limit=many", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(error_code(&answer), "VALIDATION");
    assert_eq!(answer["error"]["details"], json!({"parameter": "limit"}));

    // --- get item -----------------------------------------------------------
    let task_uri = format!("/api/tasks/{first_code}");
    let allowed = stack
        .parameter_refused(
            Method::GET,
            &format!("{task_uri}?include_deleted=true"),
            "include_deleted",
        )
        .await;
    assert!(allowed.is_empty(), "the route has no parameters");
    stack.ok(Method::GET, &task_uri, None).await;
    // A `?` with nothing after it is not a parameter.
    stack.ok(Method::GET, &format!("{task_uri}?"), None).await;

    // --- board items --------------------------------------------------------
    let items_uri = format!("/api/boards/{board_id}/items");
    let allowed = stack
        .parameter_refused(
            Method::GET,
            &format!("{items_uri}?repository=payments&column=Todo"),
            "column",
        )
        .await;
    assert_eq!(
        allowed,
        [
            "repository",
            "include_deleted",
            "limit",
            "offset",
            "team",
            "no_team"
        ]
    );
    stack
        .ok(
            Method::GET,
            &format!("{items_uri}?repository=payments&include_deleted=true"),
            None,
        )
        .await;

    // --- board detail -------------------------------------------------------
    stack
        .parameter_refused(
            Method::GET,
            &format!("/api/boards/{board_id}?include_deleted=true"),
            "include_deleted",
        )
        .await;
    stack
        .ok(
            Method::GET,
            &format!("/api/boards/{board_id}?include_removed_columns=true"),
            None,
        )
        .await;

    // --- search: the inputs are in the body ---------------------------------
    let query = json!({"q": "task"});
    let (status, answer) = stack
        .send(Method::POST, "/api/search?limit=5", Some(query.clone()))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(
        answer["error"]["details"],
        json!({"parameter": "limit", "allowed": []})
    );
    stack.ok(Method::POST, "/api/search", Some(query)).await;

    // --- relationships, graph -----------------------------------------------
    let edges_uri = format!("/api/tasks/{first_code}/relationships");
    stack
        .parameter_refused(Method::GET, &format!("{edges_uri}?type=blocks"), "type")
        .await;
    stack.ok(Method::GET, &edges_uri, None).await;
    let graph_uri = format!("/api/tasks/{first_code}/graph");
    let allowed = stack
        .parameter_refused(
            Method::GET,
            &format!("{graph_uri}?depth=1&direction=both"),
            "direction",
        )
        .await;
    assert_eq!(allowed, ["depth"]);
    stack
        .ok(Method::GET, &format!("{graph_uri}?depth=1"), None)
        .await;

    // --- activity, history --------------------------------------------------
    let allowed = stack
        .parameter_refused(Method::GET, "/api/activity?action=create&actor=me", "actor")
        .await;
    assert_eq!(
        allowed,
        [
            "entity_id",
            "actor_id",
            "action",
            "since",
            // COLLIERY-T-0265: the filter by team.
            "team",
            "limit",
            "offset"
        ]
    );
    stack
        .ok(Method::GET, "/api/activity?action=create&limit=5", None)
        .await;
    stack
        .parameter_refused(
            Method::GET,
            &format!("/api/tasks/{first_code}/history?version=1&diff=true"),
            "diff",
        )
        .await;
    stack
        .ok(
            Method::GET,
            &format!("/api/tasks/{first_code}/history?version=1"),
            None,
        )
        .await;

    // --- the lists of the administration ------------------------------------
    for list in [
        "/api/members",
        "/api/teams",
        "/api/boards",
        "/api/delivery-streams",
        "/api/templates",
    ] {
        let allowed = stack
            .parameter_refused(
                Method::GET,
                &format!("{list}?include_deleted=true"),
                "include_deleted",
            )
            .await;
        assert_eq!(allowed, ["limit", "offset"], "{list}");
        stack
            .ok(Method::GET, &format!("{list}?limit=200&offset=0"), None)
            .await;
    }
    let allowed = stack
        .parameter_refused(
            Method::GET,
            "/api/repositories?team=platform&limit=5",
            "limit",
        )
        .await;
    assert_eq!(allowed, ["team", "forge", "name"]);
    stack
        .ok(Method::GET, "/api/repositories?team=platform", None)
        .await;
    // No parameters: the lists of the keys and of the tokens.
    stack
        .parameter_refused(Method::GET, "/api/service-accounts?limit=5", "limit")
        .await;
    stack.ok(Method::GET, "/api/service-accounts", None).await;

    // --- a write with a parameter: nothing is written -----------------------
    let before = (rows(&mut stack.conn, "tasks"), activity(&mut stack.conn));
    let (status, answer) = stack
        .send(
            Method::POST,
            "/api/tasks?column=Done",
            Some(json!({"board_id": board_id, "title": "Placed by hand"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["error"]["details"]["parameter"], "column");
    assert_eq!(
        (rows(&mut stack.conn, "tasks"), activity(&mut stack.conn)),
        before,
        "no task was written"
    );

    // --- a DELETE -----------------------------------------------------------
    // `force` and `cascade` are not parameters of a delete. The refusal
    // deletes nothing.
    let before = activity(&mut stack.conn);
    stack
        .parameter_refused(
            Method::DELETE,
            &format!("/api/relationships/{edge_id}?force=true"),
            "force",
        )
        .await;
    stack
        .body_refused(
            Method::DELETE,
            &format!("/api/relationships/{edge_id}"),
            r#"{"force": true}"#,
        )
        .await;
    let second_uri = format!("/api/tasks/{second_code}");
    stack
        .parameter_refused(
            Method::DELETE,
            &format!("{second_uri}?cascade=false"),
            "cascade",
        )
        .await;
    stack.body_refused(Method::DELETE, &second_uri, "{}").await;
    let edges = stack.ok(Method::GET, &edges_uri, None).await;
    assert!(
        edges.to_string().contains(&edge_id),
        "the edge is there: {edges}"
    );
    let task = stack.ok(Method::GET, &second_uri, None).await;
    assert!(task["archived_at"].is_null(), "the task is live: {task}");
    assert_eq!(activity(&mut stack.conn), before, "nothing was deleted");
    // With no input the two deletes pass. A body of zero length is no body.
    stack
        .ok(
            Method::DELETE,
            &format!("/api/relationships/{edge_id}"),
            None,
        )
        .await;
    let (status, text) = stack.send_text(Method::DELETE, &second_uri, Some("")).await;
    assert!(
        status.is_success(),
        "a body of zero length: {status} {text}"
    );

    // --- a restore ----------------------------------------------------------
    let restore_uri = format!("{second_uri}/restore");
    let before = activity(&mut stack.conn);
    stack
        .parameter_refused(
            Method::POST,
            &format!("{restore_uri}?cascade=true"),
            "cascade",
        )
        .await;
    for body in ["{}", "null"] {
        stack.body_refused(Method::POST, &restore_uri, body).await;
    }
    let task = stack.ok(Method::GET, &second_uri, None).await;
    assert!(task["archived_at"].is_string(), "still archived: {task}");
    assert_eq!(activity(&mut stack.conn), before, "nothing was restored");
    stack.ok(Method::POST, &restore_uri, None).await;
    let task = stack.ok(Method::GET, &second_uri, None).await;
    assert!(task["archived_at"].is_null(), "restored: {task}");

    stack.shutdown();
}

/// The routes that are not in the rule accept a parameter as before, and
/// `/ws/events` has its one parameter.
#[tokio::test]
async fn the_routes_that_are_not_in_the_rule_against_live_stack() {
    let stack = Stack::boot("kairos_unknown_inputs_outside_t0256_test").await;

    // A probe with a parameter gets its answer.
    for probe in ["/healthz?probe=docker", "/readyz?probe=docker"] {
        let (status, text) = stack.send_text(Method::GET, probe, None).await;
        assert_eq!(status, StatusCode::OK, "{probe}: {text}");
    }
    let (status, _) = stack.send_text(Method::GET, "/metrics?name=up", None).await;
    assert_ne!(status, StatusCode::BAD_REQUEST, "/metrics");

    // A page of the GUI with its own parameters is not a route of the API.
    let (status, text) = stack
        .send_text(Method::GET, "/boards/platform-delivery?lane=support", None)
        .await;
    assert_ne!(status, StatusCode::BAD_REQUEST, "a deep link: {text}");

    // SCIM: RFC 7644 gives the parameters, and a client can send more.
    let (status, text) = stack
        .send_text(
            Method::GET,
            "/scim/v2/Users?startIndex=1&count=10&zz_unknown=1",
            None,
        )
        .await;
    assert_ne!(status, StatusCode::BAD_REQUEST, "SCIM: {text}");

    // `/ws/events`: `access_token` is the parameter of the route. The
    // request is not an upgrade, so the answer is not 101. It is not the
    // refusal of the rule.
    let ws = format!("/ws/events?access_token={}", stack.token);
    let (_, text) = stack.send_text(Method::GET, &ws, None).await;
    assert!(
        !text.contains("VALIDATION"),
        "access_token is known: {text}"
    );
    let allowed = stack
        .parameter_refused(Method::GET, &format!("{ws}&board=platform"), "board")
        .await;
    assert_eq!(allowed, ["access_token"]);

    stack.shutdown();
}
