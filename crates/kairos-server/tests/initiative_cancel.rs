//! The cancel of an initiative (KAIROS-T-0678), over MCP and REST.
//!
//! 1. MCP `cancel_item` takes an initiative: it moves to the done column of
//!    its board (Completed), from a column with no transition there, with
//!    the mark. The owner stays. The answer says that it has no reopen.
//! 2. The mark shows on the board: MCP `board_items` tags `[cancelled]`,
//!    and the REST board card has `cancellation`. MCP `get_item` has the
//!    cancel line.
//! 3. The activity has the row `cancel` with the reason.
//! 4. A second cancel is 422 `INITIATIVE_DONE`.
//! 5. A move of the initiative out of the done column (an edge that an
//!    admin adds) removes the mark.
//! 6. A strategy is refused with `VALIDATION`, and the text names the
//!    route; the description says how to retire each type.
//! 7. The migration removed the foreign key of `task_cancellations`.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_initiative_cancel_t0678_test`.

mod common;

use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::types_meta::ActivityQuery;
use kairos_client::types_org::BoardItemsQuery;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_initiative_cancel_t0678_test";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_initiative_is_cancelled_with_the_mark() {
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
    admin
        .create_team(&kairos_client::types_org::CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("team");
    let initiatives = admin.get_board("initiatives").await.expect("board");
    let column = |name: &str| {
        initiatives
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("column {name}"))
            .id
            .clone()
    };

    let mut mcp = Mcp::connect(&server.base_url, &svc_token).await;
    let initiative = mcp
        .create(json!({"item_type": "initiative", "board": "initiatives", "title": "Old plan"}))
        .await;
    mcp.ok("claim_item", json!({"short_code": initiative}))
        .await;
    mcp.walk(&initiative, &["Design"]).await;

    // 1. The cancel: Design has no transition to Completed.
    let text = mcp
        .ok(
            "cancel_item",
            json!({"short_code": initiative, "reason": "We write a new plan."}),
        )
        .await;
    assert!(
        text.contains(&format!("Cancelled {initiative}: Design -> Completed."))
            && text.contains("Reason: We write a new plan.")
            && text.contains("An initiative has no reopen"),
        "{text}"
    );
    let got = admin.get_initiative(&initiative).await.expect("initiative");
    assert_eq!(got.column_id, column("Completed"));
    assert!(got.owner.is_some(), "the owner stays");

    // 2. The mark on the board, in get_item, and on the edge of the child.
    let text = mcp.ok("board_items", json!({"board": "initiatives"})).await;
    let line = text
        .lines()
        .find(|line| line.contains(&initiative))
        .unwrap_or_else(|| panic!("no card of {initiative}: {text}"));
    assert!(line.contains("[cancelled]"), "{line}");
    let page = admin
        .board_items("initiatives", &BoardItemsQuery::default())
        .await
        .expect("board items");
    let card = page
        .columns
        .iter()
        .flat_map(|group| group.initiatives.iter())
        .find(|card| card.short_code == initiative)
        .expect("the card");
    assert_eq!(
        card.cancellation.as_ref().map(|m| m.reason.as_str()),
        Some("We write a new plan.")
    );
    let text = mcp.ok("get_item", json!({"short_code": initiative})).await;
    assert!(
        text.contains("- cancelled: ") && text.contains("Reason: We write a new plan."),
        "{text}"
    );
    // 3. The activity.
    let rows = admin
        .activity(&ActivityQuery {
            entity_id: Some(got.id.clone()),
            action: Some("cancel".into()),
            ..ActivityQuery::default()
        })
        .await
        .expect("activity")
        .items;
    assert!(
        rows.iter()
            .any(|row| row.details == "column:Design->Completed reason:We write a new plan."),
        "{rows:?}"
    );

    // 4. A second cancel.
    mcp.refused(
        "cancel_item",
        json!({"short_code": initiative, "reason": "Again."}),
        "INITIATIVE_DONE",
    )
    .await;

    // 5. An edge out of Completed: the move removes the mark.
    mcp.ok(
        "set_transition",
        json!({"board": "initiatives", "from": "Completed", "to": "Discovery", "allowed": true}),
    )
    .await;
    mcp.walk(&initiative, &["Discovery"]).await;
    let text = mcp.ok("get_item", json!({"short_code": initiative})).await;
    assert!(!text.contains("- cancelled: "), "{text}");

    // 6. A strategy is not cancelled; the text names the route.
    let strategy = mcp
        .create(json!({"item_type": "strategy", "board": "strategy", "title": "Plan"}))
        .await;
    let text = mcp
        .refused(
            "cancel_item",
            json!({"short_code": strategy, "reason": "No."}),
            "VALIDATION",
        )
        .await;
    assert!(
        text.contains("You can cancel only a task or an initiative.")
            && text.contains("transition_item")
            && text.contains("append_note"),
        "{text}"
    );
    mcp.next_id += 1;
    let id = mcp.next_id;
    let (_, body) = mcp
        .post(json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}))
        .await;
    let tools = rpc_message(&body)["result"]["tools"].clone();
    let description = tools
        .as_array()
        .expect("tools")
        .iter()
        .find(|tool| tool["name"] == "cancel_item")
        .and_then(|tool| tool["description"].as_str())
        .expect("cancel_item description")
        .to_string();
    for part in [
        "Cancel a TASK or an INITIATIVE",
        "it has no reopen",
        "a strategy or an ADR",
        "a document, `set_lifecycle` archived",
    ] {
        assert!(description.contains(part), "{part}: {description}");
    }

    // 7. The foreign key is gone.
    #[derive(diesel::QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let keys = diesel::sql_query(
        "SELECT count(*) AS n FROM pg_constraint \
          WHERE conrelid = 'org_acme.task_cancellations'::regclass AND contype = 'f'",
    )
    .get_result::<Count>(&mut conn)
    .expect("count")
    .n;
    assert_eq!(keys, 0);
}
