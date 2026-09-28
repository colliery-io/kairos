//! Integration test for COLLIERY-T-0235: a link to a document with no
//! parent does not give the authority on the document, and a document
//! always has a parent.
//!
//! THE FACTS. A document has no board. Its authorization board is the
//! board of its EARLIEST `supports` parent. The link rule
//! (COLLIERY-T-0228) lets a principal write an edge when it may edit the
//! item at EITHER end.
//!
//! THE ATTACK (defect 1). A document has no `supports` edge. A person who
//! can edit some task writes `supports` from that task to the document.
//! That edge is the first one, so the document takes its authority from
//! the board of the task, and the person can edit and archive a document
//! that was not theirs.
//!
//! THE SECOND ROUTE (defect 2). A document has the parents A (the
//! earliest) and B. A person can edit A and B, and cannot edit the
//! document. The person removes A -> document. B is now the earliest, and
//! the document takes its authority from the board of B.
//!
//! THE RULES.
//! 1. No person can remove the last `supports` edge of a document: 422
//!    `LAST_PARENT`.
//! 2. To write `supports` to a document that has no parent, the principal
//!    must be able to edit the DOCUMENT: its creator, or an organization
//!    admin.
//! 3. To remove a `supports` edge of a document, the principal must be
//!    able to edit the DOCUMENT.
//!
//! This is a security boundary, so the negative cases are the contract.
//! Each refusal is checked where the data is stored.
//!
//! The cases are numbered as in the work item. Every check is recorded and
//! the test fails at the end with the full list, so one run shows each
//! broken case and not only the first.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_document_orphan_t0235_test`.
//!
//! Cast:
//! - `svc`   — org admin: sets the world up,
//! - `alice` — member of team `web`; NO grant on the board of `platform`,
//! - `bob`   — member of team `platform`,
//! - `carol` — org member in no team, with no grant,
//! - `globex-alice` — alice in ANOTHER tenant.

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
    CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest, CreateTaskRequest, Document,
    Task, UpdateContentRequest,
};
use kairos_client::types_meta::{CreateRelationshipRequest, Relationship};
use kairos_client::types_org::{AddBoardMemberRequest, AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::{Error, KairosClient};
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_document_orphan_t0235_test";

/// The code of the refusal of rule 1.
const LAST_PARENT: &str = "LAST_PARENT";

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

/// The number of `supports` edges that point at `target`, from any source.
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

/// Each edge of the tenant.
fn edge_count(conn: &mut PgConnection) -> i64 {
    diesel::sql_query("SELECT count(*)::bigint AS count FROM item_relationships")
        .get_result::<Count>(conn)
        .expect("counting edges")
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

/// OLD DATA. Make a document an orphan: delete each `supports` edge that
/// points at it, by SQL.
///
/// The server does not permit this state from COLLIERY-T-0235 on. Until
/// then a person could remove the only `supports` edge of a document, and
/// the owner decided not to migrate the data. So a database can hold a
/// document with no parent, and the test writes that state where the
/// server stores it.
fn make_orphan(conn: &mut PgConnection, document: &str) {
    let removed = diesel::sql_query(
        "DELETE FROM item_relationships r USING entity_directory t \
          WHERE t.id = r.target_id AND t.short_code = $1 \
            AND r.relationship::text = 'supports'",
    )
    .bind::<Text, _>(document)
    .execute(conn)
    .expect("deleting the supports edges");
    assert!(removed >= 1, "{document} had no supports edge to delete");
    assert_eq!(parents_of(conn, document), 0, "{document} is an orphan");
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

    /// The call must be refused with 422 `LAST_PARENT`. Returns the
    /// message and the details of the refusal.
    fn last_parent<T: std::fmt::Debug>(
        &mut self,
        case: &str,
        result: Result<T, Error>,
    ) -> Option<(String, Value)> {
        match result {
            Err(Error::Other {
                status: 422,
                code,
                message,
                details,
            }) if code == LAST_PARENT => {
                self.passed += 1;
                Some((message, details))
            }
            Err(err) => {
                self.failures
                    .push(format!("{case}: expected 422 {LAST_PARENT}, got {err}"));
                None
            }
            Ok(value) => {
                self.failures.push(format!(
                    "{case}: expected 422 {LAST_PARENT}, but the call SUCCEEDED: {value:?}"
                ));
                None
            }
        }
    }

    /// The MCP call must succeed.
    fn mcp_allowed(&mut self, case: &str, (is_error, text): &(bool, String)) {
        self.check(case, !is_error, format!("expected success, got {text}"));
    }

    /// The MCP call must be refused with this code.
    fn mcp_refused(&mut self, case: &str, code: &str, (is_error, text): &(bool, String)) {
        self.check(
            case,
            *is_error && text.contains(code),
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
        eprintln!("document_orphan: {} checks passed", self.passed);
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
                    "clientInfo": {"name": "kairos-t0235-test", "version": "0.0.0"},
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

    async fn link(&mut self, tool: &str, source: &str, target: &str, rel: &str) -> (bool, String) {
        self.call(
            tool,
            json!({"source": source, "target": target, "relationship": rel}),
        )
        .await
    }
}

fn edge(source: &str, target: &str, relationship: &str) -> CreateRelationshipRequest {
    CreateRelationshipRequest {
        source_short_code: source.into(),
        target_short_code: target.into(),
        relationship: relationship.into(),
    }
}

async fn task_by(client: &KairosClient, board: &str, title: &str) -> Task {
    client
        .create_task(&CreateTaskRequest {
            board_id: Some(board.into()),
            column_id: None,
            title: title.into(),
            content: "original content".into(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
        })
        .await
        .unwrap_or_else(|e| panic!("creating task {title:?}: {e}"))
}

fn document_on(parent: Option<&str>, title: &str) -> CreateDocumentRequest {
    CreateDocumentRequest {
        title: title.into(),
        content: Some("original content".into()),
        template_id: None,
        parent_short_code: parent.map(str::to_string),
    }
}

async fn document_by(client: &KairosClient, parent: &str, title: &str) -> Document {
    client
        .create_document(&document_on(Some(parent), title))
        .await
        .unwrap_or_else(|e| panic!("creating document {title:?}: {e}"))
}

/// A precondition of the fixture: write an edge as `client`. The test
/// stops if the server refuses it.
async fn linked(client: &KairosClient, source: &str, target: &str, rel: &str) -> Relationship {
    client
        .create_relationship(&edge(source, target, rel))
        .await
        .unwrap_or_else(|e| panic!("linking {source} -[{rel}]-> {target}: {e}"))
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

/// No sentence of the message has more than 20 words.
fn short_sentences(message: &str) -> bool {
    message
        .split(['.', '?', '!'])
        .all(|sentence| sentence.split_whitespace().count() <= 20)
}

#[tokio::test]
async fn a_document_keeps_a_parent_and_a_link_gives_no_authority_against_live_stack() {
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
    let base = server.base_url.clone();
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
    let initiative_board = board_of_level(&mut conn, BoardLevel::Initiative);
    let adr_board = board_of_level(&mut conn, BoardLevel::Adr);

    let mut checks = Checks::default();
    let mut alice_mcp = McpSession::open(&base, &alice_token, "acme").await;
    let mut bob_mcp = McpSession::open(&base, &bob_token, "acme").await;
    let mut carol_mcp = McpSession::open(&base, &carol_token, "acme").await;
    let mut svc_mcp = McpSession::open(&base, &svc_token, "acme").await;

    // The items that the cases share. `of_alice` is on the board of web:
    // alice edits it as a member of the team. `of_bob` is on the board of
    // platform: alice can NOT edit it.
    let of_alice = task_by(&alice, &web_board, "a task of alice").await;
    let of_bob = task_by(&bob, &platform_board, "a task of bob").await;
    // The admin created it, so bob edits it as a member of platform only.
    let of_platform = task_by(&svc, &platform_board, "a task of platform").await;

    // =======================================================================
    // Case 8. Document create does not change
    // =======================================================================
    let created = checks.allowed(
        "8 REST: a document with a parent is created",
        bob.create_document(&document_on(Some(&of_bob.short_code), "8: with a parent"))
            .await,
    );
    if let Some(created) = &created {
        checks.check(
            "8 REST: the new document has one supports parent",
            parents_of(&mut conn, &created.short_code) == 1
                && edges_between(
                    &mut conn,
                    &of_bob.short_code,
                    &created.short_code,
                    "supports",
                ) == 1,
            "the supports edge of the create is not there",
        );
    }
    let result = bob
        .create_document(&document_on(None, "8: with no parent"))
        .await;
    checks.check(
        "8 REST: a document with no parent is refused, 422 VALIDATION",
        matches!(result, Err(Error::Validation { status: 422, .. })),
        format!("{result:?}"),
    );
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "8: MCP, with a parent",
                   "parent": of_bob.short_code}),
        )
        .await;
    checks.mcp_allowed("8 MCP: a document with a parent is created", &reply);
    let reply = bob_mcp
        .call(
            "create_item",
            json!({"item_type": "document", "title": "8: MCP, with no parent"}),
        )
        .await;
    checks.mcp_refused(
        "8 MCP: a document with no parent is refused",
        "VALIDATION",
        &reply,
    );
    // The create gate: alice holds nothing on the board of the parent.
    checks.refused(
        "8 REST: alice creates no document on a task of platform",
        alice
            .create_document(&document_on(Some(&of_bob.short_code), "8: by alice"))
            .await,
    );

    // =======================================================================
    // Case 1. The takeover, end to end
    // (criteria d, e)
    // =======================================================================
    let orphan = document_by(&bob, &of_bob.short_code, "1: the document of bob").await;
    // OLD DATA: see `make_orphan`.
    make_orphan(&mut conn, &orphan.short_code);
    let before = svc.get_document(&orphan.short_code).await.expect("read");
    let edges_before = edge_count(&mut conn);

    checks.refused(
        "1 (e): BEFORE the link, alice cannot edit the orphan",
        edit(&alice, &orphan.short_code, "taken by alice").await,
    );
    let refusal = checks.refused(
        "1 REST (d, e): alice links her task to the orphan of bob",
        alice
            .create_relationship(&edge(&of_alice.short_code, &orphan.short_code, "supports"))
            .await,
    );
    let reply = alice_mcp
        .link(
            "link_items",
            &of_alice.short_code,
            &orphan.short_code,
            "supports",
        )
        .await;
    checks.mcp_refused(
        "1 MCP (d, e): alice links her task to the orphan of bob",
        "FORBIDDEN",
        &reply,
    );
    checks.check(
        "1 (d): no edge was written, and the orphan has no parent",
        edge_count(&mut conn) == edges_before && parents_of(&mut conn, &orphan.short_code) == 0,
        format!(
            "{edges_before} edges before, {} now; {} parents",
            edge_count(&mut conn),
            parents_of(&mut conn, &orphan.short_code)
        ),
    );
    // What the edge gave, until COLLIERY-T-0235: the right to edit and to
    // archive.
    checks.refused(
        "1 (e): AFTER the link, alice cannot edit the orphan",
        edit(&alice, &orphan.short_code, "taken by alice").await,
    );
    checks.refused(
        "1 (e): AFTER the link, alice cannot archive the orphan",
        alice.delete_document(&orphan.short_code).await,
    );
    let reply = alice_mcp
        .call(
            "update_item",
            json!({"short_code": orphan.short_code, "content": "taken by alice",
                   "version": before.version}),
        )
        .await;
    checks.mcp_refused(
        "1 MCP (e): AFTER the link, alice cannot edit the orphan",
        "FORBIDDEN",
        &reply,
    );
    let after = svc.get_document(&orphan.short_code).await.expect("read");
    checks.check(
        "1 (d): the orphan is LIVE and unchanged",
        after == before && !is_archived(&mut conn, &orphan.short_code),
        format!("before {before:?}\n      after  {after:?}"),
    );
    // carol edits neither end. The refusal is the same code.
    checks.refused(
        "1 REST: carol links the task of alice to the orphan",
        carol
            .create_relationship(&edge(&of_alice.short_code, &orphan.short_code, "supports"))
            .await,
    );
    // A member of platform held `manage_documents` on the board that the
    // orphan took its authority from. With no parent, no board answers
    // for the document: a member who did not create it cannot link it.
    let orphan_of_admin = document_by(
        &svc,
        &of_platform.short_code,
        "1: the document of the admin",
    )
    .await;
    make_orphan(&mut conn, &orphan_of_admin.short_code);
    checks.refused(
        "1 REST (e): bob did not create this orphan, and cannot link it to his task",
        bob.create_relationship(&edge(
            &of_bob.short_code,
            &orphan_of_admin.short_code,
            "supports",
        ))
        .await,
    );
    checks.check(
        "1 (e): that orphan has no parent",
        parents_of(&mut conn, &orphan_of_admin.short_code) == 0,
        "an edge was written",
    );

    // =======================================================================
    // Case 9 (part). The message of the refusal of a link to an orphan
    // =======================================================================
    if let Some((message, details)) = &refusal {
        checks.check(
            "9: the refusal of the link says who can link the document",
            message.contains("no parent")
                && message.contains("creator")
                && message.contains("organization admin"),
            message,
        );
        checks.check(
            "9: each sentence of that refusal has 20 words or fewer",
            short_sentences(message),
            message,
        );
        checks.check(
            "9: the details name manage_documents, no board, and the target only",
            details["required_capability"] == json!("manage_documents")
                && details["board_id"].is_null()
                && details["relationship"] == json!("supports")
                && details["any_of"]
                    .as_array()
                    .is_some_and(|ends| ends.len() == 1 && ends[0]["end"] == json!("target")),
            details,
        );
    }
    checks.check(
        "9 MCP: the refusal of the link says who can link the document",
        reply_names(&alice_mcp_refusal(&mut alice_mcp, &of_alice, &orphan).await),
        "the MCP refusal does not name the creator and the organization admin",
    );

    // =======================================================================
    // Case 2. The creator and an organization admin link an orphan
    // (criterion f)
    // =======================================================================
    checks.allowed(
        "2 REST (f): bob, the creator, links the orphan to a task he can edit",
        bob.create_relationship(&edge(&of_bob.short_code, &orphan.short_code, "supports"))
            .await,
    );
    checks.check(
        "2 (f): the orphan has one parent now",
        parents_of(&mut conn, &orphan.short_code) == 1,
        format!("{} parents", parents_of(&mut conn, &orphan.short_code)),
    );
    // Criterion g: the document has a parent, so the rule is the old one.
    // alice is not on the board of the parent.
    checks.refused(
        "2 (g): the document has a parent on the board of platform: alice cannot edit it",
        edit(&alice, &orphan.short_code, "taken by alice").await,
    );

    let second = document_by(&bob, &of_bob.short_code, "2: a second orphan of bob").await;
    make_orphan(&mut conn, &second.short_code);
    let reply = bob_mcp
        .link(
            "link_items",
            &of_bob.short_code,
            &second.short_code,
            "supports",
        )
        .await;
    checks.mcp_allowed("2 MCP (f): bob, the creator, links his orphan", &reply);

    // The creator can edit the document, so the source can be any item:
    // bob can NOT edit the task of alice.
    let third = document_by(&bob, &of_bob.short_code, "2: a third orphan of bob").await;
    make_orphan(&mut conn, &third.short_code);
    checks.allowed(
        "2 REST (f): the creator links his orphan to a task that he cannot edit",
        bob.create_relationship(&edge(&of_alice.short_code, &third.short_code, "supports"))
            .await,
    );
    // Criterion g: the document has a parent on the board of web, and
    // alice holds `manage_documents` there. That is the rule for a
    // document that has a parent, and the creator chose the parent.
    checks.allowed(
        "2 (g): a member of the board of the parent can edit the document",
        edit(
            &alice,
            &third.short_code,
            "edited by alice, a member of web",
        )
        .await,
    );
    // Rule 7: the creator keeps the edit rule wherever the document is.
    checks.allowed(
        "2: the creator can edit his document on the board of a different team",
        edit(&bob, &third.short_code, "edited by bob, the creator").await,
    );

    let fourth = document_by(&bob, &of_bob.short_code, "2: a fourth orphan of bob").await;
    make_orphan(&mut conn, &fourth.short_code);
    checks.allowed(
        "2 REST (f): an organization admin links an orphan",
        svc.create_relationship(&edge(
            &of_platform.short_code,
            &fourth.short_code,
            "supports",
        ))
        .await,
    );
    let fifth = document_by(&bob, &of_bob.short_code, "2: a fifth orphan of bob").await;
    make_orphan(&mut conn, &fifth.short_code);
    let reply = svc_mcp
        .link(
            "link_items",
            &of_platform.short_code,
            &fifth.short_code,
            "supports",
        )
        .await;
    checks.mcp_allowed("2 MCP (f): an organization admin links an orphan", &reply);
    checks.check(
        "2 (f): each of those orphans has one parent",
        [&second, &third, &fourth, &fifth]
            .iter()
            .all(|document| parents_of(&mut conn, &document.short_code) == 1),
        "an orphan has no parent, or more than one",
    );

    // =======================================================================
    // Case 3. No person removes the last supports edge of a document
    // (criteria a, b)
    // =======================================================================
    // carol creates a document with a grant, and the admin takes the grant
    // away. What is left is creation alone.
    svc.add_board_member(
        &platform_board,
        &AddBoardMemberRequest {
            user_id: carol_id.to_string(),
            capabilities: vec!["manage_documents".into()],
        },
    )
    .await
    .expect("grant");
    let of_carol = document_by(&carol, &of_platform.short_code, "3: the document of carol").await;
    svc.remove_board_member(&platform_board, &carol_id.to_string())
        .await
        .expect("the grant is taken away");
    // Rule 7: the creator keeps the edit rule.
    checks.allowed(
        "3: carol, the creator, can edit her document with no grant",
        edit(&carol, &of_carol.short_code, "edited by carol").await,
    );
    // The admin created this one: bob edits it as a member of the board of
    // its parent.
    let of_admin = document_by(
        &svc,
        &of_platform.short_code,
        "3: the document of the admin",
    )
    .await;

    let edges_before = edge_count(&mut conn);
    let mut messages = Vec::new();
    let last_of_carol = edge_id(
        &mut conn,
        &of_platform.short_code,
        &of_carol.short_code,
        "supports",
    );
    let last_of_admin = edge_id(
        &mut conn,
        &of_platform.short_code,
        &of_admin.short_code,
        "supports",
    );
    for (who, client, edge) in [
        ("the creator of the document", &carol, &last_of_carol),
        ("a manager of the board of the parent", &bob, &last_of_admin),
        ("an organization admin", &svc, &last_of_admin),
        (
            "an organization admin, on the document of carol",
            &svc,
            &last_of_carol,
        ),
    ] {
        if let Some(refusal) = checks.last_parent(
            &format!("3 REST (a): {who} removes the last supports edge"),
            client.delete_relationship(edge).await,
        ) {
            messages.push(refusal);
        }
    }
    for (who, session, document) in [
        ("the creator of the document", &mut carol_mcp, &of_carol),
        (
            "a manager of the board of the parent",
            &mut bob_mcp,
            &of_admin,
        ),
        ("an organization admin", &mut svc_mcp, &of_admin),
    ] {
        let reply = session
            .link(
                "unlink_items",
                &of_platform.short_code,
                &document.short_code,
                "supports",
            )
            .await;
        checks.mcp_refused(
            &format!("3 MCP (a): {who} removes the last supports edge"),
            LAST_PARENT,
            &reply,
        );
        checks.check(
            &format!("9 MCP (b): the refusal to {who} says what to do"),
            reply
                .1
                .contains("Link the document to a different item first")
                && reply.1.contains("archive the document"),
            &reply.1,
        );
    }
    checks.check(
        "3 (a): each edge is still there",
        edge_count(&mut conn) == edges_before
            && parents_of(&mut conn, &of_carol.short_code) == 1
            && parents_of(&mut conn, &of_admin.short_code) == 1,
        format!("{edges_before} edges before, {} now", edge_count(&mut conn)),
    );
    // A person who can edit NOTHING gets the refusal of the permission,
    // and learns nothing about the parents of the document.
    checks.refused(
        "3 REST: alice edits neither end, and gets 403 and not 422",
        alice.delete_relationship(&last_of_admin).await,
    );

    // =======================================================================
    // Case 9. The message of the refusal of the remove
    // (criterion b)
    // =======================================================================
    checks.check(
        "9: the four REST refusals of case 3 gave a message",
        messages.len() == 4,
        format!("{} messages", messages.len()),
    );
    for (message, details) in &messages {
        checks.check(
            "9 (b): the refusal says to link the document to a different item first",
            message.contains("Link the document to a different item first"),
            message,
        );
        checks.check(
            "9: the refusal says that the caller can archive the document",
            message.contains("archive the document"),
            message,
        );
        checks.check(
            "9: each sentence of the refusal has 20 words or fewer",
            short_sentences(message),
            message,
        );
        checks.check(
            "9: the details name the document and its parent",
            details["relationship"] == json!("supports")
                && details["parent"] == json!(of_platform.short_code)
                && (details["document"] == json!(of_carol.short_code)
                    || details["document"] == json!(of_admin.short_code)),
            details,
        );
    }

    // =======================================================================
    // Case 4. A document with two parents
    // (criterion c)
    // =======================================================================
    let first_parent = task_by(&svc, &platform_board, "4: the first parent").await;
    let second_parent = task_by(&svc, &platform_board, "4: the second parent").await;
    for (label, mcp) in [("REST", false), ("MCP", true)] {
        let document = document_by(
            &bob,
            &first_parent.short_code,
            &format!("4 {label}: two parents"),
        )
        .await;
        let first = edge_id(
            &mut conn,
            &first_parent.short_code,
            &document.short_code,
            "supports",
        );
        // Adding a parent to a document that has one: the link rule as it
        // was. bob can edit the two ends.
        let second = checks.allowed(
            &format!("4 {label} (g): bob gives his document a second parent"),
            bob.create_relationship(&edge(
                &second_parent.short_code,
                &document.short_code,
                "supports",
            ))
            .await,
        );
        if mcp {
            let reply = bob_mcp
                .link(
                    "unlink_items",
                    &first_parent.short_code,
                    &document.short_code,
                    "supports",
                )
                .await;
            checks.mcp_allowed("4 MCP (c): bob removes one of two supports edges", &reply);
        } else {
            checks.allowed(
                "4 REST (c): bob removes one of two supports edges",
                bob.delete_relationship(&first).await,
            );
        }
        checks.check(
            &format!("4 {label} (c): the document has one parent, the second"),
            parents_of(&mut conn, &document.short_code) == 1
                && edges_between(
                    &mut conn,
                    &second_parent.short_code,
                    &document.short_code,
                    "supports",
                ) == 1,
            format!("{} parents", parents_of(&mut conn, &document.short_code)),
        );
        if mcp {
            let reply = bob_mcp
                .link(
                    "unlink_items",
                    &second_parent.short_code,
                    &document.short_code,
                    "supports",
                )
                .await;
            checks.mcp_refused(
                "4 MCP (a): bob removes the edge that is now the last",
                LAST_PARENT,
                &reply,
            );
        } else if let Some(second) = &second {
            checks.last_parent(
                "4 REST (a): bob removes the edge that is now the last",
                bob.delete_relationship(&second.id).await,
            );
        }
        checks.check(
            &format!("4 {label} (a): the document has its one parent"),
            parents_of(&mut conn, &document.short_code) == 1,
            format!("{} parents", parents_of(&mut conn, &document.short_code)),
        );
        // "Link the document to a different item first": the advice of the
        // refusal works. The document moves from the second parent to the
        // first.
        checks.allowed(
            &format!("4 {label} (b): bob links the document to a different item first"),
            bob.create_relationship(&edge(
                &first_parent.short_code,
                &document.short_code,
                "supports",
            ))
            .await,
        );
        if let Some(second) = &second {
            checks.allowed(
                &format!("4 {label} (b): and then removes the old edge"),
                bob.delete_relationship(&second.id).await,
            );
        }
        checks.check(
            &format!("4 {label} (b): the document has one parent, the first"),
            parents_of(&mut conn, &document.short_code) == 1
                && edges_between(
                    &mut conn,
                    &first_parent.short_code,
                    &document.short_code,
                    "supports",
                ) == 1,
            format!("{} parents", parents_of(&mut conn, &document.short_code)),
        );
    }

    // An ARCHIVED parent is a parent: it gives the document its board as
    // it did while live. So the edge to it counts, and the remove of the
    // other edge is not the remove of the last.
    let put_away = task_by(&svc, &platform_board, "4: a parent that is archived").await;
    let stays = task_by(&svc, &platform_board, "4: a parent that stays").await;
    let document = document_by(&bob, &put_away.short_code, "4: one parent is archived").await;
    let live_edge = linked(&bob, &stays.short_code, &document.short_code, "supports").await;
    let archived_edge = edge_id(
        &mut conn,
        &put_away.short_code,
        &document.short_code,
        "supports",
    );
    svc.delete_task(&put_away.short_code)
        .await
        .expect("the admin archives the first parent");
    checks.allowed(
        "4: the edge to an archived parent counts, so bob removes the other edge",
        bob.delete_relationship(&live_edge.id).await,
    );
    checks.allowed(
        "4: the document takes its authority from the archived parent: bob edits it",
        edit(&bob, &document.short_code, "edited by bob").await,
    );
    // As before COLLIERY-T-0235: a write resolves live items only, so an
    // edge with an archived end is not there to remove.
    let result = svc.delete_relationship(&archived_edge).await;
    checks.check(
        "4: the edge to the archived parent is 404 to a remove, as before",
        matches!(result, Err(Error::NotFound { .. })),
        format!("{result:?}"),
    );
    checks.check(
        "4: the document has its archived parent",
        parents_of(&mut conn, &document.short_code) == 1,
        format!("{} parents", parents_of(&mut conn, &document.short_code)),
    );

    // =======================================================================
    // Case 5. The earliest edge
    // (criteria d, e)
    // =======================================================================
    // THE REACHABLE ATTACK, with one person and no grant. alice sends a
    // request to platform: she created the task, so she can edit it. bob,
    // a member of platform, writes a document on that task. alice cannot
    // edit the document. She links a task of her own board to the
    // document (the link rule: she can edit the source), and removes the
    // edge from the request (the link rule: she can edit the source). The
    // edge from her board is now the earliest.
    let request = task_by(&alice, &platform_board, "5: the request of alice").await;
    let target = document_by(&bob, &request.short_code, "5: the document of bob").await;
    let earliest = edge_id(
        &mut conn,
        &request.short_code,
        &target.short_code,
        "supports",
    );
    let before = svc.get_document(&target.short_code).await.expect("read");
    checks.refused(
        "5 (e): BEFORE, alice cannot edit the document of bob",
        edit(&alice, &target.short_code, "taken by alice").await,
    );
    // Adding a parent to a document that has one does not change its
    // authority, so it stays on the link rule as it was.
    let later = checks.allowed(
        "5 (g): alice links her task to a document that has a parent",
        alice
            .create_relationship(&edge(&of_alice.short_code, &target.short_code, "supports"))
            .await,
    );
    checks.refused(
        "5 (e, g): the second edge gave alice nothing: she cannot edit the document",
        edit(&alice, &target.short_code, "taken by alice").await,
    );
    checks.refused(
        "5 REST (d, e): alice removes the EARLIEST edge, from the request that she created",
        alice.delete_relationship(&earliest).await,
    );
    let reply = alice_mcp
        .link(
            "unlink_items",
            &request.short_code,
            &target.short_code,
            "supports",
        )
        .await;
    checks.mcp_refused(
        "5 MCP (d, e): alice removes the EARLIEST edge",
        "FORBIDDEN",
        &reply,
    );
    checks.check(
        "5 (d): the two edges are there",
        parents_of(&mut conn, &target.short_code) == 2
            && edges_between(
                &mut conn,
                &request.short_code,
                &target.short_code,
                "supports",
            ) == 1,
        format!("{} parents", parents_of(&mut conn, &target.short_code)),
    );
    checks.refused(
        "5 (e): AFTER, alice cannot edit the document of bob",
        edit(&alice, &target.short_code, "taken by alice").await,
    );
    checks.refused(
        "5 (e): AFTER, alice cannot archive the document of bob",
        alice.delete_document(&target.short_code).await,
    );
    let after = svc.get_document(&target.short_code).await.expect("read");
    checks.check(
        "5 (d): the document of bob is LIVE and unchanged",
        after == before && !is_archived(&mut conn, &target.short_code),
        format!("before {before:?}\n      after  {after:?}"),
    );
    // The consequence of rule 3, pinned: alice wrote the later edge, and
    // she cannot remove it, because she cannot edit the document.
    if let Some(later) = &later {
        let refusal = checks.refused(
            "5: alice cannot remove the edge that she wrote: she cannot edit the document",
            alice.delete_relationship(&later.id).await,
        );
        if let Some((message, details)) = refusal {
            checks.check(
                "9: the refusal of the remove names the document and manage_documents",
                message.contains("edit the document")
                    && message.contains("manage_documents")
                    && short_sentences(&message),
                &message,
            );
            checks.check(
                "9: the details name manage_documents on the board of the document",
                details["required_capability"] == json!("manage_documents")
                    && details["board_id"] == json!(platform_board)
                    && details["relationship"] == json!("supports"),
                details,
            );
        }
        // The people who can edit the document can remove it.
        checks.allowed(
            "5 (c): bob, who can edit the document, removes the later edge",
            bob.delete_relationship(&later.id).await,
        );
    }

    // The same, with two people. `foreign` is a task of platform: alice
    // cannot edit it. The admin adds the task of alice as a second parent.
    let shared = document_by(&bob, &of_bob.short_code, "5: two teams").await;
    let earliest = edge_id(
        &mut conn,
        &of_bob.short_code,
        &shared.short_code,
        "supports",
    );
    linked(&svc, &of_alice.short_code, &shared.short_code, "supports").await;
    checks.refused(
        "5 REST: alice edits neither the earliest parent nor the document",
        alice.delete_relationship(&earliest).await,
    );
    // carol can edit the earliest parent (she created it, as a request),
    // and not the document.
    let of_carol_task = task_by(&carol, &platform_board, "5: the request of carol").await;
    let guarded = document_by(
        &bob,
        &of_carol_task.short_code,
        "5: on the request of carol",
    )
    .await;
    let earliest = edge_id(
        &mut conn,
        &of_carol_task.short_code,
        &guarded.short_code,
        "supports",
    );
    linked(&svc, &of_alice.short_code, &guarded.short_code, "supports").await;
    checks.refused(
        "5 REST (d): carol can edit the earliest parent, and not the document",
        carol.delete_relationship(&earliest).await,
    );
    let reply = carol_mcp
        .link(
            "unlink_items",
            &of_carol_task.short_code,
            &guarded.short_code,
            "supports",
        )
        .await;
    checks.mcp_refused(
        "5 MCP (d): carol can edit the earliest parent, and not the document",
        "FORBIDDEN",
        &reply,
    );
    checks.check(
        "5 (d): the earliest edges are there",
        parents_of(&mut conn, &shared.short_code) == 2
            && parents_of(&mut conn, &guarded.short_code) == 2,
        "an earliest edge is gone",
    );
    checks.refused(
        "5 (e): alice cannot edit the document that her task supports second",
        edit(&alice, &guarded.short_code, "taken by alice").await,
    );
    // A person who can edit the document gives the authority away, which
    // the rule permits: bob removes the earliest edge, and the board of
    // web answers for the document.
    checks.allowed(
        "5 (c): bob, who can edit the document, removes the earliest edge",
        bob.delete_relationship(&earliest).await,
    );
    checks.allowed(
        "5 (g): the board of web answers for the document now: alice edits it",
        edit(&alice, &guarded.short_code, "edited by alice").await,
    );

    // =======================================================================
    // Case 6. The archive of the only parent
    // =======================================================================
    let only_parent = task_by(&svc, &platform_board, "6: the only parent").await;
    let kept = document_by(&svc, &only_parent.short_code, "6: the document").await;
    checks.allowed(
        "6: BEFORE the archive, bob can edit the document",
        edit(&bob, &kept.short_code, "edited by bob, before").await,
    );
    checks.refused(
        "6: BEFORE the archive, alice cannot",
        edit(&alice, &kept.short_code, "taken by alice").await,
    );
    checks.allowed(
        "6: the admin archives the only parent",
        svc.delete_task(&only_parent.short_code).await,
    );
    checks.check(
        "6: the archive does not follow supports: the document is live, with its edge",
        !is_archived(&mut conn, &kept.short_code)
            && is_archived(&mut conn, &only_parent.short_code)
            && parents_of(&mut conn, &kept.short_code) == 1,
        format!("{} parents", parents_of(&mut conn, &kept.short_code)),
    );
    checks.allowed(
        "6: AFTER the archive, bob can edit the document",
        edit(&bob, &kept.short_code, "edited by bob, after").await,
    );
    checks.refused(
        "6: AFTER the archive, alice cannot",
        edit(&alice, &kept.short_code, "taken by alice").await,
    );
    // The document is not an orphan, so the link rule is the one for a
    // document that has a parent.
    checks.allowed(
        "6 (g): a document with an archived parent is not an orphan to a link",
        alice
            .create_relationship(&edge(&of_alice.short_code, &kept.short_code, "supports"))
            .await,
    );
    checks.refused(
        "6 (e): and that link gave alice nothing",
        edit(&alice, &kept.short_code, "taken by alice").await,
    );

    // =======================================================================
    // Case 7. Each other edge is as it was
    // (criterion g)
    // =======================================================================
    let initiative = svc
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board.clone(),
            column_id: None,
            title: "7: an initiative".into(),
            content: String::new(),
            complexity: None,
            bucket_type: None,
        })
        .await
        .expect("initiative");
    let adr = |board: Option<&str>, title: &str| CreateAdrRequest {
        board_id: board.map(str::to_string),
        column_id: None,
        title: title.into(),
        content: "original content".into(),
        decision_maker: None,
        decision_date: None,
    };
    let adr_one = svc
        .create_adr(&adr(Some(&adr_board), "7: an ADR"))
        .await
        .expect("ADR");
    let adr_two = svc
        .create_adr(&adr(Some(&adr_board), "7: a second ADR"))
        .await
        .expect("ADR");
    let off_board = svc
        .create_adr(&adr(None, "7: an ADR on no board"))
        .await
        .expect("off-board ADR");
    svc.add_board_member(
        &adr_board,
        &AddBoardMemberRequest {
            user_id: bob_id.to_string(),
            capabilities: vec!["manage_adrs".into()],
        },
    )
    .await
    .expect("grant manage_adrs to bob");
    let note = document_by(&alice, &of_alice.short_code, "7: the note of alice").await;

    // (relationship, source, target, who can edit one end, which end)
    let spot = [
        (
            "parent",
            &initiative.short_code,
            &of_alice.short_code,
            &alice,
            "the target",
        ),
        (
            "blocks",
            &of_alice.short_code,
            &of_bob.short_code,
            &alice,
            "the source",
        ),
        (
            "informs",
            &note.short_code,
            &of_bob.short_code,
            &alice,
            "the source",
        ),
        (
            "informs",
            &adr_one.short_code,
            &of_alice.short_code,
            &alice,
            "the target",
        ),
        (
            "supersedes",
            &adr_two.short_code,
            &adr_one.short_code,
            &bob,
            "the two ends",
        ),
        (
            "supports",
            &of_alice.short_code,
            &adr_one.short_code,
            &alice,
            "the source",
        ),
        (
            "supports",
            &of_alice.short_code,
            &off_board.short_code,
            &alice,
            "the source",
        ),
    ];
    for (relationship, source, target, client, end) in spot {
        let case = format!("7 (g) {relationship} {source} -> {target}");
        let edges_before = edge_count(&mut conn);
        checks.refused(
            &format!("{case}: carol edits neither end"),
            carol
                .create_relationship(&edge(source, target, relationship))
                .await,
        );
        let written = checks.allowed(
            &format!("{case}: the link, by a person who can edit {end}"),
            client
                .create_relationship(&edge(source, target, relationship))
                .await,
        );
        if let Some(written) = written {
            checks.refused(
                &format!("{case}: carol cannot remove it"),
                carol.delete_relationship(&written.id).await,
            );
            // For `supports` to an ADR this is the remove of the LAST
            // supports edge, by a person who can edit the source only. An
            // ADR does not take its authority from the edge, so the link
            // rule is as it was.
            checks.allowed(
                &format!("{case}: the remove, by the same person"),
                client.delete_relationship(&written.id).await,
            );
        }
        checks.check(
            &format!("{case}: no edge is left"),
            edge_count(&mut conn) == edges_before,
            format!("{edges_before} before, {} now", edge_count(&mut conn)),
        );
    }
    // The same over MCP, for supports to an ADR and for blocks.
    for (relationship, source, target) in [
        ("supports", &of_alice.short_code, &adr_two.short_code),
        ("blocks", &of_alice.short_code, &of_bob.short_code),
    ] {
        for tool in ["link_items", "unlink_items"] {
            let reply = alice_mcp.link(tool, source, target, relationship).await;
            checks.mcp_allowed(&format!("7 MCP (g) {tool} {relationship}"), &reply);
        }
    }
    // AN ADR ON NO BOARD takes no authority from a supports edge. The edge
    // that alice wrote and removed gave her nothing, and one that stays
    // gives her nothing.
    linked(
        &alice,
        &of_alice.short_code,
        &off_board.short_code,
        "supports",
    )
    .await;
    let current = svc.get_adr(&off_board.short_code).await.expect("read");
    checks.refused(
        "7: a supports edge to an ADR on no board gives no right to edit the ADR",
        alice
            .update_adr(
                &off_board.short_code,
                &UpdateContentRequest {
                    title: None,
                    content: "taken by alice".into(),
                    version: current.version,
                },
            )
            .await,
    );

    // =======================================================================
    // Case 10. Another tenant
    // =======================================================================
    // alice is a member of globex too. The edges and the short codes of
    // acme name nothing there, so she gets the 404 and the VALIDATION of
    // before, and none of the new refusals.
    let edges_before = edge_count(&mut conn);
    let result = globex_alice.delete_relationship(&last_of_admin).await;
    checks.check(
        "10 REST: the remove of a last supports edge from another tenant is 404",
        matches!(result, Err(Error::NotFound { .. })),
        format!("{result:?}"),
    );
    let result = globex_alice
        .create_relationship(&edge(
            &of_alice.short_code,
            &orphan_of_admin.short_code,
            "supports",
        ))
        .await;
    checks.check(
        "10 REST: a link to an orphan from another tenant names no live item",
        matches!(result, Err(Error::Validation { .. })),
        format!("{result:?}"),
    );
    let mut globex_mcp = McpSession::open(&base, &alice_token, "globex").await;
    for (tool, target) in [
        ("unlink_items", &of_admin.short_code),
        ("link_items", &orphan_of_admin.short_code),
    ] {
        let reply = globex_mcp
            .link(tool, &of_platform.short_code, target, "supports")
            .await;
        checks.check(
            &format!("10 MCP {tool} from another tenant"),
            reply.0
                && !reply.1.contains(LAST_PARENT)
                && !reply.1.contains("FORBIDDEN")
                && !reply.1.contains("no parent"),
            &reply.1,
        );
    }
    checks.check(
        "10: no edge was written or removed",
        edge_count(&mut conn) == edges_before,
        format!("{edges_before} before, {} now", edge_count(&mut conn)),
    );

    // =======================================================================
    // Criterion h. The reference page gives the rule
    // =======================================================================
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/src");
    let page = |name: &str| {
        std::fs::read_to_string(docs.join(name)).unwrap_or_else(|e| panic!("reading {name}: {e}"))
    };
    let capabilities = page("reference/capabilities.md");
    checks.check(
        "h: the page Capabilities gives the rule for a document with no parent",
        capabilities.contains("A document with no parent")
            && capabilities.contains("A document always has a parent")
            && capabilities.contains(LAST_PARENT),
        "reference/capabilities.md does not give the rule",
    );
    checks.check(
        "h: the page Errors has the code",
        page("reference/errors.md").contains(&format!("`{LAST_PARENT}`")),
        "reference/errors.md does not have LAST_PARENT",
    );
    checks.check(
        "h: the page MCP tools gives the rule on unlink_items",
        page("reference/mcp-tools.md").contains(LAST_PARENT),
        "reference/mcp-tools.md does not have LAST_PARENT",
    );

    drop((alice_mcp, bob_mcp, carol_mcp, svc_mcp, globex_mcp));
    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
    checks.finish();
}

/// The text of the MCP refusal of a link from the task of alice to the
/// orphan.
async fn alice_mcp_refusal(session: &mut McpSession, source: &Task, orphan: &Document) -> String {
    session
        .link(
            "link_items",
            &source.short_code,
            &orphan.short_code,
            "supports",
        )
        .await
        .1
}

/// Does the refusal say who can link a document with no parent?
fn reply_names(text: &str) -> bool {
    text.contains("no parent") && text.contains("creator") && text.contains("organization admin")
}
