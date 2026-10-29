//! The agent dispatch rules of a board (KAIROS-T-0385, COLLIERY-I-0614).
//!
//! 1. Only a board manager makes targets and rules; a person who belongs to
//!    the board reads the rules and the log, and gets the URL of a target
//!    with no query; a person who does not belong to the board reads
//!    nothing.
//! 2. A task that enters the column of a rule and matches its filter fires
//!    one signed delivery (a transition, a move to the board, a cancel); a
//!    move that does not match fires none; a removed rule fires none.
//! 3. The payload: code, title, task type, link (with KAIROS_PUBLIC_URL),
//!    board, column, repository, actor.
//! 4. The log shows each delivery with its tries.
//! 5. MCP: create_dispatch_rule (with a new target and its secret),
//!    list_dispatch_rules, delete_dispatch_rule, and the refusal for a
//!    person who cannot configure the board.
//! 6. KAIROS-T-0427: an `unblocked` rule fires when the last open blocker
//!    of a task closes (done, an archive), once for a task with two
//!    blockers, with the code of the blocker; its column and its filter
//!    must match. KAIROS-T-0567: a cancelled blocker does not close.
//! 7. KAIROS-T-0577: enable sends the skipped deliveries again, with
//!    their ids; redeliver does it on request (with `since`),
//!    needs configure_boards, and refuses a target that is off.
//! 8. KAIROS-T-0566: a create into the entry column fires its rules (REST
//!    and MCP); a create that names another column is a 422 that names the
//!    entry column, and fires nothing.
//! 9. KAIROS-T-0558: MCP remove_webhook_target, enable_webhook_target and
//!    test_webhook_target need configure_boards, refuse an unknown
//!    argument, and do what REST does.
//! 10. KAIROS-T-0718: a `review_changed` rule fires when the review state
//!     of a task changes, with the value in `data.review`; its column and
//!     its filter must match; a write of the value the task has fires
//!     nothing; the task reads carry the review; MCP `set_review` and the
//!     line of `get_item`.
//! 11. KAIROS-T-0719: an `entered_column` rule on the initiatives board
//!     fires `initiative.entered_column` on a transition and on a create,
//!     with `entity_type` initiative and no task type; a rule there cannot
//!     filter on a task type or a repository.
//! 12. KAIROS-T-0720: a `claim_released` rule fires on a release and on
//!     the end of a claim by a move to another board, with the live state
//!     of the task in `data.state` as the holder left it; not on a
//!     hand-off, not on a move to a done column.
//!
//! Runs against the LIVE compose stack (`angreal services up`), with
//! KAIROS_WEBHOOK_ALLOW_PRIVATE on (the receiver is on 127.0.0.1). Owns the
//! scratch database `kairos_dispatch_rules_t0385_test`.

mod common;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types::{
    CreateInitiativeRequest, CreateTaskRequest, SetTaskReviewRequest, SetTaskStateRequest,
};
use kairos_client::types_dispatch::{
    CreateDispatchRuleRequest, CreateWebhookTargetRequest, RedeliverQuery, WebhookDeliveriesQuery,
};
use kairos_client::types_meta::{CreateRelationshipRequest, UpdateMetadataRequest};
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::{EntityKind, Error, KairosClient};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::jobs::{Worker, kinds};
use kairos_server::middleware::auth::Authenticator;
use kairos_server::secrets::SecretsKey;
use kairos_server::webhooks::signature;

const SCRATCH_DB: &str = "kairos_dispatch_rules_t0385_test";

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
                    "clientInfo": {"name": "kairos-t0385-dispatch", "version": "0.0.0"},
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

    /// The text of the tool, and whether it is a tool error.
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

/// Run the worker until no delivery job is open.
async fn deliver(worker: &Arc<Worker>, url: &str) {
    let url = url.to_string();
    let run = async {
        loop {
            worker.pass().await;
            let open = {
                let mut conn = PgConnection::establish(&url).expect("connect");
                kairos_db::jobs::list(&mut conn, kinds::WEBHOOK_DELIVERY)
                    .expect("jobs")
                    .iter()
                    .any(|j| j.state == "queued" || j.state == "leased")
            };
            if !open && worker.running() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the deliveries went in time");
}

async fn new_task(client: &KairosClient, board: &str, title: &str, task_type: &str) -> String {
    client
        .create_task(&CreateTaskRequest {
            board_id: Some(board.to_string()),
            column_id: None,
            title: title.to_string(),
            content: String::new(),
            task_type: Some(task_type.to_string()),
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
            parent: None,
        })
        .await
        .expect("task")
        .short_code
}

fn forbidden(result: Result<impl std::fmt::Debug, Error>) {
    let err = result.expect_err("refused");
    assert!(matches!(err, Error::Forbidden { .. }), "{err:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_that_enters_the_column_of_a_rule_fires_one_signed_delivery() {
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
    let mut config = base_config(&scratch_url);
    config.secrets_key = Some(SecretsKey::from_bytes([9u8; 32]));
    config.webhooks.allow_private = true;
    config.webhooks.retry_base = Duration::from_millis(10);
    let state = app::state_with(config, pool, auth);
    let worker = Arc::clone(&state.jobs);
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
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");

    // The board of the team web: alice manages it, bob is in the team, carol
    // is in neither.
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
    admin
        .add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: bob_id.to_string(),
            },
        )
        .await
        .expect("bob in the team");
    admin
        .add_board_member(
            &board,
            &AddBoardMemberRequest {
                user_id: alice_id.to_string(),
                capabilities: vec![
                    "configure_boards".into(),
                    "manage_tasks".into(),
                    "transition_items".into(),
                ],
            },
        )
        .await
        .expect("alice manages the board");
    let detail = admin.get_board(&board).await.expect("board");
    let slug = detail.board.slug.clone();
    let column: HashMap<String, String> = detail
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    // A second board, for a move into its entry column.
    let api = admin
        .create_team(&CreateTeamRequest {
            name: "Api".into(),
            slug: "api".into(),
            code_prefix: "API".into(),
            team_type: None,
        })
        .await
        .expect("team api");
    let api_board = api.delivery_board_id.clone().expect("api board");

    let receiver = common::hook_http::serve().await;

    // --- 1. Rights ---------------------------------------------------------
    let request = CreateWebhookTargetRequest {
        name: "Runner".into(),
        url: format!("{}?token=abc", receiver.url),
    };
    forbidden(bob.create_webhook_target(&slug, &request).await);
    forbidden(carol.create_webhook_target(&slug, &request).await);
    let created = alice
        .create_webhook_target(&slug, &request)
        .await
        .expect("alice makes a target");
    let secret = created.secret.clone();
    assert!(secret.starts_with("kairos_whsec_"), "{secret}");
    let target = created.target.id.clone();
    // The secret is never shown again.
    let listed = alice.list_webhook_targets(&slug).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert!(!serde_json::to_string(&listed).unwrap().contains(&secret));
    assert!(listed[0].url.ends_with("?token=abc"), "a manager sees it");
    let listed = bob.list_webhook_targets(&slug).await.expect("bob reads");
    assert_eq!(listed[0].url, receiver.url, "a member gets no query");
    forbidden(carol.list_webhook_targets(&slug).await);
    forbidden(carol.list_dispatch_rules(&slug).await);
    forbidden(
        carol
            .list_webhook_deliveries(&slug, &WebhookDeliveriesQuery::default())
            .await,
    );
    // An unknown or private input is refused and named.
    let err = alice
        .create_webhook_target(
            &slug,
            &CreateWebhookTargetRequest {
                name: "Bad".into(),
                url: "ftp://example.com/x".into(),
            },
        )
        .await
        .expect_err("ftp");
    assert!(matches!(err, Error::Validation { .. }), "{err:?}");

    let rule = |column: &str, task_type: Option<&str>| CreateDispatchRuleRequest {
        column: Some(column.into()),
        target_id: target.clone(),
        task_type: task_type.map(str::to_string),
        ..Default::default()
    };
    forbidden(bob.create_dispatch_rule(&slug, &rule("Active", None)).await);
    let err = alice
        .create_dispatch_rule(&slug, &rule("Doing", None))
        .await
        .expect_err("no such column");
    assert!(err.to_string().contains("Todo"), "{err}");
    let err = alice
        .create_dispatch_rule(&slug, &rule("Active", Some("epic")))
        .await
        .expect_err("no such type");
    assert!(err.to_string().contains("tech_debt"), "{err}");
    let err = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                metadata_slug: Some("nope".into()),
                metadata_value: Some("x".into()),
                ..rule("Todo", None)
            },
        )
        .await
        .expect_err("no such definition");
    assert!(err.to_string().contains("nope"), "{err}");

    // R1: a bug that enters Active. R2: a task with priority high that
    // enters Todo. R3: each task that is cancelled (enters Completed).
    let r1 = alice
        .create_dispatch_rule(&slug, &rule("Active", Some("bug")))
        .await
        .expect("r1");
    assert_eq!(
        (r1.column.as_deref(), r1.task_type.as_deref()),
        (Some("Active"), Some("bug"))
    );
    let r2 = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                metadata_slug: Some("priority".into()),
                metadata_value: Some("High".into()),
                ..rule("todo", None)
            },
        )
        .await
        .expect("r2");
    assert_eq!(r2.column.as_deref(), Some("Todo"));
    assert_eq!(r2.trigger, "entered_column");
    let r3 = alice
        .create_dispatch_rule(&slug, &rule("Completed", None))
        .await
        .expect("r3");
    // The rules of the api board: each task that lands in its entry column.
    let api_target = admin
        .create_webhook_target(
            &api_board,
            &CreateWebhookTargetRequest {
                name: "Api runner".into(),
                url: receiver.url.clone(),
            },
        )
        .await
        .expect("api target");
    let api_detail = admin.get_board(&api_board).await.expect("api board");
    let api_entry = api_detail.columns[0].name.clone();
    admin
        .create_dispatch_rule(
            &api_board,
            &CreateDispatchRuleRequest {
                column: Some(api_entry.clone()),
                target_id: api_target.target.id.clone(),
                ..Default::default()
            },
        )
        .await
        .expect("api rule");
    // A bob reads the rules.
    let rules = bob.list_dispatch_rules(&slug).await.expect("bob reads");
    assert_eq!(rules.len(), 3);

    // --- 2. The moves ------------------------------------------------------
    let bug = new_task(&alice, &board, "Fix the login", "bug").await;
    let plain = new_task(&alice, &board, "Write the guide", "task").await;
    let urgent = new_task(&alice, &board, "Ship the fix", "task").await;
    let moved = new_task(&alice, &board, "Move me", "task").await;
    let dropped = new_task(&alice, &board, "Drop me", "task").await;
    alice
        .update_metadata(
            EntityKind::Task,
            &urgent,
            &UpdateMetadataRequest {
                values: BTreeMap::from([("priority".to_string(), Some("high".to_string()))]),
            },
        )
        .await
        .expect("priority");

    // Backlog -> Todo: the bug has no priority, so R2 does not match.
    alice
        .transition_task(&bug, &column["Todo"])
        .await
        .expect("bug to Todo");
    // Todo -> Active: R1 matches the bug (1).
    alice
        .transition_task(&bug, &column["Active"])
        .await
        .expect("bug to Active");
    // A task to Active: R1 is for bugs only.
    alice
        .transition_task(&plain, &column["Todo"])
        .await
        .expect("plain to Todo");
    alice
        .transition_task(&plain, &column["Active"])
        .await
        .expect("plain to Active");
    // A task with priority high to Todo: R2 matches (2).
    alice
        .transition_task(&urgent, &column["Todo"])
        .await
        .expect("urgent to Todo");
    // A move to the api board: its rule matches (3).
    admin.move_task(&moved, &api_board).await.expect("move");
    // A cancel: R3 matches (4).
    alice
        .cancel_task(&dropped, "Not needed")
        .await
        .expect("cancel");
    deliver(&worker, &scratch_url).await;

    let received = receiver.received();
    let mut by_code: HashMap<String, Vec<Value>> = HashMap::new();
    for request in &received {
        // Each request is signed with the secret of its target.
        let key = if request.json()["data"]["board"]["slug"] == json!(slug) {
            secret.clone()
        } else {
            api_target.secret.clone()
        };
        assert_eq!(
            request.headers["x-kairos-signature"],
            signature(&key, &request.body),
            "{:?}",
            request.json()
        );
        assert_eq!(request.headers["x-kairos-event"], "task.entered_column");
        let body = request.json();
        assert_eq!(body["id"], json!(request.headers["x-kairos-delivery"]));
        by_code
            .entry(body["data"]["code"].as_str().unwrap().to_string())
            .or_default()
            .push(body);
    }
    let mut codes: Vec<&String> = by_code.keys().collect();
    codes.sort();
    let mut expected = vec![&bug, &urgent, &moved, &dropped];
    expected.sort();
    assert_eq!(codes, expected, "one delivery for each matching move");
    assert!(by_code.values().all(|v| v.len() == 1), "{by_code:?}");
    assert!(!by_code.contains_key(&plain), "no match, no delivery");

    // --- 3. The payload ----------------------------------------------------
    let data = &by_code[&bug][0]["data"];
    assert_eq!(data["title"], "Fix the login");
    assert_eq!(data["task_type"], "bug");
    assert_eq!(data["work_class"], "planned", "KAIROS-T-0429: the lane");
    assert_eq!(data["link"], format!("https://kairos.test/items/{bug}"));
    assert_eq!(data["board"]["slug"], json!(slug));
    assert_eq!(data["column"]["name"], "Active");
    assert_eq!(data["repository"], Value::Null);
    assert_eq!(data["actor"]["id"], json!(alice_id));
    assert_eq!(data["rule_id"], json!(r1.id));
    assert_eq!(by_code[&urgent][0]["data"]["rule_id"], json!(r2.id));
    assert_eq!(by_code[&dropped][0]["data"]["rule_id"], json!(r3.id));
    assert_eq!(by_code[&dropped][0]["data"]["column"]["name"], "Completed");
    assert_eq!(
        by_code[&moved][0]["data"]["column"]["name"],
        json!(api_entry)
    );
    assert_eq!(by_code[&moved][0]["organization"], "acme");

    // --- 4. The log --------------------------------------------------------
    let log = bob
        .list_webhook_deliveries(&slug, &WebhookDeliveriesQuery::default())
        .await
        .expect("bob reads the log");
    assert_eq!(log.len(), 3, "{log:?}");
    assert!(log.iter().all(|d| d.status == "delivered" && d.tries == 1));
    assert!(log.iter().all(|d| d.log.len() == 1 && d.log[0].ok));
    assert!(log.iter().all(|d| d.log[0].response_code == Some(204)));
    let of_r1 = bob
        .list_webhook_deliveries(
            &slug,
            &WebhookDeliveriesQuery {
                rule_id: Some(r1.id.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("the log of r1");
    assert_eq!(of_r1.len(), 1);
    assert_eq!(of_r1[0].short_code.as_deref(), Some(bug.as_str()));
    let err = bob
        .list_webhook_deliveries(
            &slug,
            &WebhookDeliveriesQuery {
                limit: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect_err("limit 0");
    assert!(matches!(err, Error::Validation { .. }), "{err:?}");

    // A removed rule fires nothing.
    forbidden(bob.delete_dispatch_rule(&slug, &r1.id).await);
    alice
        .delete_dispatch_rule(&slug, &r1.id)
        .await
        .expect("remove r1");
    let other_bug = new_task(&alice, &board, "Another bug", "bug").await;
    alice
        .transition_task(&other_bug, &column["Todo"])
        .await
        .expect("to Todo");
    alice
        .transition_task(&other_bug, &column["Active"])
        .await
        .expect("to Active");
    deliver(&worker, &scratch_url).await;
    assert_eq!(receiver.received().len(), received.len());

    // A test delivery (ping) by a manager.
    let ping = alice
        .test_webhook_target(&slug, &target)
        .await
        .expect("ping");
    assert_eq!(ping.event, "ping");
    deliver(&worker, &scratch_url).await;
    let last = receiver.received().pop().expect("the ping");
    assert_eq!(last.headers["x-kairos-event"], "ping");
    assert_eq!(last.headers["x-kairos-delivery"], ping.id);

    // --- 5. MCP ------------------------------------------------------------
    let mut alice_mcp = Mcp::connect(&server.base_url, &alice_token).await;
    let (text, error) = alice_mcp
        .call(
            "create_dispatch_rule",
            json!({
                "board": slug,
                "column": "Todo",
                "url": receiver.url,
                "target_name": "Claude runner",
                "task_type": "bug",
            }),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("secret: kairos_whsec_"), "{text}");
    assert!(text.contains("Claude runner"), "{text}");
    let (text, error) = alice_mcp
        .call("list_dispatch_rules", json!({ "board": slug }))
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("Dispatch rules of"), "{text}");
    assert!(text.contains("Claude runner"), "{text}");
    assert!(text.contains("delivered"), "{text}");
    assert!(
        !text.contains("kairos_whsec_"),
        "no secret in a list: {text}"
    );
    let rules = alice.list_dispatch_rules(&slug).await.expect("rules");
    let made = rules
        .iter()
        .find(|r| r.target == "Claude runner")
        .expect("the MCP rule");
    let mut bob_mcp = Mcp::connect(&server.base_url, &bob_token).await;
    let (text, error) = bob_mcp
        .call(
            "create_dispatch_rule",
            json!({ "board": slug, "column": "Todo", "target_id": target }),
        )
        .await;
    assert!(error, "{text}");
    assert!(text.contains("FORBIDDEN"), "{text}");
    let (text, error) = bob_mcp
        .call(
            "create_dispatch_rule",
            json!({ "board": slug, "column": "Todo" }),
        )
        .await;
    assert!(error && text.contains("VALIDATION"), "{text}");
    let (text, error) = alice_mcp
        .call(
            "delete_dispatch_rule",
            json!({ "board": slug, "rule_id": made.id }),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("Removed the dispatch rule"), "{text}");
    assert_eq!(
        alice.list_dispatch_rules(&slug).await.expect("rules").len(),
        2
    );

    // --- 6. KAIROS-T-0427: unblocked -----------------------------------
    let err = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                column: None,
                ..rule("Todo", None)
            },
        )
        .await
        .expect_err("entered_column needs a column");
    assert!(err.to_string().contains("column"), "{err}");
    let err = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("closed".into()),
                ..rule("Todo", None)
            },
        )
        .await
        .expect_err("no such trigger");
    assert!(err.to_string().contains("unblocked"), "{err}");
    // KAIROS-T-0457: an unblocked rule on a done column would never fire.
    let err = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("unblocked".into()),
                ..rule("Completed", None)
            },
        )
        .await
        .expect_err("a done column");
    assert!(err.to_string().contains("done column"), "{err}");
    // U1: each task (type task) that is unblocked, in each column. U2: a
    // bug in Todo that is unblocked.
    let u1 = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("unblocked".into()),
                column: None,
                ..rule("x", Some("task"))
            },
        )
        .await
        .expect("u1");
    assert_eq!(
        (u1.trigger.as_str(), u1.column.as_deref()),
        ("unblocked", None)
    );
    let u2 = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("unblocked".into()),
                ..rule("Todo", Some("bug"))
            },
        )
        .await
        .expect("u2");
    let blocks = |source: &str, target: &str| CreateRelationshipRequest {
        source_short_code: source.into(),
        target_short_code: target.into(),
        relationship: "blocks".into(),
    };
    let first = new_task(&alice, &board, "First blocker", "task").await;
    let second = new_task(&alice, &board, "Second blocker", "task").await;
    let waiting = new_task(&alice, &board, "Waits for two", "task").await;
    let bug_blocker = new_task(&alice, &board, "Bug blocker", "task").await;
    let bug_in_todo = new_task(&alice, &board, "Bug in Todo", "bug").await;
    let other_blocker = new_task(&alice, &board, "Other blocker", "task").await;
    let bug_in_backlog = new_task(&alice, &board, "Bug in Backlog", "bug").await;
    for (source, target) in [
        (&first, &waiting),
        (&second, &waiting),
        (&bug_blocker, &bug_in_todo),
        (&other_blocker, &bug_in_backlog),
    ] {
        alice
            .create_relationship(&blocks(source, target))
            .await
            .expect("blocks");
    }
    alice
        .transition_task(&bug_in_todo, &column["Todo"])
        .await
        .expect("to Todo");
    let before = receiver.received().len();
    // A blocker goes to Completed, one hop each.
    let finish = |code: String| {
        let alice = &alice;
        let column = &column;
        async move {
            for name in ["Todo", "Active", "Completed"] {
                alice
                    .transition_task(&code, &column[name])
                    .await
                    .unwrap_or_else(|e| panic!("{code} to {name}: {e}"));
            }
        }
    };
    // The first of two blockers closes: nothing.
    finish(first.clone()).await;
    deliver(&worker, &scratch_url).await;
    let unblocked = |from: usize| -> Vec<Value> {
        receiver.received()[from..]
            .iter()
            .filter(|r| r.headers["x-kairos-event"] == "task.unblocked")
            .map(|r| r.json())
            .collect()
    };
    assert!(unblocked(before).is_empty(), "{:?}", unblocked(before));
    // The second closes: one delivery, with its code.
    finish(second.clone()).await;
    // An archived blocker unblocks the bug in Todo (U2).
    alice.delete_task(&bug_blocker).await.expect("archive");
    // The bug in Backlog: U2 wants Todo, U1 wants a task. Nothing.
    finish(other_blocker.clone()).await;
    deliver(&worker, &scratch_url).await;
    let mut got: Vec<(String, String, String)> = unblocked(before)
        .iter()
        .map(|body| {
            (
                body["data"]["code"].as_str().unwrap().to_string(),
                body["data"]["blocker"].as_str().unwrap().to_string(),
                body["data"]["rule_id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    got.sort();
    let mut want = vec![
        (waiting.clone(), second.clone(), u1.id.clone()),
        (bug_in_todo.clone(), bug_blocker.clone(), u2.id.clone()),
    ];
    want.sort();
    assert_eq!(got, want);
    // KAIROS-T-0457: two blockers of one task close at one time. The task
    // is unblocked once, by one of them.
    let race_a = new_task(&alice, &board, "Race blocker A", "task").await;
    let race_b = new_task(&alice, &board, "Race blocker B", "task").await;
    let racing = new_task(&alice, &board, "Waits for the race", "task").await;
    for source in [&race_a, &race_b] {
        alice
            .create_relationship(&blocks(source, &racing))
            .await
            .expect("blocks");
    }
    for source in [&race_a, &race_b] {
        for name in ["Todo", "Active"] {
            alice
                .transition_task(source, &column[name])
                .await
                .expect("to Active");
        }
    }
    let before_race = receiver.received().len();
    let (a, b) = tokio::join!(
        alice.transition_task(&race_a, &column["Completed"]),
        alice.transition_task(&race_b, &column["Completed"]),
    );
    a.expect("done a");
    b.expect("done b");
    deliver(&worker, &scratch_url).await;
    let raced: Vec<(String, String)> = unblocked(before_race)
        .iter()
        .map(|body| {
            (
                body["data"]["code"].as_str().unwrap().to_string(),
                body["data"]["blocker"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(raced.len(), 1, "{raced:?}");
    assert_eq!(raced[0].0, racing);
    assert!(raced[0].1 == race_a || raced[0].1 == race_b, "{raced:?}");
    let (text, error) = alice_mcp
        .call(
            "create_dispatch_rule",
            json!({ "board": slug, "trigger": "unblocked", "target_id": target }),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("a task that is unblocked"), "{text}");

    // --- 7. KAIROS-T-0577: the skipped deliveries go again ---------------
    let set_on = |on: bool| {
        diesel::sql_query(format!(
            "UPDATE org_acme.webhook_targets SET enabled = {on} WHERE id = '{target}'"
        ))
        .execute(&mut PgConnection::establish(&scratch_url).expect("connect"))
        .expect("set the target");
    };
    set_on(false);
    let before_off = receiver.received().len();
    let mut skipped = Vec::new();
    for _ in 0..2 {
        let ping = alice
            .test_webhook_target(&slug, &target)
            .await
            .expect("ping");
        assert_eq!(ping.status, "skipped");
        skipped.push(ping.id);
    }
    deliver(&worker, &scratch_url).await;
    assert_eq!(receiver.received().len(), before_off, "nothing while off");
    // The capability: configure_boards, as for enable.
    forbidden(
        bob.redeliver_webhook_target(&slug, &target, &RedeliverQuery::default())
            .await,
    );
    forbidden(
        carol
            .redeliver_webhook_target(&slug, &target, &RedeliverQuery::default())
            .await,
    );
    forbidden(bob.enable_webhook_target(&slug, &target).await);
    // A target that is off: redeliver is refused, and names enable.
    let err = alice
        .redeliver_webhook_target(&slug, &target, &RedeliverQuery::default())
        .await
        .expect_err("the target is off");
    assert!(matches!(err, Error::Conflict { .. }), "{err:?}");
    assert!(err.to_string().contains("/enable"), "{err}");
    // An unknown parameter and a bad time are refused and named.
    let response = http
        .post(format!(
            "{}/api/boards/{slug}/webhook-targets/{target}/redeliver?nope=1",
            server.base_url
        ))
        .bearer_auth(&alice_token)
        .header("x-tenant", "acme")
        .send()
        .await
        .expect("POST");
    assert_eq!(response.status(), 400);
    assert!(response.text().await.unwrap().contains("nope"));
    let err = alice
        .redeliver_webhook_target(
            &slug,
            &target,
            &RedeliverQuery {
                since: Some("yesterday".into()),
            },
        )
        .await
        .expect_err("not a time");
    assert!(matches!(err, Error::Validation { .. }), "{err:?}");
    assert!(err.to_string().contains("since"), "{err}");
    // Enable: the skipped deliveries go, with their ids. (The order of a
    // worker that sends one at a time is in webhook_deliveries.rs.)
    let on = alice
        .enable_webhook_target(&slug, &target)
        .await
        .expect("enable");
    assert!(on.enabled);
    assert_eq!(on.redelivered, Some(2));
    deliver(&worker, &scratch_url).await;
    let mut sent: Vec<String> = receiver.received()[before_off..]
        .iter()
        .map(|r| r.headers["x-kairos-delivery"].clone())
        .collect();
    sent.sort();
    skipped.sort();
    assert_eq!(sent, skipped);
    let log = alice
        .list_webhook_deliveries(
            &slug,
            &WebhookDeliveriesQuery {
                target_id: Some(target.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("log");
    for id in &skipped {
        let row = log.iter().find(|d| &d.id == id).expect("in the log");
        assert_eq!((row.status.as_str(), row.tries), ("delivered", 1));
    }
    // A target that was turned on before KAIROS-T-0577 keeps its skipped
    // deliveries: redeliver sends them, and `since` limits them.
    set_on(false);
    let mut old = Vec::new();
    for _ in 0..2 {
        old.push(
            alice
                .test_webhook_target(&slug, &target)
                .await
                .expect("ping")
                .id,
        );
    }
    diesel::sql_query(format!(
        "UPDATE org_acme.webhook_deliveries SET created_at = now() - interval '2 hours' \
         WHERE id = '{}'",
        old[0]
    ))
    .execute(&mut conn)
    .expect("an older event");
    set_on(true);
    let before = receiver.received().len();
    let since = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let again = alice
        .redeliver_webhook_target(&slug, &target, &RedeliverQuery { since: Some(since) })
        .await
        .expect("redeliver since");
    assert_eq!(again.redelivered, Some(1));
    deliver(&worker, &scratch_url).await;
    let sent: Vec<String> = receiver.received()[before..]
        .iter()
        .map(|r| r.headers["x-kairos-delivery"].clone())
        .collect();
    assert_eq!(sent, vec![old[1].clone()]);
    let again = alice
        .redeliver_webhook_target(&slug, &target, &RedeliverQuery::default())
        .await
        .expect("redeliver");
    assert_eq!(again.redelivered, Some(1));
    deliver(&worker, &scratch_url).await;
    assert_eq!(
        receiver.received().last().unwrap().headers["x-kairos-delivery"],
        old[0]
    );
    let again = alice
        .redeliver_webhook_target(&slug, &target, &RedeliverQuery::default())
        .await
        .expect("nothing left");
    assert_eq!(again.redelivered, Some(0));

    // --- 8. KAIROS-T-0566: a create fires the rules of the entry column ---
    let before_create = receiver.received().len();
    let born = new_task(&admin, &api_board, "Born in the entry column", "task").await;
    let mut admin_mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let (text, error) = admin_mcp
        .call(
            "create_item",
            json!({"item_type": "task", "title": "Born over MCP", "board": api_board}),
        )
        .await;
    assert!(!error, "{text}");
    let born_mcp = text
        .split_whitespace()
        .find(|word| word.starts_with("API-T-"))
        .expect("the code")
        .trim_end_matches(['.', ':', ','])
        .to_string();
    // A create into another column is refused for a manager too, names the
    // entry column, and fires nothing.
    let api_active = api_detail
        .columns
        .iter()
        .find(|c| c.name == "Active")
        .expect("Active")
        .id
        .clone();
    let (status, body) = admin
        .raw_request(
            reqwest::Method::POST,
            "/api/tasks",
            Some(&json!({
                "board_id": api_board,
                "column_id": api_active,
                "title": "Born in Active",
            })),
        )
        .await
        .expect("POST");
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "VALIDATION", "{body}");
    assert_eq!(body["error"]["details"]["field"], "column_id", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains(&format!("entry column of its board ({api_entry})")),
        "{body}"
    );
    deliver(&worker, &scratch_url).await;
    let mut created: Vec<(String, String)> = receiver.received()[before_create..]
        .iter()
        .filter(|r| r.headers["x-kairos-event"] == "task.entered_column")
        .map(|r| {
            let body = r.json();
            (
                body["data"]["code"].as_str().unwrap().to_string(),
                body["data"]["column"]["name"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    created.sort();
    let mut want = vec![(born, api_entry.clone()), (born_mcp, api_entry.clone())];
    want.sort();
    assert_eq!(created, want, "one delivery for each create");

    // A removed target takes its rules and its log.
    alice
        .delete_webhook_target(&slug, &target)
        .await
        .expect("remove the target");
    assert!(
        alice
            .list_dispatch_rules(&slug)
            .await
            .expect("rules")
            .iter()
            .all(|r| r.target_id != target)
    );

    // --- 9. KAIROS-T-0558: the webhook targets over MCP -----------------
    // The target that create_dispatch_rule made in section 5.
    let runner = alice
        .list_webhook_targets(&slug)
        .await
        .expect("targets")
        .into_iter()
        .find(|t| t.name == "Claude runner")
        .expect("the MCP target")
        .id;
    let args = json!({ "board": slug, "target_id": runner });
    let mut carol_mcp = Mcp::connect(&server.base_url, &carol_token).await;
    // The capability of REST: configure_boards. bob is in the team, carol
    // is in no team; both are refused, and nothing changes.
    for tool in [
        "test_webhook_target",
        "enable_webhook_target",
        "remove_webhook_target",
    ] {
        for mcp in [&mut bob_mcp, &mut carol_mcp] {
            let (text, error) = mcp.call(tool, args.clone()).await;
            assert!(error && text.starts_with("FORBIDDEN"), "{tool}: {text}");
        }
        // An unknown argument is refused and named.
        let (text, error) = alice_mcp
            .call(
                tool,
                json!({ "board": slug, "target_id": runner, "force": true }),
            )
            .await;
        assert!(
            error && text.starts_with("VALIDATION") && text.contains("force"),
            "{tool}: {text}"
        );
        // A target id that is not a UUID, and a target of no board.
        let (text, error) = alice_mcp
            .call(tool, json!({ "board": slug, "target_id": "nope" }))
            .await;
        assert!(error && text.starts_with("VALIDATION"), "{tool}: {text}");
        let (text, error) = alice_mcp
            .call(
                tool,
                json!({ "board": slug, "target_id": Uuid::new_v4().to_string() }),
            )
            .await;
        assert!(error && text.starts_with("NOT_FOUND"), "{tool}: {text}");
    }
    let before = receiver.received().len();
    let (text, error) = alice_mcp.call("test_webhook_target", args.clone()).await;
    assert!(!error, "{text}");
    assert!(
        text.contains("event ping") && text.contains("pending"),
        "{text}"
    );
    deliver(&worker, &scratch_url).await;
    let pings = receiver.received();
    assert_eq!(pings.len(), before + 1, "one ping to the local receiver");
    assert_eq!(pings[before].headers["x-kairos-event"], "ping");
    assert!(
        text.contains(&pings[before].headers["x-kairos-delivery"]),
        "the answer names the delivery: {text}"
    );
    // Off, a ping is skipped; enable turns it on and sends the skipped one.
    diesel::sql_query(format!(
        "UPDATE org_acme.webhook_targets SET enabled = false, consecutive_failures = 3 \
         WHERE id = '{runner}'"
    ))
    .execute(&mut conn)
    .expect("turn the runner off");
    let (text, error) = alice_mcp.call("test_webhook_target", args.clone()).await;
    assert!(!error && text.contains("skipped"), "{text}");
    let (text, error) = alice_mcp.call("enable_webhook_target", args.clone()).await;
    assert!(!error, "{text}");
    assert!(
        text.contains("is on") && text.contains("queued 1 skipped"),
        "{text}"
    );
    let on = alice
        .list_webhook_targets(&slug)
        .await
        .expect("targets")
        .into_iter()
        .find(|t| t.id == runner)
        .expect("the runner");
    assert!(on.enabled && on.consecutive_failures == 0, "{on:?}");
    deliver(&worker, &scratch_url).await;
    assert_eq!(
        receiver.received().len(),
        before + 2,
        "the skipped ping went"
    );
    // Remove: the target is gone.
    let (text, error) = alice_mcp.call("remove_webhook_target", args.clone()).await;
    assert!(
        !error && text.contains("Removed the webhook target Claude runner"),
        "{text}"
    );
    assert!(
        alice
            .list_webhook_targets(&slug)
            .await
            .expect("targets")
            .iter()
            .all(|t| t.id != runner)
    );
    let (text, error) = alice_mcp.call("remove_webhook_target", args).await;
    assert!(error && text.starts_with("NOT_FOUND"), "{text}");

    // --- 10. KAIROS-T-0718: review_changed ------------------------------
    // The first target went in section 9: a target of its own.
    let review_target = alice
        .create_webhook_target(
            &slug,
            &CreateWebhookTargetRequest {
                name: "Reviewer".into(),
                url: receiver.url.clone(),
            },
        )
        .await
        .expect("review target")
        .target
        .id;
    let rule = |column: &str, task_type: Option<&str>| CreateDispatchRuleRequest {
        column: Some(column.into()),
        target_id: review_target.clone(),
        task_type: task_type.map(str::to_string),
        ..Default::default()
    };
    let err = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("review_changed".into()),
                ..rule("Completed", None)
            },
        )
        .await
        .expect_err("a done column");
    assert!(err.to_string().contains("done column"), "{err}");
    // R1: each task whose review state changes, in each column. R2: a task
    // in Active whose review state changes.
    let r1 = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("review_changed".into()),
                column: None,
                ..rule("x", None)
            },
        )
        .await
        .expect("r1");
    assert_eq!(
        (r1.trigger.as_str(), r1.column.as_deref()),
        ("review_changed", None)
    );
    let r2 = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("review_changed".into()),
                ..rule("Active", None)
            },
        )
        .await
        .expect("r2");
    let reviewed = new_task(&alice, &board, "Needs a review", "task").await;
    alice
        .transition_task(&reviewed, &column["Todo"])
        .await
        .expect("to Todo");
    let err = alice
        .set_task_review(
            &reviewed,
            &SetTaskReviewRequest {
                review: "later".into(),
            },
        )
        .await
        .expect_err("no such value");
    assert!(err.to_string().contains("not_needed"), "{err}");
    let before_review = receiver.received().len();
    let task = alice
        .set_task_review(
            &reviewed,
            &SetTaskReviewRequest {
                review: " Needed ".into(),
            },
        )
        .await
        .expect("needed");
    let review = task.review.expect("the task carries its review");
    assert_eq!(review.review, "needed");
    assert_eq!(review.updated_by_name, "alice");
    deliver(&worker, &scratch_url).await;
    let reviews = |from: usize| -> Vec<(String, String, String)> {
        receiver.received()[from..]
            .iter()
            .filter(|r| r.headers["x-kairos-event"] == "task.review_changed")
            .map(|r| r.json())
            .map(|body| {
                (
                    body["data"]["code"].as_str().unwrap().to_string(),
                    body["data"]["review"].as_str().unwrap().to_string(),
                    body["data"]["rule_id"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    // In Todo: R1 matches, R2 wants Active.
    assert_eq!(
        reviews(before_review),
        vec![(reviewed.clone(), "needed".to_string(), r1.id.clone())]
    );
    // The same value again: no change, nothing fires.
    let before_same = receiver.received().len();
    alice
        .set_task_review(
            &reviewed,
            &SetTaskReviewRequest {
                review: "needed".into(),
            },
        )
        .await
        .expect("needed again");
    deliver(&worker, &scratch_url).await;
    assert!(
        reviews(before_same).is_empty(),
        "{:?}",
        reviews(before_same)
    );
    // In Active: both rules.
    alice
        .transition_task(&reviewed, &column["Active"])
        .await
        .expect("to Active");
    let before_done = receiver.received().len();
    alice
        .set_task_review(
            &reviewed,
            &SetTaskReviewRequest {
                review: "done".into(),
            },
        )
        .await
        .expect("done");
    deliver(&worker, &scratch_url).await;
    let mut got_done = reviews(before_done);
    got_done.sort();
    let mut want_done = vec![
        (reviewed.clone(), "done".to_string(), r1.id.clone()),
        (reviewed.clone(), "done".to_string(), r2.id.clone()),
    ];
    want_done.sort();
    assert_eq!(got_done, want_done);
    // The read of the task and the board carry the review.
    let read = alice.get_task(&reviewed).await.expect("read");
    assert_eq!(read.review.map(|r| r.review).as_deref(), Some("done"));
    // Carol is in neither the team nor the board: the edit rule refuses
    // her.
    forbidden(
        carol
            .set_task_review(
                &reviewed,
                &SetTaskReviewRequest {
                    review: "needed".into(),
                },
            )
            .await,
    );
    // MCP: set_review and the line of get_item.
    let (text, error) = alice_mcp
        .call(
            "set_review",
            json!({ "short_code": reviewed, "review": "not_needed" }),
        )
        .await;
    assert!(!error, "{text}");
    assert!(
        text.starts_with(&format!("Review of {reviewed}: not_needed (set")),
        "{text}"
    );
    let (text, error) = alice_mcp
        .call("get_item", json!({ "short_code": reviewed }))
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("- review: not_needed (set"), "{text}");
    let (text, error) = alice_mcp
        .call(
            "set_review",
            json!({ "short_code": reviewed, "review": "maybe" }),
        )
        .await;
    assert!(error, "{text}");
    assert!(text.contains("not_needed, needed, done"), "{text}");

    // --- 11. KAIROS-T-0719: an initiative enters a column -----------------
    let initiatives = admin
        .get_board("initiatives")
        .await
        .expect("initiatives board");
    let initiative_column: HashMap<String, String> = initiatives
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.id.clone()))
        .collect();
    let initiative_target = admin
        .create_webhook_target(
            "initiatives",
            &CreateWebhookTargetRequest {
                name: "Coordinator".into(),
                url: receiver.url.clone(),
            },
        )
        .await
        .expect("initiatives target")
        .target
        .id;
    let initiative_rule = |column: &str| CreateDispatchRuleRequest {
        column: Some(column.into()),
        target_id: initiative_target.clone(),
        ..Default::default()
    };
    // A task filter has no meaning on an initiative board.
    for request in [
        CreateDispatchRuleRequest {
            task_type: Some("bug".into()),
            ..initiative_rule("Design")
        },
        CreateDispatchRuleRequest {
            repository: Some("kairos".into()),
            ..initiative_rule("Design")
        },
    ] {
        let err = admin
            .create_dispatch_rule("initiatives", &request)
            .await
            .expect_err("no task filter on an initiative board");
        assert!(err.to_string().contains("initiative board"), "{err}");
    }
    // The metadata filter stays.
    admin
        .create_dispatch_rule(
            "initiatives",
            &CreateDispatchRuleRequest {
                metadata_slug: Some("priority".into()),
                metadata_value: Some("high".into()),
                ..initiative_rule("Ready")
            },
        )
        .await
        .expect("a metadata filter on an initiative board");
    admin
        .create_dispatch_rule("initiatives", &initiative_rule("Discovery"))
        .await
        .expect("rule on Discovery");
    let design_rule = admin
        .create_dispatch_rule("initiatives", &initiative_rule("Design"))
        .await
        .expect("rule on Design");
    let before_initiative = receiver.received().len();
    let initiative = admin
        .create_initiative(&CreateInitiativeRequest {
            board_id: "initiatives".into(),
            column_id: None,
            title: "Fleet".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
            parent: None,
        })
        .await
        .expect("initiative")
        .short_code;
    admin
        .transition_initiative(&initiative, &initiative_column["Design"])
        .await
        .expect("to Design");
    deliver(&worker, &scratch_url).await;
    let entered: Vec<Value> = receiver.received()[before_initiative..]
        .iter()
        .filter(|r| r.headers["x-kairos-event"] == "initiative.entered_column")
        .map(|r| r.json())
        .collect();
    assert_eq!(entered.len(), 2, "{entered:?}");
    for body in &entered {
        assert_eq!(body["event"], "initiative.entered_column");
        assert_eq!(body["data"]["entity_type"], "initiative");
        assert_eq!(body["data"]["code"], initiative);
        assert_eq!(body["data"]["board"]["slug"], "initiatives");
        assert!(body["data"]["task_type"].is_null(), "{body}");
        assert!(body["data"]["work_class"].is_null(), "{body}");
        assert!(body["data"]["repository"].is_null(), "{body}");
    }
    // The two deliveries went in parallel: compare as a set.
    let mut columns: Vec<(String, String)> = entered
        .iter()
        .map(|body| {
            (
                body["data"]["column"]["name"].as_str().unwrap().to_string(),
                body["data"]["rule_id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    columns.sort();
    assert_eq!(columns[0].0, "Design");
    assert_eq!(columns[0].1, design_rule.id);
    assert_eq!(columns[1].0, "Discovery");
    // A task that enters a column still fires `task.entered_column`, no
    // other event (the rules of the delivery board are unchanged).
    assert!(
        receiver.received()[before_initiative..]
            .iter()
            .all(|r| r.headers["x-kairos-event"] != "task.entered_column"),
        "no task moved"
    );

    // --- 12. KAIROS-T-0720: claim_released --------------------------------
    let err = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("claim_released".into()),
                ..rule("Completed", None)
            },
        )
        .await
        .expect_err("a done column");
    assert!(err.to_string().contains("done column"), "{err}");
    let released_rule = alice
        .create_dispatch_rule(
            &slug,
            &CreateDispatchRuleRequest {
                trigger: Some("claim_released".into()),
                column: None,
                ..rule("x", None)
            },
        )
        .await
        .expect("claim_released rule");
    assert_eq!(
        (
            released_rule.trigger.as_str(),
            released_rule.column.as_deref()
        ),
        ("claim_released", None)
    );
    // The api board too, for the end of a claim by a move there.
    admin
        .create_dispatch_rule(
            &api_board,
            &CreateDispatchRuleRequest {
                trigger: Some("claim_released".into()),
                column: None,
                target_id: api_target.target.id.clone(),
                ..Default::default()
            },
        )
        .await
        .expect("claim_released rule on the api board");
    let handed = new_task(&alice, &board, "Handed back", "task").await;
    for name in ["Todo", "Active"] {
        alice
            .transition_task(&handed, &column[name])
            .await
            .expect("to Active");
    }
    alice
        .set_task_state(
            &handed,
            &SetTaskStateRequest {
                branch: Some(Some("web-t-handed".into())),
                commit: Some(Some("abc1234".into())),
                pushed: Some(true),
                pr_url: Some(Some("https://example.com/pr/7".into())),
                next_step: Some(Some("Wait for the review".into())),
                expected_updated_at: None,
            },
        )
        .await
        .expect("state");
    let before_claim = receiver.received().len();
    let released = |from: usize| -> Vec<Value> {
        receiver.received()[from..]
            .iter()
            .filter(|r| r.headers["x-kairos-event"] == "task.claim_released")
            .map(|r| r.json())
            .collect()
    };
    // A hand-off fires nothing.
    alice
        .hand_off_task(&handed, &bob_id.to_string())
        .await
        .expect("hand-off to bob");
    deliver(&worker, &scratch_url).await;
    assert!(
        released(before_claim).is_empty(),
        "{:?}",
        released(before_claim)
    );
    // A release fires once, with the state as bob left it (the hand-off
    // cleared alice's next step; bob writes his own); Kairos clears the
    // next step after.
    bob.set_task_state(
        &handed,
        &SetTaskStateRequest {
            next_step: Some(Some("Wait for the review".into())),
            ..Default::default()
        },
    )
    .await
    .expect("bob's next step");
    bob.release_task(&handed).await.expect("bob releases");
    deliver(&worker, &scratch_url).await;
    let got = released(before_claim);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["data"]["code"], handed);
    assert_eq!(got[0]["data"]["column"]["name"], "Active");
    assert_eq!(got[0]["data"]["rule_id"], released_rule.id);
    assert_eq!(got[0]["data"]["state"]["branch"], "web-t-handed");
    assert_eq!(got[0]["data"]["state"]["commit"], "abc1234");
    assert_eq!(got[0]["data"]["state"]["pushed"], true);
    assert_eq!(
        got[0]["data"]["state"]["pr_url"],
        "https://example.com/pr/7"
    );
    assert_eq!(got[0]["data"]["state"]["next_step"], "Wait for the review");
    // A move to a done column ends the claim and fires nothing.
    alice.claim_task(&handed).await.expect("alice claims");
    let before_done = receiver.received().len();
    alice
        .transition_task(&handed, &column["Completed"])
        .await
        .expect("to Completed");
    deliver(&worker, &scratch_url).await;
    assert!(
        released(before_done).is_empty(),
        "{:?}",
        released(before_done)
    );
    // A move to another board ends the claim: the rule of that board fires,
    // in its entry column, with no state.
    let moved = new_task(&alice, &board, "Moved away", "task").await;
    for name in ["Todo", "Active"] {
        alice
            .transition_task(&moved, &column[name])
            .await
            .expect("to Active");
    }
    let before_move = receiver.received().len();
    admin.move_task(&moved, &api_board).await.expect("move");
    deliver(&worker, &scratch_url).await;
    let got = released(before_move);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["data"]["board"]["id"], api_board);
    assert!(got[0]["data"]["state"].is_null(), "{}", got[0]);
}
