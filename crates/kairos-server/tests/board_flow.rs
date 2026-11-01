//! The flow metrics of a board (KAIROS-T-0372), from transitions at
//! known times.
//!
//! 4 tasks on a delivery board (Backlog, Todo, Blocked, Active, Completed).
//! The test makes the moves over REST, then sets the time of each history
//! row:
//!
//! - A: Active Mon 2026-09-28 09:00, Completed Wed 2026-09-30 09:00 (2 days);
//! - B: Active Tue 2026-09-29 10:00, still in Active (in progress);
//! - C: cancelled Thu 2026-10-01 10:00 (counted apart);
//! - D: Active Mon 2026-10-05 10:00, Completed Tue 2026-10-06 10:00 (1 day).
//!
//! REST `GET /api/boards/{id}/flow`, MCP `board_flow`, and the refusals.
//!
//! KAIROS-T-0571: then "Archive completed" puts A, C and D away on
//! 2026-10-08, and B is archived on 2026-10-09. The done count, the cancel
//! and the cycle times stay; B is not in progress; the cumulative flow
//! drops each item on the day of its archive.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_board_flow_t0372_test`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::KairosClient;
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_org::{BoardFlowQuery, CreateTeamRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_board_flow_t0372_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

/// One JSON-RPC message of an MCP response: plain JSON or SSE `data:`.
fn rpc_message(body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return value;
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .find(|value| value.get("jsonrpc").is_some())
        .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"))
}

/// A minimal MCP session over HTTP.
struct Mcp {
    http: reqwest::Client,
    url: String,
    token: String,
    session: Option<String>,
    next_id: i64,
}

impl Mcp {
    async fn post(&mut self, body: Value) -> (reqwest::StatusCode, String) {
        let mut request = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("x-tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        let response = request.send().await.expect("MCP request");
        let status = response.status();
        if let Some(session) = response.headers().get("mcp-session-id") {
            self.session = Some(session.to_str().expect("session id").to_string());
        }
        (status, response.text().await.expect("MCP body"))
    }

    async fn connect(base_url: &str, token: &str) -> Mcp {
        let mut mcp = Mcp {
            http: reqwest::Client::new(),
            url: format!("{base_url}/mcp"),
            token: token.to_string(),
            session: None,
            next_id: 0,
        };
        let (status, body) = mcp
            .post(json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "kairos-t0372-flow", "version": "0.0.0"},
                },
            }))
            .await;
        assert!(status.is_success(), "initialize: {body}");
        let (status, body) = mcp
            .post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        assert!(status.is_success(), "initialized: {body}");
        mcp
    }

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        self.next_id += 1;
        let (status, body) = self
            .post(json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .await;
        assert!(status.is_success(), "{tool}: {body}");
        let result = rpc_message(&body)["result"].clone();
        assert!(
            !result["isError"].as_bool().unwrap_or(false),
            "{tool}: {result}"
        );
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} gave no text: {result}"))
            .to_string()
    }
}

/// Set the time of the history row of a move of `task`.
fn set_time(conn: &mut PgConnection, task: Uuid, action: &str, details_prefix: &str, at: &str) {
    let changed = diesel::sql_query(
        "UPDATE org_acme.activity_log SET occurred_at = $1::timestamptz \
         WHERE entity_id = $2 AND action = $3 AND details LIKE $4",
    )
    .bind::<diesel::sql_types::Text, _>(at)
    .bind::<diesel::sql_types::Uuid, _>(task)
    .bind::<diesel::sql_types::Text, _>(action)
    .bind::<diesel::sql_types::Text, _>(format!("{details_prefix}%"))
    .execute(conn)
    .expect("set the time of a row");
    assert_eq!(changed, 1, "{action} {details_prefix} of {task}");
}

#[tokio::test]
async fn the_flow_of_a_board_from_transitions_at_known_times() {
    let _admin_conn = recreate_scratch_db(SCRATCH_DB);
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
    let svc_token = user_token(&http, "svc").await;
    let bob_token = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let server = spawn_server(app::router(app::state_with(
        base_config(&scratch_url),
        pool,
        auth,
    )))
    .await;
    let admin = server.client(&svc_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let _ = admin.whoami().await;
    let _ = bob.whoami().await;
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("bob@kairos.test", OrgRole::Member),
    ] {
        let user_id = user_id(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    let board = team.delivery_board_id.clone().expect("delivery board");
    let detail = admin.get_board(&board).await.expect("board");
    let column: HashMap<String, String> = detail
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let slug = detail.board.slug.clone();

    // The tasks and their moves.
    let mut ids = HashMap::new();
    for name in ["A", "B", "C", "D"] {
        let task = admin
            .create_task(&CreateTaskRequest {
                board_id: Some(board.clone()),
                column_id: None,
                title: format!("Task {name}"),
                content: String::new(),
                task_type: None,
                work_class: None,
                team_id: None,
                repository: None,
                from_team: None,
                parent: None,
            })
            .await
            .expect("task");
        let id: Uuid = task.id.parse().expect("uuid");
        diesel::sql_query(
            "UPDATE org_acme.tasks SET created_at = '2026-09-28T08:00:00Z' WHERE id = $1",
        )
        .bind::<diesel::sql_types::Uuid, _>(id)
        .execute(&mut conn)
        .expect("created_at");
        ids.insert(name, (id, task.short_code));
    }
    let code = |name: &str| ids[name].1.clone();
    let id = |name: &str| ids[name].0;
    for (name, path) in [
        ("A", &["Todo", "Active", "Completed"][..]),
        ("B", &["Todo", "Active"][..]),
        ("D", &["Todo", "Active", "Completed"][..]),
    ] {
        for to in path {
            admin
                .transition_task(&code(name), &column[*to])
                .await
                .expect("transition");
        }
    }
    admin
        .cancel_task(&code("C"), "Not needed")
        .await
        .expect("cancel");
    for (name, details, at) in [
        ("A", "column:Backlog->Todo", "2026-09-28T08:30:00Z"),
        ("A", "column:Todo->Active", "2026-09-28T09:00:00Z"),
        ("A", "column:Active->Completed", "2026-09-30T09:00:00Z"),
        ("B", "column:Backlog->Todo", "2026-09-29T09:00:00Z"),
        ("B", "column:Todo->Active", "2026-09-29T10:00:00Z"),
        ("D", "column:Backlog->Todo", "2026-10-05T09:00:00Z"),
        ("D", "column:Todo->Active", "2026-10-05T10:00:00Z"),
        ("D", "column:Active->Completed", "2026-10-06T10:00:00Z"),
    ] {
        set_time(&mut conn, id(name), "transition", details, at);
    }
    set_time(
        &mut conn,
        id("C"),
        "cancel",
        "column:Backlog->Completed",
        "2026-10-01T10:00:00Z",
    );
    diesel::sql_query(
        "UPDATE org_acme.task_cancellations SET cancelled_at = '2026-10-01T10:00:00Z' \
         WHERE task_id = $1",
    )
    .bind::<diesel::sql_types::Uuid, _>(id("C"))
    .execute(&mut conn)
    .expect("cancelled_at");

    // REST, by a member with no capability on the board.
    let flow = bob
        .board_flow(
            &slug,
            &BoardFlowQuery {
                from: Some("2026-09-28".into()),
                to: Some("2026-10-11".into()),
            },
        )
        .await
        .expect("the flow");
    assert_eq!(flow.start_columns, vec!["Active"]);
    assert_eq!(flow.done_columns, vec!["Completed"]);
    assert_eq!(flow.done_count, 2);
    assert_eq!(flow.cancelled_count, 1);
    assert_eq!(flow.done_without_start, 0);
    assert_eq!(flow.cycle_time.count, 2);
    assert_eq!(flow.cycle_time.mean_days, Some(1.5));
    assert_eq!(flow.cycle_time.median_days, Some(1.0));
    assert_eq!(flow.cycle_time.p85_days, Some(2.0));
    let cycle: Vec<(String, f64)> = flow
        .cycle_time
        .items
        .iter()
        .map(|c| (c.short_code.clone(), c.days))
        .collect();
    assert_eq!(cycle, vec![(code("A"), 2.0), (code("D"), 1.0)]);
    let weeks: Vec<(String, i64, i64)> = flow
        .throughput
        .iter()
        .map(|w| (w.week_start.clone(), w.done, w.cancelled))
        .collect();
    assert_eq!(
        weeks,
        vec![
            ("2026-09-28".to_string(), 1, 1),
            ("2026-10-05".to_string(), 1, 0),
        ]
    );
    assert_eq!(flow.ageing.len(), 1);
    assert_eq!(flow.ageing[0].short_code, code("B"));
    assert_eq!(flow.ageing[0].column, "Active");
    assert_eq!(flow.ageing[0].started_at, "2026-09-29T10:00:00Z");
    let cfd = &flow.cumulative_flow;
    assert_eq!(
        cfd.columns,
        vec!["Backlog", "Todo", "Blocked", "Active", "Completed"]
    );
    assert_eq!(cfd.days.len(), 14);
    let day = |date: &str| {
        cfd.days
            .iter()
            .find(|d| d.date == date)
            .map(|d| d.counts.clone())
            .unwrap()
    };
    assert_eq!(day("2026-09-28"), vec![3, 0, 0, 1, 0]);
    assert_eq!(day("2026-09-30"), vec![2, 0, 0, 1, 1]);
    assert_eq!(day("2026-10-01"), vec![1, 0, 0, 1, 2]);
    assert_eq!(day("2026-10-06"), vec![0, 0, 0, 1, 3]);

    // A range of the first week only.
    let first = bob
        .board_flow(
            &slug,
            &BoardFlowQuery {
                from: Some("2026-09-28".into()),
                to: Some("2026-10-04".into()),
            },
        )
        .await
        .expect("the first week");
    assert_eq!((first.done_count, first.cancelled_count), (1, 1));
    assert_eq!(first.throughput.len(), 1);

    // The refusals.
    for query in [
        "from=2026-10-12&to=2026-10-11",
        "from=2025-01-01&to=2026-10-11",
        "from=10%2F01%2F2026",
    ] {
        let (status, body) = bob
            .raw_request(
                Method::GET,
                &format!("/api/boards/{slug}/flow?{query}"),
                None,
            )
            .await
            .unwrap();
        assert_eq!(status, 422, "{query}: {body}");
    }
    let (status, body) = bob
        .raw_request(
            Method::GET,
            &format!("/api/boards/{slug}/flow?since=1"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(status, 400, "an unknown parameter: {body}");
    let (status, _) = bob
        .raw_request(Method::GET, "/api/boards/no-such-board/flow", None)
        .await
        .unwrap();
    assert_eq!(status, 404);

    // MCP.
    let mut mcp = Mcp::connect(&server.base_url, &bob_token).await;
    let text = mcp
        .call_ok(
            "board_flow",
            json!({"board": slug, "from": "2026-09-28", "to": "2026-10-11"}),
        )
        .await;
    for needed in [
        "Start columns: Active. Done columns: Completed.",
        "Done: 2. Cancelled: 1 (counted apart).",
        "count 2, mean 1.5, median 1, 85th percentile 2",
        "- week of 2026-09-28: done 1, cancelled 1",
        "date | Backlog | Todo | Blocked | Active | Completed",
        "2026-10-06 | 0 | 0 | 0 | 1 | 3",
    ] {
        assert!(text.contains(needed), "{needed:?} not in {text}");
    }
    assert!(text.contains(&code("B")), "{text}");

    // KAIROS-T-0571: archived work keeps its numbers.
    let archived = admin
        .archive_completed(&slug, None)
        .await
        .expect("archive completed");
    assert_eq!(archived.count, 3, "{archived:?}");
    admin.delete_task(&code("B")).await.expect("archive B");
    for (name, at) in [
        ("A", "2026-10-08T12:00:00Z"),
        ("C", "2026-10-08T12:00:00Z"),
        ("D", "2026-10-08T12:00:00Z"),
        ("B", "2026-10-09T12:00:00Z"),
    ] {
        diesel::sql_query("UPDATE org_acme.tasks SET deleted_at = $1::timestamptz WHERE id = $2")
            .bind::<diesel::sql_types::Text, _>(at)
            .bind::<diesel::sql_types::Uuid, _>(id(name))
            .execute(&mut conn)
            .expect("deleted_at");
    }
    let after = bob
        .board_flow(
            &slug,
            &BoardFlowQuery {
                from: Some("2026-09-28".into()),
                to: Some("2026-10-11".into()),
            },
        )
        .await
        .expect("the flow after the archive");
    assert_eq!((after.done_count, after.cancelled_count), (2, 1));
    assert_eq!(after.cycle_time, flow.cycle_time);
    assert_eq!(after.throughput, flow.throughput);
    assert!(after.ageing.is_empty(), "{:?}", after.ageing);
    let after_day = |date: &str| {
        after
            .cumulative_flow
            .days
            .iter()
            .find(|d| d.date == date)
            .map(|d| d.counts.clone())
            .unwrap()
    };
    assert_eq!(after_day("2026-10-06"), vec![0, 0, 0, 1, 3]);
    assert_eq!(after_day("2026-10-08"), vec![0, 0, 0, 1, 0]);
    assert_eq!(after_day("2026-10-09"), vec![0, 0, 0, 0, 0]);
    // A range that starts after each archive has none of them.
    let later = bob
        .board_flow(
            &slug,
            &BoardFlowQuery {
                from: Some("2026-10-10".into()),
                to: Some("2026-10-11".into()),
            },
        )
        .await
        .expect("a range after the archives");
    assert_eq!((later.done_count, later.cancelled_count), (0, 0));
    assert!(
        later
            .cumulative_flow
            .days
            .iter()
            .all(|d| d.counts.iter().all(|c| *c == 0))
    );

    // KAIROS-T-0627: the read is bounded by the range, the numbers stay.
    bounded_read_and_cache(&admin, &bob, &mut conn).await;
}

/// KAIROS-T-0627 on a second board, with history long before the range:
///
/// - E: Active 2026-08-02, Blocked 08-03, Active 09-01, Completed 10-07;
/// - F: Active 2026-08-06, Completed 08-10 (all before the range);
/// - G: Active 2026-08-06, still in Active;
/// - H: Active 2026-08-04, Completed 08-08, archived 08-20 (KAIROS-T-0571).
///
/// The bounded read gives the numbers of the read of every move, for
/// ranges that start before, inside and after the history. A transition
/// changes the next read: the cache does not keep the old result.
async fn bounded_read_and_cache(admin: &KairosClient, bob: &KairosClient, conn: &mut PgConnection) {
    use kairos_db::flow;

    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Ops".into(),
            slug: "ops".into(),
            code_prefix: "OPS".into(),
            team_type: None,
        })
        .await
        .expect("team");
    let board = team.delivery_board_id.clone().expect("delivery board");
    let detail = admin.get_board(&board).await.expect("board");
    let column: HashMap<String, String> = detail
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let slug = detail.board.slug.clone();
    let board_id: Uuid = detail.board.id.parse().expect("board uuid");

    let paths: [(&str, &[(&str, &str)]); 4] = [
        (
            "E",
            &[
                ("Todo", "2026-08-01T09:00:00Z"),
                ("Active", "2026-08-02T09:00:00Z"),
                ("Blocked", "2026-08-03T09:00:00Z"),
                ("Active", "2026-09-01T09:00:00Z"),
                ("Completed", "2026-10-07T09:00:00Z"),
            ],
        ),
        (
            "F",
            &[
                ("Todo", "2026-08-05T09:00:00Z"),
                ("Active", "2026-08-06T09:00:00Z"),
                ("Completed", "2026-08-10T09:00:00Z"),
            ],
        ),
        (
            "G",
            &[
                ("Todo", "2026-08-05T09:00:00Z"),
                ("Active", "2026-08-06T09:00:00Z"),
            ],
        ),
        (
            "H",
            &[
                ("Todo", "2026-08-03T09:00:00Z"),
                ("Active", "2026-08-04T09:00:00Z"),
                ("Completed", "2026-08-08T09:00:00Z"),
            ],
        ),
    ];
    let mut codes = HashMap::new();
    for (name, path) in paths {
        let task = admin
            .create_task(&CreateTaskRequest {
                board_id: Some(board.clone()),
                column_id: None,
                title: format!("Task {name}"),
                content: String::new(),
                task_type: None,
                work_class: None,
                team_id: None,
                repository: None,
                from_team: None,
                parent: None,
            })
            .await
            .expect("task");
        let id: Uuid = task.id.parse().expect("uuid");
        diesel::sql_query(
            "UPDATE org_acme.tasks SET created_at = '2026-07-01T08:00:00Z' WHERE id = $1",
        )
        .bind::<diesel::sql_types::Uuid, _>(id)
        .execute(conn)
        .expect("created_at");
        let mut from = "Backlog";
        for (to, at) in path {
            admin
                .transition_task(&task.short_code, &column[*to])
                .await
                .expect("transition");
            set_time(conn, id, "transition", &format!("column:{from}->{to}"), at);
            from = to;
        }
        if name == "H" {
            admin
                .delete_task(&task.short_code)
                .await
                .expect("archive H");
            diesel::sql_query(
                "UPDATE org_acme.tasks SET deleted_at = '2026-08-20T09:00:00Z' WHERE id = $1",
            )
            .bind::<diesel::sql_types::Uuid, _>(id)
            .execute(conn)
            .expect("deleted_at");
        }
        codes.insert(name, task.short_code);
    }

    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(conn)
        .expect("search_path");
    let now: chrono::DateTime<chrono::Utc> = "2026-10-11T12:00:00Z".parse().expect("now");
    let every_move =
        load_input(conn, board_id, flow::start_of(day("1970-01-01"))).expect("every move");
    assert_eq!(
        every_move.items.len(),
        4,
        "the archived H is in the full read"
    );
    // KAIROS-T-0571: H counts in a range of its time on the board.
    let august = flow::compute(&every_move, day("2026-08-03"), day("2026-08-09"), now);
    assert!(
        august
            .cycle_times
            .iter()
            .any(|c| c.short_code == codes["H"]),
        "{:?}",
        august.cycle_times
    );
    for (from, to) in [
        ("2026-07-01", "2026-10-11"),
        ("2026-08-03", "2026-08-09"),
        ("2026-09-01", "2026-09-30"),
        ("2026-10-05", "2026-10-11"),
    ] {
        let (from, to) = (day(from), day(to));
        let bounded = load_input(conn, board_id, flow::start_of(from)).expect("bounded input");
        assert!(bounded.moves.len() <= every_move.moves.len());
        // An item archived before the range is not read.
        let has_h = bounded.items.iter().any(|i| i.short_code == codes["H"]);
        assert_eq!(has_h, from <= day("2026-08-20"), "the range {from}..{to}");
        assert_eq!(
            flow::compute(&bounded, from, to, now),
            flow::compute(&every_move, from, to, now),
            "the range {from}..{to}"
        );
    }
    let bounded =
        load_input(conn, board_id, flow::start_of(day("2026-10-05"))).expect("bounded input");
    assert!(
        bounded.moves.len() < every_move.moves.len(),
        "the read of the last week has fewer moves: {} of {}",
        bounded.moves.len(),
        every_move.moves.len()
    );
    diesel::sql_query("SET search_path TO public")
        .execute(conn)
        .expect("search_path");

    // REST: E started on 2026-08-02, before the range.
    let query = BoardFlowQuery {
        from: Some("2026-10-05".into()),
        to: Some("2026-10-11".into()),
    };
    let flow = bob.board_flow(&slug, &query).await.expect("the flow");
    let cycle: Vec<(String, f64)> = flow
        .cycle_time
        .items
        .iter()
        .map(|c| (c.short_code.clone(), c.days))
        .collect();
    assert_eq!(cycle, vec![(codes["E"].clone(), 66.0)]);
    let ageing: Vec<(String, String)> = flow
        .ageing
        .iter()
        .map(|a| (a.short_code.clone(), a.started_at.clone()))
        .collect();
    assert_eq!(
        ageing,
        vec![(codes["G"].clone(), "2026-08-06T09:00:00Z".to_string())]
    );
    assert_eq!(
        flow.cumulative_flow.columns,
        vec!["Backlog", "Todo", "Blocked", "Active", "Completed"]
    );
    assert_eq!(flow.cumulative_flow.days[0].counts, vec![0, 0, 0, 2, 1]);
    // A second read is the same (from the cache).
    let again = bob.board_flow(&slug, &query).await.expect("again");
    assert_eq!(again.cycle_time, flow.cycle_time);
    assert_eq!(again.cumulative_flow, flow.cumulative_flow);
    assert_eq!(again.ageing.len(), 1);

    // A transition: the next read has G done, not in progress.
    admin
        .transition_task(&codes["G"], &column["Completed"])
        .await
        .expect("G done");
    let after = bob.board_flow(&slug, &query).await.expect("after");
    assert!(after.ageing.is_empty(), "{:?}", after.ageing);
}

/// The input of the flow of the board `board_id` from `since`: the board,
/// then its moves, as the route reads them.
fn load_input(
    conn: &mut PgConnection,
    board_id: Uuid,
    since: chrono::DateTime<chrono::Utc>,
) -> Result<kairos_db::flow::FlowInput, diesel::result::Error> {
    let board = kairos_db::flow::load_board(conn, board_id, since)?;
    let moves = kairos_db::flow::load_moves(conn, board_id, &board, since)?;
    Ok(kairos_db::flow::FlowInput::new(board, moves))
}

fn day(text: &str) -> chrono::NaiveDate {
    text.parse().expect("a date")
}
