//! Team and board administration over MCP (KAIROS-T-0443): update_team,
//! remove_team_member, set_board_capabilities, add_column, update_column,
//! remove_column, set_transition, delete_repository,
//! save_metadata_definition, delete_metadata_definition, save_template and
//! delete_template. Each tool calls the function of its REST route, so the
//! gate is the same: a member is refused, an organization admin is not. The
//! REST routes of team and board members take an email too.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_admin_tools_t0443_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_admin_tools_t0443_test";

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
                    "clientInfo": {"name": "kairos-t0443-admin", "version": "0.0.0"},
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

impl Mcp {
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn team_and_board_administration_over_mcp() {
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
    let bob = server.client(&bob_token, "acme");
    for client in [&admin, &alice, &bob] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
        ("bob@kairos.test", OrgRole::Member),
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
    let team = admin
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    let board = "web-delivery";

    // REST: a team member by email (KAIROS-T-0443; it took a UUID only).
    admin
        .add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: "bob@kairos.test".into(),
            },
        )
        .await
        .expect("bob by email");

    let mut admin_mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let mut alice_mcp = Mcp::connect(&server.base_url, &alice_token).await;

    // --- update_team, remove_team_member --------------------------------
    alice_mcp
        .refused(
            "update_team",
            json!({"team": "web", "name": "Webby"}),
            "FORBIDDEN",
        )
        .await;
    let text = admin_mcp
        .ok(
            "update_team",
            json!({"team": "web", "name": "Web front", "team_type": "platform"}),
        )
        .await;
    assert!(
        text.contains("\"Web front\"") && text.contains("platform"),
        "{text}"
    );
    admin_mcp
        .refused("update_team", json!({"team": "web"}), "VALIDATION")
        .await;
    admin_mcp
        .refused(
            "update_team",
            json!({"team": "web", "colour": "red"}),
            "colour",
        )
        .await;
    admin_mcp
        .ok(
            "remove_team_member",
            json!({"team": "web", "user": "bob@kairos.test"}),
        )
        .await;
    admin_mcp
        .refused(
            "remove_team_member",
            json!({"team": "web", "user": "bob@kairos.test"}),
            "NOT_FOUND",
        )
        .await;

    // --- set_board_capabilities ------------------------------------------
    alice_mcp
        .refused(
            "set_board_capabilities",
            json!({"board": board, "user": "bob@kairos.test", "capabilities": ["manage_tasks"]}),
            "FORBIDDEN",
        )
        .await;
    let text = admin_mcp
        .ok(
            "set_board_capabilities",
            json!({"board": board, "user": "alice@kairos.test",
                   "capabilities": ["manage_tasks", "configure_boards"]}),
        )
        .await;
    assert!(
        text.contains("configure_boards") && text.contains("manage_tasks"),
        "{text}"
    );
    let text = admin_mcp
        .ok(
            "set_board_capabilities",
            json!({"board": board, "user": "alice@kairos.test", "capabilities": ["configure_boards"]}),
        )
        .await;
    assert!(!text.contains("manage_tasks"), "replaced: {text}");

    // --- columns and transitions: alice has configure_boards now ----------
    let text = alice_mcp
        .ok("add_column", json!({"board": board, "name": "Review"}))
        .await;
    assert!(text.contains("Review"), "{text}");
    alice_mcp
        .refused(
            "add_column",
            json!({"board": board, "name": "Review"}),
            "DUPLICATE",
        )
        .await;
    alice_mcp
        .ok(
            "update_column",
            json!({"board": board, "column": "review", "name": "In review", "wip_limit": 2}),
        )
        .await;
    alice_mcp
        .refused(
            "update_column",
            json!({"board": board, "column": "In review", "wip_limit": 2, "no_wip_limit": true}),
            "VALIDATION",
        )
        .await;
    let text = alice_mcp
        .ok(
            "set_transition",
            // `from` and `to` are aliases of `from_column` and `to_column`.
            json!({"board": board, "from": "Active", "to": "In review", "allowed": true}),
        )
        .await;
    assert!(text.contains("allows the move"), "{text}");
    let detail = admin.get_board(board).await.expect("board");
    let review = detail
        .columns
        .iter()
        .find(|c| c.name == "In review")
        .expect("the column");
    assert_eq!(review.wip_limit, Some(2));
    assert!(
        detail
            .transitions
            .iter()
            .any(|t| t.to_column_id == review.id)
    );
    alice_mcp
        .ok(
            "set_transition",
            // KAIROS-T-0688: the names of the columns, as on REST.
            json!({"board": board, "from_column": "Active", "to_column": "In review", "allowed": false}),
        )
        .await;
    alice_mcp
        .ok(
            "remove_column",
            json!({"board": board, "column": "In review"}),
        )
        .await;
    let detail = admin.get_board(board).await.expect("board");
    assert!(detail.columns.iter().all(|c| c.name != "In review"));

    // An empty set revokes each capability: alice cannot configure now.
    admin_mcp
        .ok(
            "set_board_capabilities",
            json!({"board": board, "user": "alice@kairos.test", "capabilities": []}),
        )
        .await;
    alice_mcp
        .refused(
            "add_column",
            json!({"board": board, "name": "Later"}),
            "FORBIDDEN",
        )
        .await;

    // --- metadata definitions and templates: admins only ------------------
    alice_mcp
        .refused(
            "save_metadata_definition",
            json!({"name": "Size", "slug": "size", "field_type": "enum", "enum_options": ["S", "M"]}),
            "FORBIDDEN",
        )
        .await;
    admin_mcp
        .refused("save_metadata_definition", json!({"name": "Size"}), "slug")
        .await;
    let text = admin_mcp
        .ok(
            "save_metadata_definition",
            json!({"name": "Size", "slug": "size", "field_type": "enum", "enum_options": ["S", "M"]}),
        )
        .await;
    assert!(text.contains("size") && text.contains("enum"), "{text}");
    admin_mcp
        .ok(
            "save_metadata_definition",
            json!({"definition": "size", "enum_options": ["S", "M", "L"]}),
        )
        .await;
    admin_mcp
        .refused(
            "save_metadata_definition",
            json!({"definition": "size", "field_type": "text"}),
            "field_type",
        )
        .await;
    let text = admin_mcp
        .ok(
            "save_template",
            json!({"name": "Spike", "slug": "spike", "content": "## Question",
                   "metadata": [{"definition_slug": "size", "default_value": "S"}]}),
        )
        .await;
    assert!(text.contains("spike"), "{text}");
    // The template has the field: the definition is in use.
    admin_mcp
        .refused(
            "delete_metadata_definition",
            json!({"definition": "size"}),
            "DEFINITION_IN_USE",
        )
        .await;
    admin_mcp
        .ok(
            "save_template",
            json!({"template": "spike", "metadata": []}),
        )
        .await;
    alice_mcp
        .refused(
            "delete_template",
            json!({"reference": "spike"}),
            "FORBIDDEN",
        )
        .await;
    admin_mcp
        .ok("delete_template", json!({"template": "spike"}))
        .await;
    admin_mcp
        .ok("delete_metadata_definition", json!({"definition": "size"}))
        .await;
    admin_mcp
        .refused(
            "delete_template",
            json!({"reference": "spike"}),
            "NOT_FOUND",
        )
        .await;

    // --- repositories: slug, owner team, delete ---------------------------
    admin_mcp
        .ok(
            "add_repository",
            json!({"forge": "github", "repo_full_name": "acme/site",
                   "repo_url": "https://github.com/acme/site", "team": "web"}),
        )
        .await;
    let text = admin_mcp
        .ok(
            "update_repository",
            json!({"repository": "acme-site", "slug": "site"}),
        )
        .await;
    assert!(text.contains("slug"), "{text}");
    alice_mcp
        .refused(
            "delete_repository",
            json!({"repository": "site"}),
            "FORBIDDEN",
        )
        .await;
    let text = admin_mcp
        .ok("delete_repository", json!({"repository": "site"}))
        .await;
    assert!(text.contains("Removed the repository site"), "{text}");
}
