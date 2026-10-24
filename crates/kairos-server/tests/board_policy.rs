//! The board policies (KAIROS-T-0641).
//!
//! `parent_gate`: on a delivery board with the policy on, a task moves
//! into a claims column (Active) only while its parent initiative is in
//! Active or Monitoring, or is a bucket. A task with no parent initiative
//! moves freely. A transition, a board move and a create are checked. The
//! refusal is 422 `PARENT_NOT_ACTIVE` over REST and MCP. With the policy
//! off, the move passes. A task in Active stays there when its initiative
//! leaves Active. `my_boards` shows the policy; `PATCH /api/boards/{id}`
//! and `set_board_policy` set it.
//!
//! `owner_gate` (KAIROS-T-0643): on an initiative board with the policy
//! on, an initiative moves into Active only with an owner, and the owner of
//! an initiative in Active or Monitoring cannot release it; a hand-off to a
//! member of the board can change it. The refusal is 422 `OWNER_REQUIRED`.
//! With the policy off, both pass. KAIROS-T-0676: `transition_item` with
//! `claim: true` makes the caller the owner in the same move; a refused
//! move keeps no owner; a different owner is a CONFLICT; a strategy is
//! VALIDATION. PARENT_NOT_ACTIVE under an initiative with no owner names
//! both steps.
//!
//! `blockers_gate` (KAIROS-T-0565): a move of a task with open blockers
//! into a claims column or a done column (a transition, a reopen, a board
//! move) gets a warning that names each blocker: the `Kairos-Warning`
//! header and the MCP warning line. A cancelled blocker is open. With the
//! policy on, the move is refused with 422 `BLOCKED`. The policy is off by
//! default.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch databases `kairos_board_policy_t0641_test`,
//! `kairos_board_policy_t0643_test` and `kairos_board_policy_t0565_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::Error;
use kairos_client::types_org::{CreateTeamRequest, UpdateBoardRequest, UpdateColumnRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_board_policy_t0641_test";
const OWNER_SCRATCH_DB: &str = "kairos_board_policy_t0643_test";
const BLOCKERS_SCRATCH_DB: &str = "kairos_board_policy_t0565_test";

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
                    "clientInfo": {"name": "kairos-t0641-policy", "version": "0.0.0"},
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

    async fn ok(&mut self, tool: &str, arguments: Value) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(!error, "{tool}: {text}");
        text
    }

    async fn refused(&mut self, tool: &str, arguments: Value, code: &str) -> String {
        let (text, error) = self.call(tool, arguments).await;
        assert!(
            error && text.contains(code),
            "{tool}: expected {code}, got {text}"
        );
        text
    }

    /// Create an item; its short code.
    async fn create(&mut self, arguments: Value) -> String {
        let text = self.ok("create_item", arguments).await;
        text.split_whitespace()
            .nth(2)
            .and_then(|code| code.strip_suffix(':'))
            .unwrap_or_else(|| panic!("no code in {text}"))
            .to_string()
    }

    async fn walk(&mut self, code: &str, columns: &[&str]) {
        for column in columns {
            self.ok(
                "transition_item",
                json!({"short_code": code, "to_column": column}),
            )
            .await;
        }
    }
}

fn parent_not_active(result: Result<impl std::fmt::Debug, Error>, initiative: &str) {
    let err = result.expect_err("refused");
    assert_eq!(err.code(), Some("PARENT_NOT_ACTIVE"), "{err:?}");
    assert_eq!(err.status(), Some(422), "{err:?}");
    let text = err.to_string();
    assert!(text.contains(initiative), "{text}");
    assert!(text.contains("transition_item"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_moves_to_active_only_under_an_active_initiative() {
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
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let admin = server.client(&svc_token, "acme");
    let _ = admin.whoami().await;
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user_id(&mut conn, "svc@kairos.test"),
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");
    for (name, slug, prefix) in [("Web", "web", "WEB"), ("Ops", "ops", "OPS")] {
        admin
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                code_prefix: prefix.into(),
                team_type: None,
            })
            .await
            .expect("team");
    }
    let web = admin.get_board("web-delivery").await.expect("board");
    let column = |name: &str| {
        web.columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("column {name}"))
            .id
            .clone()
    };
    let active = column("Active");
    // The policy is on for a new delivery board, and null elsewhere.
    assert_eq!(web.board.parent_gate, Some(true));
    let initiatives = admin.get_board("initiatives").await.expect("board");
    assert_eq!(initiatives.board.parent_gate, None);

    let mut mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let initiative = mcp
        .create(
            json!({"item_type": "initiative", "board": "initiatives", "title": "Discovery work"}),
        )
        .await;
    let task = |title: &str, parent: Option<&str>| {
        let mut args = json!({"item_type": "task", "board": "web-delivery", "title": title});
        if let Some(parent) = parent {
            args["parent"] = json!(parent);
        }
        args
    };
    let first = mcp.create(task("First", Some(&initiative))).await;
    mcp.walk(&first, &["Todo"]).await;

    // --- A Discovery initiative refuses, over MCP and REST alike.
    let text = mcp
        .refused(
            "transition_item",
            json!({"short_code": first, "to_column": "Active"}),
            "PARENT_NOT_ACTIVE",
        )
        .await;
    assert!(
        text.contains(&initiative) && text.contains("Discovery"),
        "{text}"
    );
    // KAIROS-T-0676: the initiative has no owner, so the refusal names
    // both steps.
    assert!(
        text.contains(&format!(
            "{initiative} also has no owner, and an initiative moves to Active only with an \
             owner. Become its owner and move it in one call: transition_item {initiative} \
             Active with claim: true."
        )),
        "{text}"
    );
    let err = admin
        .transition_task(&first, &active)
        .await
        .expect_err("refused");
    let Error::Other { details, .. } = &err else {
        panic!("expected PARENT_NOT_ACTIVE, got {err:?}");
    };
    assert_eq!(details["owner_needed"], json!(true), "{details}");
    assert_eq!(
        details["fix"],
        json!(format!("transition_item {initiative} Active claim: true")),
        "{details}"
    );
    parent_not_active(admin.transition_task(&first, &active).await, &initiative);
    // Kairos does not move the initiative.
    assert_eq!(
        admin.get_initiative(&initiative).await.unwrap().column_id,
        initiatives
            .columns
            .iter()
            .find(|c| c.name == "Discovery")
            .unwrap()
            .id
    );

    // --- No parent initiative: the move passes.
    let free = mcp.create(task("Free", None)).await;
    mcp.walk(&free, &["Todo", "Active"]).await;

    // --- A bucket initiative is always active.
    let bucket = mcp
        .create(json!({"item_type": "initiative", "board": "initiatives", "title": "Bugs", "bucket_type": "bug"}))
        .await;
    let intake = mcp.create(task("Bug fix", Some(&bucket))).await;
    mcp.walk(&intake, &["Todo", "Active"]).await;

    // --- An Active initiative and a Monitoring initiative let it pass.
    // KAIROS-T-0643: an initiative moves to Active only with an owner.
    mcp.ok("claim_item", json!({"short_code": initiative}))
        .await;
    mcp.walk(&initiative, &["Design", "Ready", "Decompose", "Active"])
        .await;
    mcp.walk(&first, &["Active"]).await;
    mcp.walk(&initiative, &["Monitoring"]).await;
    let second = mcp.create(task("Second", Some(&initiative))).await;
    mcp.walk(&second, &["Todo", "Active"]).await;

    // --- Completed refuses. The tasks in Active stay there, and leave it.
    mcp.walk(&initiative, &["Completed"]).await;
    let late = mcp.create(task("Late", Some(&initiative))).await;
    mcp.walk(&late, &["Todo"]).await;
    let text = mcp
        .refused(
            "transition_item",
            json!({"short_code": late, "to_column": "Active"}),
            "PARENT_NOT_ACTIVE",
        )
        .await;
    assert!(text.contains("Completed"), "{text}");
    // The initiative has an owner: one step only.
    assert!(!text.contains("claim: true"), "{text}");
    assert_eq!(admin.get_task(&first).await.unwrap().column_id, active);
    mcp.walk(&first, &["Completed"]).await;

    // --- A board move into a claims entry column is checked.
    let ops = admin.get_board("ops-delivery").await.expect("ops board");
    let ops_backlog = ops
        .columns
        .iter()
        .find(|c| c.name == "Backlog")
        .unwrap()
        .id
        .clone();
    admin
        .update_column(
            &ops.board.id,
            &ops_backlog,
            &UpdateColumnRequest {
                name: None,
                position: None,
                is_done: None,
                claims: Some(true),
                keeps_claims: None,
                wip_limit: None,
            },
        )
        .await
        .expect("claims on the entry column");
    parent_not_active(admin.move_task(&late, "ops-delivery").await, &initiative);
    assert_eq!(admin.get_task(&late).await.unwrap().board_id, web.board.id);
    mcp.refused(
        "move_item",
        json!({"short_code": late, "to_board": "ops-delivery"}),
        "PARENT_NOT_ACTIVE",
    )
    .await;

    // --- A create into a claims column is checked, and writes nothing.
    mcp.refused(
        "create_item",
        json!({"item_type": "task", "board": "ops-delivery", "title": "Refused create", "parent": initiative}),
        "PARENT_NOT_ACTIVE",
    )
    .await;
    #[derive(diesel::QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let created = diesel::sql_query(
        "SELECT COUNT(*) AS n FROM org_acme.tasks WHERE title = 'Refused create'",
    )
    .get_result::<Count>(&mut conn)
    .expect("count")
    .n;
    assert_eq!(created, 0);
    // With no parent, the same create passes.
    mcp.create(json!({"item_type": "task", "board": "ops-delivery", "title": "Free create"}))
        .await;

    // --- my_boards shows the policy.
    let boards = mcp.ok("my_boards", json!({})).await;
    assert!(boards.contains("policy: parent_gate on"), "{boards}");

    // --- The policy off (REST): the move passes.
    let off = admin
        .update_board(
            &web.board.id,
            &UpdateBoardRequest {
                parent_gate: Some(false),
                ..Default::default()
            },
        )
        .await
        .expect("policy off");
    assert_eq!(off.parent_gate, Some(false));
    mcp.walk(&late, &["Active"]).await;
    let boards = mcp.ok("my_boards", json!({})).await;
    assert!(boards.contains("policy: parent_gate off"), "{boards}");

    // --- set_board_policy turns it on again; a policy of a different
    // level is refused, and so is a call with no policy.
    let text = mcp
        .ok(
            "set_board_policy",
            json!({"board": "web-delivery", "parent_gate": true}),
        )
        .await;
    assert!(text.contains("parent_gate on"), "{text}");
    assert_eq!(
        admin
            .get_board("web-delivery")
            .await
            .unwrap()
            .board
            .parent_gate,
        Some(true)
    );
    mcp.refused(
        "set_board_policy",
        json!({"board": "initiatives", "parent_gate": false}),
        "parent_gate",
    )
    .await;
    mcp.refused(
        "set_board_policy",
        json!({"board": "web-delivery"}),
        "no policy",
    )
    .await;
    let err = admin
        .update_board(
            &initiatives.board.id,
            &UpdateBoardRequest {
                parent_gate: Some(false),
                ..Default::default()
            },
        )
        .await
        .expect_err("not a delivery board");
    assert_eq!(err.code(), Some("VALIDATION"), "{err:?}");

    // The activity log names each change of the policy.
    let lines = diesel::sql_query(
        "SELECT COUNT(*) AS n FROM org_acme.activity_log WHERE details LIKE 'parent_gate:%'",
    )
    .get_result::<Count>(&mut conn)
    .expect("count")
    .n;
    assert_eq!(lines, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_initiative_moves_to_active_only_with_an_owner() {
    let _admin_conn = recreate_scratch_db(OWNER_SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), OWNER_SCRATCH_DB);
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
    let _ = admin.whoami().await;
    let _ = alice.whoami().await;
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
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
    let initiatives = admin.get_board("initiatives").await.expect("board");
    // The policy is on for an initiative board, and null elsewhere.
    assert_eq!(initiatives.board.owner_gate, Some(true));
    let strategies = admin.get_board("strategy").await.expect("board");
    assert_eq!(strategies.board.owner_gate, None);
    // alice and svc are members of the initiative board: each can be its
    // owner.
    for email in ["alice@kairos.test", "svc@kairos.test"] {
        let id = user_id(&mut conn, email);
        admin
            .add_board_member(
                &initiatives.board.id,
                &kairos_client::types_org::AddBoardMemberRequest {
                    user_id: id.to_string(),
                    capabilities: vec!["transition_items".into()],
                },
            )
            .await
            .expect("a member of the board");
    }
    let active = initiatives
        .columns
        .iter()
        .find(|c| c.name == "Active")
        .unwrap()
        .id
        .clone();

    // --- An initiative starts in the entry column (Dylan, 2026-10-17).
    let create_in = |column: &str| kairos_client::types::CreateInitiativeRequest {
        board_id: "initiatives".into(),
        column_id: Some(column.into()),
        title: "Straight to Active".into(),
        content: String::new(),
        complexity: None,
        bucket_type: None,
        parent: None,
    };
    let err = admin
        .create_initiative(&create_in(&active))
        .await
        .expect_err("not the entry column");
    assert_eq!(err.code(), Some("VALIDATION"), "{err:?}");
    assert!(err.to_string().contains("Discovery"), "{err}");
    let discovery = initiatives.columns[0].id.clone();
    admin
        .create_initiative(&create_in(&discovery))
        .await
        .expect("the entry column");

    let mut mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let mut alice_mcp = Mcp::connect(&server.base_url, &alice_token).await;
    let initiative = mcp
        .create(json!({"item_type": "initiative", "board": "initiatives", "title": "Owned work"}))
        .await;
    mcp.walk(&initiative, &["Design", "Ready", "Decompose"])
        .await;

    // --- No owner: the move to Active is refused, over MCP and REST alike.
    let text = mcp
        .refused(
            "transition_item",
            json!({"short_code": initiative, "to_column": "Active"}),
            "OWNER_REQUIRED",
        )
        .await;
    assert!(
        text.contains(&initiative) && text.contains("claim_item"),
        "{text}"
    );
    let err = admin
        .transition_initiative(&initiative, &active)
        .await
        .expect_err("refused");
    assert_eq!(err.code(), Some("OWNER_REQUIRED"), "{err:?}");
    assert_eq!(err.status(), Some(422), "{err:?}");

    // --- With an owner, the move passes, and on to Monitoring.
    alice_mcp
        .ok("claim_item", json!({"short_code": initiative}))
        .await;
    mcp.walk(&initiative, &["Active"]).await;

    // --- The owner cannot release an active initiative: a hand-off can
    // change the owner. An org admin cannot clear it either.
    let text = alice_mcp
        .refused(
            "release_item",
            json!({"short_code": initiative}),
            "OWNER_REQUIRED",
        )
        .await;
    assert!(text.contains("hand_off_item"), "{text}");
    mcp.refused(
        "release_item",
        json!({"short_code": initiative}),
        "OWNER_REQUIRED",
    )
    .await;
    alice_mcp
        .ok(
            "hand_off_item",
            json!({"short_code": initiative, "to": "svc@kairos.test"}),
        )
        .await;
    mcp.walk(&initiative, &["Monitoring"]).await;
    mcp.refused(
        "release_item",
        json!({"short_code": initiative}),
        "OWNER_REQUIRED",
    )
    .await;
    // In Completed the release passes.
    mcp.walk(&initiative, &["Completed"]).await;
    mcp.ok("release_item", json!({"short_code": initiative}))
        .await;

    // --- my_boards shows the policy.
    let boards = mcp.ok("my_boards", json!({})).await;
    assert!(boards.contains("policy: owner_gate on"), "{boards}");

    // --- The policy off (MCP): the move and the release pass.
    let text = mcp
        .ok(
            "set_board_policy",
            json!({"board": "initiatives", "owner_gate": false}),
        )
        .await;
    assert!(text.contains("owner_gate off"), "{text}");
    let free = mcp
        .create(json!({"item_type": "initiative", "board": "initiatives", "title": "Free work"}))
        .await;
    mcp.walk(&free, &["Design", "Ready", "Decompose", "Active"])
        .await;
    alice_mcp
        .ok("claim_item", json!({"short_code": free}))
        .await;
    alice_mcp
        .ok("release_item", json!({"short_code": free}))
        .await;

    // --- REST sets it on again; a policy of a different level is refused.
    let on = admin
        .update_board(
            &initiatives.board.id,
            &UpdateBoardRequest {
                owner_gate: Some(true),
                ..Default::default()
            },
        )
        .await
        .expect("policy on");
    assert_eq!(on.owner_gate, Some(true));
    let err = admin
        .update_board(
            &strategies.board.id,
            &UpdateBoardRequest {
                owner_gate: Some(false),
                ..Default::default()
            },
        )
        .await
        .expect_err("not an initiative board");
    assert_eq!(err.code(), Some("VALIDATION"), "{err:?}");

    // The initiative in Active with no owner stays there: moves only.
    let still = admin.get_initiative(&free).await.expect("free");
    assert_eq!(still.column_id, active);
    assert!(still.owner.is_none());

    #[derive(diesel::QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let lines = diesel::sql_query(
        "SELECT COUNT(*) AS n FROM org_acme.activity_log WHERE details LIKE 'owner_gate:%'",
    )
    .get_result::<Count>(&mut conn)
    .expect("count")
    .n;
    assert_eq!(lines, 2);

    // --- KAIROS-T-0676: `claim: true` takes the owner in the move.
    let claimed = mcp
        .create(json!({"item_type": "initiative", "board": "initiatives", "title": "Claimed in the move"}))
        .await;
    alice_mcp
        .walk(&claimed, &["Design", "Ready", "Decompose"])
        .await;
    // A refused move keeps no owner.
    alice_mcp
        .refused(
            "transition_item",
            json!({"short_code": claimed, "to_column": "Completed", "claim": true}),
            "INVALID_TRANSITION",
        )
        .await;
    assert!(
        admin
            .get_initiative(&claimed)
            .await
            .expect("claimed")
            .owner
            .is_none(),
        "a refused move keeps no owner"
    );
    let text = alice_mcp
        .ok(
            "transition_item",
            json!({"short_code": claimed, "to_column": "Active", "claim": true}),
        )
        .await;
    assert!(
        text.contains(&format!("Transitioned {claimed}: Decompose -> Active.")),
        "{text}"
    );
    assert!(text.contains("- owner: "), "{text}");
    let got = admin.get_initiative(&claimed).await.expect("claimed");
    assert_eq!(got.column_id, active);
    assert_eq!(
        got.owner.as_ref().map(|o| o.user_id.clone()),
        Some(user_id(&mut conn, "alice@kairos.test").to_string())
    );
    // The owner sends `claim: true` again: the move passes, no new claim.
    alice_mcp
        .ok(
            "transition_item",
            json!({"short_code": claimed, "to_column": "Monitoring", "claim": true}),
        )
        .await;
    // A different person: the claim is refused, and the move with it.
    mcp.refused(
        "transition_item",
        json!({"short_code": claimed, "to_column": "Completed", "claim": true}),
        "CONFLICT",
    )
    .await;
    let monitoring = initiatives
        .columns
        .iter()
        .find(|c| c.name == "Monitoring")
        .unwrap()
        .id
        .clone();
    assert_eq!(
        admin.get_initiative(&claimed).await.unwrap().column_id,
        monitoring
    );
    // A strategy takes no `claim` in a move.
    let strategy = mcp
        .create(json!({"item_type": "strategy", "board": "strategy", "title": "Plan"}))
        .await;
    let text = mcp
        .refused(
            "transition_item",
            json!({"short_code": strategy, "to_column": strategies.columns[1].name, "claim": true}),
            "VALIDATION",
        )
        .await;
    assert!(text.contains("claim_item"), "{text}");
    // The description states the initiative case.
    mcp.next_id += 1;
    let id = mcp.next_id;
    let (_, body) = mcp
        .post(json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}))
        .await;
    let tools = rpc_message(&body)["result"]["tools"].clone();
    let tool = tools
        .as_array()
        .expect("tools")
        .iter()
        .find(|tool| tool["name"] == "transition_item")
        .expect("transition_item")
        .clone();
    let description = tool["description"].as_str().expect("description");
    assert!(
        description.contains("A move of an INITIATIVE does not set its owner")
            && description.contains("`claim: true`"),
        "{description}"
    );
    assert!(
        tool["inputSchema"]["properties"]["claim"].is_object(),
        "{tool}"
    );
}

/// The values of the header `Kairos-Warning` of a REST transition, and the
/// status.
async fn rest_transition(
    base_url: &str,
    token: &str,
    code: &str,
    column_id: &str,
) -> (reqwest::StatusCode, Vec<String>, Value) {
    let response = reqwest::Client::new()
        .post(format!("{base_url}/api/tasks/{code}/transition"))
        .bearer_auth(token)
        .header("x-tenant", "acme")
        .json(&json!({ "to_column_id": column_id }))
        .send()
        .await
        .expect("transition");
    let status = response.status();
    let warnings = response
        .headers()
        .get_all("kairos-warning")
        .iter()
        .map(|v| v.to_str().expect("header").to_string())
        .collect();
    (status, warnings, response.json().await.expect("body"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_move_of_a_blocked_task_warns_and_the_gate_refuses_it() {
    let _admin_conn = recreate_scratch_db(BLOCKERS_SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), BLOCKERS_SCRATCH_DB);
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
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let admin = server.client(&svc_token, "acme");
    let _ = admin.whoami().await;
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: user_id(&mut conn, "svc@kairos.test"),
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");
    for (name, slug, prefix) in [("Web", "web", "WEB"), ("Ops", "ops", "OPS")] {
        admin
            .create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                code_prefix: prefix.into(),
                team_type: None,
            })
            .await
            .expect("team");
    }
    let web = admin.get_board("web-delivery").await.expect("board");
    let column = |name: &str| {
        web.columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("column {name}"))
            .id
            .clone()
    };
    // The policy is off for a new delivery board, and null elsewhere.
    assert_eq!(web.board.blockers_gate, Some(false));
    let initiatives = admin.get_board("initiatives").await.expect("board");
    assert_eq!(initiatives.board.blockers_gate, None);

    let mut mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let task = |title: &str| json!({"item_type": "task", "board": "web-delivery", "title": title});
    let first = mcp.create(task("First blocker")).await;
    let second = mcp.create(task("Second blocker")).await;
    let work = mcp.create(task("Blocked work")).await;
    for blocker in [&first, &second] {
        mcp.ok(
            "link_items",
            json!({"source": blocker, "target": work, "relationship": "blocks"}),
        )
        .await;
    }

    // --- Policy off: a move into Todo gives no warning. A move into a
    // claims column names each open blocker.
    let text = mcp
        .ok(
            "transition_item",
            json!({"short_code": work, "to_column": "Todo"}),
        )
        .await;
    assert!(!text.contains("blocked by"), "{text}");
    let text = mcp
        .ok(
            "transition_item",
            json!({"short_code": work, "to_column": "Active"}),
        )
        .await;
    assert!(
        text.contains(&format!(
            "Warning: {work} is blocked by {first} (open), {second} (open). Your move into \
             Active is recorded."
        )),
        "{text}"
    );

    // --- A cancelled blocker is open. A move into a done column over
    // REST gets the header.
    mcp.ok(
        "cancel_item",
        json!({"short_code": second, "reason": "Not needed now"}),
    )
    .await;
    let (status, warnings, _) =
        rest_transition(&server.base_url, &svc_token, &work, &column("Completed")).await;
    assert!(status.is_success(), "{status}");
    assert_eq!(
        warnings,
        vec![format!(
            "{work} moved into Completed, blocked by {first} (open), {second} (cancelled)"
        )]
    );

    // --- Policy on (MCP): a reopen into a claims column is refused.
    let backlog = column("Backlog");
    let claims = |on: bool| UpdateColumnRequest {
        name: None,
        position: None,
        is_done: None,
        claims: Some(on),
        keeps_claims: None,
        wip_limit: None,
    };
    admin
        .update_column(&web.board.id, &backlog, &claims(true))
        .await
        .expect("claims on Backlog");
    let text = mcp
        .ok(
            "set_board_policy",
            json!({"board": "web-delivery", "blockers_gate": true}),
        )
        .await;
    assert!(text.contains("blockers_gate on"), "{text}");
    let boards = mcp.ok("my_boards", json!({})).await;
    assert!(boards.contains("blockers_gate on"), "{boards}");
    let text = mcp
        .refused("reopen_item", json!({"short_code": work}), "BLOCKED")
        .await;
    assert!(
        text.contains(&format!("{first} (open), {second} (cancelled)")),
        "{text}"
    );
    assert_eq!(
        admin.get_task(&work).await.unwrap().column_id,
        column("Completed")
    );

    // --- Policy off (REST): the reopen passes with the warning.
    let off = admin
        .update_board(
            &web.board.id,
            &UpdateBoardRequest {
                blockers_gate: Some(false),
                ..Default::default()
            },
        )
        .await
        .expect("policy off");
    assert_eq!(off.blockers_gate, Some(false));
    let text = mcp.ok("reopen_item", json!({"short_code": work})).await;
    assert!(
        text.contains(&format!("Warning: {work} is blocked by {first} (open)")),
        "{text}"
    );
    admin
        .update_column(&web.board.id, &backlog, &claims(false))
        .await
        .expect("claims off Backlog");

    // --- Policy on: a transition into Active is refused, over MCP and
    // REST. A move into Todo passes.
    admin
        .update_board(
            &web.board.id,
            &UpdateBoardRequest {
                blockers_gate: Some(true),
                ..Default::default()
            },
        )
        .await
        .expect("policy on");
    mcp.walk(&work, &["Todo"]).await;
    let text = mcp
        .refused(
            "transition_item",
            json!({"short_code": work, "to_column": "Active"}),
            "BLOCKED",
        )
        .await;
    assert!(text.contains("unlink_items"), "{text}");
    let err = admin
        .transition_task(&work, &column("Active"))
        .await
        .expect_err("refused");
    assert_eq!(err.code(), Some("BLOCKED"), "{err:?}");
    assert_eq!(err.status(), Some(422), "{err:?}");
    let details = err.details().expect("details");
    assert_eq!(details["policy"], "blockers_gate");
    assert_eq!(
        details["blockers"],
        json!([
            {"short_code": first, "cancelled": false},
            {"short_code": second, "cancelled": true},
        ])
    );
    assert_eq!(
        admin.get_task(&work).await.unwrap().column_id,
        column("Todo")
    );

    // --- A board move into a claims entry column: the policy of the
    // target board decides.
    let ops = admin.get_board("ops-delivery").await.expect("ops board");
    let ops_backlog = ops
        .columns
        .iter()
        .find(|c| c.name == "Backlog")
        .unwrap()
        .id
        .clone();
    admin
        .update_column(&ops.board.id, &ops_backlog, &claims(true))
        .await
        .expect("claims on the entry column");
    mcp.ok(
        "set_board_policy",
        json!({"board": "ops-delivery", "blockers_gate": true}),
    )
    .await;
    mcp.refused(
        "move_item",
        json!({"short_code": work, "to_board": "ops-delivery"}),
        "BLOCKED",
    )
    .await;
    assert_eq!(admin.get_task(&work).await.unwrap().board_id, web.board.id);
    mcp.ok(
        "set_board_policy",
        json!({"board": "ops-delivery", "blockers_gate": false}),
    )
    .await;
    let text = mcp
        .ok(
            "move_item",
            json!({"short_code": work, "to_board": "ops-delivery"}),
        )
        .await;
    assert!(
        text.contains(&format!(
            "Warning: {work} is blocked by {first} (open), {second} (cancelled). Your move \
             into Backlog is recorded."
        )),
        "{text}"
    );

    // --- With no open blocker, the gate lets the task pass, with no
    // warning.
    mcp.ok(
        "set_board_policy",
        json!({"board": "ops-delivery", "blockers_gate": true}),
    )
    .await;
    for blocker in [&first, &second] {
        mcp.ok(
            "unlink_items",
            json!({"source": blocker, "target": work, "relationship": "blocks"}),
        )
        .await;
    }
    let text = mcp
        .ok(
            "transition_item",
            json!({"short_code": work, "to_column": "Todo"}),
        )
        .await;
    assert!(!text.contains("blocked by"), "{text}");
    let text = mcp
        .ok(
            "transition_item",
            json!({"short_code": work, "to_column": "Active"}),
        )
        .await;
    assert!(!text.contains("blocked by"), "{text}");

    // --- A policy of a different level is refused.
    mcp.refused(
        "set_board_policy",
        json!({"board": "initiatives", "blockers_gate": true}),
        "blockers_gate",
    )
    .await;
    let err = admin
        .update_board(
            &initiatives.board.id,
            &UpdateBoardRequest {
                blockers_gate: Some(true),
                ..Default::default()
            },
        )
        .await
        .expect_err("not a delivery board");
    assert_eq!(err.code(), Some("VALIDATION"), "{err:?}");

    // The activity log names each change of the policy: web on, off, on;
    // ops on, off, on.
    #[derive(diesel::QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let lines = diesel::sql_query(
        "SELECT COUNT(*) AS n FROM org_acme.activity_log WHERE details LIKE 'blockers_gate:%'",
    )
    .get_result::<Count>(&mut conn)
    .expect("count")
    .n;
    assert_eq!(lines, 6);
}
