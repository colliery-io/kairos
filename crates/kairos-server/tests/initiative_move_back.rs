//! An initiative moves back one column, and its tasks follow
//! (KAIROS-T-0775, COLLIERY-I-0678).
//!
//! 1. A board member who is not the owner and not a board manager is
//!    refused with 403 `OWNED`, for the plan and for the move.
//! 2. The owner reads the plan: the column before, and the Todo and Active
//!    tasks with their claimants. Blocked, completed and Backlog tasks are
//!    not in it.
//! 3. The owner moves it back. The Todo and Active tasks go to Backlog and
//!    their claims end; the Blocked task keeps its column and its claim;
//!    the completed task stays.
//! 4. `moved_back` goes to the claimant of a moved task and to a watcher
//!    of the initiative, and not to the actor.
//! 5. A board manager moves it back again over MCP `transition_item`.
//!
//! The test walks the initiative by the columns of its board, in their
//! order, so it holds with Decompose before or after Ready (KAIROS-T-0773).
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_initiative_move_back_t0775_test`.

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
use kairos_client::types::CreateTaskRequest;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_initiative_move_back_t0775_test";

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
                    "clientInfo": {"name": "kairos-t0775-back", "version": "0.0.0"},
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
}

/// A REST call as `token`: the status and the body.
async fn rest(
    base_url: &str,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{base_url}{path}"))
        .bearer_auth(token)
        .header("x-tenant", "acme");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.expect("request");
    let status = response.status().as_u16();
    (
        status,
        response.json::<Value>().await.unwrap_or(Value::Null),
    )
}

fn count(conn: &mut PgConnection, sql: &str, code: &str) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    diesel::sql_query(sql)
        .bind::<diesel::sql_types::Text, _>(code)
        .get_result::<Count>(conn)
        .expect("count")
        .n
}

fn moved_back_for(conn: &mut PgConnection, email: &str) -> i64 {
    count(
        conn,
        "SELECT count(*) AS n FROM org_acme.notifications n \
         JOIN public.users u ON u.id = n.user_id \
         WHERE n.kind = 'moved_back' AND u.email = $1",
        email,
    )
}

fn claims(conn: &mut PgConnection, code: &str) -> i64 {
    count(
        conn,
        "SELECT count(*) AS n FROM org_acme.task_claims c \
         JOIN org_acme.tasks t ON t.id = c.task_id WHERE t.short_code = $1",
        code,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_initiative_moves_back_and_its_tasks_go_to_backlog() {
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
    let base = server.base_url.clone();
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
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: user_id(&mut conn, email),
                role,
            })
            .execute(&mut conn)
            .expect("membership");
    }
    // alice and bob: the `member` grant on the initiative board, and the
    // team Web (its delivery board). Neither has configure_boards.
    let initiatives = admin.get_board("initiatives").await.expect("board");
    for email in ["alice@kairos.test", "bob@kairos.test"] {
        let (status, body) = rest(
            &base,
            &svc_token,
            reqwest::Method::POST,
            &format!("/api/boards/{}/members", initiatives.board.id),
            Some(json!({"user_id": user_id(&mut conn, email), "capabilities": ["member"]})),
        )
        .await;
        assert!((200..300).contains(&status), "{status}: {body}");
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
    for email in ["alice@kairos.test", "bob@kairos.test"] {
        admin
            .add_team_member(
                &team.id,
                &AddTeamMemberRequest {
                    user_id: user_id(&mut conn, email).to_string(),
                },
            )
            .await
            .expect("team member");
    }
    let web = admin.get_board("web-delivery").await.expect("board");
    let columns: HashMap<String, String> = web
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let to = |name: &str| columns[name].clone();
    let mut alice_mcp = Mcp::connect(&base, &alice_token).await;
    let mut admin_mcp = Mcp::connect(&base, &svc_token).await;
    let mut carol_mcp = Mcp::connect(&base, &carol_token).await;

    // --- An initiative that alice owns, walked to Active by the order of
    //     its board.
    let text = alice_mcp
        .call(
            "create_item",
            json!({"item_type": "initiative", "board": "initiatives", "title": "Too early"}),
        )
        .await
        .0;
    let initiative = text
        .split_whitespace()
        .nth(2)
        .and_then(|code| code.strip_suffix(':'))
        .unwrap_or_else(|| panic!("no code in {text}"))
        .to_string();
    let mut order: Vec<(i32, String)> = initiatives
        .columns
        .iter()
        .map(|c| (c.position, c.name.clone()))
        .collect();
    order.sort();
    let names: Vec<String> = order.into_iter().map(|(_, n)| n).collect();
    let active_at = names.iter().position(|n| n == "Active").expect("Active");
    let before_active = names[active_at - 1].clone();
    for name in &names[1..=active_at] {
        let (text, error) = alice_mcp
            .call(
                "transition_item",
                json!({"short_code": initiative, "to_column": name, "claim": name == "Active"}),
            )
            .await;
        assert!(!error, "{name}: {text}");
    }
    let (text, error) = carol_mcp
        .call("watch_item", json!({"short_code": initiative}))
        .await;
    assert!(!error, "{text}");

    // --- Its tasks: bob in Active, one in Todo, alice's in Blocked (the
    //     claim stays), one Completed, one in Backlog.
    let new_task = |title: &str| CreateTaskRequest {
        board_id: Some(web.board.id.clone()),
        column_id: None,
        title: title.into(),
        content: "one".into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
        from_team: None,
        parent: Some(initiative.clone()),
    };
    let active = bob
        .create_task(&new_task("Started"))
        .await
        .expect("task")
        .short_code;
    for c in ["Todo", "Active"] {
        bob.transition_task(&active, &to(c)).await.expect("walk");
    }
    let todo = alice
        .create_task(&new_task("Next"))
        .await
        .expect("task")
        .short_code;
    alice
        .transition_task(&todo, &to("Todo"))
        .await
        .expect("walk");
    let blocked = alice
        .create_task(&new_task("Waits"))
        .await
        .expect("task")
        .short_code;
    for c in ["Todo", "Active", "Blocked"] {
        alice.transition_task(&blocked, &to(c)).await.expect("walk");
    }
    let done = alice
        .create_task(&new_task("Done"))
        .await
        .expect("task")
        .short_code;
    for c in ["Todo", "Active", "Completed"] {
        alice.transition_task(&done, &to(c)).await.expect("walk");
    }
    let backlog = alice
        .create_task(&new_task("Later"))
        .await
        .expect("task")
        .short_code;
    assert_eq!(claims(&mut conn, &active), 1);
    assert_eq!(claims(&mut conn, &blocked), 1);

    // 1. bob is a member, not the owner, not a board manager.
    let path = format!("/api/initiatives/{initiative}/back");
    for method in [reqwest::Method::GET, reqwest::Method::POST] {
        let (status, body) = rest(&base, &bob_token, method.clone(), &path, None).await;
        assert_eq!(status, 403, "{method}: {body}");
        assert_eq!(body["error"]["code"], "OWNED", "{body}");
    }

    // 2. The plan: the Todo and the Active task, with bob's claim.
    let (status, plan) = rest(&base, &alice_token, reqwest::Method::GET, &path, None).await;
    assert_eq!(status, 200, "{plan}");
    assert_eq!(plan["from_column"], "Active");
    assert_eq!(plan["to_column"], before_active.as_str());
    let listed: Vec<(String, Option<String>)> = plan["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|t| {
            (
                t["short_code"].as_str().unwrap().to_string(),
                t["claimant"].as_str().map(str::to_string),
            )
        })
        .collect();
    let mut want = [active.clone(), todo.clone()];
    want.sort();
    let mut got = listed.clone();
    got.sort();
    assert_eq!(
        got.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>(),
        want.to_vec(),
        "{plan}"
    );
    assert!(
        got.iter().any(|(c, who)| *c == active && who.is_some()),
        "{plan}"
    );
    assert!(
        got.iter().any(|(c, who)| *c == todo && who.is_none()),
        "{plan}"
    );

    // 3. The owner moves it back.
    let (status, body) = rest(&base, &alice_token, reqwest::Method::POST, &path, None).await;
    assert_eq!(status, 200, "{body}");
    let got = alice.get_initiative(&initiative).await.expect("initiative");
    let column_name = |id: &str| {
        initiatives
            .columns
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
    };
    assert_eq!(
        column_name(&got.column_id).as_deref(),
        Some(before_active.as_str())
    );
    for code in [&active, &todo] {
        let task = alice.get_task(code).await.expect("task");
        assert_eq!(task.column_id, to("Backlog"), "{code}");
        assert!(task.claim.is_none(), "{code}");
    }
    assert_eq!(claims(&mut conn, &active), 0);
    assert_eq!(
        alice.get_task(&blocked).await.expect("task").column_id,
        to("Blocked")
    );
    assert_eq!(claims(&mut conn, &blocked), 1, "Blocked keeps its claim");
    assert_eq!(
        alice.get_task(&done).await.expect("task").column_id,
        to("Completed")
    );
    assert_eq!(
        alice.get_task(&backlog).await.expect("task").column_id,
        to("Backlog")
    );

    // 4. moved_back: bob (his claim ended) and carol (she watches it); not
    //    alice, who moved it.
    assert_eq!(moved_back_for(&mut conn, "bob@kairos.test"), 1);
    assert_eq!(moved_back_for(&mut conn, "carol@kairos.test"), 1);
    assert_eq!(moved_back_for(&mut conn, "alice@kairos.test"), 0);

    // 5. A board manager (the org admin) moves it back again over MCP.
    let to_column = names[active_at - 2].clone();
    let (text, error) = admin_mcp
        .call(
            "transition_item",
            json!({"short_code": initiative, "to_column": to_column}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(
        text.contains(&format!("Moved {initiative} back:")),
        "{text}"
    );
}
