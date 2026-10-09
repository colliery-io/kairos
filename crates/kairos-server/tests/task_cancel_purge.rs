//! A task can be cancelled with a reason, and deleted for good
//! (KAIROS-T-0362), on the REST and MCP surfaces.
//!
//! Cancel:
//! 1. The reason is required: an empty reason is a 422 that names the
//!    field.
//! 2. The task moves to the done column of its board, also from a column
//!    with no transition to it.
//! 3. The mark and the reason show on REST get, the board items and MCP
//!    `get_item`; the history (activity) has the row `cancel` with the
//!    reason.
//! 4. A cancel ends the claim of the task.
//! 5. A move out of the done column removes the mark; the activity keeps
//!    the reason.
//! 6. A task in a done column cannot be cancelled (422 TASK_DONE).
//! 7. A person with no `transition_items` gets a 403.
//!
//! Purge:
//! 1. A board manager deletes a task for good: REST get is a 404, the
//!    board and the search do not have it, and no row of it stays.
//! 2. A person who does not manage the board gets a 403, and the task
//!    stays.
//! 3. The activity has one row `purge` that names the code and the title.
//! 4. An archived task can be purged too, and MCP `purge_task` needs
//!    confirm=true.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_cancel_purge_t0362_server_test`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::Error;
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_meta::ActivityQuery;
use kairos_client::types_org::{
    AddTeamMemberRequest, BoardItemsQuery, CreateTeamRequest, CreateTransitionRequest,
};
use kairos_client::types_search::SearchRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_task_cancel_purge_t0362_server_test";

fn user(conn: &mut PgConnection, email: &str) -> (Uuid, String) {
    users::table
        .filter(users::email.eq(email))
        .select((users::id, users::display_name))
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
                    "clientInfo": {"name": "kairos-t0362-cancel-purge", "version": "0.0.0"},
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

    /// The text of a tool call, and whether it is an error.
    async fn call(&mut self, tool: &str, arguments: Value) -> (String, bool) {
        self.next_id += 1;
        let (status, body) = self
            .post(json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .await;
        assert!(status.is_success(), "{tool}: {body}");
        let result = rpc_message(&body)["result"].clone();
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} gave no text: {result}"))
            .to_string();
        (text, result["isError"].as_bool().unwrap_or(false))
    }

    async fn call_ok(&mut self, tool: &str, arguments: Value) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(!error, "{tool}: {text}");
        text
    }
}

/// The number of rows in `table` with `column = id`, in the tenant schema.
fn rows(conn: &mut PgConnection, table: &str, column: &str, id: Uuid) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    diesel::sql_query(format!(
        "SELECT count(*) AS n FROM org_acme.{table} WHERE {column} = $1"
    ))
    .bind::<diesel::sql_types::Uuid, _>(id)
    .get_result::<Count>(conn)
    .expect("count")
    .n
}

#[tokio::test]
async fn a_task_can_be_cancelled_and_purged() {
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
    let alice_token = user_token(&http, "alice").await;
    let bob_token = user_token(&http, "bob").await;
    let carol_token = user_token(&http, "carol").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let carol = server.client(&carol_token, "acme");
    for client in [&admin, &alice, &bob, &carol] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
        ("carol@kairos.test", OrgRole::Member),
    ] {
        let (user_id, _) = user(&mut conn, email);
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
    }
    let (alice_id, alice_name) = user(&mut conn, "alice@kairos.test");
    let (bob_id, _) = user(&mut conn, "bob@kairos.test");
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    // alice and bob are in the team: the team gives them `transition_items`
    // and `manage_tasks` on its board. carol is a member of the
    // organization only.
    for user_id in [alice_id, bob_id] {
        admin
            .add_team_member(
                &team.id,
                &AddTeamMemberRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await
            .expect("team member");
    }
    let board = team.delivery_board_id.clone().expect("delivery board");
    let detail = admin.get_board(&board).await.expect("board");
    let columns: HashMap<String, String> = detail
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let done_column = detail
        .columns
        .iter()
        .find(|c| c.is_done)
        .expect("a done column")
        .clone();
    let to = |name: &str| columns[name].clone();
    let new_task = |title: &'static str| CreateTaskRequest {
        board_id: Some(board.clone()),
        column_id: None,
        title: title.into(),
        content: "one".into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
    };

    // =======================================================================
    // Cancel
    // =======================================================================
    let task = alice
        .create_task(&new_task("Will not do"))
        .await
        .expect("alice makes a task");
    alice
        .transition_task(&task.short_code, &to("Todo"))
        .await
        .expect("to Todo");
    let active = alice
        .transition_task(&task.short_code, &to("Active"))
        .await
        .expect("to Active");
    assert!(active.claim.is_some(), "alice has the claim");

    // 1. The reason is required.
    for reason in ["", "   "] {
        let err = bob
            .cancel_task(&task.short_code, reason)
            .await
            .expect_err("an empty reason");
        assert!(
            matches!(&err, Error::Validation { status: 422, field: Some(field), .. } if field == "reason"),
            "{err}"
        );
    }

    // 7. carol has no `transition_items` on the board.
    let err = carol
        .cancel_task(&task.short_code, "No need")
        .await
        .expect_err("carol cannot cancel");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");

    // 2. Active has no transition to the done column; the cancel moves the
    //    task there all the same.
    let cancelled = bob
        .cancel_task(&task.short_code, "The customer does not need it.")
        .await
        .expect("bob cancels");
    assert_eq!(cancelled.column_id, done_column.id, "the task is done");
    let mark = cancelled.cancellation.clone().expect("the mark");
    assert_eq!(mark.reason, "The customer does not need it.");
    assert_eq!(mark.cancelled_by, bob_id.to_string());
    // 4. The cancel ends the claim.
    assert_eq!(cancelled.claim, None, "the claim ended");

    // 3. The mark on each read.
    let got = carol.get_task(&task.short_code).await.expect("REST get");
    assert_eq!(got.cancellation, cancelled.cancellation);
    let items = carol
        .board_items(&board, &BoardItemsQuery::default())
        .await
        .expect("board items");
    let card = items
        .columns
        .iter()
        .flat_map(|group| group.tasks.iter())
        .find(|t| t.short_code == task.short_code)
        .expect("the card");
    assert_eq!(
        card.cancellation, cancelled.cancellation,
        "the card has the mark"
    );
    let mut carol_mcp = Mcp::connect(&server.base_url, &carol_token).await;
    let text = carol_mcp
        .call_ok("get_item", json!({"short_code": task.short_code}))
        .await;
    assert!(
        text.contains("- cancelled: ") && text.contains("Reason: The customer does not need it."),
        "{text}"
    );
    let text = carol_mcp
        .call_ok("board_items", json!({"board": board}))
        .await;
    assert!(
        text.lines()
            .any(|line| line.contains(&task.short_code) && line.contains("[cancelled]")),
        "{text}"
    );
    let activity = carol
        .activity(&ActivityQuery {
            entity_id: Some(task.id.clone()),
            action: Some("cancel".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert_eq!(activity.len(), 1, "{activity:?}");
    assert!(
        activity[0]
            .details
            .contains("reason:The customer does not need it."),
        "{}",
        activity[0].details
    );
    let releases = carol
        .activity(&ActivityQuery {
            entity_id: Some(task.id.clone()),
            action: Some("release".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert!(
        releases
            .iter()
            .any(|row| row.details.ends_with("reason:cancel")),
        "{releases:?}"
    );

    // 6. A task in a done column cannot be cancelled.
    let err = bob
        .cancel_task(&task.short_code, "Again")
        .await
        .expect_err("done already");
    assert!(
        matches!(&err, Error::Other { status: 422, code, .. } if code == "TASK_DONE"),
        "{err}"
    );

    // 5. A move out of done removes the mark; the activity keeps it. The
    //    seeded board has no transition out of Completed: an admin adds
    //    one.
    admin
        .add_transition(
            &board,
            &CreateTransitionRequest {
                from_column_id: done_column.id.clone(),
                to_column_id: to("Active"),
            },
        )
        .await
        .expect("a transition out of done");
    let back = bob
        .transition_task(&task.short_code, &to("Active"))
        .await
        .expect("back to Active");
    assert_eq!(back.cancellation, None, "the mark is gone");
    let activity = carol
        .activity(&ActivityQuery {
            entity_id: Some(task.id.clone()),
            action: Some("cancel".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert_eq!(activity.len(), 1, "the history keeps the cancel");

    // MCP `cancel_item`: the same rule.
    let mut bob_mcp = Mcp::connect(&server.base_url, &bob_token).await;
    let (text, error) = bob_mcp
        .call(
            "cancel_item",
            json!({"short_code": task.short_code, "reason": ""}),
        )
        .await;
    assert!(error && text.contains("VALIDATION"), "{text}");
    let text = bob_mcp
        .call_ok(
            "cancel_item",
            json!({"short_code": task.short_code, "reason": "Out of scope."}),
        )
        .await;
    assert!(
        text.contains(&format!("Cancelled {}", task.short_code)) && text.contains("Out of scope."),
        "{text}"
    );
    let got = carol.get_task(&task.short_code).await.expect("REST get");
    assert_eq!(
        got.cancellation.map(|m| m.reason),
        Some("Out of scope.".to_string())
    );
    let _ = alice_name;

    // =======================================================================
    // Purge
    // =======================================================================
    let doomed = alice
        .create_task(&new_task("Zanzibar purge target"))
        .await
        .expect("a task to purge");
    let other = alice
        .create_task(&new_task("The other end"))
        .await
        .expect("a second task");
    let mut alice_mcp = Mcp::connect(&server.base_url, &alice_token).await;
    alice_mcp
        .call_ok(
            "link_items",
            json!({"source": doomed.short_code, "target": other.short_code,
                   "relationship": "blocks"}),
        )
        .await;
    let doomed_id: Uuid = doomed.id.parse().expect("id");

    // 2. carol does not manage the board.
    let err = carol
        .purge_task(&doomed.short_code)
        .await
        .expect_err("carol cannot purge");
    assert!(
        matches!(&err, Error::Forbidden { capability: Some(c), .. } if c == "manage_tasks"),
        "{err}"
    );
    alice
        .get_task(&doomed.short_code)
        .await
        .expect("the task stays");

    // 1. bob manages the board (the team gives him `manage_tasks`).
    let purged = bob
        .purge_task(&doomed.short_code)
        .await
        .expect("bob purges");
    assert_eq!(purged.short_code, doomed.short_code);
    assert_eq!(purged.title, "Zanzibar purge target");
    let err = carol.get_task(&doomed.short_code).await.expect_err("gone");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let items = carol
        .board_items(
            &board,
            &BoardItemsQuery {
                include_deleted: true,
                ..BoardItemsQuery::default()
            },
        )
        .await
        .expect("board items");
    assert!(
        !items
            .columns
            .iter()
            .flat_map(|group| group.tasks.iter())
            .any(|t| t.short_code == doomed.short_code),
        "the board has no card of it, archived or live"
    );
    let found = carol
        .search(&SearchRequest {
            q: Some("Zanzibar".into()),
            ..SearchRequest::default()
        })
        .await
        .expect("search");
    assert_eq!(found.total, 0, "the search does not find it");
    for (table, column) in [
        ("tasks", "id"),
        ("item_history", "item_id"),
        ("item_relationships", "source_id"),
        ("item_relationships", "target_id"),
    ] {
        assert_eq!(
            rows(&mut conn, table, column, doomed_id),
            0,
            "{table}.{column}"
        );
    }
    alice
        .get_task(&other.short_code)
        .await
        .expect("the other end of the edge stays");

    // 3. One activity row names the code and the title.
    let activity = carol
        .activity(&ActivityQuery {
            entity_id: Some(doomed.id.clone()),
            action: Some("purge".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert_eq!(activity.len(), 1, "{activity:?}");
    assert_eq!(
        activity[0].details,
        format!(
            "short_code:{} title:Zanzibar purge target",
            doomed.short_code
        )
    );
    assert_eq!(activity[0].actor_id, bob_id.to_string());

    // 4. An archived task, by MCP: confirm is required.
    let archived = alice
        .create_task(&new_task("Archived then purged"))
        .await
        .expect("a task");
    alice
        .delete_task(&archived.short_code)
        .await
        .expect("archive");
    let (text, error) = bob_mcp
        .call(
            "purge_task",
            json!({"short_code": archived.short_code, "confirm": false}),
        )
        .await;
    assert!(error && text.contains("confirm=true"), "{text}");
    let text = bob_mcp
        .call_ok(
            "purge_task",
            json!({"short_code": archived.short_code, "confirm": true}),
        )
        .await;
    assert!(text.contains("for good"), "{text}");
    let err = carol
        .restore_task(&archived.short_code)
        .await
        .expect_err("nothing to restore");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    assert_eq!(
        rows(&mut conn, "tasks", "id", archived.id.parse().expect("id")),
        0
    );
}
