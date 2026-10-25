//! Each kind of reference has one set of forms in each tool (KAIROS-T-0689,
//! pass 2 of KAIROS-D-0016). For a team, a board, a repository, a column, a
//! person and an item, the test sends each form of `crate::references` to
//! each tool argument of that kind in the list below, and expects the same
//! answer for each form. A form that is not in the list is refused.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_references_t0689_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types_org::CreateTeamRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, repositories, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_references_t0689_test";

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
                    "clientInfo": {"name": "kairos-t0689-references", "version": "0.0.0"},
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

    /// The call gives the same answer for each form of the reference: the
    /// value at `{}` in `template` is each form in turn.
    async fn same_for_each(&mut self, tool: &str, template: Value, forms: &[String]) -> String {
        let mut first: Option<String> = None;
        for form in forms {
            let arguments = fill(&template, form);
            let text = self.ok(tool, arguments.clone()).await;
            match &first {
                None => first = Some(text),
                Some(expected) => assert_eq!(
                    &text, expected,
                    "{tool} {arguments}: a different answer for the form {form:?}"
                ),
            }
        }
        first.expect("one form at least")
    }

    /// A write that refuses to repeat itself: each form gives the same
    /// answer, the same refusal too ("has the capability already").
    async fn same_answer_for_each(&mut self, tool: &str, template: Value, forms: &[String]) {
        let mut first: Option<(String, bool)> = None;
        for form in forms {
            let arguments = fill(&template, form);
            let answer = self.call(tool, arguments.clone()).await;
            match &first {
                None => first = Some(answer),
                Some(expected) => assert_eq!(
                    &answer, expected,
                    "{tool} {arguments}: a different answer for the form {form:?}"
                ),
            }
        }
    }

    /// A form that is not in the list is refused.
    async fn refused(&mut self, tool: &str, template: Value, form: &str) {
        let arguments = fill(&template, form);
        let (text, error) = self.call(tool, arguments.clone()).await;
        assert!(
            error && (text.starts_with("VALIDATION") || text.starts_with("NOT_FOUND")),
            "{tool} {arguments}: expected a refusal, got {text}"
        );
    }
}

/// `template` with each string `{}` changed to `form`.
fn fill(template: &Value, form: &str) -> Value {
    match template {
        Value::String(s) if s == "{}" => Value::String(form.to_string()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), fill(v, form)))
                .collect(),
        ),
        Value::Array(list) => Value::Array(list.iter().map(|v| fill(v, form)).collect()),
        other => other.clone(),
    }
}

fn padded(text: &str) -> String {
    format!("  {text} ")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_kind_of_reference_has_one_set_of_forms() {
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
    let state = app::state_with(base_config(&scratch_url), pool, auth);
    let server = spawn_server(app::router(state)).await;
    let admin = server.client(&svc_token, "acme");
    let alice = server.client(&alice_token, "acme");
    for client in [&admin, &alice] {
        let _ = client.whoami().await;
    }
    for (email, role) in [
        ("svc@kairos.test", OrgRole::Admin),
        ("alice@kairos.test", OrgRole::Member),
    ] {
        let user_id: Uuid = users::table
            .filter(users::email.eq(email))
            .select(users::id)
            .first(&mut conn)
            .expect("user row");
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
    let board = admin.get_board("web-delivery").await.expect("board");
    let backlog = board
        .columns
        .iter()
        .find(|c| c.name == "Backlog")
        .expect("the entry column");

    let mut admin_mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let mut alice_mcp = Mcp::connect(&server.base_url, &alice_token).await;
    admin_mcp
        .ok(
            "add_repository",
            json!({"forge": "github", "repo_full_name": "acme/site",
                   "repo_url": "https://github.com/acme/site", "team": "web"}),
        )
        .await;
    let text = admin_mcp
        .ok(
            "create_item",
            json!({"item_type": "task", "title": "Fix the header", "board": "web-delivery",
                   "repository": "acme-site"}),
        )
        .await;
    let code = text
        .split_whitespace()
        .find(|w| w.starts_with("WEB-T-"))
        .expect("the code of the task")
        .trim_end_matches([':', ',', '.'])
        .to_string();
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let repository_id: Uuid = repositories::table
        .filter(repositories::slug.eq("acme-site"))
        .select(repositories::id)
        .first(&mut conn)
        .expect("repository row");
    let (svc_id, svc_user_name): (Uuid, String) = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select((users::id, users::user_name))
        .first(&mut conn)
        .expect("svc row");

    // --- team: slug, UUID -----------------------------------------------
    let teams = vec!["web".to_string(), team.id.clone(), padded("web")];
    for (tool, template) in [
        ("list_members", json!({"team": "{}"})),
        ("list_repositories", json!({"team": "{}"})),
        ("team_pages", json!({"team": "{}"})),
        ("search", json!({"filter": {"team": "{}"}})),
    ] {
        let text = admin_mcp
            .same_for_each(tool, template.clone(), &teams)
            .await;
        assert!(!text.is_empty(), "{tool}");
        // A team by its name or by its code prefix is not a form
        // (KAIROS-T-0685).
        admin_mcp.refused(tool, template.clone(), "Web").await;
        admin_mcp.refused(tool, template, "WEB").await;
    }

    // --- board: slug, UUID ----------------------------------------------
    let boards = vec![
        "web-delivery".to_string(),
        board.board.id.clone(),
        padded("web-delivery"),
    ];
    for (tool, template) in [
        ("board_items", json!({"board": "{}"})),
        ("list_members", json!({"board": "{}"})),
        ("list_dispatch_rules", json!({"board": "{}"})),
        ("board_flow", json!({"board": "{}"})),
        ("search", json!({"filter": {"board": "{}"}})),
    ] {
        let text = admin_mcp
            .same_for_each(tool, template.clone(), &boards)
            .await;
        if tool == "board_items" || tool == "search" {
            assert!(text.contains(&code), "{tool}: {text}");
        }
        admin_mcp.refused(tool, template, "Web delivery").await;
    }

    // --- repository: slug, UUID -----------------------------------------
    let repositories = vec![
        "acme-site".to_string(),
        repository_id.to_string(),
        padded("acme-site"),
    ];
    for (tool, template) in [
        ("get_repository", json!({"repository": "{}"})),
        (
            "board_items",
            json!({"board": "web-delivery", "repository": "{}"}),
        ),
        ("search", json!({"filter": {"repository": "{}"}})),
    ] {
        let text = admin_mcp
            .same_for_each(tool, template.clone(), &repositories)
            .await;
        if tool != "get_repository" {
            assert!(text.contains(&code), "{tool}: {text}");
        }
        admin_mcp.refused(tool, template, "acme/site").await;
    }

    // --- column: name in any case, UUID ---------------------------------
    let columns = vec![
        "Backlog".to_string(),
        "backlog".to_string(),
        "BACKLOG".to_string(),
        backlog.id.clone(),
        padded("Backlog"),
    ];
    for (tool, template) in [
        (
            "board_items",
            json!({"board": "web-delivery", "column": "{}"}),
        ),
        (
            "search",
            json!({"filter": {"board": "web-delivery", "column": "{}"}}),
        ),
    ] {
        let text = admin_mcp
            .same_for_each(tool, template.clone(), &columns)
            .await;
        assert!(text.contains(&code), "{tool}: {text}");
        admin_mcp.refused(tool, template, "Back log").await;
    }

    // --- person: me, UUID, email, user name -----------------------------
    let people = vec![
        "me".to_string(),
        "ME".to_string(),
        svc_id.to_string(),
        "svc@kairos.test".to_string(),
        svc_user_name.clone(),
        padded("svc@kairos.test"),
    ];
    for (tool, template) in [
        ("search", json!({"filter": {"created_by": "{}"}})),
        (
            "set_board_capabilities",
            json!({"board": "web-delivery", "user": "{}", "capabilities": ["manage_tasks"]}),
        ),
        (
            "grant_capability",
            json!({"board": "web-delivery", "user": "{}", "capability": "manage_tasks"}),
        ),
        ("add_team_member", json!({"team": "web", "user": "{}"})),
    ] {
        // A write is the same write for each form: the first call writes,
        // the next ones find it done. So the answers from the second form
        // on are the same.
        let _ = admin_mcp.call(tool, fill(&template, "me")).await;
        admin_mcp
            .same_answer_for_each(tool, template.clone(), &people)
            .await;
        admin_mcp.refused(tool, template, "Svc Account").await;
    }
    // A person who is not the caller: alice, by each form, from the admin.
    let alice_forms = vec![
        "alice@kairos.test".to_string(),
        users::table
            .filter(users::email.eq("alice@kairos.test"))
            .select(users::id)
            .first::<Uuid>(&mut conn)
            .expect("alice row")
            .to_string(),
        users::table
            .filter(users::email.eq("alice@kairos.test"))
            .select(users::user_name)
            .first::<String>(&mut conn)
            .expect("alice row"),
    ];
    let _ = admin_mcp
        .ok(
            "add_team_member",
            json!({"team": "web", "user": "alice@kairos.test"}),
        )
        .await;
    admin_mcp
        .same_answer_for_each(
            "add_team_member",
            json!({"team": "web", "user": "{}"}),
            &alice_forms,
        )
        .await;
    let mine = alice_mcp
        .same_for_each(
            "search",
            json!({"filter": {"created_by": "{}"}}),
            &[
                "me".to_string(),
                alice_forms[0].clone(),
                alice_forms[2].clone(),
            ],
        )
        .await;
    assert!(!mine.contains(&code), "alice created no item: {mine}");

    // --- item: short code in any case -----------------------------------
    let items = vec![code.clone(), code.to_lowercase(), padded(&code)];
    for (tool, template) in [
        ("get_item", json!({"short_code": "{}"})),
        ("get_history", json!({"short_code": "{}"})),
    ] {
        let text = admin_mcp
            .same_for_each(tool, template.clone(), &items)
            .await;
        assert!(text.contains(&code), "{tool}: {text}");
        admin_mcp.refused(tool, template, "WEB-T-9999").await;
    }
}
