//! Integration test for KAIROS-T-0681 (KAIROS-A-0065): a `request_key`
//! makes a retried MCP write safe.
//!
//! Proves:
//! 1. The 15 tools of decision 7 (and KAIROS-T-0737) list `request_key` and say when to send
//!    it; the patches (`set_state`) and `update_item` do not.
//! 2. A second `create_item` with the same key and arguments writes
//!    nothing and gives the first answer, with a `Replayed:` line.
//! 3. A replay after the first call committed and its answer was lost: the
//!    first answer is not read; the retry gives the stored answer, and
//!    there is one item.
//! 4. Two calls at the same time with one key make one item.
//! 5. The same key with other arguments is REQUEST_KEY_REUSED and writes
//!    nothing.
//! 6. A key that does not obey the rule is VALIDATION.
//! 7. `append_note` and `transition_item` replay: one note; no
//!    INVALID_TRANSITION.
//! 8. The caller is the person and the agent key: the agent key of the same
//!    person with the same key writes again.
//! 9. A refused write leaves no row, so its retry runs.
//! 10. A row past the window of 24 hours replays nothing, and the delete of
//!     the retention pass removes it.
//!
//! Runs against the compose stack (`angreal services up`). Owns the scratch
//! database `kairos_request_keys_t0681_test`.

mod common;

use std::sync::Arc;

use chrono::Utc;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Nullable, Text as SqlText};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, spawn_server, user_token,
    with_database,
};
use kairos_client::types_service_accounts::CreateApiKeyRequest;
use kairos_db::models::{BoardLevel, NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_request_keys_t0681_test";

/// The tools of decision 7 of KAIROS-A-0065.
const KEYED_TOOLS: [&str; 15] = [
    "create_item",
    "append_note",
    "link_items",
    "propose_edge",
    "promote_note",
    "transition_item",
    "move_item",
    "edit_item",
    "add_repository",
    "create_team",
    "create_board",
    "add_column",
    "create_team_page",
    "create_dispatch_rule",
    // KAIROS-T-0737.
    "send_notice",
];

const SENTENCE: &str = "Send a `request_key` when you may retry this call.";

/// A minimal MCP client over HTTP, as in tests/create_refusal.rs.
#[derive(Clone)]
struct Mcp {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Mcp {
    fn new(base_url: &str, token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base_url.to_string(),
            token: token.to_string(),
        }
    }

    /// Post one JSON-RPC message to `path`; the response, not read.
    async fn send(&self, path: &str, message: Value) -> reqwest::Response {
        self.http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header("X-Tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&message)
            .send()
            .await
            .expect("post to /mcp")
    }

    async fn message(response: reqwest::Response) -> Value {
        assert_eq!(response.status(), 200, "HTTP status of /mcp");
        let body = response.text().await.expect("body");
        serde_json::from_str::<Value>(&body)
            .ok()
            .filter(|v| v.get("jsonrpc").is_some())
            .or_else(|| {
                body.lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
                    .find(|v| v.get("jsonrpc").is_some())
            })
            .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"))
    }

    fn call_message(tool: &str, arguments: Value) -> Value {
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        })
    }

    /// Call a tool: `(is_error, text)`. The text is the first block, with
    /// no notifications block.
    async fn call(&self, tool: &str, arguments: Value) -> (bool, String) {
        let message =
            Self::message(self.send("/mcp", Self::call_message(tool, arguments)).await).await;
        let result = &message["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text in {message}"))
            .to_string();
        (result["isError"].as_bool().unwrap_or(false), text)
    }

    async fn ok(&self, tool: &str, arguments: Value) -> String {
        let (is_error, text) = self.call(tool, arguments).await;
        assert!(!is_error, "{tool} failed: {text}");
        text
    }

    async fn tools(&self) -> Vec<Value> {
        let message = Self::message(
            self.send(
                "/mcp?profile=all",
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            )
            .await,
        )
        .await;
        message["result"]["tools"]
            .as_array()
            .expect("tools")
            .clone()
    }
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count_by(conn: &mut PgConnection, sql: &str, value: &str) -> i64 {
    sql_query(sql)
        .bind::<SqlText, _>(value)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

fn tasks_titled(conn: &mut PgConnection, title: &str) -> i64 {
    count_by(
        conn,
        "SELECT COUNT(*) AS n FROM org_acme.tasks WHERE title = $1",
        title,
    )
}

fn key_rows(conn: &mut PgConnection, key: &str) -> i64 {
    count_by(
        conn,
        "SELECT COUNT(*) AS n FROM org_acme.request_keys WHERE request_key = $1",
        key,
    )
}

#[derive(QueryableByName)]
struct AnswerRow {
    #[diesel(sql_type = Nullable<SqlText>)]
    answer: Option<String>,
}

fn stored_answer(conn: &mut PgConnection, key: &str) -> String {
    sql_query("SELECT answer FROM org_acme.request_keys WHERE request_key = $1")
        .bind::<SqlText, _>(key)
        .get_result::<AnswerRow>(conn)
        .expect("the row of the key")
        .answer
        .expect("a committed row has its answer")
}

/// The short code in the answer of a create (the first `ACME-T-`).
fn code_in(text: &str) -> String {
    let start = text
        .find("ACME-T-")
        .unwrap_or_else(|| panic!("no code in {text}"));
    text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

fn create(title: &str, key: &str) -> Value {
    json!({
        "item_type": "task",
        "title": title,
        "board": "platform-delivery",
        "request_key": key,
    })
}

#[tokio::test]
async fn a_request_key_makes_a_retried_write_safe() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let acme: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("org row");

    let http = reqwest::Client::new();
    let svc_token = user_token(&http, "svc").await;
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
    let _ = svc.whoami().await;
    let svc_id: Uuid = users::table
        .filter(users::email.eq("svc@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("svc provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: acme,
            user_id: svc_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("membership");

    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let team = common::seed_team(&mut conn, "Platform", "platform");
    kairos_db::boards::create_board(
        &mut conn,
        BoardLevel::Delivery,
        "Platform Delivery",
        "platform-delivery",
        kairos_db::CodePrefix::Given("ACME"),
        Some(team),
        None,
    )
    .expect("creating the delivery board");
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");

    let mcp = Mcp::new(&server.base_url, &svc_token);

    // 1. The schemas and the descriptions.
    let tools = mcp.tools().await;
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        let keyed = tool["inputSchema"]["properties"]
            .get("request_key")
            .is_some();
        let says = tool["description"].as_str().unwrap().contains(SENTENCE);
        let expected = KEYED_TOOLS.contains(&name);
        assert_eq!(keyed, expected, "{name}: request_key in the schema");
        assert_eq!(says, expected, "{name}: the sentence in the description");
    }
    assert_eq!(
        tools
            .iter()
            .filter(|t| KEYED_TOOLS.contains(&t["name"].as_str().unwrap()))
            .count(),
        15
    );

    // KAIROS-T-0693: the table of kairos_client::mcp_tools has the same
    // keyed tools.
    let mut from_table: Vec<&str> = kairos_client::mcp_tools::TOOLS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| kairos_client::mcp_tools::takes_request_key(name))
        .collect();
    let mut listed = KEYED_TOOLS.to_vec();
    from_table.sort_unstable();
    listed.sort_unstable();
    assert_eq!(from_table, listed);

    // 2. A replay of create_item.
    let first = mcp
        .ok("create_item", create("Keyed one", "key-create-0001"))
        .await;
    let code = code_in(&first);
    let again = mcp
        .ok("create_item", create("Keyed one", "key-create-0001"))
        .await;
    assert!(
        again.starts_with(&first),
        "the replay gives the first answer: {again}"
    );
    assert!(
        again.contains("Replayed: the call with this request_key at ")
            && again.contains("Nothing new was written."),
        "{again}"
    );
    assert_eq!(tasks_titled(&mut conn, "Keyed one"), 1);

    // 3. The first call commits, and its answer is lost: the client does
    //    not read it. The retry gives the stored answer.
    let lost = mcp
        .send(
            "/mcp",
            Mcp::call_message("create_item", create("Answer lost", "key-lost-00001")),
        )
        .await;
    drop(lost);
    assert_eq!(
        tasks_titled(&mut conn, "Answer lost"),
        1,
        "the first call committed"
    );
    let retry = mcp
        .ok("create_item", create("Answer lost", "key-lost-00001"))
        .await;
    let stored = stored_answer(&mut conn, "key-lost-00001");
    assert!(retry.starts_with(&stored), "{retry}");
    assert!(retry.contains("Replayed:"), "{retry}");
    assert_eq!(
        tasks_titled(&mut conn, "Answer lost"),
        1,
        "the retry made no item"
    );

    // 4. Two calls at the same time with one key.
    let (a, b) = tokio::join!(
        mcp.ok("create_item", create("Same time", "key-race-00001")),
        mcp.ok("create_item", create("Same time", "key-race-00001")),
    );
    assert_eq!(tasks_titled(&mut conn, "Same time"), 1);
    assert_eq!(code_in(&a), code_in(&b));
    assert!(
        a.contains("Replayed:") != b.contains("Replayed:"),
        "{a}\n---\n{b}"
    );

    // 5. The same key with other arguments.
    let (is_error, text) = mcp
        .call("create_item", create("Other title", "key-create-0001"))
        .await;
    assert!(is_error, "{text}");
    assert!(text.starts_with("REQUEST_KEY_REUSED: "), "{text}");
    assert!(
        text.contains("Use a new request_key for a new request."),
        "{text}"
    );
    assert!(text.contains("\"first_tool\":\"create_item\""), "{text}");
    assert_eq!(tasks_titled(&mut conn, "Other title"), 0);
    // ... and with another tool.
    let (is_error, text) = mcp
        .call(
            "append_note",
            json!({"short_code": code, "text": "x", "request_key": "key-create-0001"}),
        )
        .await;
    assert!(
        is_error && text.starts_with("REQUEST_KEY_REUSED: "),
        "{text}"
    );

    // 6. A key that does not obey the rule.
    for bad in ["short", "has a space in it", &"k".repeat(129)] {
        let (is_error, text) = mcp.call("create_item", create("Bad key", bad)).await;
        assert!(
            is_error && text.starts_with("VALIDATION: "),
            "{bad}: {text}"
        );
        assert!(text.contains("request_key"), "{text}");
    }
    assert_eq!(tasks_titled(&mut conn, "Bad key"), 0);

    // 7. append_note and transition_item.
    let note = json!({"short_code": code, "text": "One note.", "request_key": "key-note-00001"});
    let n1 = mcp.ok("append_note", note.clone()).await;
    let n2 = mcp.ok("append_note", note).await;
    assert!(n2.starts_with(&n1) && n2.contains("Replayed:"), "{n2}");
    assert_eq!(
        count_by(
            &mut conn,
            "SELECT COUNT(*) AS n FROM org_acme.item_notes n \
             JOIN org_acme.tasks t ON t.id = n.item_id WHERE t.short_code = $1",
            &code,
        ),
        1,
        "one note"
    );
    let first_column = {
        let move_to =
            |key: &str| json!({"short_code": code, "to_column": "Todo", "request_key": key});
        let t1 = mcp.ok("transition_item", move_to("key-move-00001")).await;
        let t2 = mcp.ok("transition_item", move_to("key-move-00001")).await;
        assert!(t2.starts_with(&t1) && t2.contains("Replayed:"), "{t2}");
        // With no key, the second move is refused: the reason for the key.
        let (is_error, text) = mcp
            .call(
                "transition_item",
                json!({"short_code": code, "to_column": "Todo"}),
            )
            .await;
        assert!(is_error, "a move to the same column is refused: {text}");
        t1
    };
    assert!(first_column.contains("Todo"), "{first_column}");

    // 8. The agent key of the same person is another caller.
    let made = svc
        .create_agent_key(&CreateApiKeyRequest {
            name: "t0681".into(),
            expires_at: None,
            no_expiry: false,
            scope: None,
        })
        .await
        .expect("an agent key for svc");
    let agent = Mcp::new(&server.base_url, &made.key);
    let by_agent = agent
        .ok("create_item", create("Keyed one", "key-create-0001"))
        .await;
    assert!(!by_agent.contains("Replayed:"), "{by_agent}");
    assert_ne!(code_in(&by_agent), code);
    assert_eq!(tasks_titled(&mut conn, "Keyed one"), 2);
    let by_agent_again = agent
        .ok("create_item", create("Keyed one", "key-create-0001"))
        .await;
    assert!(by_agent_again.contains("Replayed:"), "{by_agent_again}");
    assert_eq!(tasks_titled(&mut conn, "Keyed one"), 2);

    // 9. A refused write leaves no row, and its retry runs.
    let refused = json!({
        "item_type": "task", "title": "Refused first", "board": "no-such-board",
        "request_key": "key-refused-01",
    });
    let (is_error, text) = mcp.call("create_item", refused).await;
    assert!(is_error, "{text}");
    assert_eq!(key_rows(&mut conn, "key-refused-01"), 0);
    mcp.ok("create_item", create("Refused first", "key-refused-01"))
        .await;
    assert_eq!(tasks_titled(&mut conn, "Refused first"), 1);

    // 10. Past the window.
    sql_query(
        "UPDATE org_acme.request_keys SET created_at = now() - interval '25 hours' \
         WHERE request_key = 'key-lost-00001'",
    )
    .execute(&mut conn)
    .expect("ageing the row");
    let after = mcp
        .ok("create_item", create("Answer lost", "key-lost-00001"))
        .await;
    assert!(!after.contains("Replayed:"), "{after}");
    assert_eq!(tasks_titled(&mut conn, "Answer lost"), 2);
    sql_query(
        "UPDATE org_acme.request_keys SET created_at = now() - interval '25 hours' \
         WHERE request_key = 'key-race-00001'",
    )
    .execute(&mut conn)
    .expect("ageing the row");
    sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .expect("pinning search_path");
    let deleted = kairos_db::request_keys::delete_before(
        &mut conn,
        Utc::now() - kairos_db::request_keys::window(),
        100,
    )
    .expect("the delete of the retention pass");
    assert_eq!(deleted, 1);
    sql_query("SET search_path TO public")
        .execute(&mut conn)
        .expect("resetting search_path");
    assert_eq!(key_rows(&mut conn, "key-race-00001"), 0);

    drop(conn);
    drop(server);
    drop(pool);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
