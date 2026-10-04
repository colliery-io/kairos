//! Integration test for COLLIERY-T-0235, as COLLIERY-T-3109 changed it: a
//! link to a document gives no authority over the document.
//!
//! THE FACTS. Until COLLIERY-T-3109 a document could have no board, and its
//! authorization board was then the board of its EARLIEST `supports`
//! parent. So a `supports` edge could give the authority over a document:
//! a person who could edit some task linked it to a document with no
//! parent, and could then edit and archive the document (defect 1), or
//! removed the earliest of two parents (defect 2). COLLIERY-T-0235 closed
//! the two with three rules: no person removes the last `supports` edge of
//! a document (422 `LAST_PARENT`), a link to a document with no parent
//! needs the right to edit the document, and the remove of a `supports`
//! edge needs the right to edit the document.
//!
//! Each document has an owner board now (COLLIERY-T-3109), and its
//! authorization board is that board. No edge changes it. So the first two
//! rules are gone, and the attacks of before must still fail:
//!
//! 1. A link to a document gives no right to edit it or to archive it.
//! 2. The remove of a parent does not move the authority.
//! 3. To remove a `supports` edge of a document, the principal must be
//!    able to edit the DOCUMENT (the third rule stays).
//! 4. The last `supports` edge of a document can go, and the document
//!    keeps its owner board.
//!
//! This is a security boundary, so the negative cases are the contract.
//! Each refusal is checked where the data is stored.
//!
//! Every check is recorded and the test fails at the end with the full
//! list, so one run shows each broken case and not only the first.
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
    CreateDocumentRequest, CreateTaskRequest, Document, Task, UpdateContentRequest,
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

/// The code of the refusal that COLLIERY-T-3109 removed.
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

/// A document on the owner board `board`, which supports `parent` when
/// there is one.
async fn document_by(
    client: &KairosClient,
    board: &str,
    parent: Option<&str>,
    title: &str,
) -> Document {
    client
        .create_document(&CreateDocumentRequest {
            board: board.into(),
            title: title.into(),
            content: Some("original content".into()),
            template_id: None,
            parent_short_code: parent.map(str::to_string),
        })
        .await
        .unwrap_or_else(|e| panic!("creating document {title:?}: {e}"))
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
async fn a_link_gives_no_authority_over_a_document_against_live_stack() {
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
                code_prefix: kairos_core::short_code::prefix_from_slug(slug),
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

    let mut checks = Checks::default();
    let mut alice_mcp = McpSession::open(&base, &alice_token, "acme").await;
    let mut bob_mcp = McpSession::open(&base, &bob_token, "acme").await;
    let mut carol_mcp = McpSession::open(&base, &carol_token, "acme").await;
    let svc_mcp = McpSession::open(&base, &svc_token, "acme").await;

    // The items that the cases share. `of_alice` is on the board of web:
    // alice edits it as a member of the team. `of_bob` is on the board of
    // platform: alice can NOT edit it.
    let of_alice = task_by(&alice, &web_board, "a task of alice").await;
    let of_bob = task_by(&bob, &platform_board, "a task of bob").await;
    // The admin created it, so bob edits it as a member of platform only.
    let of_platform = task_by(&svc, &platform_board, "a task of platform").await;

    // =======================================================================
    // Case 1. A link gives no right to edit or to archive (defect 1)
    // =======================================================================
    // `owned` is owned by the board of platform and supports nothing. The
    // admin created it, so alice can NOT edit it.
    let owned = document_by(&svc, &platform_board, None, "1: owned by platform").await;
    checks.check(
        "1: the fixture: the document supports nothing",
        parents_of(&mut conn, &owned.short_code) == 0,
        "the document has a parent",
    );
    checks.refused(
        "1: before the link, alice cannot edit",
        edit(&alice, &owned.short_code, "taken by alice").await,
    );
    // alice can edit the source, so the link rule lets her write the edge.
    checks.allowed(
        "1 REST: alice links her task to the document",
        alice
            .create_relationship(&edge(&of_alice.short_code, &owned.short_code, "supports"))
            .await,
    );
    let refusal = checks.refused(
        "1 REST: the link gives alice no right to edit",
        edit(&alice, &owned.short_code, "taken by alice").await,
    );
    if let Some((_, details)) = refusal {
        checks.check(
            "1: the refusal names the owner board, and not the board of the parent",
            details["board_id"].as_str() == Some(platform_board.as_str()),
            details,
        );
    }
    checks.refused(
        "1 REST: the link gives alice no right to archive",
        alice.delete_document(&owned.short_code).await,
    );
    let reply = alice_mcp
        .call(
            "update_item",
            json!({"short_code": owned.short_code, "content": "taken by alice", "version": 1}),
        )
        .await;
    checks.mcp_refused("1 MCP: update_item stays refused", "FORBIDDEN", &reply);
    let reply = alice_mcp
        .call(
            "delete_item",
            json!({"short_code": owned.short_code, "confirm": true}),
        )
        .await;
    checks.mcp_refused("1 MCP: delete_item stays refused", "FORBIDDEN", &reply);
    let after = svc.get_document(&owned.short_code).await.expect("read");
    checks.check(
        "1: the document is as it was",
        after.content == "original content"
            && after.board_id == platform_board
            && !is_archived(&mut conn, &owned.short_code),
        format!("{after:?}"),
    );
    // carol edits neither end: the link rule refuses her.
    checks.refused(
        "1: carol edits neither end, and writes no edge",
        carol
            .create_relationship(&edge(&of_platform.short_code, &owned.short_code, "informs"))
            .await,
    );
    let reply = carol_mcp
        .link(
            "link_items",
            &of_alice.short_code,
            &owned.short_code,
            "supports",
        )
        .await;
    checks.mcp_refused("1 MCP: carol writes no edge", "FORBIDDEN", &reply);

    // =======================================================================
    // Case 2. The remove of a parent does not move the authority (defect 2)
    // =======================================================================
    // `two` supports the task of alice (the earliest parent) and the task of
    // bob. Its owner board is the board of platform.
    let two = document_by(
        &svc,
        &platform_board,
        Some(&of_alice.short_code),
        "2: two parents",
    )
    .await;
    linked(&svc, &of_bob.short_code, &two.short_code, "supports").await;
    checks.refused(
        "2: with two parents, alice cannot edit",
        edit(&alice, &two.short_code, "taken by alice").await,
    );
    // bob manages the owner board: he removes the earliest parent.
    let earliest = edge_id(&mut conn, &of_alice.short_code, &two.short_code, "supports");
    checks.allowed(
        "2: bob removes the earliest parent",
        bob.delete_relationship(&earliest).await,
    );
    checks.refused(
        "2: alice still cannot edit",
        edit(&alice, &two.short_code, "taken by alice").await,
    );
    checks.allowed(
        "2: bob edits as a manager of the owner board",
        edit(&bob, &two.short_code, "edited by bob").await,
    );

    // =======================================================================
    // Case 3. The remove of a supports edge needs the right to edit the
    // document
    // =======================================================================
    let edge_of_alice = edge_id(
        &mut conn,
        &of_alice.short_code,
        &owned.short_code,
        "supports",
    );
    let edges_before = edge_count(&mut conn);
    let refusal = checks.refused(
        "3 REST: alice wrote the edge, and cannot remove it",
        alice.delete_relationship(&edge_of_alice).await,
    );
    if let Some((message, details)) = refusal {
        checks.check(
            "3: the refusal says what is missing",
            message
                == "To remove a supports edge of a document, you must be able to edit the \
                    document. You need \"manage_documents\" on the owner board of the \
                    document. The creator of the document and an organization admin can also \
                    remove it."
                && short_sentences(&message),
            &message,
        );
        checks.check(
            "3: the details name the owner board",
            details["board_id"].as_str() == Some(platform_board.as_str())
                && details["required_capability"] == "manage_documents",
            details,
        );
    }
    let reply = alice_mcp
        .link(
            "unlink_items",
            &of_alice.short_code,
            &owned.short_code,
            "supports",
        )
        .await;
    checks.mcp_refused("3 MCP: unlink_items stays refused", "FORBIDDEN", &reply);
    checks.check(
        "3: no edge was removed",
        edge_count(&mut conn) == edges_before
            && edges_between(
                &mut conn,
                &of_alice.short_code,
                &owned.short_code,
                "supports",
            ) == 1,
        format!("{edges_before} before, {} now", edge_count(&mut conn)),
    );

    // =======================================================================
    // Case 4. The last supports edge can go, and the owner board stays
    // =======================================================================
    let reply = bob_mcp
        .link(
            "unlink_items",
            &of_alice.short_code,
            &owned.short_code,
            "supports",
        )
        .await;
    checks.mcp_allowed("4 MCP: bob removes the last supports edge", &reply);
    checks.check(
        "4 MCP: the reply does not name LAST_PARENT",
        !reply.1.contains(LAST_PARENT),
        &reply.1,
    );
    let last = edge_id(&mut conn, &of_bob.short_code, &two.short_code, "supports");
    checks.allowed(
        "4 REST: the admin removes the last supports edge",
        svc.delete_relationship(&last).await,
    );
    for document in [&owned, &two] {
        let read = svc.get_document(&document.short_code).await.expect("read");
        checks.check(
            "4: the document supports nothing, and it keeps its owner board",
            parents_of(&mut conn, &document.short_code) == 0
                && read.board_id == platform_board
                && read.archived_at.is_none(),
            format!("{read:?}"),
        );
    }
    // The creator of a document can remove its last edge with no grant.
    grant(&svc, &initiative_board, carol_id, "manage_documents").await;
    let of_carol = document_by(
        &carol,
        &initiative_board,
        Some(&of_platform.short_code),
        "4: by carol",
    )
    .await;
    svc.remove_board_member(&initiative_board, &carol_id.to_string())
        .await
        .expect("removing the grant of carol");
    let reply = carol_mcp
        .link(
            "unlink_items",
            &of_platform.short_code,
            &of_carol.short_code,
            "supports",
        )
        .await;
    checks.mcp_allowed("4 MCP: the creator removes the last supports edge", &reply);

    // =======================================================================
    // Case 5. Another tenant
    // =======================================================================
    let target = document_by(
        &svc,
        &platform_board,
        Some(&of_platform.short_code),
        "5: one parent",
    )
    .await;
    let edges_before = edge_count(&mut conn);
    let id = edge_id(
        &mut conn,
        &of_platform.short_code,
        &target.short_code,
        "supports",
    );
    let result = globex_alice.delete_relationship(&id).await;
    checks.check(
        "5 REST: the remove of an edge from another tenant is 404",
        matches!(result, Err(Error::NotFound { .. })),
        format!("{result:?}"),
    );
    let result = globex_alice
        .create_relationship(&edge(&of_alice.short_code, &target.short_code, "supports"))
        .await;
    checks.check(
        "5 REST: a link from another tenant names no live item",
        matches!(result, Err(Error::Validation { .. })),
        format!("{result:?}"),
    );
    let mut globex_mcp = McpSession::open(&base, &alice_token, "globex").await;
    for tool in ["unlink_items", "link_items"] {
        let reply = globex_mcp
            .link(
                tool,
                &of_platform.short_code,
                &target.short_code,
                "supports",
            )
            .await;
        checks.check(
            &format!("5 MCP {tool} from another tenant"),
            reply.0 && !reply.1.contains("FORBIDDEN"),
            &reply.1,
        );
    }
    checks.check(
        "5: no edge was written or removed",
        edge_count(&mut conn) == edges_before,
        format!("{edges_before} before, {} now", edge_count(&mut conn)),
    );

    // =======================================================================
    // The reference pages give the rule of today
    // =======================================================================
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/src");
    let page = |name: &str| {
        std::fs::read_to_string(docs.join(name)).unwrap_or_else(|e| panic!("reading {name}: {e}"))
    };
    for name in [
        "reference/capabilities.md",
        "reference/errors.md",
        "reference/mcp-tools.md",
    ] {
        checks.check(
            &format!("docs: {name} does not give LAST_PARENT"),
            !page(name).contains(LAST_PARENT),
            format!("{name} has LAST_PARENT"),
        );
    }
    checks.check(
        "docs: the page Capabilities says that a document has an owner board",
        page("reference/capabilities.md").contains("Each document has an owner board"),
        "reference/capabilities.md does not give the rule",
    );

    drop((alice_mcp, bob_mcp, carol_mcp, svc_mcp, globex_mcp));
    drop(server);
    drop(pool);
    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
    checks.finish();
}
