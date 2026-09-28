//! Integration test for COLLIERY-T-0228: the EDIT rule and the LINK rule.
//!
//! The edit rule. A principal may edit an item when one of these is true:
//!
//! 1. the principal created the item,
//! 2. the principal holds `manage_<type>` on the authorization board of the
//!    item,
//! 3. the principal is an organization admin.
//!
//! An edit is: title and content, metadata, the repository of a task, the
//! lifecycle of a document, archive, restore. A MOVE is not an edit:
//! `transition`, `work-class` and `move` keep the capability check, and
//! creation grants nothing there (the rule of COLLIERY-T-0218: a team
//! controls its own plan).
//!
//! The link rule. A principal may write an edge of ANY relationship type
//! when they may edit the item at EITHER end. No relationship type needs
//! the organization admin role.
//!
//! This is a security boundary, so the negative cases are the contract.
//! Each refusal is checked where the data is stored: the row is read after
//! the refusal and must be as it was.
//!
//! The cases are numbered as in the work item. Every check is recorded and
//! the test fails at the end with the full list, so one run shows each
//! broken case and not only the first.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_edit_rule_t0228_test`.
//!
//! Cast:
//! - `svc`   — org admin: sets the world up,
//! - `alice` — member of team `web`; NO grant on the board of `platform`,
//! - `bob`   — member of team `platform`; later `manage_adrs` on the ADR
//!   board,
//! - `carol` — org member in no team, with no grant; she creates nothing,
//! - `maker` and `other` — two service accounts, both org members,
//! - `globex-alice` — alice in ANOTHER tenant.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{
    CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest, CreateStrategyRequest,
    CreateTaskRequest, UpdateContentRequest,
};
use kairos_client::types_meta::{
    CreateMetadataDefinitionRequest, CreateRelationshipRequest, UpdateMetadataRequest,
};
use kairos_client::types_org::{
    AddBoardMemberRequest, AddTeamMemberRequest, CreateColumnRequest, CreateTeamRequest,
};
use kairos_client::{EntityKind, Error, KairosClient};
use kairos_db::api_keys::{self, NewApiKey};
use kairos_db::models::enums::Forge;
use kairos_db::models::public::{NewServiceAccountUser, User};
use kairos_db::models::repositories::NewRepository;
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, repositories, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::service_accounts::auth::{generate_key, hash_key};

const SCRATCH_DB: &str = "kairos_edit_rule_t0228_test";

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
}

fn org_id(conn: &mut PgConnection, slug: &str) -> Uuid {
    organizations::table
        .filter(organizations::slug.eq(slug))
        .select(organizations::id)
        .first(conn)
        .expect("org row")
}

fn add_member(conn: &mut PgConnection, org: Uuid, user: Uuid, role: OrgRole) {
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org,
            user_id: user,
            role,
        })
        .execute(conn)
        .expect("granting membership");
}

fn board_of_level(conn: &mut PgConnection, level: BoardLevel) -> String {
    use kairos_db::schema::boards::dsl;
    let id: Uuid = dsl::boards
        .filter(dsl::board_level.eq(level))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("no {level:?} board: {e}"));
    id.to_string()
}

/// The number of edges in the tenant: what a refused link must not change.
fn edge_count(conn: &mut PgConnection) -> i64 {
    use kairos_db::schema::item_relationships::dsl;
    dsl::item_relationships
        .count()
        .get_result(conn)
        .expect("counting edges")
}

/// A service account that is a member of `org`, and a client for its key.
fn service_account(
    conn: &mut PgConnection,
    org: Uuid,
    created_by: Uuid,
    name: &str,
    base_url: &str,
) -> (Uuid, KairosClient, String) {
    let account: User = diesel::insert_into(users::table)
        .values(NewServiceAccountUser::new(
            format!("svc:{name}"),
            format!("{name}@svc.acme.kairos"),
            name,
        ))
        .returning(User::as_returning())
        .get_result(conn)
        .expect("service account");
    add_member(conn, org, account.id, OrgRole::Member);
    let raw_key = generate_key("acme");
    api_keys::create_key(
        conn,
        NewApiKey {
            user_id: account.id,
            name: name.into(),
            token_hash: hash_key(&raw_key),
            prefix: name.into(),
            created_by,
            expires_at: None,
        },
    )
    .expect("key");
    let client = KairosClient::with_static_token(base_url, &raw_key).with_tenant("acme");
    (account.id, client, raw_key)
}

/// Every check of the run. A failed check is recorded, and the test goes
/// on, so that one run reports each broken case.
#[derive(Default)]
struct Checks {
    passed: usize,
    failures: Vec<String>,
}

impl Checks {
    fn check(&mut self, case: &str, holds: bool, detail: impl std::fmt::Display) {
        if holds {
            self.passed += 1;
        } else {
            self.failures.push(format!("{case}: {detail}"));
        }
    }

    /// The call must succeed.
    fn allowed<T>(&mut self, case: &str, result: Result<T, Error>) -> Option<T> {
        match result {
            Ok(value) => {
                self.passed += 1;
                Some(value)
            }
            Err(err) => {
                self.failures
                    .push(format!("{case}: expected success, got {err}"));
                None
            }
        }
    }

    /// The call must be refused with 403 `FORBIDDEN`. Returns the name of
    /// the capability the refusal gave, if it gave one.
    fn refused<T: std::fmt::Debug>(
        &mut self,
        case: &str,
        result: Result<T, Error>,
    ) -> Option<String> {
        match result {
            Err(Error::Forbidden {
                code, capability, ..
            }) if code == "FORBIDDEN" => {
                self.passed += 1;
                capability
            }
            Err(err) => {
                self.failures
                    .push(format!("{case}: expected 403 FORBIDDEN, got {err}"));
                None
            }
            Ok(value) => {
                self.failures.push(format!(
                    "{case}: expected 403 FORBIDDEN, but the call SUCCEEDED: {value:?}"
                ));
                None
            }
        }
    }

    /// The MCP call must succeed.
    fn mcp_allowed(&mut self, case: &str, (is_error, text): &(bool, String)) {
        self.check(case, !is_error, format!("expected success, got {text}"));
    }

    /// The MCP call must be refused with `FORBIDDEN`.
    fn mcp_refused(&mut self, case: &str, (is_error, text): &(bool, String)) {
        self.check(
            case,
            *is_error && text.contains("FORBIDDEN"),
            format!("expected FORBIDDEN, got is_error={is_error}: {text}"),
        );
    }

    fn finish(self) {
        assert!(
            self.failures.is_empty(),
            "{} of {} checks FAILED:\n  - {}",
            self.failures.len(),
            self.failures.len() + self.passed,
            self.failures.join("\n  - ")
        );
        eprintln!("edit_rule: {} checks passed", self.passed);
    }
}

/// Minimal MCP session over HTTP (initialize → initialized → tools/call).
struct McpSession {
    http: reqwest::Client,
    url: String,
    token: String,
    tenant: String,
    session_id: String,
    next_id: u64,
}

impl McpSession {
    async fn open(base_url: &str, token: &str, tenant: &str) -> Self {
        let http = reqwest::Client::new();
        let url = format!("{base_url}/mcp");
        let response = http
            .post(&url)
            .bearer_auth(token)
            .header("X-Tenant", tenant)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "kairos-t0228-test", "version": "0.0.0"},
                },
            }))
            .send()
            .await
            .expect("initialize");
        assert_eq!(response.status(), 200, "initialize failed");
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .expect("session id")
            .to_string();
        let session = Self {
            http,
            url,
            token: token.to_string(),
            tenant: tenant.to_string(),
            session_id,
            next_id: 0,
        };
        let response = session
            .http
            .post(&session.url)
            .bearer_auth(&session.token)
            .header("X-Tenant", &session.tenant)
            .header("mcp-session-id", &session.session_id)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .send()
            .await
            .expect("initialized");
        assert_eq!(response.status(), 202, "initialized notify");
        session
    }

    /// Call a tool; returns `(is_error, text)`.
    async fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        self.next_id += 1;
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("X-Tenant", &self.tenant)
            .header("mcp-session-id", &self.session_id)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }))
            .send()
            .await
            .expect("tools/call");
        assert_eq!(response.status(), 200, "tools/call HTTP status");
        let body = response.text().await.expect("body");
        let message: Value = serde_json::from_str::<Value>(&body)
            .ok()
            .filter(|v| v.get("jsonrpc").is_some())
            .or_else(|| {
                body.lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
                    .find(|v| v.get("jsonrpc").is_some())
            })
            .unwrap_or_else(|| panic!("no JSON-RPC message in response body: {body:?}"));
        let result = &message["result"];
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content: {message}"))
            .to_string();
        (is_error, text)
    }
}

/// The first short code with `prefix` in an MCP reply.
fn short_code_in(text: &str, prefix: &str) -> String {
    let start = text
        .find(prefix)
        .unwrap_or_else(|| panic!("no {prefix} short code in: {text}"));
    text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

fn edge(source: &str, target: &str, relationship: &str) -> CreateRelationshipRequest {
    CreateRelationshipRequest {
        source_short_code: source.into(),
        target_short_code: target.into(),
        relationship: relationship.into(),
    }
}

fn content(title: Option<&str>, content: &str, version: i32) -> UpdateContentRequest {
    UpdateContentRequest {
        title: title.map(str::to_string),
        content: content.into(),
        version,
    }
}

fn metadata(value: Option<&str>) -> UpdateMetadataRequest {
    UpdateMetadataRequest {
        values: [("urgency".to_string(), value.map(str::to_string))]
            .into_iter()
            .collect(),
    }
}

fn task_on(board: &str, title: &str) -> CreateTaskRequest {
    CreateTaskRequest {
        board_id: Some(board.into()),
        column_id: None,
        title: title.into(),
        content: "original content".into(),
        task_type: None,
        work_class: None,
        team_id: None,
        repository: None,
    }
}

#[tokio::test]
async fn edit_rule_and_link_rule_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    provision_tenant(&mut conn, "globex", "Globex").expect("provisioning globex");
    let acme = org_id(&mut conn, "acme");
    let globex = org_id(&mut conn, "globex");

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
    let router = app::router(app::state_with(
        base_config(&scratch_url),
        pool.clone(),
        auth.clone(),
    ));
    let server = spawn_server(router).await;
    let svc = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let carol = server.client(&carol_token, "acme");
    let globex_alice = server.client(&alice_token, "globex");

    for client in [&svc, &alice, &bob, &carol] {
        let _ = client.whoami().await;
    }
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    let alice_id = user_id(&mut conn, "alice@kairos.test");
    let bob_id = user_id(&mut conn, "bob@kairos.test");
    let carol_id = user_id(&mut conn, "carol@kairos.test");
    add_member(&mut conn, acme, svc_id, OrgRole::Admin);
    add_member(&mut conn, acme, alice_id, OrgRole::Member);
    add_member(&mut conn, acme, bob_id, OrgRole::Member);
    add_member(&mut conn, acme, carol_id, OrgRole::Member);
    add_member(&mut conn, globex, alice_id, OrgRole::Member);

    // Three teams. `ops` has no member of the cast: its board is where an
    // item goes to prove that the right of a creator survives a move.
    let mut teams = Vec::new();
    for (name, slug) in [("Platform", "platform"), ("Web", "web"), ("Ops", "ops")] {
        teams.push(
            svc.create_team(&CreateTeamRequest {
                name: name.into(),
                slug: slug.into(),
                team_type: None,
            })
            .await
            .expect("team"),
        );
    }
    let platform_board = teams[0].delivery_board_id.clone().expect("platform board");
    let web_board = teams[1].delivery_board_id.clone().expect("web board");
    let ops_board = teams[2].delivery_board_id.clone().expect("ops board");
    for (team, member) in [(&teams[0], bob_id), (&teams[1], alice_id)] {
        svc.add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: member.to_string(),
            },
        )
        .await
        .expect("team member");
    }

    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    repositories::create(
        &mut conn,
        NewRepository {
            slug: "payments-api".into(),
            forge: Forge::Github,
            repo_full_name: "acme/payments-api".into(),
            repo_url: "https://github.com/acme/payments-api".into(),
            default_branch: "main".into(),
            team_id: teams[0].id.parse().expect("uuid"),
            description: String::new(),
            created_by: svc_id,
            updated_by: svc_id,
        },
    )
    .expect("payments repo");
    let strategy_board = board_of_level(&mut conn, BoardLevel::Strategy);
    let initiative_board = board_of_level(&mut conn, BoardLevel::Initiative);
    let adr_board = board_of_level(&mut conn, BoardLevel::Adr);
    let columns = svc.list_columns(&platform_board).await.expect("columns");
    let backlog = columns
        .iter()
        .find(|c| c.position == 0)
        .expect("entry column")
        .id
        .clone();
    let todo = columns
        .iter()
        .find(|c| c.position == 1)
        .expect("second column")
        .id
        .clone();
    svc.create_metadata_definition(&CreateMetadataDefinitionRequest {
        name: "Urgency".into(),
        slug: "urgency".into(),
        field_type: "string".into(),
        enum_options: vec![],
        entity_types: vec![],
    })
    .await
    .expect("metadata definition");

    let (maker_id, maker, _maker_key) =
        service_account(&mut conn, acme, svc_id, "maker", &server.base_url);
    let (_other_id, other, other_key) =
        service_account(&mut conn, acme, svc_id, "other", &server.base_url);

    // The items of the ORGANIZATION, all made by the admin: nobody else in
    // the cast created them, and nobody else holds a grant on their boards.
    let strategy = svc
        .create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.clone(),
            column_id: None,
            title: "The strategy".into(),
            content: "original content".into(),
            hypothesis: None,
        })
        .await
        .expect("strategy");
    let initiative = svc
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.clone(),
            column_id: None,
            title: "The initiative of the organization".into(),
            content: "original content".into(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("initiative");
    let adr_request = |title: &str| CreateAdrRequest {
        board_id: Some(adr_board.clone()),
        column_id: None,
        title: title.into(),
        content: "original content".into(),
        decision_maker: None,
        decision_date: None,
    };
    let adr_one = svc
        .create_adr(&adr_request("ADR one"))
        .await
        .expect("adr one");
    let adr_two = svc
        .create_adr(&adr_request("ADR two"))
        .await
        .expect("adr two");
    let adr_three = svc
        .create_adr(&adr_request("ADR three"))
        .await
        .expect("adr three");
    let bob_task = bob
        .create_task(&task_on(&platform_board, "The task of bob"))
        .await
        .expect("bob creates on his board");
    let bob_second = bob
        .create_task(&task_on(&platform_board, "The second task of bob"))
        .await
        .expect("bob creates on his board");
    let document = svc
        .create_document(&CreateDocumentRequest {
            title: "The document of the organization".into(),
            content: Some("original content".into()),
            template_id: None,
            parent_short_code: Some(bob_task.short_code.clone()),
        })
        .await
        .expect("document");
    let archived = svc
        .create_task(&task_on(&platform_board, "A task that is archived"))
        .await
        .expect("task");
    svc.delete_task(&archived.short_code)
        .await
        .expect("archive");

    let mut checks = Checks::default();
    let mut alice_mcp = McpSession::open(&server.base_url, &alice_token, "acme").await;
    let mut bob_mcp = McpSession::open(&server.base_url, &bob_token, "acme").await;
    let mut carol_mcp = McpSession::open(&server.base_url, &carol_token, "acme").await;

    // =======================================================================
    // Case 1. alice sends a request to platform and then edits it
    // (criterion i: the creator edits with no capability on the board)
    // =======================================================================
    let request = alice
        .create_task(&task_on(&platform_board, "The request of alice"))
        .await
        .expect("alice sends a request to platform (COLLIERY-T-0218)");
    assert_eq!(request.column_id, backlog);
    assert_eq!(request.work_class, "support");
    assert_eq!(request.created_by, alice_id.to_string());
    let code = request.short_code.clone();

    let edited = checks.allowed(
        "1 REST: the creator changes title and content",
        alice
            .update_task(
                &code,
                &content(Some("A new title"), "new content", request.version),
            )
            .await,
    );
    let stored = svc.get_task(&code).await.expect("read");
    checks.check(
        "1 REST: the edit is stored",
        stored.title == "A new title" && stored.content == "new content",
        format!("{} / {}", stored.title, stored.content),
    );
    checks.check(
        "1 REST: an edit does not move the request",
        stored.column_id == backlog
            && stored.work_class == "support"
            && stored.board_id == platform_board,
        format!("{stored:?}"),
    );
    let _ = edited;
    checks.allowed(
        "1 REST: the creator sets metadata",
        alice
            .update_metadata(EntityKind::Task, &code, &metadata(Some("high")))
            .await,
    );
    let values = svc
        .metadata(EntityKind::Task, &code)
        .await
        .expect("metadata");
    checks.check(
        "1 REST: the metadata is stored",
        values
            .values
            .iter()
            .any(|v| v.slug == "urgency" && v.value == "high"),
        format!("{values:?}"),
    );
    let bound = checks.allowed(
        "1 REST: the creator sets the repository",
        alice.set_task_repository(&code, Some("payments-api")).await,
    );
    checks.check(
        "1 REST: the repository is stored",
        bound
            .and_then(|t| t.repository)
            .is_some_and(|r| r.slug == "payments-api"),
        "no repository on the task",
    );
    let cleared = checks.allowed(
        "1 REST: the creator clears the repository",
        alice.set_task_repository(&code, None).await,
    );
    checks.check(
        "1 REST: the repository is cleared",
        cleared.is_some_and(|t| t.repository.is_none()),
        "the repository stayed",
    );
    checks.allowed(
        "1 REST: the creator archives",
        alice.delete_task(&code).await,
    );
    checks.check(
        "1 REST: the request is archived",
        svc.get_task(&code)
            .await
            .expect("read")
            .archived_at
            .is_some(),
        "archived_at is empty",
    );
    checks.allowed(
        "1 REST: the creator restores",
        alice.restore_task(&code).await,
    );
    checks.check(
        "1 REST: the request is live again, where it was",
        {
            let task = svc.get_task(&code).await.expect("read");
            task.archived_at.is_none() && task.column_id == backlog
        },
        "still archived, or moved",
    );

    // The same over MCP, on a request that alice sends over MCP.
    let reply = alice_mcp
        .call(
            "create_item",
            json!({"item_type": "task", "title": "The MCP request of alice",
                   "content": "original content", "board": platform_board}),
        )
        .await;
    assert!(!reply.0, "alice sends a request over MCP: {}", reply.1);
    let mcp_code = short_code_in(&reply.1, "ACME-T-");
    let reply = alice_mcp
        .call(
            "update_item",
            json!({"short_code": mcp_code, "title": "A new MCP title",
                   "content": "new content here", "version": 1}),
        )
        .await;
    checks.mcp_allowed("1 MCP update_item: the creator", &reply);
    let reply = alice_mcp
        .call(
            "edit_item",
            json!({"short_code": mcp_code, "search": "here", "replace": "there"}),
        )
        .await;
    checks.mcp_allowed("1 MCP edit_item: the creator", &reply);
    let stored = svc.get_task(&mcp_code).await.expect("read");
    checks.check(
        "1 MCP: the edits are stored",
        stored.title == "A new MCP title" && stored.content == "new content there",
        format!("{} / {}", stored.title, stored.content),
    );
    let reply = alice_mcp
        .call(
            "set_metadata",
            json!({"short_code": mcp_code, "values": {"urgency": "high"}}),
        )
        .await;
    checks.mcp_allowed("1 MCP set_metadata: the creator", &reply);
    let reply = alice_mcp
        .call(
            "set_repository",
            json!({"short_code": mcp_code, "repository": "payments-api"}),
        )
        .await;
    checks.mcp_allowed("1 MCP set_repository: the creator sets", &reply);
    let reply = alice_mcp
        .call("set_repository", json!({"short_code": mcp_code}))
        .await;
    checks.mcp_allowed("1 MCP set_repository: the creator clears", &reply);
    let reply = alice_mcp
        .call(
            "delete_item",
            json!({"short_code": mcp_code, "confirm": true}),
        )
        .await;
    checks.mcp_allowed("1 MCP delete_item: the creator", &reply);
    let reply = alice_mcp
        .call("restore_item", json!({"short_code": mcp_code}))
        .await;
    checks.mcp_allowed("1 MCP restore_item: the creator", &reply);
    checks.check(
        "1 MCP: the request is live, in the entry column, in the support lane",
        {
            let task = svc.get_task(&mcp_code).await.expect("read");
            task.archived_at.is_none() && task.column_id == backlog && task.work_class == "support"
        },
        "archived, or moved",
    );

    // =======================================================================
    // Case 2. alice links her request
    // =======================================================================
    // Her own initiative. She needs `manage_initiatives` to CREATE it (the
    // create gate does not change), so the admin gives the grant and takes
    // it away after the create. What is left is creation alone.
    svc.add_board_member(
        &initiative_board,
        &AddBoardMemberRequest {
            user_id: alice_id.to_string(),
            capabilities: vec!["manage_initiatives".into()],
        },
    )
    .await
    .expect("grant");
    let alice_initiative = alice
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.clone(),
            column_id: None,
            title: "The initiative of alice".into(),
            content: "original content".into(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("alice creates her initiative");
    svc.remove_board_member(&initiative_board, &alice_id.to_string())
        .await
        .expect("the grant is taken away");
    forbidden_now(
        alice
            .create_initiative(&CreateInitiativeRequest {
                board_id: initiative_board.clone(),
                column_id: None,
                title: "A second one".into(),
                content: String::new(),
                complexity: None,
                bucket_type: None,
            })
            .await,
        "alice holds no grant on the initiative board now",
    );
    let alice_own = alice
        .create_task(&task_on(&web_board, "The task of alice on her board"))
        .await
        .expect("alice creates on her board");

    checks.allowed(
        "2 REST: parent from her own initiative to her request",
        alice
            .create_relationship(&edge(&alice_initiative.short_code, &code, "parent"))
            .await,
    );
    let blocks = checks.allowed(
        "2 REST: blocks from her request to her own task",
        alice
            .create_relationship(&edge(&code, &alice_own.short_code, "blocks"))
            .await,
    );
    if let Some(blocks) = blocks {
        checks.allowed(
            "2 REST: she removes the edge that she made",
            alice.delete_relationship(&blocks.id).await,
        );
    }
    // Only the TARGET is hers: the source is bob's task, on a board where
    // she holds nothing. The old rule looked at the creator of the source
    // only.
    let target_only = checks.allowed(
        "2 REST: an edge where she can edit only the TARGET",
        alice
            .create_relationship(&edge(&bob_task.short_code, &code, "blocks"))
            .await,
    );
    if let Some(target_only) = target_only {
        checks.allowed(
            "2 REST: she removes an edge where she can edit only the TARGET",
            alice.delete_relationship(&target_only.id).await,
        );
    }
    // The edit rule reaches the initiative too, with no grant.
    checks.allowed(
        "2 REST: the creator edits her initiative with no grant",
        alice
            .update_initiative(
                &alice_initiative.short_code,
                &content(None, "new content", alice_initiative.version),
            )
            .await,
    );
    let reply = alice_mcp
        .call(
            "link_items",
            json!({"source": alice_initiative.short_code, "target": mcp_code,
                   "relationship": "parent"}),
        )
        .await;
    checks.mcp_allowed("2 MCP link_items: parent from her own initiative", &reply);
    let reply = alice_mcp
        .call(
            "link_items",
            json!({"source": bob_second.short_code, "target": mcp_code,
                   "relationship": "blocks"}),
        )
        .await;
    checks.mcp_allowed("2 MCP link_items: only the TARGET is hers", &reply);
    let reply = alice_mcp
        .call(
            "unlink_items",
            json!({"source": bob_second.short_code, "target": mcp_code,
                   "relationship": "blocks"}),
        )
        .await;
    checks.mcp_allowed("2 MCP unlink_items: only the TARGET is hers", &reply);

    // =======================================================================
    // Case 3. A member with `manage_adrs` creates an ADR with a parent that
    // he does not manage (criterion g)
    // =======================================================================
    svc.add_board_member(
        &adr_board,
        &AddBoardMemberRequest {
            user_id: bob_id.to_string(),
            capabilities: vec!["manage_adrs".into()],
        },
    )
    .await
    .expect("grant manage_adrs to bob");
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "adr", "title": "The ADR of bob",
                   "content": "original content", "parent": initiative.short_code}),
        )
        .await;
    checks.mcp_allowed(
        "3 MCP create_item: an ADR with a parent he does not manage",
        &reply,
    );
    if !reply.0 {
        let bob_adr = short_code_in(&reply.1, "ACME-A-");
        let edges = svc
            .relationships(EntityKind::Adr, &bob_adr)
            .await
            .expect("edges");
        checks.check(
            "3: the ADR has a supports edge from the initiative",
            edges.incoming.iter().any(|group| {
                group.relationship == "supports"
                    && group
                        .items
                        .iter()
                        .any(|item| item.short_code == initiative.short_code)
            }),
            format!("{edges:?}"),
        );
    }

    // =======================================================================
    // Case 4. A member who can edit an ADR links it, with no admin role
    // (criteria a, b, c)
    // =======================================================================
    // bob can edit the ADRs (`manage_adrs`). He can edit neither the
    // initiative nor the strategy.
    let supports = checks.allowed(
        "4 REST supports: he can edit the ADR (the target)",
        bob.create_relationship(&edge(
            &initiative.short_code,
            &adr_one.short_code,
            "supports",
        ))
        .await,
    );
    checks.allowed(
        "4b REST informs: he can edit the ADR (the source)",
        bob.create_relationship(&edge(
            &adr_one.short_code,
            &initiative.short_code,
            "informs",
        ))
        .await,
    );
    checks.allowed(
        "4c REST supersedes: he can edit the two ADRs",
        bob.create_relationship(&edge(
            &adr_two.short_code,
            &adr_one.short_code,
            "supersedes",
        ))
        .await,
    );
    if let Some(supports) = supports {
        checks.allowed(
            "4 REST: he removes the supports edge",
            bob.delete_relationship(&supports.id).await,
        );
    }
    for (relationship, source, target) in [
        ("supports", &strategy.short_code, &adr_two.short_code),
        ("informs", &adr_two.short_code, &strategy.short_code),
        ("supersedes", &adr_three.short_code, &adr_two.short_code),
    ] {
        let reply = bob_mcp
            .call(
                "link_items",
                json!({"source": source, "target": target, "relationship": relationship}),
            )
            .await;
        checks.mcp_allowed(&format!("4 MCP link_items {relationship}"), &reply);
    }
    let reply = bob_mcp
        .call(
            "unlink_items",
            json!({"source": adr_two.short_code, "target": strategy.short_code,
                   "relationship": "informs"}),
        )
        .await;
    checks.mcp_allowed("4 MCP unlink_items informs", &reply);
    // Criterion a, by creation and not by a grant: alice can edit her
    // initiative (she created it). She cannot edit the ADR.
    checks.allowed(
        "4a REST supports: she can edit the initiative (the source) only",
        alice
            .create_relationship(&edge(
                &alice_initiative.short_code,
                &adr_three.short_code,
                "supports",
            ))
            .await,
    );

    // =======================================================================
    // Case 5. The right of the creator survives a move to another board
    // =======================================================================
    let travels = alice
        .create_task(&task_on(&platform_board, "A request that is moved"))
        .await
        .expect("request");
    svc.move_task(&travels.short_code, &ops_board)
        .await
        .expect("the admin moves it to the board of ops");
    let moved = svc.get_task(&travels.short_code).await.expect("read");
    assert_eq!(moved.board_id, ops_board);
    checks.allowed(
        "5 REST: the creator edits after the move",
        alice
            .update_task(
                &travels.short_code,
                &content(None, "edited after the move", moved.version),
            )
            .await,
    );
    let reply = alice_mcp
        .call(
            "set_metadata",
            json!({"short_code": travels.short_code, "values": {"urgency": "low"}}),
        )
        .await;
    checks.mcp_allowed("5 MCP set_metadata: the creator, after the move", &reply);
    checks.refused(
        "5: carol cannot edit it on the new board either",
        carol
            .update_task(&travels.short_code, &content(None, "carol", 2))
            .await,
    );
    // A document follows the board of its parent. Its creator keeps the
    // right when the parent goes to a board where she holds nothing.
    let alice_parent = alice
        .create_task(&task_on(&web_board, "The parent of a document"))
        .await
        .expect("task");
    let alice_document = alice
        .create_document(&CreateDocumentRequest {
            title: "The document of alice".into(),
            content: Some("original content".into()),
            template_id: None,
            parent_short_code: Some(alice_parent.short_code.clone()),
        })
        .await
        .expect("alice creates a document on her board");
    svc.move_task(&alice_parent.short_code, &ops_board)
        .await
        .expect("the parent moves to the board of ops");
    checks.allowed(
        "5 REST: the creator edits her document after its parent moved",
        alice
            .update_document(
                &alice_document.short_code,
                &content(None, "new content", alice_document.version),
            )
            .await,
    );
    checks.allowed(
        "5 REST: the creator sets the lifecycle of her document",
        alice
            .set_document_lifecycle(&alice_document.short_code, "review")
            .await,
    );

    // =======================================================================
    // Case 6. An admin and a board manager can do what they could before
    // =======================================================================
    let managed = alice
        .create_task(&task_on(&platform_board, "A request that bob manages"))
        .await
        .expect("request");
    checks.allowed(
        "6: the manager edits a request that he did not create",
        bob.update_task(
            &managed.short_code,
            &content(None, "triaged", managed.version),
        )
        .await,
    );
    checks.allowed(
        "6: the manager sets metadata",
        bob.update_metadata(EntityKind::Task, &managed.short_code, &metadata(Some("x")))
            .await,
    );
    checks.allowed(
        "6: the manager sets the repository",
        bob.set_task_repository(&managed.short_code, Some("payments-api"))
            .await,
    );
    checks.allowed(
        "6: the manager changes the lane",
        bob.set_task_work_class(&managed.short_code, "planned")
            .await,
    );
    checks.allowed(
        "6: the manager moves it out of the entry column",
        bob.transition_task(&managed.short_code, &todo).await,
    );
    checks.allowed(
        "6: the manager links it",
        bob.create_relationship(&edge(&managed.short_code, &bob_second.short_code, "blocks"))
            .await,
    );
    checks.allowed(
        "6: the manager archives it",
        bob.delete_task(&managed.short_code).await,
    );
    checks.allowed(
        "6: the manager restores it",
        bob.restore_task(&managed.short_code).await,
    );
    checks.allowed(
        "6: the admin edits an item of a member",
        svc.update_task(
            &bob_second.short_code,
            &content(None, "by the admin", bob_second.version),
        )
        .await,
    );
    checks.allowed(
        "6: the admin links two items of members",
        svc.create_relationship(&edge(
            &bob_second.short_code,
            &alice_own.short_code,
            "blocks",
        ))
        .await,
    );
    checks.allowed(
        "6: the admin moves a task to another board",
        svc.move_task(&managed.short_code, &ops_board).await,
    );

    // =======================================================================
    // Cases 7, 8, 9. Creation grants NO movement (criterion j)
    // =======================================================================
    let before = svc.get_task(&code).await.expect("read");
    checks.refused(
        "7 REST: the creator cannot transition her request",
        alice.transition_task(&code, &todo).await,
    );
    checks.refused(
        "8 REST: the creator cannot put her request in the planned lane",
        alice.set_task_work_class(&code, "planned").await,
    );
    checks.refused(
        "8 REST: nor name the lane that it is in",
        alice.set_task_work_class(&code, "support").await,
    );
    // Her own board is the target: she manages it. What she lacks is the
    // source half.
    checks.refused(
        "9 REST: the creator cannot move her request to her own board",
        alice.move_task(&code, &web_board).await,
    );
    checks.refused(
        "9 REST: nor to a third board",
        alice.move_task(&code, &ops_board).await,
    );
    let reply = alice_mcp
        .call(
            "transition_item",
            json!({"short_code": mcp_code, "to_column": "Todo"}),
        )
        .await;
    checks.mcp_refused("7 MCP transition_item: the creator", &reply);
    // The explanation is for the creator of a request in the entry column.
    // It must say what is true now: she can edit, link and archive it.
    checks.check(
        "7 MCP: the refusal explains the request rule, and it is true",
        reply.1.contains("is a request in the entry column")
            && reply.1.contains("file_backlog")
            && reply.1.contains("transition_items")
            && !reply.1.contains("To move, edit or delete it"),
        &reply.1,
    );
    let reply = alice_mcp
        .call(
            "move_item",
            json!({"short_code": mcp_code, "to_board": web_board}),
        )
        .await;
    checks.mcp_refused("9 MCP move_item: the creator", &reply);
    let after = svc.get_task(&code).await.expect("read");
    checks.check(
        "7-9 REST: the request did not move",
        after.column_id == before.column_id
            && after.work_class == before.work_class
            && after.board_id == before.board_id
            && after.team_id == before.team_id
            && after.version == before.version,
        format!("before {before:?}, after {after:?}"),
    );
    let after = svc.get_task(&mcp_code).await.expect("read");
    checks.check(
        "7-9 MCP: the request did not move",
        after.column_id == backlog
            && after.work_class == "support"
            && after.board_id == platform_board,
        format!("{after:?}"),
    );
    // Movement of the other types is not an edit either.
    let initiative_columns = svc.list_columns(&initiative_board).await.expect("columns");
    let next = initiative_columns
        .iter()
        .find(|c| c.position == 1)
        .expect("second column")
        .id
        .clone();
    checks.refused(
        "7 REST: the creator cannot transition her initiative",
        alice
            .transition_initiative(&alice_initiative.short_code, &next)
            .await,
    );

    // =======================================================================
    // Case 10. carol created nothing and holds nothing: no edit, anywhere
    // =======================================================================
    let before = svc.get_task(&code).await.expect("read");
    checks.refused(
        "10 REST: carol edits the request of alice",
        carol
            .update_task(&code, &content(Some("carol"), "carol", before.version))
            .await,
    );
    checks.refused(
        "10 REST: carol sets metadata on it",
        carol
            .update_metadata(EntityKind::Task, &code, &metadata(Some("carol")))
            .await,
    );
    checks.refused(
        "10 REST: carol sets its repository",
        carol.set_task_repository(&code, Some("payments-api")).await,
    );
    checks.refused("10 REST: carol archives it", carol.delete_task(&code).await);
    checks.refused(
        "10 REST: carol restores an archived task",
        carol.restore_task(&archived.short_code).await,
    );
    let after = svc.get_task(&code).await.expect("read");
    checks.check(
        "10 REST: the request of alice is as it was",
        after.title == before.title
            && after.content == before.content
            && after.version == before.version
            && after.repository_id == before.repository_id
            && after.archived_at.is_none(),
        format!("before {before:?}, after {after:?}"),
    );
    checks.check(
        "10 REST: its metadata is as it was",
        svc.metadata(EntityKind::Task, &code)
            .await
            .expect("metadata")
            .values
            .iter()
            .all(|v| v.value != "carol"),
        "carol's value is stored",
    );
    checks.check(
        "10 REST: the archived task is still archived",
        svc.get_task(&archived.short_code)
            .await
            .expect("read")
            .archived_at
            .is_some(),
        "it was restored",
    );
    // Every type, on every board.
    checks.refused(
        "10 REST: carol edits a strategy",
        carol
            .update_strategy(&strategy.short_code, &content(None, "carol", 1))
            .await,
    );
    checks.refused(
        "10 REST: carol archives a strategy",
        carol.delete_strategy(&strategy.short_code).await,
    );
    checks.refused(
        "10 REST: carol edits an initiative",
        carol
            .update_initiative(&initiative.short_code, &content(None, "carol", 1))
            .await,
    );
    checks.refused(
        "10 REST: carol archives an initiative",
        carol.delete_initiative(&initiative.short_code).await,
    );
    checks.refused(
        "10 REST: carol edits an ADR",
        carol
            .update_adr(&adr_one.short_code, &content(None, "carol", 1))
            .await,
    );
    checks.refused(
        "10 REST: carol archives an ADR",
        carol.delete_adr(&adr_one.short_code).await,
    );
    checks.refused(
        "10 REST: carol edits a document",
        carol
            .update_document(&document.short_code, &content(None, "carol", 1))
            .await,
    );
    checks.refused(
        "10 REST: carol sets the lifecycle of a document",
        carol
            .set_document_lifecycle(&document.short_code, "published")
            .await,
    );
    checks.refused(
        "10 REST: carol archives a document",
        carol.delete_document(&document.short_code).await,
    );
    checks.refused(
        "10 REST: carol edits the document that alice created",
        carol
            .update_document(&alice_document.short_code, &content(None, "carol", 2))
            .await,
    );
    for (kind, short_code) in [
        (EntityKind::Strategy, &strategy.short_code),
        (EntityKind::Initiative, &initiative.short_code),
        (EntityKind::Adr, &adr_one.short_code),
        (EntityKind::Document, &document.short_code),
        (EntityKind::Task, &bob_task.short_code),
    ] {
        checks.refused(
            &format!("10 REST: carol sets metadata on {short_code}"),
            carol
                .update_metadata(kind, short_code, &metadata(Some("carol")))
                .await,
        );
    }
    checks.check(
        "10 REST: the items of the organization are as they were",
        svc.get_strategy(&strategy.short_code)
            .await
            .expect("read")
            .content
            == "original content"
            && svc
                .get_initiative(&initiative.short_code)
                .await
                .expect("read")
                .content
                == "original content"
            && svc
                .get_adr(&adr_one.short_code)
                .await
                .expect("read")
                .content
                == "original content"
            && {
                let document = svc.get_document(&document.short_code).await.expect("read");
                document.content == "original content"
                    && document.lifecycle == "draft"
                    && document.archived_at.is_none()
            },
        "an item changed",
    );
    for (tool, arguments) in [
        (
            "update_item",
            json!({"short_code": mcp_code, "content": "carol", "version": 3}),
        ),
        (
            "edit_item",
            json!({"short_code": mcp_code, "search": "new", "replace": "carol"}),
        ),
        (
            "set_metadata",
            json!({"short_code": mcp_code, "values": {"urgency": "carol"}}),
        ),
        (
            "set_repository",
            json!({"short_code": mcp_code, "repository": "payments-api"}),
        ),
        (
            "delete_item",
            json!({"short_code": mcp_code, "confirm": true}),
        ),
        ("restore_item", json!({"short_code": archived.short_code})),
        (
            "update_item",
            json!({"short_code": initiative.short_code, "content": "carol", "version": 1}),
        ),
        (
            "delete_item",
            json!({"short_code": adr_one.short_code, "confirm": true}),
        ),
        (
            "update_item",
            json!({"short_code": document.short_code, "content": "carol", "version": 1}),
        ),
    ] {
        let reply = carol_mcp.call(tool, arguments.clone()).await;
        checks.mcp_refused(&format!("10 MCP {tool} by carol: {arguments}"), &reply);
    }
    let after = svc.get_task(&mcp_code).await.expect("read");
    checks.check(
        "10 MCP: the MCP request of alice is as it was",
        after.content == "new content there"
            && after.repository.is_none()
            && after.archived_at.is_none(),
        format!("{after:?}"),
    );

    // =======================================================================
    // Case 11. carol can edit neither end: no edge, of any type
    // (criteria d and e)
    // =======================================================================
    let edges_before = edge_count(&mut conn);
    let impossible_for_carol = [
        (
            "parent",
            &initiative.short_code,
            &bob_second.short_code,
            "manage_initiatives",
            "manage_tasks",
        ),
        (
            "blocks",
            &bob_task.short_code,
            &code,
            "manage_tasks",
            "manage_tasks",
        ),
        (
            "supports",
            &initiative.short_code,
            &adr_two.short_code,
            "manage_initiatives",
            "manage_adrs",
        ),
        (
            "informs",
            &adr_three.short_code,
            &initiative.short_code,
            "manage_adrs",
            "manage_initiatives",
        ),
        (
            "supersedes",
            &adr_three.short_code,
            &adr_one.short_code,
            "manage_adrs",
            "manage_adrs",
        ),
    ];
    for (relationship, source, target, source_capability, target_capability) in impossible_for_carol
    {
        let result = carol
            .create_relationship(&edge(source, target, relationship))
            .await;
        let message = match &result {
            Err(Error::Forbidden { message, .. }) => message.clone(),
            _ => String::new(),
        };
        let named = checks.refused(&format!("11 REST {relationship} by carol"), result);
        checks.check(
            &format!("11e REST {relationship}: the refusal names the capability"),
            named.as_deref() == Some(source_capability)
                && message.contains(source_capability)
                && message.contains(target_capability),
            format!("details.required_capability = {named:?}, message = {message}"),
        );
        let reply = carol_mcp
            .call(
                "link_items",
                json!({"source": source, "target": target, "relationship": relationship}),
            )
            .await;
        checks.mcp_refused(
            &format!("11 MCP link_items {relationship} by carol"),
            &reply,
        );
        checks.check(
            &format!("11e MCP {relationship}: the refusal names the capability"),
            reply.1.contains(source_capability) && reply.1.contains(target_capability),
            &reply.1,
        );
    }
    // Removing an edge is gated as writing it is.
    let kept = svc
        .create_relationship(&edge(
            &bob_task.short_code,
            &bob_second.short_code,
            "blocks",
        ))
        .await
        .expect("an edge of the admin");
    checks.refused(
        "11 REST: carol removes an edge",
        carol.delete_relationship(&kept.id).await,
    );
    let reply = carol_mcp
        .call(
            "unlink_items",
            json!({"source": bob_task.short_code, "target": bob_second.short_code,
                   "relationship": "blocks"}),
        )
        .await;
    checks.mcp_refused("11 MCP unlink_items by carol", &reply);
    // A supports edge that exists: bob made `strategy -> adr_two` in case 4.
    let reply = carol_mcp
        .call(
            "unlink_items",
            json!({"source": strategy.short_code, "target": adr_two.short_code,
                   "relationship": "supports"}),
        )
        .await;
    checks.mcp_refused("11 MCP unlink_items supports by carol", &reply);
    checks.check(
        "11: no edge was written or removed",
        edge_count(&mut conn) == edges_before + 1,
        format!(
            "{edges_before} edges before, plus the one of the admin; {} now",
            edge_count(&mut conn)
        ),
    );

    // =======================================================================
    // Case 12. alice cannot edit what she did not create
    // =======================================================================
    let before = svc.get_task(&bob_task.short_code).await.expect("read");
    checks.refused(
        "12 REST: alice edits the task of bob",
        alice
            .update_task(
                &bob_task.short_code,
                &content(None, "alice", before.version),
            )
            .await,
    );
    checks.refused(
        "12 REST: alice archives the task of bob",
        alice.delete_task(&bob_task.short_code).await,
    );
    checks.refused(
        "12 REST: alice sets metadata on the task of bob",
        alice
            .update_metadata(
                EntityKind::Task,
                &bob_task.short_code,
                &metadata(Some("alice")),
            )
            .await,
    );
    checks.refused(
        "12 REST: alice sets the repository of the task of bob",
        alice
            .set_task_repository(&bob_task.short_code, Some("payments-api"))
            .await,
    );
    checks.refused(
        "12 REST: alice restores a task that she did not create",
        alice.restore_task(&archived.short_code).await,
    );
    let reply = alice_mcp
        .call(
            "update_item",
            json!({"short_code": bob_task.short_code, "content": "alice", "version": 1}),
        )
        .await;
    checks.mcp_refused("12 MCP update_item: alice on the task of bob", &reply);
    checks.check(
        "12 MCP: the refusal names the capability, not the request rule",
        reply.1.contains("manage_tasks") && !reply.1.contains("entry column"),
        &reply.1,
    );
    let after = svc.get_task(&bob_task.short_code).await.expect("read");
    checks.check(
        "12: the task of bob is as it was",
        after.content == before.content
            && after.version == before.version
            && after.repository_id == before.repository_id
            && after.archived_at.is_none(),
        format!("before {before:?}, after {after:?}"),
    );
    // A request that she created does not let her create a document on
    // that board: the create gate of a document does not change.
    checks.refused(
        "12 REST: the create gate of a document does not change",
        alice
            .create_document(&CreateDocumentRequest {
                title: "A document under her request".into(),
                content: None,
                template_id: None,
                parent_short_code: Some(code.clone()),
            })
            .await,
    );

    // =======================================================================
    // Case 13. Creation gives no right on a BOARD
    // =======================================================================
    let tasks_before = svc
        .list_tasks(kairos_client::types::Pagination {
            limit: Some(200),
            offset: None,
        })
        .await
        .expect("count")
        .total;
    checks.refused(
        "13: alice cannot create a planned task on the board of platform",
        alice
            .create_task(&CreateTaskRequest {
                work_class: Some("planned".into()),
                ..task_on(&platform_board, "Planned by alice")
            })
            .await,
    );
    checks.refused(
        "13: nor a task in a column that is not the entry column",
        alice
            .create_task(&CreateTaskRequest {
                column_id: Some(todo.clone()),
                ..task_on(&platform_board, "In Todo by alice")
            })
            .await,
    );
    checks.refused(
        "13: alice cannot add a column to the board of platform",
        alice
            .add_column(
                &platform_board,
                &CreateColumnRequest {
                    name: "Column of alice".into(),
                    position: 9,
                },
            )
            .await,
    );
    checks.refused(
        "13: alice cannot add a member to the board of platform",
        alice
            .add_board_member(
                &platform_board,
                &AddBoardMemberRequest {
                    user_id: alice_id.to_string(),
                    capabilities: vec!["manage_tasks".into()],
                },
            )
            .await,
    );
    checks.refused(
        "13: alice cannot create an initiative on the initiative board",
        alice
            .create_initiative(&CreateInitiativeRequest {
                board_id: initiative_board.clone(),
                column_id: None,
                title: "No grant".into(),
                content: String::new(),
                complexity: None,
                bucket_type: None,
            })
            .await,
    );
    checks.check(
        "13: the board of platform is as it was",
        svc.list_columns(&platform_board)
            .await
            .expect("columns")
            .len()
            == columns.len()
            && svc
                .list_tasks(kairos_client::types::Pagination {
                    limit: Some(200),
                    offset: None,
                })
                .await
                .expect("count")
                .total
                == tasks_before,
        "a column or a task was added",
    );

    // =======================================================================
    // Case 14. A service account is a creator like a person
    // =======================================================================
    let by_maker = maker
        .create_task(&task_on(
            &platform_board,
            "The request of a service account",
        ))
        .await
        .expect("a service account sends a request");
    assert_eq!(by_maker.created_by, maker_id.to_string());
    checks.allowed(
        "14 REST: the service account that created it edits it",
        maker
            .update_task(
                &by_maker.short_code,
                &content(None, "by maker", by_maker.version),
            )
            .await,
    );
    checks.refused(
        "14 REST: a DIFFERENT service account cannot edit it",
        other
            .update_task(&by_maker.short_code, &content(None, "by other", 2))
            .await,
    );
    checks.refused(
        "14 REST: a DIFFERENT service account cannot archive it",
        other.delete_task(&by_maker.short_code).await,
    );
    checks.refused(
        "14 REST: a DIFFERENT service account cannot link it",
        other
            .create_relationship(&edge(&by_maker.short_code, &bob_task.short_code, "blocks"))
            .await,
    );
    checks.refused(
        "14 REST: the service account that created it cannot move it",
        maker.transition_task(&by_maker.short_code, &todo).await,
    );
    let mut other_mcp = McpSession::open(&server.base_url, &other_key, "acme").await;
    let reply = other_mcp
        .call(
            "update_item",
            json!({"short_code": by_maker.short_code, "content": "by other", "version": 2}),
        )
        .await;
    checks.mcp_refused("14 MCP update_item: a DIFFERENT service account", &reply);
    let after = svc.get_task(&by_maker.short_code).await.expect("read");
    checks.check(
        "14: the request holds the edit of its creator only",
        after.content == "by maker" && after.archived_at.is_none() && after.column_id == backlog,
        format!("{after:?}"),
    );

    // =======================================================================
    // Case 15. The type rules and the cycle check do not change
    // (criterion f)
    // =======================================================================
    // alice can edit the two ends. The edge is impossible, so the refusal
    // is about the edge and not about her.
    let edges_before = edge_count(&mut conn);
    let result = alice
        .create_relationship(&edge(&code, &alice_own.short_code, "supersedes"))
        .await;
    checks.check(
        "15 REST: supersedes between two tasks is RELATIONSHIP_RULE, not FORBIDDEN",
        matches!(&result, Err(err) if !matches!(err, Error::Forbidden { .. })
            && err.to_string().contains("RELATIONSHIP_RULE")),
        format!("{result:?}"),
    );
    let reply = alice_mcp
        .call(
            "link_items",
            json!({"source": code, "target": alice_own.short_code,
                   "relationship": "supersedes"}),
        )
        .await;
    checks.check(
        "15 MCP: supersedes between two tasks is RELATIONSHIP_RULE, not FORBIDDEN",
        reply.0 && reply.1.contains("RELATIONSHIP_RULE") && !reply.1.contains("FORBIDDEN"),
        &reply.1,
    );
    let result = alice
        .create_relationship(&edge(&code, &alice_initiative.short_code, "parent"))
        .await;
    checks.check(
        "15 REST: parent from a task to an initiative is RELATIONSHIP_RULE",
        matches!(&result, Err(err) if err.to_string().contains("RELATIONSHIP_RULE")),
        format!("{result:?}"),
    );
    checks.allowed(
        "15 REST: blocks, one way",
        alice
            .create_relationship(&edge(&code, &alice_own.short_code, "blocks"))
            .await,
    );
    let result = alice
        .create_relationship(&edge(&alice_own.short_code, &code, "blocks"))
        .await;
    checks.check(
        "15 REST: blocks, the other way, is CYCLE_DETECTED",
        matches!(&result, Err(err) if err.to_string().contains("CYCLE_DETECTED")),
        format!("{result:?}"),
    );
    let result = alice
        .create_relationship(&edge(&code, &alice_own.short_code, "blocks"))
        .await;
    checks.check(
        "15 REST: the same edge again is ALREADY_LINKED",
        matches!(&result, Err(err) if err.to_string().contains("ALREADY_LINKED")),
        format!("{result:?}"),
    );
    checks.check(
        "15: one edge was written, the one that is possible",
        edge_count(&mut conn) == edges_before + 1,
        format!("{edges_before} before, {} now", edge_count(&mut conn)),
    );

    // =======================================================================
    // Case 16. Another tenant
    // =======================================================================
    // alice is a member of globex too. The short codes of acme name nothing
    // there.
    let before = svc.get_task(&code).await.expect("read");
    let edges_before = edge_count(&mut conn);
    let result = globex_alice
        .update_task(&code, &content(None, "from globex", before.version))
        .await;
    checks.check(
        "16 REST: an edit from another tenant is 404",
        matches!(result, Err(Error::NotFound { .. })),
        format!("{result:?}"),
    );
    let result = globex_alice.delete_task(&code).await;
    checks.check(
        "16 REST: an archive from another tenant is 404",
        matches!(result, Err(Error::NotFound { .. })),
        format!("{result:?}"),
    );
    let result = globex_alice
        .update_metadata(EntityKind::Task, &code, &metadata(Some("globex")))
        .await;
    checks.check(
        "16 REST: metadata from another tenant is 404",
        matches!(result, Err(Error::NotFound { .. })),
        format!("{result:?}"),
    );
    let result = globex_alice
        .create_relationship(&edge(&code, &alice_own.short_code, "blocks"))
        .await;
    checks.check(
        "16 REST: a link from another tenant names no live item",
        matches!(result, Err(Error::Validation { .. })),
        format!("{result:?}"),
    );
    let result = globex_alice.delete_relationship(&kept.id).await;
    checks.check(
        "16 REST: an unlink from another tenant is 404",
        matches!(result, Err(Error::NotFound { .. })),
        format!("{result:?}"),
    );
    let mut globex_mcp = McpSession::open(&server.base_url, &alice_token, "globex").await;
    for (tool, arguments) in [
        (
            "update_item",
            json!({"short_code": code, "content": "from globex", "version": before.version}),
        ),
        ("delete_item", json!({"short_code": code, "confirm": true})),
        (
            "link_items",
            json!({"source": code, "target": alice_own.short_code, "relationship": "parent"}),
        ),
    ] {
        let reply = globex_mcp.call(tool, arguments).await;
        checks.check(
            &format!("16 MCP {tool} from another tenant"),
            reply.0 && !reply.1.contains(&before.title),
            &reply.1,
        );
    }
    let after = svc.get_task(&code).await.expect("read");
    checks.check(
        "16: the request is as it was, and no edge was written",
        after.content == before.content
            && after.version == before.version
            && after.archived_at.is_none()
            && edge_count(&mut conn) == edges_before,
        format!("{after:?}"),
    );

    drop((alice_mcp, bob_mcp, carol_mcp, other_mcp, globex_mcp));
    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
    checks.finish();
}

/// A precondition of the fixture, not a case: it must hold before and after
/// the change, so it stops the test at once.
fn forbidden_now<T: std::fmt::Debug>(result: Result<T, Error>, what: &str) {
    match result {
        Err(Error::Forbidden { .. }) => {}
        other => panic!("{what}: expected a 403, got {other:?}"),
    }
}
