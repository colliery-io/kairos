//! Integration test for KAIROS-T-0686 (KAIROS-A-0065): the REST
//! `Idempotency-Key` header makes a retried write safe, on the
//! `request_keys` table of the MCP `request_key`.
//!
//! Proves:
//! 1. A second `POST /api/tasks` with the same key and body writes nothing
//!    and gives the first status and body, with `Idempotent-Replayed: true`.
//! 2. The same key with another body, or on another route, is 409
//!    REQUEST_KEY_REUSED and writes nothing.
//! 3. A key that does not obey the rule, or two keys, is 422 VALIDATION on
//!    the header.
//! 4. A note and a transition replay: one note; no INVALID_TRANSITION.
//! 5. A refused write leaves no row, so its retry runs.
//! 6. A request with no key is unchanged: two creates make two tasks.
//! 7. A key of an MCP `request_key` is the same row: the REST route with
//!    it is REQUEST_KEY_REUSED (the tool differs).
//! 8. The OpenAPI document names the header on each keyed route.
//!
//! Runs against the compose stack (`angreal services up`). Owns the scratch
//! database `kairos_idempotency_key_t0686_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text as SqlText, Uuid as SqlUuid};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_idempotency_key_t0686_test";

/// The keyed REST routes (the writes of decision 7 of KAIROS-A-0065 that
/// have a REST route).
const KEYED_ROUTES: [(&str, &str); 19] = [
    ("post", "/api/tasks"),
    ("post", "/api/documents"),
    ("post", "/api/adrs"),
    ("post", "/api/initiatives"),
    ("post", "/api/strategies"),
    ("post", "/api/{entity_type}/{short_code}/notes"),
    (
        "post",
        "/api/{entity_type}/{short_code}/notes/{note_id}/promote",
    ),
    ("post", "/api/relationships"),
    ("post", "/api/tasks/{short_code}/transition"),
    ("post", "/api/adrs/{short_code}/transition"),
    ("post", "/api/initiatives/{short_code}/transition"),
    ("post", "/api/strategies/{short_code}/transition"),
    ("post", "/api/tasks/{short_code}/move"),
    ("post", "/api/repositories"),
    ("post", "/api/teams"),
    ("post", "/api/boards"),
    ("post", "/api/boards/{id}/columns"),
    ("post", "/api/teams/{id}/pages"),
    ("post", "/api/boards/{id}/dispatch-rules"),
];

#[derive(Clone)]
struct Rest {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Rest {
    /// POST a JSON body, with the key when there is one: the status, the
    /// replay header and the body.
    async fn post(&self, path: &str, key: Option<&str>, body: Value) -> (u16, bool, Value) {
        let mut request = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header("X-Tenant", "acme")
            .json(&body);
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        Self::read(request.send().await.expect("post")).await
    }

    async fn read(response: reqwest::Response) -> (u16, bool, Value) {
        let status = response.status().as_u16();
        let replayed = response
            .headers()
            .get("Idempotent-Replayed")
            .is_some_and(|v| v == "true");
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        (status, replayed, body)
    }
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count_by(conn: &mut PgConnection, sql: &str, value: &str) -> i64 {
    sql_query(sql)
        .bind::<SqlText, _>(value)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

fn tasks_titled(conn: &mut PgConnection, title: &str) -> i64 {
    count_by(
        conn,
        "SELECT COUNT(*) AS n FROM org_acme.tasks WHERE title = $1",
        title,
    )
}

fn key_rows(conn: &mut PgConnection, key: &str) -> i64 {
    count_by(
        conn,
        "SELECT COUNT(*) AS n FROM org_acme.request_keys WHERE request_key = $1",
        key,
    )
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

fn task(title: &str) -> Value {
    json!({"title": title, "board_id": "platform-delivery"})
}

#[tokio::test]
async fn an_idempotency_key_makes_a_retried_rest_write_safe() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let acme: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("org row");

    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let server = spawn_server(router).await;
    let svc = server.client(&svc_token, "acme");
    let _ = svc.whoami().await;
    let svc_id: Uuid = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("svc provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: acme,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");

    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let team = common::seed_team(&mut conn, "Platform", "platform");
    let board = kairos_db::boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        kairos_db::CodePrefix::Given("ACME"),
        Some(team),
        None,
    )
    .expect("creating the delivery board");
    let todo = sql_query("SELECT id FROM board_columns WHERE board_id = $1 AND name = 'Todo'")
        .bind::<SqlUuid, _>(board.id)
        .get_result::<IdRow>(&mut conn)
        .expect("the Todo column")
        .id;
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");

    let rest = Rest {
        http: http.clone(),
        base: server.base_url.clone(),
        token: svc_token.clone(),
    };

    // 1. A replay of a create.
    let (s1, r1, first) = rest
        .post("/api/tasks", Some("rest-create-01"), task("Rest one"))
        .await;
    assert_eq!((s1, r1), (201, false), "{first}");
    let (s2, r2, again) = rest
        .post("/api/tasks", Some("rest-create-01"), task("Rest one"))
        .await;
    assert_eq!((s2, r2), (201, true), "{again}");
    assert_eq!(again, first, "the replay gives the first body");
    assert_eq!(tasks_titled(&mut conn, "Rest one"), 1);
    let code = first["short_code"]
        .as_str()
        .expect("short_code")
        .to_string();

    // 2. The same key with another body, and on another route.
    let (s, _, refused) = rest
        .post("/api/tasks", Some("rest-create-01"), task("Rest other"))
        .await;
    assert_eq!(s, 409, "{refused}");
    assert_eq!(refused["error"]["code"], "REQUEST_KEY_REUSED", "{refused}");
    assert_eq!(refused["error"]["details"]["first_tool"], "POST /api/tasks");
    assert_eq!(tasks_titled(&mut conn, "Rest other"), 0);
    let (s, _, refused) = rest
        .post(
            &format!("/api/tasks/{code}/notes"),
            Some("rest-create-01"),
            json!({"text": "x"}),
        )
        .await;
    assert_eq!(s, 409, "{refused}");
    assert_eq!(refused["error"]["code"], "REQUEST_KEY_REUSED", "{refused}");

    // 3. A key that does not obey the rule, and two keys.
    for bad in ["short", "has a space in it", &"k".repeat(129)] {
        let (s, _, refused) = rest.post("/api/tasks", Some(bad), task("Bad key")).await;
        assert_eq!(s, 422, "{bad}: {refused}");
        assert_eq!(refused["error"]["code"], "VALIDATION");
        assert_eq!(refused["error"]["details"]["header"], "Idempotency-Key");
    }
    let (s, _, refused) = Rest::read(
        http.post(format!("{}/api/tasks", server.base_url))
            .bearer_auth(&svc_token)
            .header("X-Tenant", "acme")
            .header("Idempotency-Key", "rest-two-key-1")
            .header("Idempotency-Key", "rest-two-key-2")
            .json(&task("Bad key"))
            .send()
            .await
            .expect("post"),
    )
    .await;
    assert_eq!(s, 422, "{refused}");
    assert_eq!(tasks_titled(&mut conn, "Bad key"), 0);

    // 4. A note and a transition.
    let note_path = format!("/api/tasks/{code}/notes");
    let (s, r, n1) = rest
        .post(
            &note_path,
            Some("rest-note-001"),
            json!({"text": "One note."}),
        )
        .await;
    assert_eq!((s, r), (200, false), "{n1}");
    let (s, r, n2) = rest
        .post(
            &note_path,
            Some("rest-note-001"),
            json!({"text": "One note."}),
        )
        .await;
    assert_eq!((s, r), (200, true), "{n2}");
    assert_eq!(n1, n2);
    assert_eq!(
        count_by(
            &mut conn,
            "SELECT COUNT(*) AS n FROM org_acme.item_notes n \
             JOIN org_acme.tasks t ON t.id = n.item_id WHERE t.short_code = $1",
            &code,
        ),
        1,
        "one note"
    );
    let transition_path = format!("/api/tasks/{code}/transition");
    let to_todo = json!({"to_column_id": todo.to_string()});
    let (s, r, t1) = rest
        .post(&transition_path, Some("rest-move-001"), to_todo.clone())
        .await;
    assert_eq!((s, r), (200, false), "{t1}");
    let (s, r, t2) = rest
        .post(&transition_path, Some("rest-move-001"), to_todo.clone())
        .await;
    assert_eq!((s, r), (200, true), "{t2}");
    assert_eq!(t1, t2);
    // With no key, the second move is refused: the reason for the key.
    let (s, _, refused) = rest.post(&transition_path, None, to_todo).await;
    assert_eq!(s, 422, "a move to the same column is refused: {refused}");

    // 5. A refused write leaves no row, and its retry runs.
    let (s, _, refused) = rest
        .post(
            "/api/tasks",
            Some("rest-refused-1"),
            json!({"title": "Refused first", "board_id": "no-such-board"}),
        )
        .await;
    assert!(s >= 400, "{refused}");
    assert_eq!(key_rows(&mut conn, "rest-refused-1"), 0);
    let (s, r, _) = rest
        .post("/api/tasks", Some("rest-refused-1"), task("Refused first"))
        .await;
    assert_eq!((s, r), (201, false));
    assert_eq!(tasks_titled(&mut conn, "Refused first"), 1);

    // 6. No key: unchanged.
    for _ in 0..2 {
        let (s, r, _) = rest.post("/api/tasks", None, task("No key")).await;
        assert_eq!((s, r), (201, false));
    }
    assert_eq!(tasks_titled(&mut conn, "No key"), 2);

    // 7. The key of an MCP request_key is the same row.
    let mcp_call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "create_item", "arguments": {
            "item_type": "task", "title": "By MCP", "board": "platform-delivery",
            "request_key": "shared-key-001",
        }},
    });
    let response = http
        .post(format!("{}/mcp", server.base_url))
        .bearer_auth(&svc_token)
        .header("X-Tenant", "acme")
        .header("accept", "application/json, text/event-stream")
        .json(&mcp_call)
        .send()
        .await
        .expect("post to /mcp");
    assert_eq!(response.status(), 200);
    let _ = response.text().await;
    assert_eq!(tasks_titled(&mut conn, "By MCP"), 1);
    let (s, _, refused) = rest
        .post("/api/tasks", Some("shared-key-001"), task("By MCP"))
        .await;
    assert_eq!(s, 409, "{refused}");
    assert_eq!(refused["error"]["details"]["first_tool"], "create_item");
    assert_eq!(tasks_titled(&mut conn, "By MCP"), 1);

    // 8. The OpenAPI document names the header on each keyed route.
    let spec = serde_json::to_value(kairos_server::api::openapi::spec()).expect("the spec");
    for (method, path) in KEYED_ROUTES {
        let params = spec["paths"][path][method]["parameters"]
            .as_array()
            .unwrap_or_else(|| panic!("{method} {path}: no parameters"));
        assert!(
            params
                .iter()
                .any(|p| p["name"] == "Idempotency-Key" && p["in"] == "header"),
            "{method} {path}: no Idempotency-Key header"
        );
    }

    drop(conn);
    drop(server);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
