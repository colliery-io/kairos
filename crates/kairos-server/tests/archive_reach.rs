//! Integration test for COLLIERY-T-0234: an archive does not reach
//! children that the caller cannot edit, and the confirm of an edge
//! proposal takes the link rule.
//!
//! THE ATTACK (defect 1). A person creates an initiative, so they can edit
//! it. The link rule (COLLIERY-T-0228) lets them write a `parent` edge from
//! their initiative to a task of a DIFFERENT team. They archive their
//! initiative. The cascade archived the task of the other team, because
//! the server checked the edit rule for the named item only.
//!
//! THE SECOND ROUTE (defect 2). `POST /api/proposals/{id}/confirm` wrote a
//! `parent` or `blocks` edge, and checked only that the caller was a
//! person. A member who could edit NEITHER end could propose an edge and
//! confirm it.
//!
//! THE RULE. An archive takes a descendant that the caller may edit, by
//! the edit rule. It stops at a descendant that the caller may not edit,
//! and takes nothing below it. The response names each descendant that
//! stays. A descendant that stays keeps its `parent` edge. The confirm of a
//! proposal takes the link rule, as each other edge write does.
//!
//! This is a security boundary, so the negative cases are the contract.
//! Each refusal is checked where the data is stored: the row is read after
//! the archive and must be as it was.
//!
//! The cases are numbered as in the work item. Every check is recorded and
//! the test fails at the end with the full list, so one run shows each
//! broken case and not only the first.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_archive_reach_t0234_test`.
//!
//! Cast, as in `edit_rule.rs`:
//! - `svc`   — org admin: sets the world up,
//! - `alice` — member of team `web`; NO grant on the board of `platform`,
//! - `bob`   — member of team `platform`, with `manage_initiatives` on the
//!   initiative board,
//! - `carol` — org member in no team, with no grant,
//! - `maker` — a service account, an org member.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{
    CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest, CreateStrategyRequest,
    CreateTaskRequest, DeleteResponse, Initiative, NotReached, Task,
};
use kairos_client::types_meta::CreateRelationshipRequest;
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::{EntityKind, Error, KairosClient};
use kairos_db::api_keys::{self, NewApiKey};
use kairos_db::models::public::{NewServiceAccountUser, User};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;
use kairos_server::service_accounts::auth::{generate_key, hash_key};

const SCRATCH_DB: &str = "kairos_archive_reach_t0234_test";

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

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

/// The number of edges of one type from `source` to `target`, read where
/// the edges are stored.
fn edges_between(conn: &mut PgConnection, source: &str, target: &str, relationship: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*)::bigint AS count \
           FROM item_relationships r \
           JOIN entity_directory s ON s.id = r.source_id \
           JOIN entity_directory t ON t.id = r.target_id \
          WHERE s.short_code = $1 AND t.short_code = $2 \
            AND r.relationship::text = $3",
    )
    .bind::<Text, _>(source)
    .bind::<Text, _>(target)
    .bind::<Text, _>(relationship)
    .get_result::<Count>(conn)
    .expect("counting edges")
    .count
}

/// The number of `parent` edges that point at `target`, from any source.
fn parents_of(conn: &mut PgConnection, target: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*)::bigint AS count \
           FROM item_relationships r \
           JOIN entity_directory t ON t.id = r.target_id \
          WHERE t.short_code = $1 AND r.relationship::text = 'parent'",
    )
    .bind::<Text, _>(target)
    .get_result::<Count>(conn)
    .expect("counting parents")
    .count
}

/// `deleted_at IS NOT NULL` for the row with this short code, read from
/// the tables and not through the API.
fn is_archived(conn: &mut PgConnection, short_code: &str) -> bool {
    diesel::sql_query(
        "SELECT count(*)::bigint AS count FROM entity_directory \
          WHERE short_code = $1 AND deleted_at IS NOT NULL",
    )
    .bind::<Text, _>(short_code)
    .get_result::<Count>(conn)
    .expect("reading deleted_at")
    .count
        == 1
}

#[derive(QueryableByName)]
struct Details {
    #[diesel(sql_type = Text)]
    details: String,
}

/// The record of the archive of `short_code` in `activity_log`.
fn archive_record(conn: &mut PgConnection, short_code: &str) -> String {
    diesel::sql_query(
        "SELECT details FROM activity_log \
          WHERE action = 'delete' AND details LIKE $1 \
          ORDER BY occurred_at DESC LIMIT 1",
    )
    .bind::<Text, _>(format!("short_code:{short_code} %"))
    .get_result::<Details>(conn)
    .map(|row| row.details)
    .unwrap_or_else(|e| panic!("no record of the archive of {short_code}: {e}"))
}

/// A service account that is a member of `org`, a client for its key, and
/// the key.
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

    /// The call must be refused with 403 `FORBIDDEN`.
    fn refused<T: std::fmt::Debug>(&mut self, case: &str, result: Result<T, Error>) {
        match result {
            Err(Error::Forbidden { code, .. }) if code == "FORBIDDEN" => self.passed += 1,
            Err(err) => self
                .failures
                .push(format!("{case}: expected 403 FORBIDDEN, got {err}")),
            Ok(value) => self.failures.push(format!(
                "{case}: expected 403 FORBIDDEN, but the call SUCCEEDED: {value:?}"
            )),
        }
    }

    /// The archive took exactly `archived` and left exactly `left`.
    fn reach(&mut self, case: &str, response: &DeleteResponse, archived: &[&str], left: &[&str]) {
        let mut want: Vec<String> = archived.iter().map(|c| c.to_string()).collect();
        want.sort();
        self.check(
            &format!("{case}: the response lists what the archive took"),
            response.cascaded_short_codes == want && response.cascade_count == want.len() as i64,
            format!("expected {want:?}, got {:?}", response.cascaded_short_codes),
        );
        let mut want: Vec<String> = left.iter().map(|c| c.to_string()).collect();
        want.sort();
        let got: Vec<String> = response
            .not_reached
            .iter()
            .map(|item| item.short_code.clone())
            .collect();
        self.check(
            &format!("{case}: the response names what the archive did not reach"),
            got == want,
            format!("expected not_reached {want:?}, got {got:?}"),
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
        eprintln!("archive_reach: {} checks passed", self.passed);
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
                    "clientInfo": {"name": "kairos-t0234-test", "version": "0.0.0"},
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

/// A raw request as one principal: the status and the JSON body. For the
/// routes that the typed client does not have (proposals), and for the
/// checks on the wire shape.
async fn raw(
    http: &reqwest::Client,
    method: reqwest::Method,
    base_url: &str,
    path: &str,
    token: &str,
) -> (u16, Value) {
    let response = http
        .request(method, format!("{base_url}{path}"))
        .bearer_auth(token)
        .header("X-Tenant", "acme")
        .json(&json!({}))
        .send()
        .await
        .expect("raw request");
    let status = response.status().as_u16();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

/// The pending proposals on an item: `(id, state)` of each.
async fn pending(http: &reqwest::Client, base_url: &str, code: &str, token: &str) -> Vec<Value> {
    let (status, body) = raw(
        http,
        reqwest::Method::GET,
        base_url,
        &format!("/api/items/{code}/proposals"),
        token,
    )
    .await;
    assert_eq!(status, 200, "listing proposals of {code}: {body}");
    body.as_array().cloned().unwrap_or_default()
}

fn edge(source: &str, target: &str, relationship: &str) -> CreateRelationshipRequest {
    CreateRelationshipRequest {
        source_short_code: source.into(),
        target_short_code: target.into(),
        relationship: relationship.into(),
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

async fn initiative_by(client: &KairosClient, board: &str, title: &str) -> Initiative {
    client
        .create_initiative(&CreateInitiativeRequest {
            board_id: board.into(),
            column_id: None,
            title: title.into(),
            content: "original content".into(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .unwrap_or_else(|e| panic!("creating initiative {title:?}: {e}"))
}

async fn task_by(client: &KairosClient, board: &str, title: &str) -> Task {
    client
        .create_task(&task_on(board, title))
        .await
        .unwrap_or_else(|e| panic!("creating task {title:?}: {e}"))
}

/// Write a `parent` edge as `client`. A precondition of the fixture: the
/// link rule permits it, and the test stops if it does not.
async fn parent(client: &KairosClient, source: &str, target: &str) {
    client
        .create_relationship(&edge(source, target, "parent"))
        .await
        .unwrap_or_else(|e| panic!("linking {source} -> {target}: {e}"));
}

/// Is the entry for `code` the refusal of the edit rule, with this
/// capability on this board?
fn cannot_edit(items: &[NotReached], code: &str, capability: &str, board: &str) -> bool {
    items.iter().any(|item| {
        item.short_code == code
            && item.required_capability.as_deref() == Some(capability)
            && item.board_id.as_deref() == Some(board)
            && item.below.is_none()
    })
}

/// Is the entry for `code` "below `stop`", with no capability?
fn is_below(items: &[NotReached], code: &str, stop: &str) -> bool {
    items.iter().any(|item| {
        item.short_code == code
            && item.below.as_deref() == Some(stop)
            && item.required_capability.is_none()
            && item.board_id.is_none()
    })
}

/// The mixed children of cases 3, 6 and 8: an initiative of alice with a
/// task that alice created, a task on the board of her team, and a task
/// of bob.
struct Mixed {
    initiative: Initiative,
    created: Task,
    managed: Task,
    of_bob: Task,
}

#[allow(clippy::too_many_arguments)]
async fn mixed(
    svc: &KairosClient,
    alice: &KairosClient,
    bob: &KairosClient,
    initiative: Initiative,
    platform_board: &str,
    web_board: &str,
    label: &str,
) -> Mixed {
    // alice created it, as a request to platform: she holds nothing on
    // that board, and edits it as its creator.
    let created = task_by(alice, platform_board, &format!("{label}: alice created")).await;
    // The admin created it on the board of web: alice edits it as a member
    // of the team (`manage_tasks`), and did not create it.
    let managed = task_by(svc, web_board, &format!("{label}: on the board of web")).await;
    let of_bob = task_by(bob, platform_board, &format!("{label}: of bob")).await;
    for task in [&created, &managed, &of_bob] {
        parent(alice, &initiative.short_code, &task.short_code).await;
    }
    Mixed {
        initiative,
        created,
        managed,
        of_bob,
    }
}

#[tokio::test]
async fn archive_reach_and_proposal_confirm_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let acme = org_id(&mut conn, "acme");

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
    let base = server.base_url.clone();
    let svc = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    let bob = server.client(&bob_token, "acme");
    let carol = server.client(&carol_token, "acme");

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

    let mut teams = Vec::new();
    for (name, slug) in [("Platform", "platform"), ("Web", "web")] {
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
    let strategy_board = board_of_level(&mut conn, BoardLevel::Strategy);
    let initiative_board = board_of_level(&mut conn, BoardLevel::Initiative);
    let adr_board = board_of_level(&mut conn, BoardLevel::Adr);

    let (_maker_id, maker, maker_key) =
        service_account(&mut conn, acme, svc_id, "maker", &server.base_url);

    // ---------------------------------------------------------------------
    // What alice and carol CREATE. They need a grant to create a strategy
    // or an initiative (the create gate does not change), so the admin
    // gives the grants and takes them away after the creates. What is left
    // is creation alone: the first fact of the edit rule.
    // ---------------------------------------------------------------------
    for (board, user, capability) in [
        (&initiative_board, alice_id, "manage_initiatives"),
        (&strategy_board, alice_id, "manage_strategies"),
        (&initiative_board, carol_id, "manage_initiatives"),
    ] {
        svc.add_board_member(
            board,
            &AddBoardMemberRequest {
                user_id: user.to_string(),
                capabilities: vec![capability.into()],
            },
        )
        .await
        .expect("grant");
    }
    let attack = initiative_by(&alice, &initiative_board, "1: the initiative of alice").await;
    let attack_mcp = initiative_by(&alice, &initiative_board, "2: the MCP initiative").await;
    let mixed_alice = initiative_by(&alice, &initiative_board, "3: mixed children").await;
    let both = initiative_by(&alice, &initiative_board, "5: both editable").await;
    let mixed_admin = initiative_by(&alice, &initiative_board, "6: mixed children").await;
    let with_documents = initiative_by(&alice, &initiative_board, "11: with documents").await;
    let alice_strategy = alice
        .create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.clone(),
            column_id: None,
            title: "4: the strategy of alice".into(),
            content: "original content".into(),
            hypothesis: None,
        })
        .await
        .expect("alice creates her strategy");
    let carol_initiative =
        initiative_by(&carol, &initiative_board, "15: the initiative of carol").await;
    for (board, user) in [
        (&initiative_board, alice_id),
        (&strategy_board, alice_id),
        (&initiative_board, carol_id),
    ] {
        svc.remove_board_member(board, &user.to_string())
            .await
            .expect("the grant is taken away");
    }
    match alice
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.clone(),
            column_id: None,
            title: "one more".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await
    {
        Err(Error::Forbidden { .. }) => {}
        other => panic!("alice holds no grant on the initiative board now: {other:?}"),
    }
    // bob manages the initiative board, for case 7.
    svc.add_board_member(
        &initiative_board,
        &AddBoardMemberRequest {
            user_id: bob_id.to_string(),
            capabilities: vec!["manage_initiatives".into()],
        },
    )
    .await
    .expect("grant manage_initiatives to bob");

    let mut checks = Checks::default();
    let mut alice_mcp = McpSession::open(&base, &alice_token, "acme").await;
    let mut carol_mcp = McpSession::open(&base, &carol_token, "acme").await;
    let mut svc_mcp = McpSession::open(&base, &svc_token, "acme").await;
    let mut maker_mcp = McpSession::open(&base, &maker_key, "acme").await;

    // =======================================================================
    // Case 1. The attack, end to end, over REST
    // (criteria b, d, f)
    // =======================================================================
    let victim = task_by(&bob, &platform_board, "1: the task of bob").await;
    // The link rule permits the edge: alice can edit the source.
    parent(&alice, &attack.short_code, &victim.short_code).await;
    let before = svc.get_task(&victim.short_code).await.expect("read");

    let response = checks.allowed(
        "1 REST: alice archives her initiative",
        alice.delete_initiative(&attack.short_code).await,
    );
    let after = svc.get_task(&victim.short_code).await.expect("read");
    checks.check(
        "1 REST (b): the task of bob is LIVE, in its column, unchanged",
        after == before && after.archived_at.is_none(),
        format!("before {before:?}\n      after  {after:?}"),
    );
    checks.check(
        "1 REST (b): the row of the task of bob has no deleted_at",
        !is_archived(&mut conn, &victim.short_code),
        "deleted_at is set on the task of bob",
    );
    checks.check(
        "1 REST: the initiative of alice IS archived",
        is_archived(&mut conn, &attack.short_code),
        "the named item was not archived",
    );
    if let Some(response) = &response {
        checks.reach("1 REST (d)", response, &[], &[&victim.short_code]);
        checks.check(
            "1 REST (d): the reason is the capability and the board",
            cannot_edit(
                &response.not_reached,
                &victim.short_code,
                "manage_tasks",
                &platform_board,
            ),
            format!("{:?}", response.not_reached),
        );
    }
    checks.check(
        "1 REST (f): the parent edge still exists",
        edges_between(&mut conn, &attack.short_code, &victim.short_code, "parent") == 1,
        "the edge is gone",
    );
    // A live child with an archived parent is a state that the system
    // serves: each read of the child still answers.
    checks.allowed(
        "1 (f): the relationships of the live child still read",
        bob.relationships(EntityKind::Task, &victim.short_code)
            .await,
    );
    let reply = svc_mcp
        .call("get_item", json!({"short_code": victim.short_code}))
        .await;
    checks.check(
        "1 (f): MCP get_item of the live child still reads, and names its parent",
        !reply.0 && reply.1.contains(&attack.short_code),
        &reply.1,
    );

    // =======================================================================
    // Case 9. Restore of the initiative after case 1
    // =======================================================================
    let restored = checks.allowed(
        "9: alice restores her initiative",
        alice.restore_initiative(&attack.short_code).await,
    );
    if let Some(restored) = restored {
        checks.check(
            "9: nothing below it is still archived",
            restored.still_archived_count == 0 && restored.still_archived_short_codes.is_empty(),
            format!("{restored:?}"),
        );
    }
    checks.check(
        "9: the initiative is live again",
        !is_archived(&mut conn, &attack.short_code),
        "still archived",
    );
    let after_restore = svc.get_task(&victim.short_code).await.expect("read");
    checks.check(
        "9: the restore does not touch the task of bob",
        after_restore == before,
        format!("{after_restore:?}"),
    );
    checks.check(
        "9: the parent edge is as it was",
        edges_between(&mut conn, &attack.short_code, &victim.short_code, "parent") == 1,
        "the edge changed",
    );

    // =======================================================================
    // Case 2. The same attack over MCP `delete_item` (criterion h)
    // =======================================================================
    let victim_mcp = task_by(&bob, &platform_board, "2: the task of bob").await;
    let reply = alice_mcp
        .call(
            "link_items",
            json!({"source": attack_mcp.short_code, "target": victim_mcp.short_code,
                   "relationship": "parent"}),
        )
        .await;
    assert!(
        !reply.0,
        "alice links over MCP (the link rule): {}",
        reply.1
    );
    let before = svc.get_task(&victim_mcp.short_code).await.expect("read");
    let reply = alice_mcp
        .call(
            "delete_item",
            json!({"short_code": attack_mcp.short_code, "confirm": true}),
        )
        .await;
    checks.check(
        "2 MCP: alice archives her initiative",
        !reply.0 && is_archived(&mut conn, &attack_mcp.short_code),
        &reply.1,
    );
    let after = svc.get_task(&victim_mcp.short_code).await.expect("read");
    checks.check(
        "2 MCP (b, h): the task of bob is LIVE, in its column, unchanged",
        after == before && !is_archived(&mut conn, &victim_mcp.short_code),
        format!("before {before:?}\n      after  {after:?}"),
    );
    checks.check(
        "2 MCP (d, h): the output names the task of bob as not reached, with the reason",
        reply.1.contains("did not reach 1 item(s)")
            && reply.1.contains(&format!(
                "- {}: you need `manage_tasks` on board {platform_board}.",
                victim_mcp.short_code
            )),
        &reply.1,
    );
    checks.check(
        "2 MCP: the output does not list the task of bob as deleted",
        reply.1.contains("Cascade: none."),
        &reply.1,
    );
    checks.check(
        "2 MCP (f): the parent edge still exists",
        edges_between(
            &mut conn,
            &attack_mcp.short_code,
            &victim_mcp.short_code,
            "parent",
        ) == 1,
        "the edge is gone",
    );

    // =======================================================================
    // Case 8. The preview of the mixed children, for alice and for the admin
    // (criterion e). Before case 3, which archives them.
    // =======================================================================
    let three = mixed(
        &svc,
        &alice,
        &bob,
        mixed_alice,
        &platform_board,
        &web_board,
        "3",
    )
    .await;
    let code = three.initiative.short_code.clone();
    let preview = checks.allowed(
        "8: alice asks for the preview",
        alice.cascade_preview(EntityKind::Initiative, &code).await,
    );
    if let Some(preview) = preview {
        let mut want = vec![
            three.created.short_code.clone(),
            three.managed.short_code.clone(),
        ];
        want.sort();
        checks.check(
            "8 (e): the preview for alice shows two that the archive reaches",
            preview.cascaded_short_codes == want && preview.cascade_count == 2,
            format!("{preview:?}"),
        );
        checks.check(
            "8 (e): the preview for alice shows one that it does not reach, with the reason",
            preview.not_reached.len() == 1
                && cannot_edit(
                    &preview.not_reached,
                    &three.of_bob.short_code,
                    "manage_tasks",
                    &platform_board,
                ),
            format!("{:?}", preview.not_reached),
        );
    }
    let preview = checks.allowed(
        "8: the admin asks for the preview",
        svc.cascade_preview(EntityKind::Initiative, &code).await,
    );
    if let Some(preview) = preview {
        checks.check(
            "8 (e): the preview for the admin shows three, and leaves none",
            preview.cascade_count == 3
                && preview.cascaded_short_codes.len() == 3
                && preview.not_reached.is_empty(),
            format!("{preview:?}"),
        );
    }
    checks.check(
        "8: a preview archives nothing",
        [
            &code,
            &three.created.short_code,
            &three.managed.short_code,
            &three.of_bob.short_code,
        ]
        .iter()
        .all(|code| !is_archived(&mut conn, code)),
        "a preview changed a row",
    );

    // =======================================================================
    // Case 3. Mixed children (criteria a, b, d)
    // =======================================================================
    let before = svc.get_task(&three.of_bob.short_code).await.expect("read");
    let response = checks.allowed(
        "3: alice archives the initiative with mixed children",
        alice.delete_initiative(&code).await,
    );
    if let Some(response) = &response {
        checks.reach(
            "3 (a, d)",
            response,
            &[&three.created.short_code, &three.managed.short_code],
            &[&three.of_bob.short_code],
        );
        checks.check(
            "3 (d): the reason is the capability and the board",
            cannot_edit(
                &response.not_reached,
                &three.of_bob.short_code,
                "manage_tasks",
                &platform_board,
            ),
            format!("{:?}", response.not_reached),
        );
    }
    checks.check(
        "3 (a): the task that alice created is archived",
        is_archived(&mut conn, &three.created.short_code),
        "still live",
    );
    checks.check(
        "3 (a): the task on the board that alice manages is archived",
        is_archived(&mut conn, &three.managed.short_code),
        "still live",
    );
    let after = svc.get_task(&three.of_bob.short_code).await.expect("read");
    checks.check(
        "3 (b): the task of bob is LIVE and unchanged",
        after == before && !is_archived(&mut conn, &three.of_bob.short_code),
        format!("{after:?}"),
    );
    let mut went = [
        three.created.short_code.clone(),
        three.managed.short_code.clone(),
    ];
    went.sort();
    let record = archive_record(&mut conn, &code);
    checks.check(
        "3: the record of the archive names what went and what stayed",
        record
            == format!(
                "short_code:{code} cascade:2 descendants:{} not_reached:{}",
                went.join(","),
                three.of_bob.short_code
            ),
        &record,
    );
    checks.check(
        "3 (f): the task of bob keeps its parent edge",
        edges_between(&mut conn, &code, &three.of_bob.short_code, "parent") == 1,
        "the edge is gone",
    );

    // =======================================================================
    // Case 6. The org admin archives everything in the mixed case
    // (criterion g)
    // =======================================================================
    let six = mixed(
        &svc,
        &alice,
        &bob,
        mixed_admin,
        &platform_board,
        &web_board,
        "6",
    )
    .await;
    let response = checks.allowed(
        "6: the admin archives the initiative with mixed children",
        svc.delete_initiative(&six.initiative.short_code).await,
    );
    if let Some(response) = &response {
        checks.reach(
            "6 (g)",
            response,
            &[
                &six.created.short_code,
                &six.managed.short_code,
                &six.of_bob.short_code,
            ],
            &[],
        );
    }
    let record = archive_record(&mut conn, &six.initiative.short_code);
    checks.check(
        "6: the record of the archive is as it was when nothing stays",
        record.contains(" cascade:3 descendants:") && !record.contains("not_reached"),
        &record,
    );
    checks.check(
        "6 (g): each of the three tasks is archived",
        [&six.created, &six.managed, &six.of_bob]
            .iter()
            .all(|task| is_archived(&mut conn, &task.short_code)),
        "a task is still live",
    );

    // =======================================================================
    // Case 4. Depth: strategy -> initiative (not editable) -> task (created
    // by the caller). The archive stops at the initiative (criterion c)
    // =======================================================================
    let middle = initiative_by(&svc, &initiative_board, "4: the initiative of the org").await;
    let leaf = task_by(&alice, &platform_board, "4: the task of alice").await;
    // alice edits the source of the first edge and the target of the second.
    parent(&alice, &alice_strategy.short_code, &middle.short_code).await;
    parent(&alice, &middle.short_code, &leaf.short_code).await;
    let middle_before = svc.get_initiative(&middle.short_code).await.expect("read");
    let leaf_before = svc.get_task(&leaf.short_code).await.expect("read");
    let response = checks.allowed(
        "4: alice archives her strategy",
        alice.delete_strategy(&alice_strategy.short_code).await,
    );
    checks.check(
        "4: the strategy is archived",
        is_archived(&mut conn, &alice_strategy.short_code),
        "still live",
    );
    checks.check(
        "4 (b): the initiative that alice cannot edit is LIVE and unchanged",
        svc.get_initiative(&middle.short_code).await.expect("read") == middle_before
            && !is_archived(&mut conn, &middle.short_code),
        "the initiative changed",
    );
    checks.check(
        "4 (c): the task below it is LIVE and unchanged, though alice created it",
        svc.get_task(&leaf.short_code).await.expect("read") == leaf_before
            && !is_archived(&mut conn, &leaf.short_code),
        "the task below the stop was archived",
    );
    if let Some(response) = &response {
        checks.reach(
            "4 (c, d)",
            response,
            &[],
            &[&middle.short_code, &leaf.short_code],
        );
        checks.check(
            "4 (d): the initiative has the capability and the board as its reason",
            cannot_edit(
                &response.not_reached,
                &middle.short_code,
                "manage_initiatives",
                &initiative_board,
            ),
            format!("{:?}", response.not_reached),
        );
        checks.check(
            "4 (d): the task has 'below the initiative' as its reason",
            is_below(&response.not_reached, &leaf.short_code, &middle.short_code),
            format!("{:?}", response.not_reached),
        );
    }
    checks.check(
        "4 (f): the two edges still exist",
        edges_between(
            &mut conn,
            &alice_strategy.short_code,
            &middle.short_code,
            "parent",
        ) == 1
            && edges_between(&mut conn, &middle.short_code, &leaf.short_code, "parent") == 1,
        "an edge is gone",
    );

    // =======================================================================
    // Case 5. Depth the other way: initiative (editable) -> task (editable)
    // (criterion a). The wire has no `not_reached` when nothing stays.
    // =======================================================================
    let own = task_by(&alice, &web_board, "5: the task of alice").await;
    parent(&alice, &both.short_code, &own.short_code).await;
    let (status, body) = raw(
        &http,
        reqwest::Method::DELETE,
        &base,
        &format!("/api/initiatives/{}", both.short_code),
        &alice_token,
    )
    .await;
    checks.check(
        "5 (a): alice archives the initiative and its task",
        status == 200
            && body["cascaded_short_codes"] == json!([own.short_code])
            && body["cascade_count"] == json!(1),
        format!("{status} {body}"),
    );
    checks.check(
        "5: the response has no `not_reached` field: it is as it was",
        body.get("not_reached").is_none(),
        format!("{body}"),
    );
    checks.check(
        "5 (a): the two rows are archived",
        is_archived(&mut conn, &both.short_code) && is_archived(&mut conn, &own.short_code),
        "a row is still live",
    );

    // =======================================================================
    // Case 7. bob manages each board involved: everything archived, as
    // before, with no "not reached" entry
    // =======================================================================
    let managed = initiative_by(&svc, &initiative_board, "7: an initiative of the org").await;
    let first = task_by(&svc, &platform_board, "7: a task on platform").await;
    let second = task_by(&svc, &platform_board, "7: a second task on platform").await;
    parent(&svc, &managed.short_code, &first.short_code).await;
    parent(&svc, &managed.short_code, &second.short_code).await;
    let (status, body) = raw(
        &http,
        reqwest::Method::DELETE,
        &base,
        &format!("/api/initiatives/{}", managed.short_code),
        &bob_token,
    )
    .await;
    let mut want = vec![first.short_code.clone(), second.short_code.clone()];
    want.sort();
    checks.check(
        "7: bob archives the initiative and the two tasks, which he did not create",
        status == 200 && body["cascaded_short_codes"] == json!(want),
        format!("{status} {body}"),
    );
    checks.check(
        "7: the response has no `not_reached` field",
        body.get("not_reached").is_none(),
        format!("{body}"),
    );
    checks.check(
        "7: the three rows are archived",
        [&managed.short_code, &first.short_code, &second.short_code]
            .iter()
            .all(|code| is_archived(&mut conn, code)),
        "a row is still live",
    );
    // The same over MCP for a manager: the output is as it was.
    let managed = initiative_by(&svc, &initiative_board, "7: an MCP initiative").await;
    let only = task_by(&svc, &platform_board, "7: an MCP task on platform").await;
    parent(&svc, &managed.short_code, &only.short_code).await;
    let mut bob_mcp = McpSession::open(&base, &bob_token, "acme").await;
    let reply = bob_mcp
        .call(
            "delete_item",
            json!({"short_code": managed.short_code, "confirm": true}),
        )
        .await;
    checks.check(
        "7 MCP: the output of delete_item is as it was",
        !reply.0
            && reply.1
                == format!(
                    "Deleted {} (soft delete).\nCascade deleted 1 descendant(s): {}\n",
                    managed.short_code, only.short_code
                ),
        &reply.1,
    );

    // =======================================================================
    // Case 10. Nothing is archived when the caller cannot edit the NAMED
    // item
    // =======================================================================
    let guarded = initiative_by(&svc, &initiative_board, "10: an initiative of the org").await;
    // carol created the child: she can edit it, and it changes nothing.
    let child = task_by(&carol, &platform_board, "10: the request of carol").await;
    parent(&carol, &guarded.short_code, &child.short_code).await;
    checks.refused(
        "10 REST: carol archives an initiative that she cannot edit",
        carol.delete_initiative(&guarded.short_code).await,
    );
    let reply = carol_mcp
        .call(
            "delete_item",
            json!({"short_code": guarded.short_code, "confirm": true}),
        )
        .await;
    checks.check(
        "10 MCP: carol archives an initiative that she cannot edit",
        reply.0 && reply.1.contains("FORBIDDEN"),
        &reply.1,
    );
    checks.check(
        "10: nothing is archived, the initiative nor the child of carol",
        !is_archived(&mut conn, &guarded.short_code) && !is_archived(&mut conn, &child.short_code),
        "a refused archive changed a row",
    );

    // =======================================================================
    // Case 11. Documents and ADRs. They attach by `supports`, and the
    // cascade follows `parent` only: an archive of the parent does not
    // touch them, whoever asks.
    // =======================================================================
    let document = svc
        .create_document(&CreateDocumentRequest {
            title: "11: the document of the organization".into(),
            content: Some("original content".into()),
            template_id: None,
            parent_short_code: Some(with_documents.short_code.clone()),
        })
        .await
        .expect("document");
    let adr = svc
        .create_adr(&CreateAdrRequest {
            board_id: Some(adr_board.clone()),
            column_id: None,
            title: "11: the ADR of the organization".into(),
            content: "original content".into(),
            decision_maker: None,
            decision_date: None,
        })
        .await
        .expect("adr");
    svc.create_relationship(&edge(
        &with_documents.short_code,
        &adr.short_code,
        "supports",
    ))
    .await
    .expect("the ADR supports the initiative");
    // alice cannot edit the document: she did not create it, and holds no
    // `manage_documents` on the initiative board.
    match alice.delete_document(&document.short_code).await {
        Err(Error::Forbidden { .. }) => {}
        other => panic!("alice cannot edit the document: {other:?}"),
    }
    let document_before = svc.get_document(&document.short_code).await.expect("read");
    let adr_before = svc.get_adr(&adr.short_code).await.expect("read");
    let response = checks.allowed(
        "11: alice archives the initiative that the document supports",
        alice.delete_initiative(&with_documents.short_code).await,
    );
    if let Some(response) = &response {
        checks.reach("11", response, &[], &[]);
    }
    checks.check(
        "11: the document that alice cannot edit is untouched",
        svc.get_document(&document.short_code).await.expect("read") == document_before
            && !is_archived(&mut conn, &document.short_code),
        "the document changed",
    );
    checks.check(
        "11: the ADR that alice cannot edit is untouched",
        svc.get_adr(&adr.short_code).await.expect("read") == adr_before
            && !is_archived(&mut conn, &adr.short_code),
        "the ADR changed",
    );
    checks.check(
        "11: the `supports` edges still exist",
        edges_between(
            &mut conn,
            &with_documents.short_code,
            &document.short_code,
            "supports",
        ) == 1
            && edges_between(
                &mut conn,
                &with_documents.short_code,
                &adr.short_code,
                "supports",
            ) == 1,
        "an edge is gone",
    );

    // =======================================================================
    // Case 12. carol edits neither end, proposes, and confirms: 403, no
    // edge, and the proposal is still pending (criterion i)
    // =======================================================================
    let source = initiative_by(&svc, &initiative_board, "12: an initiative of the org").await;
    let target = task_by(&bob, &platform_board, "12: the task of bob").await;
    let reply = carol_mcp
        .call(
            "propose_edge",
            json!({"source": source.short_code, "target": target.short_code,
                   "relationship": "parent", "why": "carol edits neither end"}),
        )
        .await;
    checks.check(
        "12: a proposal is a suggestion, and carol can make one",
        !reply.0 && reply.1.contains("NOT an edge yet"),
        &reply.1,
    );
    checks.check(
        "12: a proposal writes no edge",
        parents_of(&mut conn, &target.short_code) == 0,
        "the proposal wrote an edge",
    );
    let waiting = pending(&http, &base, &target.short_code, &carol_token).await;
    assert_eq!(waiting.len(), 1, "one proposal waits on the task of bob");
    let proposal = waiting[0]["id"].as_str().expect("proposal id").to_string();
    let (status, body) = raw(
        &http,
        reqwest::Method::POST,
        &base,
        &format!("/api/proposals/{proposal}/confirm"),
        &carol_token,
    )
    .await;
    checks.check(
        "12 (i): the confirm by carol is 403 FORBIDDEN",
        status == 403 && body["error"]["code"] == json!("FORBIDDEN"),
        format!("{status} {body}"),
    );
    let details = &body["error"]["details"];
    checks.check(
        "12 (i): the refusal has the details of each other edge refusal",
        details["relationship"] == json!("parent")
            && details["required_capability"] == json!("manage_initiatives")
            && details["board_id"] == json!(initiative_board)
            && details["any_of"][0]["end"] == json!("source")
            && details["any_of"][1]["end"] == json!("target")
            && details["any_of"][1]["required_capability"] == json!("manage_tasks")
            && details["any_of"][1]["board_id"] == json!(platform_board),
        format!("{details}"),
    );
    checks.check(
        "12 (i): no edge was written",
        parents_of(&mut conn, &target.short_code) == 0,
        "the refused confirm wrote an edge",
    );
    let waiting = pending(&http, &base, &target.short_code, &svc_token).await;
    checks.check(
        "12: the proposal is still pending: a refused confirm does not consume it",
        waiting.len() == 1
            && waiting[0]["id"] == json!(proposal)
            && waiting[0]["state"] == json!("pending"),
        format!("{waiting:?}"),
    );

    // =======================================================================
    // Case 15. After the refused confirm, the attack cannot start by that
    // route: an archive by carol reaches nothing of bob
    // =======================================================================
    let request = task_by(&carol, &platform_board, "15: the request of carol").await;
    parent(&carol, &carol_initiative.short_code, &request.short_code).await;
    let bob_before = svc.get_task(&target.short_code).await.expect("read");
    let response = checks.allowed(
        "15: carol archives her own initiative",
        carol.delete_initiative(&carol_initiative.short_code).await,
    );
    if let Some(response) = &response {
        checks.reach("15", response, &[&request.short_code], &[]);
    }
    checks.check(
        "15: the task of bob is LIVE and unchanged, with no parent",
        svc.get_task(&target.short_code).await.expect("read") == bob_before
            && !is_archived(&mut conn, &target.short_code)
            && parents_of(&mut conn, &target.short_code) == 0,
        "the task of bob changed",
    );
    checks.check(
        "15: the initiative of the org in the proposal is LIVE",
        !is_archived(&mut conn, &source.short_code),
        "the source of the refused proposal was archived",
    );

    // =======================================================================
    // Case 13. alice edits one end and confirms: the edge exists
    // (criterion j). An agent made the proposal.
    // =======================================================================
    let blocker = task_by(&alice, &web_board, "13: the task of alice").await;
    let blocked = task_by(&bob, &platform_board, "13: the task of bob").await;
    let reply = maker_mcp
        .call(
            "propose_edge",
            json!({"source": blocker.short_code, "target": blocked.short_code,
                   "relationship": "blocks", "why": "an agent sees a dependency"}),
        )
        .await;
    assert!(!reply.0, "the agent proposes: {}", reply.1);
    let waiting = pending(&http, &base, &blocked.short_code, &alice_token).await;
    assert_eq!(waiting.len(), 1, "one proposal waits");
    let proposal = waiting[0]["id"].as_str().expect("proposal id").to_string();
    // carol edits neither end of this one either.
    let (status, body) = raw(
        &http,
        reqwest::Method::POST,
        &base,
        &format!("/api/proposals/{proposal}/confirm"),
        &carol_token,
    )
    .await;
    checks.check(
        "13 (i): carol cannot confirm the proposal of an agent",
        status == 403
            && body["error"]["code"] == json!("FORBIDDEN")
            && edges_between(
                &mut conn,
                &blocker.short_code,
                &blocked.short_code,
                "blocks",
            ) == 0,
        format!("{status} {body}"),
    );
    let (status, body) = raw(
        &http,
        reqwest::Method::POST,
        &base,
        &format!("/api/proposals/{proposal}/confirm"),
        &alice_token,
    )
    .await;
    checks.check(
        "13 (j): alice edits the source, and her confirm succeeds",
        status == 200 && body["state"] == json!("confirmed"),
        format!("{status} {body}"),
    );
    checks.check(
        "13 (j): the edge exists",
        edges_between(
            &mut conn,
            &blocker.short_code,
            &blocked.short_code,
            "blocks",
        ) == 1,
        "no edge",
    );
    // bob edits the TARGET of a proposal, and confirms: either end.
    let second = task_by(&bob, &platform_board, "13: a second task of bob").await;
    let reply = maker_mcp
        .call(
            "propose_edge",
            json!({"source": blocker.short_code, "target": second.short_code,
                   "relationship": "blocks", "why": "an agent sees a second dependency"}),
        )
        .await;
    assert!(!reply.0, "the agent proposes: {}", reply.1);
    let waiting = pending(&http, &base, &second.short_code, &bob_token).await;
    let proposal = waiting[0]["id"].as_str().expect("proposal id").to_string();
    let (status, body) = raw(
        &http,
        reqwest::Method::POST,
        &base,
        &format!("/api/proposals/{proposal}/confirm"),
        &bob_token,
    )
    .await;
    checks.check(
        "13 (j): bob edits the target, and his confirm succeeds",
        status == 200
            && edges_between(&mut conn, &blocker.short_code, &second.short_code, "blocks") == 1,
        format!("{status} {body}"),
    );

    // =======================================================================
    // Case 14. A service account cannot confirm, as before. It created the
    // source, so the link rule is not what refuses it.
    // =======================================================================
    let by_maker = task_by(&maker, &platform_board, "14: the request of the agent").await;
    let other = task_by(&bob, &platform_board, "14: the task of bob").await;
    let reply = maker_mcp
        .call(
            "propose_edge",
            json!({"source": by_maker.short_code, "target": other.short_code,
                   "relationship": "blocks", "why": "the agent proposes on its own item"}),
        )
        .await;
    assert!(!reply.0, "the agent proposes: {}", reply.1);
    let waiting = pending(&http, &base, &other.short_code, &svc_token).await;
    let proposal = waiting[0]["id"].as_str().expect("proposal id").to_string();
    for verb in ["confirm", "reject"] {
        let (status, body) = raw(
            &http,
            reqwest::Method::POST,
            &base,
            &format!("/api/proposals/{proposal}/{verb}"),
            &maker_key,
        )
        .await;
        checks.check(
            &format!("14: a service account cannot {verb}"),
            status == 403
                && body["error"]["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("service account")),
            format!("{status} {body}"),
        );
    }
    let waiting = pending(&http, &base, &other.short_code, &svc_token).await;
    checks.check(
        "14: no edge, and the proposal is still pending",
        edges_between(&mut conn, &by_maker.short_code, &other.short_code, "blocks") == 0
            && waiting.len() == 1
            && waiting[0]["state"] == json!("pending"),
        format!("{waiting:?}"),
    );
    // A reject writes no edge, so it has no link rule: carol can reject.
    let (status, body) = raw(
        &http,
        reqwest::Method::POST,
        &base,
        &format!("/api/proposals/{proposal}/reject"),
        &carol_token,
    )
    .await;
    checks.check(
        "14: a person who edits neither end can reject, as before, and no edge results",
        status == 200
            && body["state"] == json!("rejected")
            && edges_between(&mut conn, &by_maker.short_code, &other.short_code, "blocks") == 0,
        format!("{status} {body}"),
    );

    drop((alice_mcp, bob_mcp, carol_mcp, svc_mcp, maker_mcp));
    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
    checks.finish();
}
