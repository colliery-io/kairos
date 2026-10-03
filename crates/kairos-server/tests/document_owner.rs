//! Integration test for COLLIERY-T-0269: a document names its board, and
//! impacts a repository.
//!
//! THE MODEL (the owner decided it on 2026-09-29).
//!
//! | Link | What it says |
//! |---|---|
//! | document -> board | The OWNER. The board gives the right to edit. |
//! | document -> repository, `impacts` | What the document is ABOUT. It gives NO right. |
//!
//! THE RULES.
//! 1. A document has an owner at its create: `board`, or a parent, or the
//!    two. With none of the two the create is refused.
//! 2. The authorization board of a document is the board that it names.
//!    When it names none, it is the board of its earliest `supports`
//!    parent, as before.
//! 3. The edit rule does not change: the creator, `manage_documents` on
//!    the authorization board, an organization admin.
//! 4. The last `supports` edge of a document can go when the document
//!    names a board, and cannot when it names none (422 `LAST_PARENT`).
//!    The board of a document that supports nothing cannot go (422
//!    `LAST_OWNER`).
//! 5. The change of the owner is a move: `manage_documents` on the board
//!    that owns the document now AND on the new board.
//! 6. An `impacts` link needs the right to edit the document or the ADR,
//!    and no right on the repository. The link gives no right.
//! 7. A document that names a board is not a card of that board.
//!
//! This is a security boundary, so the negative cases are the contract.
//! Each refusal is checked where the data is stored.
//!
//! Every check is recorded and the test fails at the end with the full
//! list, so one run shows each broken case and not only the first.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_document_owner_t0269_test`.
//!
//! Cast:
//! - `svc`   — org admin: sets the world up,
//! - `alice` — member of team `web`; NO grant on the board of `platform`,
//! - `bob`   — member of team `platform`,
//! - `carol` — org member in no team.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable, Text};
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types::{
    Adr, CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest, CreateStrategyRequest,
    CreateTaskRequest, Document, ImpactListQuery, Pagination, Task, UpdateContentRequest,
};
use kairos_client::types_meta::{ActivityQuery, CreateRelationshipRequest};
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_repositories::CreateRepositoryRequest;
use kairos_client::types_search::{SearchFilter, SearchRequest};
use kairos_client::{EntityKind, Error, KairosClient};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_document_owner_t0269_test";

/// The text of `LAST_PARENT`, for the document `{0}` and the parent `{1}`.
/// COLLIERY-T-0269 did not change it.
fn last_parent_text(document: &str, parent: &str) -> String {
    format!(
        "{document} supports only {parent}. A document always has a parent. Link the document \
         to a different item first, or archive the document."
    )
}

/// The text of `LAST_OWNER`.
fn last_owner_text(document: &str) -> String {
    format!(
        "{document} supports no item. A document always has an owner. Link the document to a \
         work item first, or name a different board."
    )
}

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

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .count
}

/// The number of documents of the tenant, archived or not.
fn document_count(conn: &mut PgConnection) -> i64 {
    count(conn, "SELECT count(*)::bigint AS count FROM documents")
}

/// The number of `impacts` links of the tenant.
fn impact_count(conn: &mut PgConnection) -> i64 {
    count(conn, "SELECT count(*)::bigint AS count FROM item_impacts")
}

/// The number of entries of the activity log of the tenant.
fn activity_count(conn: &mut PgConnection) -> i64 {
    count(conn, "SELECT count(*)::bigint AS count FROM activity_log")
}

/// The number of `supports` edges that point at `target`.
fn parents_of(conn: &mut PgConnection, target: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*)::bigint AS count \
           FROM item_relationships r \
           JOIN entity_directory t ON t.id = r.target_id \
          WHERE t.short_code = $1 AND r.relationship::text = 'supports'",
    )
    .bind::<Text, _>(target)
    .get_result::<Count>(conn)
    .expect("counting parents")
    .count
}

/// The number of `impacts` links from `item` to the repository `slug`,
/// read where the links are stored.
fn impacts_between(conn: &mut PgConnection, item: &str, slug: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*)::bigint AS count \
           FROM item_impacts i \
           JOIN entity_directory d ON d.id = i.item_id \
           JOIN repositories r ON r.id = i.target_id \
          WHERE d.short_code = $1 AND r.slug = $2 AND i.target_kind = 'repository'",
    )
    .bind::<Text, _>(item)
    .bind::<Text, _>(slug)
    .get_result::<Count>(conn)
    .expect("counting impacts")
    .count
}

/// `documents.board_id` of the document, read from the table.
fn stored_board(conn: &mut PgConnection, document: &str) -> Option<String> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Nullable<Text>)]
        board_id: Option<String>,
    }
    diesel::sql_query("SELECT board_id::text AS board_id FROM documents WHERE short_code = $1")
        .bind::<Text, _>(document)
        .get_result::<Row>(conn)
        .unwrap_or_else(|e| panic!("no document {document}: {e}"))
        .board_id
}

/// `updated_at` of the document as text, read from the table.
fn stored_updated_at(conn: &mut PgConnection, document: &str) -> String {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Text)]
        updated_at: String,
    }
    diesel::sql_query("SELECT updated_at::text AS updated_at FROM documents WHERE short_code = $1")
        .bind::<Text, _>(document)
        .get_result::<Row>(conn)
        .unwrap_or_else(|e| panic!("no document {document}: {e}"))
        .updated_at
}

/// The id of the edge, read where the edges are stored.
fn edge_id(conn: &mut PgConnection, source: &str, target: &str, relationship: &str) -> String {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Text)]
        id: String,
    }
    diesel::sql_query(
        "SELECT r.id::text AS id \
           FROM item_relationships r \
           JOIN entity_directory s ON s.id = r.source_id \
           JOIN entity_directory t ON t.id = r.target_id \
          WHERE s.short_code = $1 AND t.short_code = $2 \
            AND r.relationship::text = $3",
    )
    .bind::<Text, _>(source)
    .bind::<Text, _>(target)
    .bind::<Text, _>(relationship)
    .get_result::<Row>(conn)
    .unwrap_or_else(|e| panic!("no edge {source} -[{relationship}]-> {target}: {e}"))
    .id
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

    /// The two values are equal.
    fn same<T: PartialEq + std::fmt::Debug>(&mut self, case: &str, got: T, want: T) {
        let holds = got == want;
        self.check(case, holds, format!("got {got:?}, want {want:?}"));
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

    /// The call must be refused with 403 `FORBIDDEN`. Returns the message
    /// and the details of the refusal.
    fn refused<T: std::fmt::Debug>(
        &mut self,
        case: &str,
        result: Result<T, Error>,
    ) -> Option<(String, Value)> {
        match result {
            Err(Error::Forbidden {
                code,
                message,
                details,
                ..
            }) if code == "FORBIDDEN" => {
                self.passed += 1;
                Some((message, details))
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

    /// The call must be refused with this status and this code. Returns
    /// the message and the details of the refusal.
    fn refused_with<T: std::fmt::Debug>(
        &mut self,
        case: &str,
        want_status: u16,
        want_code: &str,
        result: Result<T, Error>,
    ) -> Option<(String, Value)> {
        let got = match result {
            Ok(value) => {
                self.failures.push(format!(
                    "{case}: expected {want_status} {want_code}, but the call SUCCEEDED: \
                     {value:?}"
                ));
                return None;
            }
            Err(Error::Other {
                status,
                code,
                message,
                details,
            }) => (status, code, message, details),
            Err(Error::Validation {
                status,
                message,
                details,
                ..
            }) => (status, "VALIDATION".to_string(), message, details),
            Err(Error::NotFound {
                code,
                message,
                details,
            }) => (404, code, message, details),
            Err(Error::Forbidden {
                code,
                message,
                details,
                ..
            }) => (403, code, message, details),
            Err(err) => {
                self.failures.push(format!(
                    "{case}: expected {want_status} {want_code}, got {err}"
                ));
                return None;
            }
        };
        let (status, code, message, details) = got;
        if status == want_status && code == want_code {
            self.passed += 1;
            Some((message, details))
        } else {
            self.failures.push(format!(
                "{case}: expected {want_status} {want_code}, got {status} {code}: {message}"
            ));
            None
        }
    }

    /// The MCP call must succeed, with this text.
    fn mcp_text(&mut self, case: &str, (is_error, text): &(bool, String), want: &str) {
        self.check(
            case,
            !is_error && text == want,
            format!("expected the text {want:?}, got is_error={is_error}: {text:?}"),
        );
    }

    /// The MCP call must be refused with this code.
    fn mcp_refused(&mut self, case: &str, code: &str, (is_error, text): &(bool, String)) {
        self.check(
            case,
            *is_error && text.starts_with(&format!("{code}: ")),
            format!("expected {code}, got is_error={is_error}: {text}"),
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
        eprintln!("document_owner: {} checks passed", self.passed);
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
                    "clientInfo": {"name": "kairos-t0269-test", "version": "0.0.0"},
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

    /// One JSON-RPC request; returns the `result`.
    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("X-Tenant", &self.tenant)
            .header("mcp-session-id", &self.session_id)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params,
            }))
            .send()
            .await
            .expect("rpc");
        assert_eq!(response.status(), 200, "{method} HTTP status");
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
        message["result"].clone()
    }

    /// Call a tool; returns `(is_error, text)`.
    async fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        let result = self
            .rpc("tools/call", json!({"name": tool, "arguments": arguments}))
            .await;
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content: {result}"))
            .to_string();
        (is_error, text)
    }

    async fn link(&mut self, tool: &str, source: &str, target: &str, rel: &str) -> (bool, String) {
        self.call(
            tool,
            json!({"source": source, "target": target, "relationship": rel}),
        )
        .await
    }
}

async fn task_by(client: &KairosClient, board: &str, title: &str, repo: Option<&str>) -> Task {
    client
        .create_task(&CreateTaskRequest {
            board_id: Some(board.into()),
            column_id: None,
            title: title.into(),
            content: "original content".into(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: repo.map(str::to_string),
        })
        .await
        .unwrap_or_else(|e| panic!("creating task {title:?}: {e}"))
}

fn document(board: Option<&str>, parent: Option<&str>, title: &str) -> CreateDocumentRequest {
    CreateDocumentRequest {
        title: title.into(),
        board: board.map(str::to_string),
        content: Some("original content".into()),
        template_id: None,
        parent_short_code: parent.map(str::to_string),
    }
}

/// A precondition of the fixture: a document as `client`. The test stops
/// if the server refuses it.
async fn document_by(
    client: &KairosClient,
    board: Option<&str>,
    parent: Option<&str>,
    title: &str,
) -> Document {
    client
        .create_document(&document(board, parent, title))
        .await
        .unwrap_or_else(|e| panic!("creating document {title:?}: {e}"))
}

async fn adr_by(client: &KairosClient, board: &str, title: &str) -> Adr {
    client
        .create_adr(&CreateAdrRequest {
            board_id: Some(board.into()),
            column_id: None,
            title: title.into(),
            content: "original content".into(),
            decision_maker: None,
            decision_date: None,
        })
        .await
        .unwrap_or_else(|e| panic!("creating ADR {title:?}: {e}"))
}

async fn repository_by(client: &KairosClient, slug: &str, team: &str) {
    client
        .create_repository(&CreateRepositoryRequest {
            slug: Some(slug.into()),
            forge: "github".into(),
            repo_full_name: format!("acme/{slug}"),
            repo_url: format!("https://github.com/acme/{slug}"),
            default_branch: None,
            team: team.into(),
            description: None,
        })
        .await
        .unwrap_or_else(|e| panic!("creating repository {slug}: {e}"));
}

/// Try to edit the document as `client`: the title stays, the content is
/// `text`. The edit rule decides.
async fn edit(client: &KairosClient, document: &str, text: &str) -> Result<Document, Error> {
    let current = client.get_document(document).await.expect("read");
    client
        .update_document(
            document,
            &UpdateContentRequest {
                title: None,
                content: text.into(),
                version: current.version,
            },
        )
        .await
}

async fn grant(svc: &KairosClient, board: &str, user: Uuid, capability: &str) {
    svc.add_board_member(
        board,
        &AddBoardMemberRequest {
            user_id: user.to_string(),
            capabilities: vec![capability.into()],
        },
    )
    .await
    .unwrap_or_else(|e| panic!("granting {capability} on {board}: {e}"));
}

/// The entries of the activity log about one item, the newest first.
async fn activity_of(client: &KairosClient, item_id: &str) -> Vec<(String, String)> {
    client
        .activity(&ActivityQuery {
            entity_id: Some(item_id.into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items
        .into_iter()
        .map(|entry| (entry.action, entry.details))
        .collect()
}

/// No sentence of the message has more than 20 words.
fn short_sentences(message: &str) -> bool {
    message
        .split(['.', '?', '!'])
        .all(|sentence| sentence.split_whitespace().count() <= 20)
}

/// The short codes of the result of a search, of the three types that a
/// repository filter keeps, sorted.
async fn search_codes(client: &KairosClient, filter: SearchFilter) -> Result<Vec<String>, Error> {
    let response = client
        .search(&SearchRequest {
            filter: Some(filter),
            limit: Some(100),
            ..SearchRequest::default()
        })
        .await?;
    let results = response.results;
    let mut codes: Vec<String> = results
        .strategies
        .into_iter()
        .map(|item| item.short_code)
        .chain(results.initiatives.into_iter().map(|item| item.short_code))
        .chain(results.tasks.into_iter().map(|item| item.short_code))
        .chain(results.documents.into_iter().map(|item| item.short_code))
        .chain(results.adrs.into_iter().map(|item| item.short_code))
        .collect();
    codes.sort();
    Ok(codes)
}

fn sorted(mut codes: Vec<String>) -> Vec<String> {
    codes.sort();
    codes
}

#[tokio::test]
async fn a_document_names_its_board_and_impacts_a_repository_against_live_stack() {
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
                code_prefix: kairos_core::short_code::prefix_from_slug(slug),
                team_type: None,
            })
            .await
            .expect("team"),
        );
    }
    let platform_board = teams[0].delivery_board_id.clone().expect("platform board");
    let web_board = teams[1].delivery_board_id.clone().expect("web board");
    const PLATFORM: &str = "platform-delivery";
    const WEB: &str = "web-delivery";
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

    // The repositories. `fidius` is of platform, `hlin` is of web, and
    // `old-lib` gets archived in the test.
    repository_by(&svc, "fidius", "platform").await;
    repository_by(&svc, "hlin", "web").await;
    repository_by(&svc, "old-lib", "platform").await;

    let mut checks = Checks::default();
    let mut alice_mcp = McpSession::open(&base, &alice_token, "acme").await;
    let mut bob_mcp = McpSession::open(&base, &bob_token, "acme").await;
    let mut svc_mcp = McpSession::open(&base, &svc_token, "acme").await;

    // `of_alice` is on the board of web: alice edits it as a member of the
    // team, and bob can NOT edit it. `of_bob` is on the board of platform.
    let of_alice = task_by(&alice, &web_board, "a task of alice", None).await;
    let of_bob = task_by(&bob, &platform_board, "a task of bob", Some("fidius")).await;

    // =======================================================================
    // A. The create
    // =======================================================================
    let created = checks.allowed(
        "A1 REST: a document with a board and no parent",
        bob.create_document(&document(Some(&platform_board), None, "A1: board only"))
            .await,
    );
    let a1 = created.expect("A1 is the base of later cases");
    checks.same(
        "A1: the response has the owner board",
        a1.board_id.clone(),
        Some(platform_board.clone()),
    );
    checks.same(
        "A1: the row has the owner board",
        stored_board(&mut conn, &a1.short_code),
        Some(platform_board.clone()),
    );
    checks.same(
        "A1: the document supports nothing",
        parents_of(&mut conn, &a1.short_code),
        0,
    );
    checks.check(
        "A1: the response has an empty list of impacts",
        a1.impacts.is_empty(),
        format!("{:?}", a1.impacts),
    );

    let a2 = checks
        .allowed(
            "A2 REST: a document with a parent and no board, as before",
            bob.create_document(&document(None, Some(&of_bob.short_code), "A2: parent only"))
                .await,
        )
        .expect("A2 is the base of later cases");
    checks.same(
        "A2: the document names no board",
        (a2.board_id.clone(), stored_board(&mut conn, &a2.short_code)),
        (None, None),
    );
    checks.same(
        "A2: the document has one parent",
        parents_of(&mut conn, &a2.short_code),
        1,
    );

    // The board of bob, and a parent that bob cannot edit: the creator of
    // the document can link it (the link rule).
    let a3 = checks
        .allowed(
            "A3 REST: a document with a board and a parent",
            bob.create_document(&document(
                Some(PLATFORM),
                Some(&of_alice.short_code),
                "A3: board and parent",
            ))
            .await,
        )
        .expect("A3 is the base of later cases");
    checks.same(
        "A3: the board is accepted by its slug, and it is the owner",
        a3.board_id.clone(),
        Some(platform_board.clone()),
    );
    checks.same(
        "A3: the document has one parent",
        parents_of(&mut conn, &a3.short_code),
        1,
    );

    let before = document_count(&mut conn);
    let refusal = checks.refused_with(
        "A4 REST: a document with no board and no parent",
        422,
        "VALIDATION",
        bob.create_document(&document(None, None, "A4: no owner"))
            .await,
    );
    if let Some((message, _)) = refusal {
        checks.same(
            "A4: the refusal says to send one of the two",
            message.as_str(),
            "The request has no board and no parent_short_code. A document must have an \
             owner. Send board: the slug or the id of the board that owns the document. Or \
             send parent_short_code: the short code of a strategy, an initiative, or a task \
             that the document supports. You can send the two.",
        );
        checks.check(
            "A4: the sentences are short",
            short_sentences(&message),
            &message,
        );
    }
    checks.refused_with(
        "A5 REST: a board that does not exist",
        404,
        "NOT_FOUND",
        bob.create_document(&document(Some("no-such-board"), None, "A5: no board"))
            .await,
    );
    let refusal = checks.refused(
        "A6 REST: alice has no manage_documents on the board of platform",
        alice
            .create_document(&document(Some(PLATFORM), None, "A6: by alice"))
            .await,
    );
    if let Some((message, details)) = refusal {
        checks.same(
            "A6: the refusal says why",
            message.as_str(),
            "This action requires the capability \"manage_documents\" on the board \
             \"platform-delivery\", which the document names as its owner. You do not have \
             that capability on that board. An organization admin has each capability.",
        );
        checks.same(
            "A6: the details name the capability and the board",
            (
                details["required_capability"].as_str(),
                details["board_id"].as_str(),
            ),
            (Some("manage_documents"), Some(platform_board.as_str())),
        );
    }
    // The parent of alice does not give her the board of bob.
    checks.refused(
        "A6 REST: a parent that alice can edit does not open the board",
        alice
            .create_document(&document(
                Some(PLATFORM),
                Some(&of_alice.short_code),
                "A6: by alice, with her parent",
            ))
            .await,
    );
    checks.same(
        "A4-A6: a refused create writes nothing",
        document_count(&mut conn),
        before,
    );

    // The same cases by MCP.
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "A7: MCP, board only", "board": PLATFORM}),
        )
        .await;
    let a7 = reply
        .1
        .split_whitespace()
        .nth(2)
        .map(|code| code.trim_end_matches(':').to_string());
    let a7 = a7.expect("the short code of A7");
    checks.mcp_text(
        "A7 MCP: a document with a board",
        &reply,
        &format!(
            "Created document {a7}: A7: MCP, board only (version 1), owner board \
             platform-delivery."
        ),
    );
    checks.same(
        "A7 MCP: the row has the owner board",
        stored_board(&mut conn, &a7),
        Some(platform_board.clone()),
    );
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "A7: MCP, the two", "board": PLATFORM,
                   "parent": of_alice.short_code}),
        )
        .await;
    checks.check(
        "A7 MCP: a document with a board and a parent",
        !reply.0
            && reply.1.ends_with(&format!(
                "(version 1), owner board platform-delivery, supports {}.",
                of_alice.short_code
            )),
        &reply.1,
    );
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "A7: MCP, parent only",
                   "parent": of_bob.short_code}),
        )
        .await;
    checks.check(
        "A7 MCP: a document with a parent has the text of before",
        !reply.0
            && reply
                .1
                .ends_with(&format!("(version 1), supports {}.", of_bob.short_code))
            && !reply.1.contains("owner board"),
        &reply.1,
    );
    let before = document_count(&mut conn);
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "A7: MCP, no owner"}),
        )
        .await;
    checks.same(
        "A7 MCP: a document with no board and no parent",
        reply.clone(),
        (
            true,
            "VALIDATION: The call has no `board` and no `parent`. A document must have an \
             owner. Send `board`: the slug or the id of the board that owns the document. Or \
             send `parent`: the short code of a strategy, an initiative, or a task that the \
             document supports. You can send the two."
                .to_string(),
        ),
    );
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "A7: MCP", "board": "no-such-board"}),
        )
        .await;
    checks.mcp_refused("A7 MCP: a board that does not exist", "NOT_FOUND", &reply);
    let reply = alice_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "A7: MCP, by alice", "board": PLATFORM}),
        )
        .await;
    checks.mcp_refused(
        "A7 MCP: alice on the board of platform",
        "FORBIDDEN",
        &reply,
    );
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "A7: MCP", "board": PLATFORM,
                   "repository": "fidius"}),
        )
        .await;
    checks.check(
        "A7 MCP: `repository` is refused for a document, and the refusal names link_items",
        reply.0 && reply.1.starts_with("VALIDATION: ") && reply.1.contains("link_items"),
        &reply.1,
    );
    checks.same(
        "A7 MCP: a refused create writes nothing",
        document_count(&mut conn),
        before,
    );

    // A8. A board of each level can own a document.
    for (level, board) in [
        ("strategy", &strategy_board),
        ("initiative", &initiative_board),
        ("adr", &adr_board),
        ("delivery", &web_board),
    ] {
        let created = checks.allowed(
            &format!("A8: a {level} board owns a document"),
            svc.create_document(&document(Some(board), None, &format!("A8: {level}")))
                .await,
        );
        if let Some(created) = created {
            checks.same(
                &format!("A8: the owner is the {level} board"),
                created.board_id,
                Some(board.to_string()),
            );
        }
    }
    // A member with a grant on a board of the organization.
    checks.refused(
        "A8: carol has no grant on the initiative board",
        carol
            .create_document(&document(Some(&initiative_board), None, "A8: by carol"))
            .await,
    );
    grant(&svc, &initiative_board, carol_id, "manage_documents").await;
    let of_carol = checks
        .allowed(
            "A8: carol has manage_documents on the initiative board",
            carol
                .create_document(&document(Some(&initiative_board), None, "A8: by carol"))
                .await,
        )
        .expect("the document of carol");

    // A9. A document that names a board is not a card of that board.
    let items = svc
        .board_items_all(&platform_board, &Default::default())
        .await;
    let items = items.expect("the items of the board of platform");
    let cards: Vec<String> = items
        .columns
        .iter()
        .flat_map(|column| {
            column
                .tasks
                .iter()
                .map(|task| task.short_code.clone())
                .chain(column.adrs.iter().map(|adr| adr.short_code.clone()))
                .chain(column.initiatives.iter().map(|i| i.short_code.clone()))
                .chain(column.strategies.iter().map(|s| s.short_code.clone()))
        })
        .collect();
    checks.same(
        "A9 REST: the items of the board have the task and no document",
        (sorted(cards), items.total),
        (vec![of_bob.short_code.clone()], 1),
    );
    let reply = svc_mcp
        .call("board_items", json!({"board": PLATFORM}))
        .await;
    checks.check(
        "A9 MCP: board_items has the task and no document",
        !reply.0 && reply.1.contains(&of_bob.short_code) && !reply.1.contains(&a1.short_code),
        &reply.1,
    );
    let found = search_codes(
        &svc,
        SearchFilter {
            board_id: Some(platform_board.clone()),
            ..SearchFilter::default()
        },
    )
    .await
    .expect("search by board");
    checks.same(
        "A9: the search filter board_id gives the cards only",
        found,
        vec![of_bob.short_code.clone()],
    );

    // =======================================================================
    // B. Who can edit a document that names a board
    // =======================================================================
    // `owned` names the board of platform and supports nothing. The admin
    // created it, so bob edits it as a member of platform only.
    let owned = document_by(&svc, Some(&platform_board), None, "B: owned by platform").await;
    checks.allowed(
        "B1: a manager of the owner board can edit",
        edit(&bob, &owned.short_code, "edited by bob").await,
    );
    checks.allowed(
        "B2: an organization admin can edit",
        edit(&svc, &owned.short_code, "edited by svc").await,
    );
    let refusal = checks.refused(
        "B3: a member with no grant on the owner board cannot edit",
        edit(&alice, &owned.short_code, "taken by alice").await,
    );
    if let Some((_, details)) = refusal {
        checks.same(
            "B3: the refusal names the owner board",
            (
                details["required_capability"].as_str(),
                details["board_id"].as_str(),
            ),
            (Some("manage_documents"), Some(platform_board.as_str())),
        );
    }
    checks.refused(
        "B3: a member of no team cannot edit",
        edit(&carol, &owned.short_code, "taken by carol").await,
    );
    // The creator. carol loses her grant, and creation alone is left.
    svc.remove_board_member(&initiative_board, &carol_id.to_string())
        .await
        .expect("removing the grant of carol");
    checks.allowed(
        "B4: the creator can edit, with no capability",
        edit(&carol, &of_carol.short_code, "edited by its creator").await,
    );
    checks.refused(
        "B4: carol creates no more document on that board",
        carol
            .create_document(&document(Some(&initiative_board), None, "B4: by carol"))
            .await,
    );

    // The board that the document NAMES is the owner, and not the board of
    // its parent. `two` names the board of platform and supports the task
    // of alice, which is on the board of web.
    let two = document_by(
        &svc,
        Some(&platform_board),
        Some(&of_alice.short_code),
        "B: names platform, supports web",
    )
    .await;
    checks.refused(
        "B5: a manager of the board of the PARENT cannot edit",
        edit(&alice, &two.short_code, "taken by alice").await,
    );
    checks.allowed(
        "B5: a manager of the board that the document names can edit",
        edit(&bob, &two.short_code, "edited by bob").await,
    );
    checks.same(
        "B5: the document has the content of bob",
        svc.get_document(&two.short_code)
            .await
            .expect("read")
            .content,
        "edited by bob".to_string(),
    );
    // A document that names NO board has the owner of before: the board
    // of its parent.
    let inherits = document_by(&svc, None, Some(&of_alice.short_code), "B: supports web").await;
    checks.allowed(
        "B6: with no board, a manager of the board of the parent can edit",
        edit(&alice, &inherits.short_code, "edited by alice").await,
    );
    checks.refused(
        "B6: with no board, a manager of a different board cannot edit",
        edit(&bob, &inherits.short_code, "taken by bob").await,
    );

    // =======================================================================
    // C. The last parent, and the last owner
    // =======================================================================
    let last_edge = edge_id(
        &mut conn,
        &of_alice.short_code,
        &inherits.short_code,
        "supports",
    );
    let refusal = checks.refused_with(
        "C1 REST: with no board, the last supports edge stays",
        422,
        "LAST_PARENT",
        svc.delete_relationship(&last_edge).await,
    );
    if let Some((message, _)) = refusal {
        checks.same(
            "C1: the text of LAST_PARENT is the text of before",
            message,
            last_parent_text(&inherits.short_code, &of_alice.short_code),
        );
    }
    let reply = svc_mcp
        .link(
            "unlink_items",
            &of_alice.short_code,
            &inherits.short_code,
            "supports",
        )
        .await;
    checks.mcp_refused(
        "C1 MCP: with no board, the last edge stays",
        "LAST_PARENT",
        &reply,
    );
    checks.same(
        "C1: the edge is there",
        parents_of(&mut conn, &inherits.short_code),
        1,
    );

    // With a board, the last supports edge can go. alice can edit the
    // parent and not the document: the remove is for an editor of the
    // document.
    let two_edge = edge_id(&mut conn, &of_alice.short_code, &two.short_code, "supports");
    let refusal = checks.refused(
        "C2 REST: an editor of the parent only cannot remove the edge",
        alice.delete_relationship(&two_edge).await,
    );
    if let Some((message, _)) = refusal {
        checks.check(
            "C2: the refusal names the board of the document",
            message.contains("You need \"manage_documents\" on the board of the document."),
            &message,
        );
    }
    checks.allowed(
        "C2 REST: with a board, the last supports edge can go",
        bob.delete_relationship(&two_edge).await,
    );
    checks.same(
        "C2: the document supports nothing, and it has its owner",
        (
            parents_of(&mut conn, &two.short_code),
            stored_board(&mut conn, &two.short_code),
        ),
        (0, Some(platform_board.clone())),
    );
    checks.refused(
        "C2: the owner does not change with the remove",
        edit(&alice, &two.short_code, "taken by alice").await,
    );
    checks.allowed(
        "C2: the manager of the owner board can edit as before",
        edit(&bob, &two.short_code, "edited again by bob").await,
    );
    // The same by MCP, on the document of case A7 that has the two.
    let mcp_two = document_by(
        &bob,
        Some(PLATFORM),
        Some(&of_bob.short_code),
        "C2: MCP, board and parent",
    )
    .await;
    let reply = bob_mcp
        .link(
            "unlink_items",
            &of_bob.short_code,
            &mcp_two.short_code,
            "supports",
        )
        .await;
    checks.mcp_text(
        "C2 MCP: with a board, the last supports edge can go",
        &reply,
        &format!(
            "Unlinked {} -[supports]-> {}.",
            of_bob.short_code, mcp_two.short_code
        ),
    );

    // The board of a document that supports nothing stays. The rule is
    // for each principal, the admin too.
    let before = activity_count(&mut conn);
    let refusal = checks.refused_with(
        "C3 REST: the board of a document that supports nothing stays",
        422,
        "LAST_OWNER",
        svc.set_document_board(&two.short_code, None).await,
    );
    if let Some((message, details)) = refusal {
        checks.same(
            "C3: the text of LAST_OWNER",
            message.clone(),
            last_owner_text(&two.short_code),
        );
        checks.check(
            "C3: the sentences are short",
            short_sentences(&message),
            &message,
        );
        checks.same(
            "C3: the details name the document and the board",
            (details["document"].as_str(), details["board"].as_str()),
            (Some(two.short_code.as_str()), Some(PLATFORM)),
        );
    }
    let reply = svc_mcp
        .call("move_item", json!({"short_code": two.short_code}))
        .await;
    checks.check(
        "C3 MCP: move_item with no to_board is refused with LAST_OWNER",
        reply.0
            && reply
                .1
                .starts_with(&format!("LAST_OWNER: {}", last_owner_text(&two.short_code))),
        &reply.1,
    );
    let reply = svc_mcp
        .call(
            "move_item",
            json!({"short_code": two.short_code, "to_board": ""}),
        )
        .await;
    checks.mcp_refused(
        "C3 MCP: an empty to_board removes the board too",
        "LAST_OWNER",
        &reply,
    );
    checks.same(
        "C3: the refusals wrote nothing",
        (
            stored_board(&mut conn, &two.short_code),
            activity_count(&mut conn),
        ),
        (Some(platform_board.clone()), before),
    );

    // With a parent, the board can go. The owner is then the board of the
    // parent, and the rule of the last parent applies again.
    let falls_back = document_by(
        &svc,
        Some(&platform_board),
        Some(&of_alice.short_code),
        "C4: the board goes",
    )
    .await;
    checks.refused(
        "C4: before, alice cannot edit",
        edit(&alice, &falls_back.short_code, "taken by alice").await,
    );
    let removed = checks.allowed(
        "C4 REST: the board of a document that supports an item can go",
        svc.set_document_board(&falls_back.short_code, None).await,
    );
    if let Some(removed) = removed {
        checks.same("C4: the response has no board", removed.board_id, None);
    }
    checks.same(
        "C4: the row has no board",
        stored_board(&mut conn, &falls_back.short_code),
        None,
    );
    checks.allowed(
        "C4: after, the manager of the board of the parent can edit",
        edit(&alice, &falls_back.short_code, "edited by alice").await,
    );
    checks.refused(
        "C4: after, the manager of the old owner board cannot edit",
        edit(&bob, &falls_back.short_code, "taken by bob").await,
    );
    let edge = edge_id(
        &mut conn,
        &of_alice.short_code,
        &falls_back.short_code,
        "supports",
    );
    checks.refused_with(
        "C4: after, the last supports edge stays",
        422,
        "LAST_PARENT",
        svc.delete_relationship(&edge).await,
    );

    // =======================================================================
    // D. The change of the owner
    // =======================================================================
    let moves = document_by(&svc, Some(&platform_board), None, "D: it moves").await;
    let before = (
        stored_updated_at(&mut conn, &moves.short_code),
        activity_count(&mut conn),
    );
    let refusal = checks.refused(
        "D1 REST: bob has the board of now, and not the new board",
        bob.set_document_board(&moves.short_code, Some(WEB)).await,
    );
    if let Some((message, details)) = refusal {
        checks.same(
            "D1: the refusal names the board that bob does not have",
            message.as_str(),
            &*format!(
                "To change the owner board of {}, you need \"manage_documents\" on the board \
                 that owns it now and on the new board. You do not have it on the board \
                 \"web-delivery\", the new board. The creator of a document gets no right to \
                 change its owner. An organization admin can change it.",
                moves.short_code
            ),
        );
        checks.same(
            "D1: the details name the new board",
            details["board_id"].as_str(),
            Some(web_board.as_str()),
        );
    }
    let refusal = checks.refused(
        "D2 REST: alice has the new board, and not the board of now",
        alice.set_document_board(&moves.short_code, Some(WEB)).await,
    );
    if let Some((message, details)) = refusal {
        checks.check(
            "D2: the refusal names the board of now",
            message.contains(
                "You do not have it on the board \"platform-delivery\", the board that owns \
                 the document now.",
            ),
            &message,
        );
        checks.same(
            "D2: the details name the board of now",
            details["board_id"].as_str(),
            Some(platform_board.as_str()),
        );
    }
    let reply = bob_mcp
        .call(
            "move_item",
            json!({"short_code": moves.short_code, "to_board": WEB}),
        )
        .await;
    checks.mcp_refused("D1 MCP: bob has not the new board", "FORBIDDEN", &reply);
    // The creator gets no right to move. `of_carol` is on the initiative
    // board, where carol has no grant now.
    checks.refused(
        "D3: the creator of a document cannot change its owner",
        carol
            .set_document_board(&of_carol.short_code, Some(&adr_board))
            .await,
    );
    checks.refused(
        "D3: the creator of a document cannot remove its board",
        carol.set_document_board(&of_carol.short_code, None).await,
    );
    checks.refused_with(
        "D4: a board that does not exist",
        404,
        "NOT_FOUND",
        svc.set_document_board(&moves.short_code, Some("no-such-board"))
            .await,
    );
    let (status, body) = svc
        .raw_request(
            Method::PATCH,
            &format!("/api/documents/{}/board", moves.short_code),
            Some(&json!({})),
        )
        .await
        .expect("PATCH with no field");
    checks.same(
        "D4: a body with no field is refused",
        (
            status,
            body["error"]["code"].as_str(),
            body["error"]["message"].as_str(),
        ),
        (
            422,
            Some("VALIDATION"),
            Some(
                "The request has no field to change. Send board: the slug or the id of a \
                 board, or null to remove the board.",
            ),
        ),
    );
    let (status, body) = svc
        .raw_request(
            Method::PATCH,
            &format!("/api/documents/{}/board", moves.short_code),
            Some(&json!({"board": WEB, "column": "Todo"})),
        )
        .await
        .expect("PATCH with an unknown field");
    checks.check(
        "D4: a field that the route does not know is refused, and named",
        status == 422
            && body["error"]["code"] == "VALIDATION"
            && body["error"]["details"]["field"] == "column",
        format!("{status} {body}"),
    );
    checks.same(
        "D1-D4: the refusals wrote nothing",
        (
            stored_board(&mut conn, &moves.short_code),
            stored_updated_at(&mut conn, &moves.short_code),
            activity_count(&mut conn),
        ),
        (Some(platform_board.clone()), before.0.clone(), before.1),
    );

    // With the capability on the two boards, bob moves the document.
    grant(&svc, &web_board, bob_id, "manage_documents").await;
    checks.refused(
        "D5: before the move, alice cannot edit",
        edit(&alice, &moves.short_code, "taken by alice").await,
    );
    let moved = checks.allowed(
        "D5 REST: bob has manage_documents on the two boards",
        bob.set_document_board(&moves.short_code, Some(WEB)).await,
    );
    if let Some(moved) = moved {
        checks.same(
            "D5: the response has the new board",
            moved.board_id,
            Some(web_board.clone()),
        );
        checks.same(
            "D5: the change of the owner is no new version",
            moved.version,
            moves.version,
        );
    }
    checks.same(
        "D5: the row has the new board",
        stored_board(&mut conn, &moves.short_code),
        Some(web_board.clone()),
    );
    checks.allowed(
        "D5: after the move, a manager of the new board can edit",
        edit(&alice, &moves.short_code, "edited by alice").await,
    );
    let log = activity_of(&svc, &moves.id).await;
    checks.check(
        "D5: the activity log has an entry with the action update",
        log.contains(&(
            "update".to_string(),
            "owner_board:platform-delivery->web-delivery".to_string(),
        )),
        format!("{log:?}"),
    );
    // The board that the document has: a success that writes nothing.
    let before = (
        stored_updated_at(&mut conn, &moves.short_code),
        activity_count(&mut conn),
    );
    checks.allowed(
        "D6 REST: the board that the document has is a success",
        bob.set_document_board(&moves.short_code, Some(WEB)).await,
    );
    let reply = bob_mcp
        .call(
            "move_item",
            json!({"short_code": moves.short_code, "to_board": WEB}),
        )
        .await;
    checks.mcp_text(
        "D6 MCP: the board that the document has",
        &reply,
        &format!(
            "No change to {}: its owner board is web-delivery already.",
            moves.short_code
        ),
    );
    checks.same(
        "D6: the two calls wrote nothing",
        (
            stored_updated_at(&mut conn, &moves.short_code),
            activity_count(&mut conn),
        ),
        before,
    );
    let reply = bob_mcp
        .call(
            "move_item",
            json!({"short_code": moves.short_code, "to_board": PLATFORM}),
        )
        .await;
    checks.mcp_text(
        "D7 MCP: bob moves the document back",
        &reply,
        &format!(
            "Moved {}: owner board web-delivery -> platform-delivery.",
            moves.short_code
        ),
    );
    // A document of before the change, with no board: the board of now is
    // the board of its parent. `a2` supports the task of bob.
    checks.refused(
        "D8: alice has the new board, and not the board of the parent",
        alice.set_document_board(&a2.short_code, Some(WEB)).await,
    );
    let reply = bob_mcp
        .call(
            "move_item",
            json!({"short_code": a2.short_code, "to_board": WEB}),
        )
        .await;
    checks.mcp_text(
        "D8 MCP: a document with no board gets a board",
        &reply,
        &format!(
            "Moved {}: owner board (none) -> web-delivery.",
            a2.short_code
        ),
    );
    checks.same(
        "D8: the document keeps its parent",
        parents_of(&mut conn, &a2.short_code),
        1,
    );
    let reply = bob_mcp
        .call("move_item", json!({"short_code": a2.short_code}))
        .await;
    checks.mcp_text(
        "D8 MCP: the board goes, and the parent is the owner again",
        &reply,
        &format!(
            "Moved {}: owner board web-delivery -> (none).",
            a2.short_code
        ),
    );
    // A task needs `to_board`, and an initiative does not move.
    let reply = bob_mcp
        .call("move_item", json!({"short_code": of_bob.short_code}))
        .await;
    checks.check(
        "D9 MCP: a task must have to_board",
        reply.0
            && reply
                .1
                .starts_with("VALIDATION: The call has no `to_board`."),
        &reply.1,
    );

    // =======================================================================
    // E. The relationship `impacts`
    // =======================================================================
    let vision = document_by(&svc, Some(&platform_board), None, "E: the vision of fidius").await;
    let before = (impact_count(&mut conn), activity_count(&mut conn));
    let refusal = checks.refused(
        "E1 REST: alice cannot edit the document, so she writes no link",
        alice
            .add_impact(EntityKind::Document, &vision.short_code, "hlin")
            .await,
    );
    if let Some((message, details)) = refusal {
        checks.same(
            "E1: the refusal says why",
            message.as_str(),
            &*format!(
                "To change an impacts link of {code}, you must be able to edit {code}. You \
                 need \"manage_documents\" on the board of {code}. The creator of {code} and \
                 an organization admin can also change it. You need no right on the \
                 repository.",
                code = vision.short_code
            ),
        );
        checks.same(
            "E1: the details name the capability and the board",
            (
                details["required_capability"].as_str(),
                details["board_id"].as_str(),
            ),
            (Some("manage_documents"), Some(platform_board.as_str())),
        );
    }
    let reply = alice_mcp
        .link("link_items", &vision.short_code, "hlin", "impacts")
        .await;
    checks.mcp_refused("E1 MCP: alice writes no link", "FORBIDDEN", &reply);
    // The refusal of a principal who cannot edit is the same for a
    // repository that does not exist: she learns nothing about it.
    checks.refused(
        "E1: the refusal comes before the repository is read",
        alice
            .add_impact(EntityKind::Document, &vision.short_code, "no-such-repo")
            .await,
    );
    checks.same(
        "E1: the refusals wrote nothing",
        (impact_count(&mut conn), activity_count(&mut conn)),
        before,
    );

    let linked = checks.allowed(
        "E2 REST: bob can edit the document, so he writes the link",
        bob.add_impact(EntityKind::Document, &vision.short_code, "fidius")
            .await,
    );
    if let Some(linked) = linked {
        checks.same(
            "E2: the response is the relationship impacts to the repository",
            (
                linked.relationship.as_str(),
                linked.target_kind.as_str(),
                linked.repository.slug.as_str(),
                linked.repository.repo_full_name.as_str(),
                linked.repository.archived_at,
            ),
            ("impacts", "repository", "fidius", "acme/fidius", None),
        );
    }
    checks.same(
        "E2: the link is stored",
        impacts_between(&mut conn, &vision.short_code, "fidius"),
        1,
    );
    // No right on the repository is needed: `hlin` is of the team web,
    // and bob is not in it.
    let reply = bob_mcp
        .link("link_items", &vision.short_code, "hlin", "impacts")
        .await;
    checks.mcp_text(
        "E2 MCP: bob links the document to a repository of a different team",
        &reply,
        &format!("Linked {} -[impacts]-> repository hlin.", vision.short_code),
    );
    let log = activity_of(&svc, &vision.id).await;
    checks.check(
        "E2: the activity log has the two links, with the action relationship_add",
        [("fidius"), ("hlin")].iter().all(|slug| {
            log.contains(&(
                "relationship_add".to_string(),
                format!(
                    "relationship:impacts:{}->repository:{slug}",
                    vision.short_code
                ),
            ))
        }),
        format!("{log:?}"),
    );

    // The items of case E5, before the count of case E3.
    let strategy = svc
        .create_strategy(&CreateStrategyRequest {
            board_id: strategy_board.clone(),
            column_id: None,
            title: "E: a strategy".into(),
            content: String::new(),
            hypothesis: None,
        })
        .await
        .expect("strategy");
    let initiative = svc
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.clone(),
            column_id: None,
            title: "E: an initiative".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("initiative");
    let before = (impact_count(&mut conn), activity_count(&mut conn));
    let refusal = checks.refused_with(
        "E3 REST: the link that is there",
        422,
        "ALREADY_LINKED",
        bob.add_impact(EntityKind::Document, &vision.short_code, "fidius")
            .await,
    );
    if let Some((message, _)) = refusal {
        checks.same(
            "E3: the text of the refusal",
            message,
            format!(
                "{} impacts the repository \"fidius\" already.",
                vision.short_code
            ),
        );
    }
    let reply = bob_mcp
        .link("link_items", &vision.short_code, "fidius", "impacts")
        .await;
    checks.mcp_refused("E3 MCP: the link that is there", "ALREADY_LINKED", &reply);
    checks.refused_with(
        "E4 REST: a repository that does not exist",
        422,
        "VALIDATION",
        bob.add_impact(EntityKind::Document, &vision.short_code, "no-such-repo")
            .await,
    );
    let reply = bob_mcp
        .link("link_items", &vision.short_code, "no-such-repo", "impacts")
        .await;
    checks.same(
        "E4 MCP: a repository that does not exist",
        reply,
        (
            true,
            "VALIDATION: No live repository has the slug \"no-such-repo\".".to_string(),
        ),
    );

    // A task does not impact a repository: it links to one.
    let refusal = checks.refused_with(
        "E5 REST: a task as the subject",
        422,
        "RELATIONSHIP_RULE",
        bob.add_impact(EntityKind::Task, &of_bob.short_code, "hlin")
            .await,
    );
    if let Some((message, details)) = refusal {
        checks.same(
            "E5: the refusal says how to link a task",
            message.as_str(),
            &*format!(
                "{code} is a task. Only a document or an ADR can impact a repository. A task \
                 links to a repository. To link {code} to a repository, use PUT \
                 /api/tasks/{code}/repository.",
                code = of_bob.short_code
            ),
        );
        checks.same(
            "E5: the details name the types that can",
            details["allowed_source_types"].clone(),
            json!(["document", "adr"]),
        );
    }
    let reply = bob_mcp
        .link("link_items", &of_bob.short_code, "hlin", "impacts")
        .await;
    checks.check(
        "E5 MCP: a task as the subject, and the refusal names set_repository",
        reply.0
            && reply.1.starts_with(&format!(
                "RELATIONSHIP_RULE: {code} is a task. Only a document or an ADR can impact a \
                 repository. A task links to a repository. To link {code} to a repository, \
                 use the tool set_repository.",
                code = of_bob.short_code
            )),
        &reply.1,
    );
    for (kind, code) in [
        (EntityKind::Strategy, &strategy.short_code),
        (EntityKind::Initiative, &initiative.short_code),
    ] {
        checks.refused_with(
            &format!("E5 REST: {code} as the subject"),
            422,
            "RELATIONSHIP_RULE",
            svc.add_impact(kind, code, "hlin").await,
        );
    }
    checks.same(
        "E3-E5: the refusals wrote nothing",
        (impact_count(&mut conn), activity_count(&mut conn)),
        before,
    );

    // An ADR is a subject.
    let adr = adr_by(&svc, &adr_board, "E: plugins are dynamic libraries").await;
    let linked = checks.allowed(
        "E6 REST: an ADR impacts a repository",
        svc.add_impact(EntityKind::Adr, &adr.short_code, "fidius")
            .await,
    );
    checks.check(
        "E6: the link of the ADR is stored",
        linked.is_some() && impacts_between(&mut conn, &adr.short_code, "fidius") == 1,
        "no link",
    );
    checks.refused(
        "E6: bob cannot edit the ADR, so he writes no link",
        bob.add_impact(EntityKind::Adr, &adr.short_code, "hlin")
            .await,
    );
    let read = svc.get_adr(&adr.short_code).await.expect("read the ADR");
    checks.same(
        "E6: the ADR shows the repository that it impacts",
        read.impacts
            .iter()
            .map(|impact| impact.repository.slug.clone())
            .collect::<Vec<_>>(),
        vec!["fidius".to_string()],
    );

    // THE LINK GIVES NO RIGHT. alice is a member of the team that owns
    // `hlin`, and the vision impacts `hlin`.
    checks.refused(
        "E7: a member of the owner team of the repository cannot edit the document",
        edit(&alice, &vision.short_code, "taken by alice").await,
    );
    checks.refused(
        "E7: and she cannot remove the link to her repository",
        alice
            .remove_impact(EntityKind::Document, &vision.short_code, "hlin")
            .await,
    );
    checks.refused(
        "E7: and she cannot archive the document",
        alice.delete_document(&vision.short_code).await,
    );
    checks.same(
        "E7: the document has the content of before, and the two links",
        (
            svc.get_document(&vision.short_code)
                .await
                .expect("read")
                .content,
            impacts_between(&mut conn, &vision.short_code, "hlin"),
            impacts_between(&mut conn, &vision.short_code, "fidius"),
        ),
        ("original content".to_string(), 1, 1),
    );

    // The remove.
    let removed = checks.allowed(
        "E8 REST: bob removes the link",
        bob.remove_impact(EntityKind::Document, &vision.short_code, "hlin")
            .await,
    );
    if let Some(removed) = removed {
        checks.same(
            "E8: the response names the item and the repository",
            (removed.short_code, removed.repository),
            (vision.short_code.clone(), "hlin".to_string()),
        );
    }
    checks.same(
        "E8: the link is not stored",
        impacts_between(&mut conn, &vision.short_code, "hlin"),
        0,
    );
    let log = activity_of(&svc, &vision.id).await;
    checks.check(
        "E8: the activity log has the remove, with the action relationship_remove",
        log.contains(&(
            "relationship_remove".to_string(),
            format!(
                "relationship:impacts:{}->repository:hlin",
                vision.short_code
            ),
        )),
        format!("{log:?}"),
    );
    let before = activity_count(&mut conn);
    let refusal = checks.refused_with(
        "E9 REST: the remove of a link that is not there",
        404,
        "NOT_FOUND",
        bob.remove_impact(EntityKind::Document, &vision.short_code, "hlin")
            .await,
    );
    if let Some((message, _)) = refusal {
        checks.same(
            "E9: the text of the refusal",
            message,
            format!(
                "{} does not impact the repository \"hlin\".",
                vision.short_code
            ),
        );
    }
    let reply = bob_mcp
        .link("unlink_items", &vision.short_code, "hlin", "impacts")
        .await;
    checks.mcp_refused("E9 MCP: the link that is not there", "NOT_FOUND", &reply);
    checks.same(
        "E9: the refusals wrote nothing",
        activity_count(&mut conn),
        before,
    );

    // An archived repository. The link stays, the read marks it, and the
    // link can be removed. A new link to it is refused.
    checks.allowed(
        "E10: the document impacts the repository old-lib",
        bob.add_impact(EntityKind::Document, &vision.short_code, "old-lib")
            .await,
    );
    checks.allowed(
        "E10: the delete of a repository does not look at the impacts links",
        svc.delete_repository("old-lib").await,
    );
    checks.same(
        "E10: the link to the archived repository stays",
        impacts_between(&mut conn, &vision.short_code, "old-lib"),
        1,
    );
    let read = svc.get_document(&vision.short_code).await.expect("read");
    let shown: Vec<(String, bool)> = read
        .impacts
        .iter()
        .map(|impact| {
            (
                impact.repository.slug.clone(),
                impact.repository.archived_at.is_some(),
            )
        })
        .collect();
    checks.same(
        "E10: the document shows the two links, and marks the archived repository",
        shown,
        vec![("fidius".to_string(), false), ("old-lib".to_string(), true)],
    );
    checks.allowed(
        "E10: the archived repository does not stop an edit of the document",
        edit(&bob, &vision.short_code, "the vision, edited").await,
    );
    let other = document_by(&svc, Some(&platform_board), None, "E10: a different one").await;
    checks.refused_with(
        "E10 REST: a new link to an archived repository",
        422,
        "VALIDATION",
        svc.add_impact(EntityKind::Document, &other.short_code, "old-lib")
            .await,
    );
    let reply = bob_mcp
        .link("unlink_items", &vision.short_code, "old-lib", "impacts")
        .await;
    checks.mcp_text(
        "E10 MCP: the link to an archived repository can be removed",
        &reply,
        &format!(
            "Unlinked {} -[impacts]-> repository old-lib.",
            vision.short_code
        ),
    );

    // The five relationships of an edge between two items are as before,
    // and an unknown one names `impacts` too.
    let reply = bob_mcp
        .link("link_items", &vision.short_code, "fidius", "is_about")
        .await;
    checks.same(
        "E11 MCP: a relationship that does not exist",
        reply,
        (
            true,
            "VALIDATION: The value \"is_about\" is not a value of relationship. The values \
             are: parent, supports, informs, supersedes, blocks, impacts."
                .to_string(),
        ),
    );
    let result = svc
        .create_relationship(&CreateRelationshipRequest {
            source_short_code: vision.short_code.clone(),
            target_short_code: of_bob.short_code.clone(),
            relationship: "impacts".into(),
        })
        .await;
    checks.check(
        "E11 REST: POST /api/relationships does not take impacts",
        matches!(result, Err(Error::Validation { status: 422, .. })),
        format!("{result:?}"),
    );

    // =======================================================================
    // F. The reads and the filters
    // =======================================================================
    // A vision from the template "Product Vision": the template gives
    // the document type.
    let template: Option<String> = svc
        .list_templates(Pagination {
            limit: Some(200),
            offset: None,
        })
        .await
        .expect("templates")
        .items
        .into_iter()
        .find(|template| template.name == "Product Vision")
        .map(|template| template.id);
    checks.check(
        "F1: a new tenant has the template Product Vision",
        template.is_some(),
        "no template",
    );
    let from_template = svc
        .create_document(&CreateDocumentRequest {
            title: "F: the vision of hlin".into(),
            board: Some(WEB.into()),
            content: None,
            template_id: template,
            parent_short_code: None,
        })
        .await
        .expect("a document from the template");
    checks.check(
        "F1: the template gives the sections of a product vision",
        [
            "# Product Vision",
            "## Purpose",
            "## Who It Is For",
            "## Current State",
            "## Future State",
            "## Principles",
            "## What It Is Not",
        ]
        .iter()
        .all(|heading| from_template.content.contains(heading)),
        &from_template.content,
    );
    svc.add_impact(EntityKind::Document, &from_template.short_code, "hlin")
        .await
        .expect("the vision of hlin impacts hlin");
    svc.set_document_lifecycle(&from_template.short_code, "published")
        .await
        .expect("publishing");
    // An archived document that impacts `hlin`.
    let put_away = document_by(&svc, Some(&web_board), None, "F: put away").await;
    svc.add_impact(EntityKind::Document, &put_away.short_code, "hlin")
        .await
        .expect("impacts");
    svc.delete_document(&put_away.short_code)
        .await
        .expect("archive");
    checks.same(
        "F2: the archive of a document removes no link",
        impacts_between(&mut conn, &put_away.short_code, "hlin"),
        1,
    );
    let hlin_adr = adr_by(&svc, &adr_board, "F: hlin has no plugin").await;
    svc.add_impact(EntityKind::Adr, &hlin_adr.short_code, "hlin")
        .await
        .expect("impacts");
    let hlin_task = task_by(&alice, &web_board, "F: a task in hlin", Some("hlin")).await;

    let detail = alice.get_repository("hlin").await.expect("the repository");
    checks.same(
        "F3 REST: the repository shows its documents and its ADRs",
        serde_json::to_value(&detail.impacted_by).expect("json"),
        json!([
            {
                "short_code": from_template.short_code,
                "title": "F: the vision of hlin",
                "entity_type": "document",
                "document_type": "vision",
                "lifecycle": "published",
                "column": null,
            },
            {
                "short_code": hlin_adr.short_code,
                "title": "F: hlin has no plugin",
                "entity_type": "adr",
                "document_type": null,
                "lifecycle": null,
                "column": "Draft",
            },
        ]),
    );
    let detail = alice
        .get_repository_with_archived("hlin")
        .await
        .expect("the repository, with archived items");
    let shown: Vec<(String, bool)> = detail
        .impacted_by
        .iter()
        .map(|item| (item.short_code.clone(), item.archived_at.is_some()))
        .collect();
    checks.same(
        "F3 REST: include_deleted adds the archived document, marked",
        shown,
        vec![
            (from_template.short_code.clone(), false),
            (put_away.short_code.clone(), true),
            (hlin_adr.short_code.clone(), false),
        ],
    );
    let (status, body) = alice
        .raw_request(Method::GET, "/api/repositories/hlin?archived=true", None)
        .await
        .expect("an unknown parameter");
    checks.check(
        "F3 REST: a parameter that the route does not know is refused, and named",
        status == 400 && body["error"]["details"]["parameter"] == "archived",
        format!("{status} {body}"),
    );
    let reply = alice_mcp
        .call("get_repository", json!({"repository": "hlin"}))
        .await;
    checks.check(
        "F3 MCP: get_repository shows the documents and the ADRs",
        !reply.0
            && reply.1.contains(&format!(
                "\n## Documents and ADRs that impact this repository\n\
                 - {} — F: the vision of hlin · document (vision) · lifecycle: published\n\
                 - {} — F: hlin has no plugin · adr · column: Draft\n",
                from_template.short_code, hlin_adr.short_code
            ))
            && !reply.1.contains(&put_away.short_code),
        &reply.1,
    );
    let reply = alice_mcp
        .call(
            "get_repository",
            json!({"repository": "hlin", "include_deleted": true}),
        )
        .await;
    checks.check(
        "F3 MCP: include_deleted adds the archived document, marked",
        !reply.0
            && reply.1.contains(&format!(
                "- {} — F: put away · document · lifecycle: draft [archived]\n",
                put_away.short_code
            )),
        &reply.1,
    );
    let reply = alice_mcp
        .call("get_repository", json!({"repository": "old-lib"}))
        .await;
    checks.mcp_refused("F3 MCP: an archived repository", "NOT_FOUND", &reply);

    let reply = alice_mcp
        .call("get_item", json!({"short_code": from_template.short_code}))
        .await;
    checks.check(
        "F4 MCP: get_item shows the owner board and the impacts",
        !reply.0
            && reply.1.contains("\n- owner board: web-delivery\n")
            && reply
                .1
                .contains("\n- impacts: repository hlin (github acme/hlin)\n")
            && !reply.1.contains("column:"),
        &reply.1,
    );
    let reply = alice_mcp
        .call("get_item", json!({"short_code": inherits.short_code}))
        .await;
    checks.check(
        "F4 MCP: get_item of a document with no board shows the board of its parent",
        !reply.0
            && reply.1.contains(&format!(
                "\n- owner board: web-delivery (the board of {}, which the document supports)\n",
                of_alice.short_code
            )),
        &reply.1,
    );
    let impacts = alice
        .item_impacts(EntityKind::Document, &put_away.short_code)
        .await;
    checks.check(
        "F4 REST: the impacts of an archived document can be read",
        impacts.as_ref().is_ok_and(|response| {
            response.impacts.len() == 1 && response.impacts[0].repository.slug == "hlin"
        }),
        format!("{impacts:?}"),
    );
    checks.refused_with(
        "F4 REST: the impacts of a task",
        422,
        "RELATIONSHIP_RULE",
        alice
            .item_impacts(EntityKind::Task, &hlin_task.short_code)
            .await,
    );

    // The lists.
    let listed = alice
        .list_documents(ImpactListQuery::of_repository("hlin"))
        .await
        .expect("the documents of hlin");
    checks.same(
        "F5 REST: the list of documents by repository, live rows only",
        (
            listed
                .items
                .iter()
                .map(|document| document.short_code.clone())
                .collect::<Vec<_>>(),
            listed.total,
        ),
        (vec![from_template.short_code.clone()], 1),
    );
    checks.same(
        "F5 REST: a document of a list has its board and its impacts",
        listed
            .items
            .first()
            .map(|document| (document.board_id.clone(), document.impacts.len())),
        Some((Some(web_board.clone()), 1)),
    );
    let listed = alice
        .list_documents(ImpactListQuery {
            include_deleted: true,
            repository: Some("hlin".into()),
            ..ImpactListQuery::default()
        })
        .await
        .expect("the documents of hlin, with archived rows");
    checks.same(
        "F5 REST: include_deleted adds the archived document",
        sorted(
            listed
                .items
                .iter()
                .map(|document| document.short_code.clone())
                .collect(),
        ),
        sorted(vec![
            from_template.short_code.clone(),
            put_away.short_code.clone(),
        ]),
    );
    let listed = alice
        .list_adrs(ImpactListQuery::of_repository("hlin"))
        .await
        .expect("the ADRs of hlin");
    checks.same(
        "F5 REST: the list of ADRs by repository",
        (
            listed
                .items
                .iter()
                .map(|adr| adr.short_code.clone())
                .collect::<Vec<_>>(),
            listed.total,
        ),
        (vec![hlin_adr.short_code.clone()], 1),
    );
    checks.refused_with(
        "F5 REST: a repository that does not exist",
        422,
        "VALIDATION",
        alice
            .list_documents(ImpactListQuery::of_repository("no-such-repo"))
            .await,
    );
    let all = alice
        .list_documents(Pagination::default())
        .await
        .expect("each document");
    checks.check(
        "F5 REST: with no filter, the list has each live document",
        all.total > 10
            && all
                .items
                .iter()
                .all(|document| document.archived_at.is_none()),
        format!("total {}", all.total),
    );
    let (status, body) = alice
        .raw_request(Method::GET, "/api/strategies?repository=hlin", None)
        .await
        .expect("an unknown parameter");
    checks.check(
        "F5 REST: the list of strategies does not take the filter",
        status == 400 && body["error"]["details"]["parameter"] == "repository",
        format!("{status} {body}"),
    );

    // The search.
    let by_repository = |include_deleted: bool| SearchFilter {
        repository: Some("hlin".into()),
        include_deleted,
        ..SearchFilter::default()
    };
    checks.same(
        "F6: the search by repository gives the tasks, the documents and the ADRs",
        search_codes(&alice, by_repository(false))
            .await
            .expect("search"),
        sorted(vec![
            from_template.short_code.clone(),
            hlin_adr.short_code.clone(),
            hlin_task.short_code.clone(),
        ]),
    );
    checks.same(
        "F6: include_deleted adds the archived document",
        search_codes(&alice, by_repository(true))
            .await
            .expect("search"),
        sorted(vec![
            from_template.short_code.clone(),
            put_away.short_code.clone(),
            hlin_adr.short_code.clone(),
            hlin_task.short_code.clone(),
        ]),
    );
    checks.same(
        "F6: with entity_type, the search gives that type",
        search_codes(
            &alice,
            SearchFilter {
                entity_type: Some(vec!["document".into()]),
                ..by_repository(false)
            },
        )
        .await
        .expect("search"),
        vec![from_template.short_code.clone()],
    );
    checks.same(
        "F6: with task_type, only tasks match",
        search_codes(
            &alice,
            SearchFilter {
                task_type: Some(vec!["task".into()]),
                ..by_repository(false)
            },
        )
        .await
        .expect("search"),
        vec![hlin_task.short_code.clone()],
    );
    checks.same(
        "F6: the search by the repository fidius",
        search_codes(
            &alice,
            SearchFilter {
                repository: Some("fidius".into()),
                ..SearchFilter::default()
            },
        )
        .await
        .expect("search"),
        sorted(vec![
            vision.short_code.clone(),
            adr.short_code.clone(),
            of_bob.short_code.clone(),
        ]),
    );
    let reply = alice_mcp
        .call("search", json!({"filter": {"repository": "hlin"}}))
        .await;
    checks.check(
        "F6 MCP: search by repository gives the three types",
        !reply.0
            && [
                &from_template.short_code,
                &hlin_adr.short_code,
                &hlin_task.short_code,
            ]
            .iter()
            .all(|code| reply.1.contains(code.as_str()))
            && !reply.1.contains(&put_away.short_code),
        &reply.1,
    );

    // The restore brings the document back with its links.
    svc.restore_document(&put_away.short_code)
        .await
        .expect("restore");
    let restored = svc.get_document(&put_away.short_code).await.expect("read");
    checks.same(
        "F7: the restored document has its board and its link",
        (
            restored.archived_at,
            restored.board_id,
            restored
                .impacts
                .iter()
                .map(|impact| impact.repository.slug.clone())
                .collect::<Vec<_>>(),
        ),
        (None, Some(web_board.clone()), vec!["hlin".to_string()]),
    );

    // =======================================================================
    // G. The archive of a work item, and the delete of a board
    // =======================================================================
    // The cascade of an archive follows `parent` edges, and a document
    // has none. So the archive of an item takes no document: the one that
    // names a board, and the one that does not.
    let doomed = task_by(&bob, &platform_board, "G: it goes to the archive", None).await;
    let with_board = document_by(
        &bob,
        Some(&platform_board),
        Some(&doomed.short_code),
        "G: names a board",
    )
    .await;
    let with_parent = document_by(&bob, None, Some(&doomed.short_code), "G: names none").await;
    let preview = bob
        .cascade_preview(EntityKind::Task, &doomed.short_code)
        .await
        .expect("preview");
    checks.same(
        "G1: the preview of the archive names no document",
        preview.cascaded_short_codes,
        Vec::<String>::new(),
    );
    let outcome = bob.delete_task(&doomed.short_code).await.expect("archive");
    checks.same(
        "G1: the archive of the item takes no document",
        outcome.cascade_count,
        0,
    );
    for (document, what) in [(&with_board, "names a board"), (&with_parent, "names none")] {
        let read = svc.get_document(&document.short_code).await.expect("read");
        checks.same(
            &format!("G1: the document that {what} is live, with its edge"),
            (
                read.archived_at,
                parents_of(&mut conn, &document.short_code),
            ),
            (None, 1),
        );
        checks.allowed(
            &format!("G1: bob can edit the document that {what}"),
            edit(&bob, &document.short_code, "edited after the archive").await,
        );
    }

    // A board that is the owner of a live document is not deleted. The
    // initiative board has the documents of A8, and no card but one.
    svc.delete_initiative(&initiative.short_code)
        .await
        .expect("archiving the one card of the initiative board");
    let refusal = checks.refused_with(
        "G2: the delete of a board that owns live documents",
        422,
        "BOARD_OWNS_DOCUMENTS",
        svc.delete_board(&initiative_board).await,
    );
    if let Some((message, details)) = refusal {
        checks.check(
            "G2: the refusal names the documents and says what to do",
            message.starts_with("The board \"Initiatives\" is the owner of 2 live documents: [")
                && message.ends_with(
                    "Name a different board for each document (PATCH \
                     /api/documents/{code}/board) or archive it. Then delete the board.",
                )
                && message.contains(&of_carol.short_code),
            &message,
        );
        checks.same(
            "G2: the details count them",
            details["item_count"].clone(),
            json!(2),
        );
    }

    // =======================================================================
    // H. The schemas of the tools
    // =======================================================================
    let listed = svc_mcp.rpc("tools/list", json!({})).await;
    let tools = listed["tools"].as_array().cloned().unwrap_or_default();
    checks.same("H1: the number of tools", tools.len(), 23);
    let schema_of = |name: &str| -> Value {
        tools
            .iter()
            .find(|tool| tool["name"] == name)
            .map(|tool| tool["inputSchema"].clone())
            .unwrap_or(Value::Null)
    };
    for name in [
        "create_item",
        "move_item",
        "link_items",
        "unlink_items",
        "get_repository",
        "get_item",
        "search",
    ] {
        checks.same(
            &format!("H1: {name} refuses an argument that it does not know"),
            schema_of(name)["additionalProperties"].clone(),
            json!(false),
        );
    }
    checks.same(
        "H1: move_item must have short_code only",
        schema_of("move_item")["required"].clone(),
        json!(["short_code"]),
    );
    checks.same(
        "H1: link_items must have the three arguments of before",
        schema_of("link_items")["required"].clone(),
        json!(["source", "target", "relationship"]),
    );
    checks.check(
        "H1: get_repository has include_deleted",
        schema_of("get_repository")["properties"]["include_deleted"].is_object(),
        schema_of("get_repository"),
    );
    checks.check(
        "H1: the description of board in create_item does not say that a document ignores it",
        schema_of("create_item")["properties"]["board"]["description"]
            .as_str()
            .is_some_and(|text| !text.contains("Ignored") && text.contains("OWNER board")),
        schema_of("create_item")["properties"]["board"].clone(),
    );
    let reply = bob_mcp
        .call(
            "move_item",
            json!({"short_code": moves.short_code, "to_board": WEB, "column": "Todo"}),
        )
        .await;
    checks.same(
        "H2: move_item refuses an argument that it does not know",
        reply,
        (
            true,
            "VALIDATION: The call has the argument \"column\". This tool does not accept \
             that argument. The arguments of this tool are: short_code, to_board, rename.\n\
             details: {\"allowed\":[\"short_code\",\"to_board\",\"rename\"],\"argument\":\"column\"}"
                .to_string(),
        ),
    );
    let reply = bob_mcp
        .call(
            "link_items",
            json!({"source": vision.short_code, "target": "hlin", "relationship": "impacts",
                   "repository": "hlin"}),
        )
        .await;
    checks.check(
        "H2: link_items refuses an argument that it does not know",
        reply.0
            && reply
                .1
                .starts_with("VALIDATION: The call has the argument \"repository\"."),
        &reply.1,
    );
    checks.same(
        "H2: the refused calls wrote nothing",
        (
            stored_board(&mut conn, &moves.short_code),
            impacts_between(&mut conn, &vision.short_code, "hlin"),
        ),
        (Some(platform_board.clone()), 0),
    );

    drop(conn);
    drop(pool);
    drop(server);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
    checks.finish();
}
