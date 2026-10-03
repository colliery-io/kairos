//! COLLIERY-T-0265 — the MCP tool `my_boards` gets the count of each
//! column from the database, against the booted production router.
//!
//! Before this ticket the tool read each row of each of my boards to count
//! the cards: 5 queries for a board (the columns, and the rows of the four
//! tables), and each card of the board came to the server. The tool now
//! gets the columns and the counts of all my boards from 1 query
//! (`kairos_db::board_items::live_column_counts`).
//!
//! - the text of the result is the same: each live column, in position
//!   order, with the count of its live cards,
//! - an archived card does not count,
//! - the tool makes 1 query that reads the tables of the cards, and that
//!   query counts (`COUNT`) and makes groups (`GROUP BY`),
//! - the number of queries is the same for 1 board and for 2 boards.
//!
//! The test reads the queries of the server with the instrumentation of
//! diesel. The instrumentation is one for the process, so this test has a
//! binary of its own.
//!
//! Runs against the LIVE compose stack (`angreal services up`). The test
//! owns a scratch database.

mod common;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::connection::{Instrumentation, InstrumentationEvent, set_default_instrumentation};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text as SqlText, Uuid as SqlUuid};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, user_token,
    with_database,
};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_my_boards_counts_t0265_test";

/// The tenant resolves from the host, as for each MCP request.
const HOST_HEADER: &str = "acme.kairos.test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

/// The text of each query that a connection of this process started.
static QUERIES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The instrumentation of each connection of this process: it records the
/// text of each query.
fn recorder() -> Option<Box<dyn Instrumentation>> {
    Some(Box::new(|event: InstrumentationEvent<'_>| {
        if let InstrumentationEvent::StartQuery { query, .. } = event {
            QUERIES
                .lock()
                .expect("the list of the queries")
                .push(query.to_string());
        }
    }))
}

/// The queries from `start` that read a table of the cards.
fn card_queries(start: usize) -> Vec<String> {
    QUERIES.lock().expect("the list of the queries")[start..]
        .iter()
        .filter(|sql| {
            let sql = sql.to_lowercase();
            ["strategies", "initiatives", "tasks", "adrs"]
                .iter()
                .any(|table| sql.contains(table))
        })
        .cloned()
        .collect()
}

/// The number of queries from `start` that read the columns of a board.
fn column_queries(start: usize) -> usize {
    QUERIES.lock().expect("the list of the queries")[start..]
        .iter()
        .filter(|sql| sql.to_lowercase().contains("board_columns"))
        .count()
}

fn query_count() -> usize {
    QUERIES.lock().expect("the list of the queries").len()
}

/// One in-process request against `/mcp`: the status, the headers and the
/// body.
async fn raw_request(
    router: &Router,
    token: &str,
    session: Option<&str>,
    body: Value,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("host", HOST_HEADER)
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    let request = builder.body(Body::from(body.to_string())).expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, headers, String::from_utf8_lossy(&bytes).to_string())
}

/// The JSON-RPC message of a response: plain JSON, or a `data:` line.
fn rpc_message(body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return value;
    }
    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data:")
            && let Ok(value) = serde_json::from_str::<Value>(data.trim())
            && value.get("jsonrpc").is_some()
        {
            return value;
        }
    }
    panic!("no JSON-RPC message in response body: {body:?}");
}

/// Start an MCP session. Returns the id of the session.
async fn connect(router: &Router, token: &str) -> String {
    let (status, headers, body) = raw_request(
        router,
        token,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "colliery-t0265-test", "version": "0.0.0"},
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "initialize: {body}");
    let session = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .expect("initialize returns Mcp-Session-Id")
        .to_string();
    let (status, _, body) = raw_request(
        router,
        token,
        Some(&session),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "initialized: {body}");
    session
}

/// The text of the result of `my_boards`.
async fn my_boards(router: &Router, token: &str, session: &str) -> String {
    let (status, _, body) = raw_request(
        router,
        token,
        Some(session),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "my_boards", "arguments": {}},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "my_boards: {body}");
    let result = rpc_message(&body)["result"].clone();
    assert_ne!(result["isError"], true, "{result}");
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text in {result}"))
        .to_string()
}

#[derive(QueryableByName)]
struct ColumnRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlText)]
    name: String,
    #[diesel(sql_type = BigInt)]
    live_tasks: i64,
}

/// The live columns of a board in position order, each with the count of
/// its live tasks. The test counts here with a query of its own.
fn columns(conn: &mut PgConnection, board_slug: &str) -> Vec<ColumnRow> {
    sql_query(
        "SELECT c.id, c.name, \
                (SELECT count(*) FROM tasks t \
                  WHERE t.column_id = c.id AND t.deleted_at IS NULL) AS live_tasks \
           FROM board_columns c JOIN boards b ON b.id = c.board_id \
          WHERE b.slug = $1 AND c.deleted_at IS NULL ORDER BY c.position",
    )
    .bind::<SqlText, _>(board_slug)
    .load(conn)
    .unwrap_or_else(|e| panic!("the columns of {board_slug}: {e}"))
}

/// The line of the columns that `my_boards` prints for a board.
fn columns_line(conn: &mut PgConnection, board_slug: &str) -> String {
    let rendered: Vec<String> = columns(conn, board_slug)
        .iter()
        .map(|column| format!("{} ({})", column.name, column.live_tasks))
        .collect();
    format!("  columns: {}\n", rendered.join(" | "))
}

#[tokio::test]
async fn my_boards_counts_in_the_database_against_live_stack() {
    // Before each connection of the test: each connection records.
    set_default_instrumentation(recorder).expect("setting the instrumentation");

    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
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
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth,
    ));
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
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");

    let send = async |method: Method, uri: &str, body: Option<Value>| -> Value {
        let (status, answer) =
            request(&router, method.clone(), uri, Some(&token), &TENANT, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {answer}");
        answer
    };

    // --- one board of mine, with cards ---------------------------------------
    let payments = send(
        Method::POST,
        "/api/teams",
        Some(json!({"name": "Payments", "slug": "payments", "code_prefix": "PAYMENTS"})),
    )
    .await;
    send(
        Method::POST,
        &format!(
            "/api/teams/{}/members",
            payments["id"].as_str().expect("team id")
        ),
        Some(json!({"user_id": svc_id})),
    )
    .await;
    let mut codes = Vec::new();
    for n in 0..12 {
        let task = send(
            Method::POST,
            "/api/tasks",
            Some(json!({"board_id": "payments-delivery", "title": format!("Task {n}")})),
        )
        .await;
        codes.push(task["short_code"].as_str().expect("short code").to_string());
    }
    // 3 tasks go to the second column and 2 to the last column. The test
    // moves them with SQL: the transitions of the board are not the subject.
    let live = columns(&mut conn, "payments-delivery");
    assert!(live.len() >= 3, "the delivery board has 3 columns or more");
    let last = live.len() - 1;
    for (code, column) in [
        (&codes[0], 1),
        (&codes[1], 1),
        (&codes[2], 1),
        (&codes[3], last),
        (&codes[4], last),
    ] {
        sql_query("UPDATE tasks SET column_id = $1 WHERE short_code = $2")
            .bind::<SqlUuid, _>(live[column].id)
            .bind::<SqlText, _>(code)
            .execute(&mut conn)
            .expect("moving a task");
    }
    // An archived card does not count: one of the first column and one of
    // the second column.
    for code in [&codes[5], &codes[0]] {
        send(Method::DELETE, &format!("/api/tasks/{code}"), None).await;
    }
    let counts: Vec<i64> = columns(&mut conn, "payments-delivery")
        .iter()
        .map(|column| column.live_tasks)
        .collect();
    assert_eq!(counts[0], 6, "{counts:?}");
    assert_eq!(counts[1], 2, "{counts:?}");
    assert_eq!(counts[last], 2, "{counts:?}");
    assert_eq!(counts.iter().sum::<i64>(), 10, "{counts:?}");

    let session = connect(&router, &token).await;

    // --- the text and the queries, with 1 board of mine ----------------------
    let start = query_count();
    let text = my_boards(&router, &token, &session).await;
    let with_one_board = query_count() - start;
    let reads = card_queries(start);
    let payments_line = columns_line(&mut conn, "payments-delivery");
    assert!(
        text.contains(&format!(
            "- payments-delivery — Payments Delivery [mine]\n{payments_line}"
        )),
        "{text}"
    );
    assert_eq!(
        reads.len(),
        1,
        "the tool makes 1 query that reads the tables of the cards: {reads:#?}"
    );
    let sql = reads[0].to_uppercase();
    assert!(
        sql.contains("COUNT(") && sql.contains("GROUP BY"),
        "the database counts the cards: {sql}"
    );

    // --- the text and the queries, with 2 boards of mine ---------------------
    let billing = send(
        Method::POST,
        "/api/teams",
        Some(json!({"name": "Billing", "slug": "billing", "code_prefix": "BILLING"})),
    )
    .await;
    send(
        Method::POST,
        &format!(
            "/api/teams/{}/members",
            billing["id"].as_str().expect("team id")
        ),
        Some(json!({"user_id": svc_id})),
    )
    .await;
    for n in 0..3 {
        send(
            Method::POST,
            "/api/tasks",
            Some(json!({"board_id": "billing-delivery", "title": format!("Invoice {n}")})),
        )
        .await;
    }
    // A delivery board that is not mine has no line of columns.
    send(
        Method::POST,
        "/api/teams",
        Some(json!({"name": "Web", "slug": "web", "code_prefix": "WEB"})),
    )
    .await;

    let start = query_count();
    let text = my_boards(&router, &token, &session).await;
    let with_two_boards = query_count() - start;
    let reads_of_columns = column_queries(start);
    let billing_line = columns_line(&mut conn, "billing-delivery");
    assert!(billing_line.contains("(3)"), "{billing_line}");
    assert!(
        text.contains(&format!(
            "- billing-delivery — Billing Delivery [mine]\n{billing_line}"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "- payments-delivery — Payments Delivery [mine]\n{payments_line}"
        )),
        "{text}"
    );
    assert!(text.contains("- web-delivery — Web Delivery\n"), "{text}");
    assert_eq!(
        text.matches("  columns: ").count(),
        2,
        "only my boards have a line of columns: {text}"
    );
    assert_eq!(
        with_two_boards, with_one_board,
        "the number of queries does not grow with the number of boards"
    );
    assert_eq!(
        reads_of_columns, 1,
        "the tool reads the columns of all my boards with 1 query"
    );

    // --- the function, for the boards of the organization --------------------
    // The initiative board has an initiative, and the function counts it.
    send(
        Method::POST,
        "/api/initiatives",
        Some(json!({"board_id": "initiatives", "title": "Faster checkout"})),
    )
    .await;
    let initiative_board: Uuid = kairos_db::schema::boards::table
        .filter(kairos_db::schema::boards::board_level.eq(BoardLevel::Initiative))
        .select(kairos_db::schema::boards::id)
        .first(&mut conn)
        .expect("the initiative board");
    let rows = kairos_db::board_items::live_column_counts(&mut conn, &[initiative_board])
        .expect("the counts of the initiative board");
    assert!(rows.iter().all(|row| row.board_id == initiative_board));
    assert_eq!(rows.iter().map(|row| row.count).sum::<i64>(), 1, "{rows:?}");
    assert!(
        kairos_db::board_items::live_column_counts(&mut conn, &[])
            .expect("no board")
            .is_empty()
    );

    drop(conn);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
