//! An admin agent (KAIROS-T-0337): the structure capability
//! `manage_structure`, at the organization and on one board, held by a
//! service account, and the MCP tools `create_team`, `create_board`,
//! `add_team_member` and `grant_capability`.
//!
//! 1. An organization admin grants `manage_structure` to one service
//!    account at the organization, and to a second one on one board. A
//!    second grant at the same scope is a 409. A member cannot grant it, and
//!    a holder of it cannot grant it.
//! 2. `whoami` (REST and MCP) of each key shows the grant and its scope.
//! 3. With the organization grant, `create_team` makes the team, its
//!    delivery board and its ADR board; `create_board` makes a board;
//!    `add_team_member` and `grant_capability` work. The activity log names
//!    the service account. An unknown argument is refused and named.
//! 4. With the board grant, the tools work on that board (a grant, a new
//!    column over REST), and refuse `create_team`, `create_board`,
//!    `add_team_member` and each other board, naming the capability and the
//!    scope.
//! 5. With a member key (no grant), each tool refuses and names the
//!    capability and who can do it.
//! 6. With either grant, a service account is still refused on the account,
//!    key, SCIM and tenant routes, and on the routes of the grants.
//! 7. The admin revokes the board grant; the board agent is refused again.
//!    The delete of a service account removes its grant.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_structure_grants_t0337_test`.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text as SqlText, Uuid as SqlUuid};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, request, spawn_server, user_token,
    with_database,
};
use kairos_client::Error;
use kairos_client::types_org::{
    CreateColumnRequest, CreateStructureGrantRequest, CreateTeamRequest,
};
use kairos_client::types_service_accounts::{CreateApiKeyRequest, CreateServiceAccountRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_structure_grants_t0337_test";

// ---------------------------------------------------------------------------
// MCP over the production router (the tenant comes from the API key)
// ---------------------------------------------------------------------------

async fn post_mcp(
    router: &Router,
    token: &str,
    session: Option<&str>,
    body: Value,
) -> (StatusCode, Option<String>, String) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("host", "acme.kairos.test")
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, session, String::from_utf8_lossy(&bytes).to_string())
}

fn rpc_message(body: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return value;
    }
    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data:")
            && let Ok(value) = serde_json::from_str::<Value>(data.trim())
            && value.get("jsonrpc").is_some()
        {
            return value;
        }
    }
    panic!("no JSON-RPC message in response body: {body:?}");
}

struct Mcp<'a> {
    router: &'a Router,
    token: String,
    session: Option<String>,
    next_id: i64,
}

impl<'a> Mcp<'a> {
    async fn connect(router: &'a Router, token: &str) -> Mcp<'a> {
        let (status, session, body) = post_mcp(
            router,
            token,
            None,
            json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "kairos-t0337-test", "version": "0.0.0"},
                },
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "initialize: {body}");
        let (status, _, body) = post_mcp(
            router,
            token,
            session.as_deref(),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "initialized: {body}");
        Mcp {
            router,
            token: token.to_string(),
            session,
            next_id: 0,
        }
    }

    async fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        self.next_id += 1;
        let (status, _, body) = post_mcp(
            self.router,
            &self.token,
            self.session.as_deref(),
            json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{tool}: {body}");
        let result = rpc_message(&body)["result"].clone();
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} returned no text: {result}"))
            .to_string();
        (result["isError"].as_bool().unwrap_or(false), text)
    }

    async fn ok(&mut self, tool: &str, arguments: Value) -> String {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(!is_error, "{tool} unexpectedly errored: {text}");
        text
    }

    async fn err(&mut self, tool: &str, arguments: Value) -> String {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(is_error, "{tool} unexpectedly succeeded: {text}");
        text
    }
}

// ---------------------------------------------------------------------------
// Database probes
// ---------------------------------------------------------------------------

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .expect("user row")
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct BoardRow {
    #[diesel(sql_type = SqlText)]
    slug: String,
    #[diesel(sql_type = SqlText)]
    board_level: String,
    #[diesel(sql_type = SqlText)]
    code_prefix: String,
}

/// The live boards of the team `team_slug`: (slug, level, prefix), by slug.
fn boards_of_team(conn: &mut PgConnection, team_slug: &str) -> Vec<(String, String, String)> {
    sql_query(
        "SELECT b.slug, b.board_level::text AS board_level, b.code_prefix \
         FROM org_acme.boards b JOIN org_acme.teams t ON t.id = b.team_id \
         WHERE t.slug = $1 AND b.deleted_at IS NULL ORDER BY b.slug",
    )
    .bind::<SqlText, _>(team_slug)
    .load::<BoardRow>(conn)
    .expect("boards of the team")
    .into_iter()
    .map(|b| (b.slug, b.board_level, b.code_prefix))
    .collect()
}

/// Activity rows of `actor` with this action and entity type.
fn activity_count(conn: &mut PgConnection, actor: Uuid, action: &str, entity_type: &str) -> i64 {
    sql_query(
        "SELECT count(*) AS n FROM org_acme.activity_log \
         WHERE actor_id = $1 AND action::text = $2 AND entity_type = $3",
    )
    .bind::<SqlUuid, _>(actor)
    .bind::<SqlText, _>(action)
    .bind::<SqlText, _>(entity_type)
    .get_result::<Count>(conn)
    .expect("activity count")
    .n
}

fn structure_grant_count(conn: &mut PgConnection, user: Uuid) -> i64 {
    sql_query("SELECT count(*) AS n FROM org_acme.structure_grants WHERE user_id = $1")
        .bind::<SqlUuid, _>(user)
        .get_result::<Count>(conn)
        .expect("grant count")
        .n
}

fn forbidden_message(err: Error) -> (String, Value) {
    match err {
        Error::Forbidden {
            message, details, ..
        } => (message, details),
        other => panic!("expected Forbidden, got {other}"),
    }
}

#[tokio::test]
async fn an_admin_agent_sets_up_a_team_with_manage_structure() {
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
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));
    let server = spawn_server(router.clone()).await;
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    for client in [&admin, &alice] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
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

    // Two teams: `web` gets the board grant, `data` is the other board.
    for (name, slug, prefix) in [("Web", "web", "WEB"), ("Data", "data", "DATA")] {
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

    // The write routes of a board take its id.
    let web_board = admin
        .get_board("web-delivery")
        .await
        .expect("board")
        .board
        .id;
    let data_board = admin
        .get_board("data-delivery")
        .await
        .expect("board")
        .board
        .id;

    // Three service accounts, one key each.
    let mut accounts = Vec::new();
    for name in ["org-agent", "board-agent", "member-agent"] {
        let account = admin
            .create_service_account(&CreateServiceAccountRequest { name: name.into() })
            .await
            .expect("service account");
        let key = admin
            .create_api_key(
                &account.id,
                &CreateApiKeyRequest {
                    name: format!("{name}-key"),
                    expires_at: None,
                    no_expiry: false,
                    scope: None,
                },
            )
            .await
            .expect("key");
        accounts.push((account.id, key.key));
    }
    let (org_agent_id, org_key) = accounts[0].clone();
    let (board_agent_id, board_key) = accounts[1].clone();
    let (_member_agent_id, member_key) = accounts[2].clone();
    let org_agent = server.client(&org_key, "acme");
    let board_agent = server.client(&board_key, "acme");
    let org_agent_uuid: Uuid = org_agent_id.parse().expect("uuid");
    let board_agent_uuid: Uuid = board_agent_id.parse().expect("uuid");

    // --- 1. The grants ---------------------------------------------------------
    let org_grant = admin
        .create_structure_grant(&CreateStructureGrantRequest {
            user: org_agent_id.clone(),
            board: None,
        })
        .await
        .expect("grant at the organization");
    assert_eq!(org_grant.scope, "organization");
    assert_eq!(org_grant.kind, "service_account");
    assert_eq!(org_grant.board_id, None);
    // The service account by its name.
    let board_grant = admin
        .create_structure_grant(&CreateStructureGrantRequest {
            user: "board-agent".into(),
            board: Some("web-delivery".into()),
        })
        .await
        .expect("grant on one board");
    assert_eq!(board_grant.scope, "board");
    assert_eq!(board_grant.user_id, board_agent_id);
    assert_eq!(board_grant.board_slug.as_deref(), Some("web-delivery"));
    match admin
        .create_structure_grant(&CreateStructureGrantRequest {
            user: org_agent_id.clone(),
            board: None,
        })
        .await
    {
        Err(Error::Conflict { message, .. }) => {
            assert!(message.contains("at the organization already"), "{message}")
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
    // A member cannot grant it.
    let (message, details) = forbidden_message(
        alice
            .create_structure_grant(&CreateStructureGrantRequest {
                user: alice_id.to_string(),
                board: None,
            })
            .await
            .expect_err("a member cannot grant manage_structure"),
    );
    assert!(message.contains("Only an organization admin"), "{message}");
    assert_eq!(details["required_capability"], "manage_structure");
    let listed = alice
        .list_structure_grants()
        .await
        .expect("each member reads the list");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].scope, "organization");
    assert_eq!(listed[1].scope, "board");
    // The grants are in the activity log, with the admin as the actor.
    let svc_id = user_id(&mut conn, "svc@kairos.test");
    assert_eq!(
        activity_count(&mut conn, svc_id, "capability_grant", "organization"),
        1
    );

    // --- 2. whoami ---------------------------------------------------------------
    let who = org_agent.whoami().await.expect("whoami");
    assert_eq!(who.structure.len(), 1);
    assert_eq!(who.structure[0].scope, "organization");
    assert_eq!(who.structure[0].board_slug, None);
    let who = board_agent.whoami().await.expect("whoami");
    assert_eq!(who.structure.len(), 1);
    assert_eq!(who.structure[0].scope, "board");
    assert_eq!(who.structure[0].board_slug.as_deref(), Some("web-delivery"));
    let who = server
        .client(&member_key, "acme")
        .whoami()
        .await
        .expect("whoami");
    assert!(who.structure.is_empty());

    let mut org_mcp = Mcp::connect(&router, &org_key).await;
    let mut board_mcp = Mcp::connect(&router, &board_key).await;
    let mut member_mcp = Mcp::connect(&router, &member_key).await;
    let text = org_mcp.ok("whoami", json!({})).await;
    assert!(text.contains("## Structure (manage_structure)"), "{text}");
    assert!(text.contains("- on the organization: teams"), "{text}");
    let text = board_mcp.ok("whoami", json!({})).await;
    assert!(
        text.contains("- on the board web-delivery: its columns"),
        "{text}"
    );
    let text = member_mcp.ok("whoami", json!({})).await;
    assert!(
        text.contains("(none. An organization admin grants it.)"),
        "{text}"
    );

    // --- 3. The organization grant -------------------------------------------------
    let text = org_mcp
        .err(
            "create_team",
            json!({"name": "Home ops", "slug": "home-ops", "code_prefix": "OPS", "colour": "red"}),
        )
        .await;
    assert!(
        text.starts_with("VALIDATION: The call has the argument \"colour\""),
        "{text}"
    );
    let text = org_mcp
        .ok(
            "create_team",
            json!({"name": "Home ops", "slug": "home-ops", "code_prefix": "OPS"}),
        )
        .await;
    assert!(text.contains("Created team home-ops"), "{text}");
    assert!(text.contains("home-ops-delivery"), "{text}");
    // KAIROS-T-0448: as REST and the CLI, the default is the delivery board
    // only. No ADR board.
    assert!(!text.contains("ADR board"), "{text}");
    assert_eq!(
        boards_of_team(&mut conn, "home-ops"),
        [(
            "home-ops-delivery".to_string(),
            "delivery".to_string(),
            "OPS".to_string()
        )]
    );
    // `adr_board: true` makes the ADR board of the team in the same call.
    let text = org_mcp
        .ok(
            "create_team",
            json!({"name": "Lab", "slug": "lab", "code_prefix": "LAB", "adr_board": true}),
        )
        .await;
    assert!(text.contains("ADR board: lab-adrs"), "{text}");
    assert_eq!(
        boards_of_team(&mut conn, "lab"),
        [
            ("lab-adrs".to_string(), "adr".to_string(), "LAB".to_string()),
            (
                "lab-delivery".to_string(),
                "delivery".to_string(),
                "LAB".to_string()
            ),
        ]
    );
    let text = org_mcp
        .ok(
            "create_board",
            json!({
                "name": "Ops strategy",
                "slug": "ops-strategy",
                "board_level": "strategy",
                "code_prefix": "OPSS",
            }),
        )
        .await;
    assert!(
        text.starts_with("Created board ops-strategy (Ops strategy, strategy, prefix OPSS)"),
        "{text}"
    );
    let text = org_mcp
        .ok(
            "add_team_member",
            json!({"team": "home-ops", "user": "alice@kairos.test"}),
        )
        .await;
    assert!(text.contains("to the team home-ops"), "{text}");
    // KAIROS-T-0679: a person who has not signed in yet. The refusal says
    // that no account has the email, and names the routes of this
    // deployment (an issuer, no local accounts).
    let text = org_mcp
        .err(
            "add_team_member",
            json!({"team": "home-ops", "user": "nobody-yet@kairos.test"}),
        )
        .await;
    assert!(
        text.starts_with("VALIDATION: No member of the organization has the id")
            && text.contains("No account has the email \"nobody-yet@kairos.test\" yet.")
            && text.contains("signs in to Kairos one time through the identity provider")
            && text.contains("kairos members add --email <email>")
            && text.contains("SCIM")
            && !text.contains("local account")
            && text.contains("Then send the reference again."),
        "{text}"
    );
    // A name that is not an email gets the routes, and no email sentence.
    let text = org_mcp
        .err(
            "grant_capability",
            json!({"user": "nobody-yet", "capability": "manage_tasks", "board": "lab-delivery"}),
        )
        .await;
    assert!(
        !text.contains("No account has the email") && text.contains("through SCIM"),
        "{text}"
    );
    // A call that does not add a person keeps the short refusal.
    let text = org_mcp
        .err(
            "remove_team_member",
            json!({"team": "home-ops", "user": "nobody-yet@kairos.test"}),
        )
        .await;
    assert!(
        text.contains("yet. Send a member of the organization."),
        "{text}"
    );
    // A service account by its name.
    org_mcp
        .ok(
            "add_team_member",
            json!({"team": "home-ops", "user": "board-agent"}),
        )
        .await;

    // --- KAIROS-T-0434: the team page over MCP ------------------------------------
    // Reads are open: the tree of a new team is its charter.
    let text = org_mcp.ok("team_pages", json!({"team": "home-ops"})).await;
    assert!(text.contains("charter — Team Charter [charter]"), "{text}");
    // manage_structure gives no right on a team page: membership does.
    let text = org_mcp
        .err(
            "create_team_page",
            json!({"team": "home-ops", "kind": "folder", "title": "Runbooks"}),
        )
        .await;
    assert!(text.contains("member of the team"), "{text}");
    // board-agent is a member of home-ops now.
    let text = board_mcp
        .ok(
            "create_team_page",
            json!({"team": "home-ops", "kind": "folder", "title": "Ops Guides"}),
        )
        .await;
    assert!(
        text.contains("path ops-guides"),
        "the slug from the title: {text}"
    );
    let text = board_mcp
        .ok(
            "create_team_page",
            json!({
                "team": "home-ops", "kind": "page", "title": "Restart the worker",
                "parent": "ops-guides", "content": "# Restart",
            }),
        )
        .await;
    assert!(
        text.contains("path ops-guides/restart-the-worker"),
        "{text}"
    );
    let text = board_mcp
        .ok(
            "team_pages",
            json!({"team": "home-ops", "page": "ops-guides/restart-the-worker"}),
        )
        .await;
    assert!(
        text.contains("- version: 1") && text.contains("# Restart"),
        "{text}"
    );
    let text = board_mcp
        .ok(
            "edit_team_page",
            json!({
                "team": "home-ops", "page": "ops-guides/restart-the-worker",
                "content": "# Restart\n\n1. Drain.", "version": 1,
            }),
        )
        .await;
    assert!(text.contains("version 2"), "{text}");
    // A stale version is refused, with the current page.
    let text = board_mcp
        .err(
            "edit_team_page",
            json!({
                "team": "home-ops", "page": "ops-guides/restart-the-worker",
                "content": "lost", "version": 1,
            }),
        )
        .await;
    assert!(text.starts_with("CONFLICT"), "{text}");
    // The charter: edited, never moved or archived.
    let text = board_mcp
        .ok(
            "edit_team_page",
            json!({
                "team": "home-ops", "page": "charter", "title": "Home ops charter",
                "content": "# Home ops\n\n## Mission\n\nKeep the house running.", "version": 1,
            }),
        )
        .await;
    assert!(text.contains("Home ops charter"), "{text}");
    board_mcp
        .err(
            "move_team_page",
            json!({"team": "home-ops", "page": "charter", "parent": "ops-guides"}),
        )
        .await;
    board_mcp
        .err(
            "delete_team_page",
            json!({"team": "home-ops", "page": "charter"}),
        )
        .await;
    // Move the page to the root, then archive the folder (KAIROS-T-0439: a
    // folder is archived with what is in it; here it is empty).
    let text = board_mcp
        .ok(
            "move_team_page",
            json!({"team": "home-ops", "page": "ops-guides/restart-the-worker", "to_root": true}),
        )
        .await;
    assert!(text.contains("path restart-the-worker"), "{text}");
    board_mcp
        .ok(
            "delete_team_page",
            json!({"team": "home-ops", "page": "ops-guides"}),
        )
        .await;
    let text = board_mcp
        .ok("team_pages", json!({"team": "home-ops"}))
        .await;
    assert!(
        text.contains("restart-the-worker") && !text.contains("ops-guides"),
        "{text}"
    );
    // KAIROS-T-0437: a document of the team is a node of its tree. Nobody
    // placed it, so it is at the end of the root; move_team_page with its
    // short code places it in a folder.
    let text = board_mcp
        .ok(
            "create_item",
            json!({"item_type": "document", "board": "home-ops-delivery", "title": "Network plan"}),
        )
        .await;
    let doc_code = text
        .split_whitespace()
        .find(|w| w.starts_with("OPS-D-"))
        .expect("the short code of the document")
        .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string();
    let text = board_mcp
        .ok("team_pages", json!({"team": "home-ops"}))
        .await;
    assert!(
        text.contains(&format!("- {doc_code} — Network plan [document, draft]")),
        "{text}"
    );
    board_mcp
        .ok(
            "create_team_page",
            json!({"team": "home-ops", "kind": "folder", "title": "Design"}),
        )
        .await;
    let text = board_mcp
        .ok(
            "move_team_page",
            json!({"team": "home-ops", "page": doc_code, "parent": "design"}),
        )
        .await;
    assert!(
        text.contains("Placed") && text.contains("in design"),
        "{text}"
    );
    let text = board_mcp
        .ok("team_pages", json!({"team": "home-ops"}))
        .await;
    assert!(
        text.contains(&format!("  - {doc_code} — Network plan")),
        "nested under design: {text}"
    );
    let text = board_mcp
        .err("team_pages", json!({"team": "home-ops", "page": "nowhere"}))
        .await;
    assert!(text.contains("has no page \"nowhere\""), "{text}");
    let text = org_mcp
        .ok(
            "grant_capability",
            json!({
                "user": "alice@kairos.test",
                "capability": "configure_boards",
                "board": "home-ops-delivery",
            }),
        )
        .await;
    assert!(
        text.starts_with("Granted configure_boards on the board home-ops-delivery"),
        "{text}"
    );
    // The organization grant reaches each board, also one it did not make.
    org_mcp
        .ok(
            "grant_capability",
            json!({"user": "alice@kairos.test", "capability": "manage_adrs", "board": "data-delivery"}),
        )
        .await;
    // Not manage_structure: only an organization admin grants it.
    let text = org_mcp
        .err(
            "grant_capability",
            json!({"user": "member-agent", "capability": "manage_structure"}),
        )
        .await;
    assert!(
        text.starts_with(
            "FORBIDDEN: Only an organization admin grants and revokes the capability \
             \"manage_structure\"."
        ),
        "{text}"
    );
    // The activity log names the service account.
    assert_eq!(
        activity_count(&mut conn, org_agent_uuid, "create", "team"),
        // KAIROS-T-0440: the ops-strategy board made its own team too.
        5
    );
    assert_eq!(
        activity_count(&mut conn, org_agent_uuid, "capability_grant", "board"),
        2
    );

    // --- 4. The board grant ----------------------------------------------------------
    let text = board_mcp
        .ok(
            "grant_capability",
            json!({"user": alice_id.to_string(), "capability": "transition_items", "board": "web-delivery"}),
        )
        .await;
    assert!(text.contains("on the board web-delivery"), "{text}");
    assert_eq!(
        activity_count(&mut conn, board_agent_uuid, "capability_grant", "board"),
        1
    );
    // The configuration of that board, over REST.
    board_agent
        .add_column(
            &web_board,
            &CreateColumnRequest {
                name: "Review".into(),
                position: 9,
            },
        )
        .await
        .expect("a column on the board of the grant");
    for (tool, arguments) in [
        (
            "create_team",
            json!({"name": "Other", "slug": "other", "code_prefix": "OTH"}),
        ),
        (
            "create_board",
            json!({"name": "Notes", "slug": "notes", "board_level": "adr", "code_prefix": "NOTE"}),
        ),
        (
            "add_team_member",
            json!({"team": "web", "user": "alice@kairos.test"}),
        ),
    ] {
        let text = board_mcp.err(tool, arguments).await;
        assert!(
            text.starts_with(
                "FORBIDDEN: This action requires the capability \"manage_structure\" on the \
                 organization, or the organization admin role. A grant of \"manage_structure\" \
                 on one board does not give it."
            ),
            "{tool}: {text}"
        );
        assert!(
            text.contains("\"scope\":\"organization\""),
            "{tool}: {text}"
        );
    }
    // Each other board.
    let text = board_mcp
        .err(
            "grant_capability",
            json!({"user": alice_id.to_string(), "capability": "transition_items", "board": "data-delivery"}),
        )
        .await;
    assert!(
        text.starts_with("FORBIDDEN: This action requires the capability \"administer_members\""),
        "{text}"
    );
    assert!(
        text.contains(
            "or the capability \"manage_structure\" on that board or on the organization"
        ),
        "{text}"
    );
    assert!(
        text.contains("\"or_capability\":\"manage_structure\""),
        "{text}"
    );
    let (message, details) = forbidden_message(
        board_agent
            .add_column(
                &data_board,
                &CreateColumnRequest {
                    name: "Review".into(),
                    position: 9,
                },
            )
            .await
            .expect_err("no column on a different board"),
    );
    assert!(message.contains("\"configure_boards\""), "{message}");
    assert_eq!(details["or_capability"], "manage_structure");

    // --- 5. A member key ---------------------------------------------------------------
    for (tool, arguments) in [
        (
            "create_team",
            json!({"name": "Other", "slug": "other", "code_prefix": "OTH"}),
        ),
        (
            "create_board",
            json!({"name": "Notes", "slug": "notes", "board_level": "adr", "code_prefix": "NOTE"}),
        ),
        (
            "add_team_member",
            json!({"team": "web", "user": "alice@kairos.test"}),
        ),
    ] {
        let text = member_mcp.err(tool, arguments).await;
        assert!(
            text.contains("requires the capability \"manage_structure\" on the organization")
                && text.contains("Ask an organization admin"),
            "{tool}: {text}"
        );
    }
    let text = member_mcp
        .err(
            "grant_capability",
            json!({"user": "alice@kairos.test", "capability": "manage_tasks", "board": "web-delivery"}),
        )
        .await;
    assert!(
        text.contains("\"administer_members\"") && text.contains("Ask an organization admin"),
        "{text}"
    );
    let text = member_mcp
        .err(
            "grant_capability",
            json!({"user": "member-agent", "capability": "manage_structure", "board": "web-delivery"}),
        )
        .await;
    assert!(text.contains("Only an organization admin"), "{text}");
    // A board capability needs a board.
    let text = member_mcp
        .err(
            "grant_capability",
            json!({"user": "alice@kairos.test", "capability": "manage_tasks"}),
        )
        .await;
    assert!(text.starts_with("VALIDATION:"), "{text}");

    // --- 6. The routes that stay with the organization admin ----------------------------
    let grant_id = board_grant.id.clone();
    for key in [&org_key, &board_key] {
        for (method, uri, body) in [
            (
                Method::POST,
                "/api/service-accounts".to_string(),
                Some(json!({"name": "sneaky"})),
            ),
            (
                Method::POST,
                format!("/api/service-accounts/{org_agent_id}/keys"),
                Some(json!({"name": "second"})),
            ),
            (Method::GET, "/api/service-accounts".to_string(), None),
            (
                Method::POST,
                "/api/scim-tokens".to_string(),
                Some(json!({"name": "idp"})),
            ),
            (
                Method::POST,
                "/api/members".to_string(),
                Some(json!({"email": "alice@kairos.test", "role": "admin"})),
            ),
            (
                Method::POST,
                "/api/structure-grants".to_string(),
                Some(json!({"user": "member-agent"})),
            ),
            (
                Method::DELETE,
                format!("/api/structure-grants/{grant_id}"),
                None,
            ),
            (
                Method::POST,
                "/api/admin/tenants".to_string(),
                Some(json!({"slug": "evil", "name": "Evil"})),
            ),
        ] {
            let (status, body) = request(&router, method.clone(), &uri, Some(key), &[], body).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {body}");
            let message = body["error"]["message"].as_str().unwrap_or_default();
            assert!(
                message.contains("organization admin") || message.contains("deployment admin"),
                "{method} {uri}: {body}"
            );
        }
    }

    // --- 7. Revoke, and delete ------------------------------------------------------------
    let revoked = admin
        .delete_structure_grant(&grant_id)
        .await
        .expect("the admin revokes the board grant");
    assert_eq!(revoked.scope, "board");
    assert!(
        board_agent
            .whoami()
            .await
            .expect("whoami")
            .structure
            .is_empty()
    );
    board_mcp
        .err(
            "grant_capability",
            json!({"user": alice_id.to_string(), "capability": "manage_tasks", "board": "web-delivery"}),
        )
        .await;
    assert_eq!(
        activity_count(&mut conn, svc_id, "capability_revoke", "board"),
        1
    );
    assert_eq!(structure_grant_count(&mut conn, org_agent_uuid), 1);
    admin
        .delete_service_account(&org_agent_id)
        .await
        .expect("delete the service account");
    assert_eq!(structure_grant_count(&mut conn, org_agent_uuid), 0);
}
