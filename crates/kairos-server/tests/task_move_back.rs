//! A task moves back one column (KAIROS-T-0774, COLLIERY-I-0678): Active →
//! Todo, Todo → Backlog, after a false start, on the REST and MCP surfaces.
//!
//! 1. A team member who is not the claimant and not a board manager is
//!    refused with 403 `CLAIMED`, over `POST /api/tasks/{code}/back` and
//!    MCP `transition_item`. The task stays in Active.
//! 2. A person with no `transition_items` gets a 403.
//! 3. The claimant moves the task back to Todo: the claim ends.
//! 4. A task with no claim moves back by any person who can move it
//!    (MCP `transition_item` to Backlog), with no transition of the board.
//! 5. The entry column has no column before: 422 `NO_COLUMN_BACK`.
//! 6. A board manager (an organization admin: `configure_boards`) moves
//!    back a task that a different person has the claim on, through the
//!    REST transition route that the board drag uses.
//! 7. A done task is a 422 `TASK_DONE`: it comes back by a reopen.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_task_move_back_t0774_test`.

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

const SCRATCH_DB: &str = "kairos_task_move_back_t0774_test";

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
                    "clientInfo": {"name": "kairos-t0774-back", "version": "0.0.0"},
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

/// `POST /api/tasks/{code}/back` as `token`: the status and the body.
async fn back(base_url: &str, token: &str, code: &str) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("{base_url}/api/tasks/{code}/back"))
        .bearer_auth(token)
        .header("x-tenant", "acme")
        .send()
        .await
        .expect("back request");
    let status = response.status().as_u16();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

fn claims(conn: &mut PgConnection, code: &str) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    diesel::sql_query(
        "SELECT count(*) AS n FROM org_acme.task_claims c \
         JOIN org_acme.tasks t ON t.id = c.task_id WHERE t.short_code = $1",
    )
    .bind::<diesel::sql_types::Text, _>(code)
    .get_result::<Count>(conn)
    .expect("count")
    .n
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_moves_back_by_its_claimant_or_a_board_manager() {
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
    // alice and bob are in the team Web: `transition_items` on its board,
    // and no `configure_boards`. carol is a member of the organization
    // only.
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
        parent: None,
    };
    let mut bob_mcp = Mcp::connect(&base, &bob_token).await;

    // --- alice starts a task: she has the claim in Active.
    let task = alice
        .create_task(&new_task("False start"))
        .await
        .expect("task");
    let code = task.short_code.clone();
    for column in ["Todo", "Active"] {
        alice
            .transition_task(&code, &to(column))
            .await
            .expect("walk");
    }
    assert_eq!(claims(&mut conn, &code), 1);

    // 1. bob is not the claimant and not a board manager.
    let (status, body) = back(&base, &bob_token, &code).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "CLAIMED", "{body}");
    let text = body.to_string();
    assert!(text.contains("configure_boards"), "{text}");
    let (text, error) = bob_mcp
        .call(
            "transition_item",
            json!({"short_code": code, "to_column": "Todo"}),
        )
        .await;
    assert!(error && text.contains("CLAIMED"), "{text}");
    let got = alice.get_task(&code).await.expect("get");
    assert_eq!(got.column_id, to("Active"), "the task stays");

    // 2. carol cannot move the task at all.
    let (status, body) = back(&base, &carol_token, &code).await;
    assert_eq!(status, 403, "{body}");

    // 3. The claimant moves it back: the claim ends.
    let (status, body) = back(&base, &alice_token, &code).await;
    assert_eq!(status, 200, "{body}");
    let got = alice.get_task(&code).await.expect("get");
    assert_eq!(got.column_id, to("Todo"));
    assert!(got.claim.is_none(), "the claim ended");
    assert_eq!(claims(&mut conn, &code), 0);

    // 4. No claim: bob moves it back to Backlog over MCP, with no
    //    transition of the board from Todo to Backlog.
    let (text, error) = bob_mcp
        .call(
            "transition_item",
            json!({"short_code": code, "to_column": "Backlog"}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("back: Todo -> Backlog"), "{text}");
    assert_eq!(
        bob.get_task(&code).await.expect("get").column_id,
        to("Backlog")
    );

    // 5. Backlog is the entry column.
    let (status, body) = back(&base, &bob_token, &code).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "NO_COLUMN_BACK", "{body}");

    // 6. A board manager moves back alice's claimed task, over the REST
    //    transition route of the board drag.
    let other = alice.create_task(&new_task("Claimed")).await.expect("task");
    for column in ["Todo", "Active"] {
        alice
            .transition_task(&other.short_code, &to(column))
            .await
            .expect("walk");
    }
    admin
        .transition_task(&other.short_code, &to("Todo"))
        .await
        .expect("a board manager moves it back");
    let got = admin.get_task(&other.short_code).await.expect("get");
    assert_eq!(got.column_id, to("Todo"));
    assert!(got.claim.is_none());

    // 7. A done task comes back by a reopen.
    for column in ["Active", "Completed"] {
        alice
            .transition_task(&other.short_code, &to(column))
            .await
            .expect("walk");
    }
    let (status, body) = back(&base, &alice_token, &other.short_code).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "TASK_DONE", "{body}");
}
