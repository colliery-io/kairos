//! The count of the MCP refusals (KAIROS-T-0694, KAIROS-A-0068): each
//! tool call that ends in a tool error writes one row of `tool_refusals`
//! (tool, code, field, caller kind; no argument value), and the admin tool
//! `refusal_counts` gives the counts by tool and code for a range of days.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_tool_refusals_t0694_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_tool_refusals_t0694_test";

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
                    "clientInfo": {"name": "kairos-t0694-refusals", "version": "0.0.0"},
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

#[derive(QueryableByName, Debug, PartialEq)]
struct Row {
    #[diesel(sql_type = diesel::sql_types::Text)]
    tool: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    code: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    field: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Text)]
    caller_kind: String,
}

fn rows(conn: &mut PgConnection) -> Vec<Row> {
    diesel::sql_query(
        "SELECT tool, code, field, caller_kind FROM org_acme.tool_refusals ORDER BY created_at",
    )
    .load(conn)
    .expect("the rows of tool_refusals")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refusals_are_counted_by_tool_and_code() {
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

    let mut admin_mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let mut alice_mcp = Mcp::connect(&server.base_url, &alice_token).await;

    // No refusal yet: the report says so.
    let text = admin_mcp.ok("refusal_counts", json!({})).await;
    assert!(text.starts_with("No refusals from "), "{text}");

    // 1. Refusals of three kinds: an argument that the tool does not know
    // (the extractor), an item that is not there (the tool), and a value
    // outside the vocabulary.
    alice_mcp
        .refused(
            "create_item",
            json!({"item_type": "task", "title": "Secret title 42", "colour": "red"}),
            "VALIDATION",
        )
        .await;
    alice_mcp
        .refused(
            "create_item",
            json!({"item_type": "task", "title": "Secret title 42", "colour": "blue"}),
            "VALIDATION",
        )
        .await;
    alice_mcp
        .refused(
            "get_item",
            json!({"short_code": "ACME-T-9999"}),
            "NOT_FOUND",
        )
        .await;
    alice_mcp
        .refused(
            "create_item",
            json!({"item_type": "epic", "title": "Secret title 42"}),
            "VALIDATION",
        )
        .await;
    // A call that works writes no row.
    alice_mcp.ok("whoami", json!({})).await;

    let found = rows(&mut conn);
    assert_eq!(found.len(), 4, "{found:?}");
    assert_eq!(
        found[0],
        Row {
            tool: "create_item".into(),
            code: "VALIDATION".into(),
            field: Some("colour".into()),
            caller_kind: "person".into(),
        }
    );
    assert_eq!(found[2].tool, "get_item");
    assert_eq!(found[2].code, "NOT_FOUND");
    assert!(found.iter().all(|r| r.caller_kind == "person"), "{found:?}");

    // 2. The row has no argument value: the columns are the five of the
    // table, and no text of the call is in them.
    let columns: Vec<String> = diesel::sql_query(
        "SELECT column_name AS name FROM information_schema.columns \
         WHERE table_schema = 'org_acme' AND table_name = 'tool_refusals' \
         ORDER BY ordinal_position",
    )
    .load::<Name>(&mut conn)
    .expect("the columns")
    .into_iter()
    .map(|n| n.name)
    .collect();
    assert_eq!(
        columns,
        ["id", "tool", "code", "field", "caller_kind", "created_at"]
    );
    assert!(
        found.iter().all(|r| !format!("{r:?}").contains("Secret")
            && !format!("{r:?}").contains("ACME-T-9999")
            && !format!("{r:?}").contains("epic")),
        "{found:?}"
    );

    // 3. The report: the counts by tool and code, the highest first, the
    // field named most, and the kinds of caller. Only an org admin.
    alice_mcp
        .refused("refusal_counts", json!({}), "FORBIDDEN")
        .await;
    let text = admin_mcp
        .ok("refusal_counts", json!({"days": 7, "top": 10}))
        .await;
    assert!(text.contains(": 5 in all,"), "{text}");
    let first = text
        .lines()
        .find(|l| l.starts_with("| create_item"))
        .expect("a row of create_item");
    assert_eq!(
        first, "| create_item | VALIDATION | 3 | colour | 3 | 0 | 0 |",
        "{text}"
    );
    assert!(
        text.contains("| get_item | NOT_FOUND | 1 | short_code | 1 | 0 | 0 |")
            || text.contains("| get_item | NOT_FOUND | 1 | - | 1 | 0 | 0 |"),
        "{text}"
    );
    // The refusal of alice to the report is counted too.
    assert!(
        text.contains("| refusal_counts | FORBIDDEN | 1 | - | 1 | 0 | 0 |"),
        "{text}"
    );
    let rank = |needle: &str| text.find(needle).expect(needle);
    assert!(rank("| create_item") < rank("| get_item"), "{text}");
    let text = admin_mcp.ok("refusal_counts", json!({"top": 1})).await;
    assert_eq!(
        text.matches("\n| ").count(),
        2,
        "the head and one row: {text}"
    );
    assert!(text.contains("The top 1 of 3 pairs"), "{text}");

    // 4. A range with no refusal, and the bounds of the arguments.
    let text = admin_mcp
        .ok("refusal_counts", json!({"until": "2020-01-01"}))
        .await;
    assert!(text.starts_with("No refusals from 2019-12-26"), "{text}");
    let text = admin_mcp
        .refused("refusal_counts", json!({"days": 0}), "VALIDATION")
        .await;
    assert!(text.contains("\"field\":\"days\""), "{text}");
    admin_mcp
        .refused("refusal_counts", json!({"days": 91}), "VALIDATION")
        .await;
    admin_mcp
        .refused("refusal_counts", json!({"top": 0}), "VALIDATION")
        .await;
    let text = admin_mcp
        .refused(
            "refusal_counts",
            json!({"until": "yesterday"}),
            "VALIDATION",
        )
        .await;
    assert!(text.contains("\"field\":\"until\""), "{text}");
}

#[derive(QueryableByName)]
struct Name {
    #[diesel(sql_type = diesel::sql_types::Text)]
    name: String,
}
