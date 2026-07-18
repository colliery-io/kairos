//! E2E golden-path runner (KAIROS-T-0035; KAIROS-A-0012 tier 4).
//!
//! Driven by `angreal test e2e` against a LIVE `kairos-server serve`
//! process backed by the compose stack (Postgres + Dex) and the seeded
//! `demo` tenant (`kairos-server seed-demo --force`). Everything here goes
//! over real HTTP with a real Dex token — nothing in-process, nothing
//! mocked:
//!
//! 1.  `/healthz` reachability
//! 2.  Dex password grant for alice (the seeded demo org admin)
//! 3.  REST `whoami` via `kairos_client::KairosClient` (X-Tenant: demo)
//! 4.  board discovery (strategy / initiatives / platform-delivery)
//! 5.  golden path: create strategy → initiative (+ parent edge) →
//!     decompose into two tasks (+ parent edges, one blocks edge) →
//!     transition the first task → search finds all of them
//! 6.  MCP session smoke over streamable HTTP (initialize →
//!     notifications/initialized → whoami → board_items), mirroring the
//!     tests/mcp.rs transport
//!
//! Every step prints `[e2e] step N (name) ... ok`; the FIRST failure
//! prints `[e2e] step N (name) FAILED: <cause>` to stderr and exits 1, so
//! a red run always names its step. Exit 0 means every step passed.
//!
//! The GUI leg (Playwright over the Leptos frontend) is a SEPARATE phase of
//! `angreal test e2e` (KAIROS-T-0045): after this runner returns, the task
//! builds the SPA, reseeds, serves it on :8080, and runs the Playwright
//! smoke suite. This runner covers the API + MCP golden path only.

use std::process::ExitCode;

use kairos_client::KairosClient;
use kairos_client::types::{
    CreateInitiativeRequest, CreateStrategyRequest, CreateTaskRequest, Pagination,
};
use kairos_client::types_meta::CreateRelationshipRequest;
use kairos_client::types_search::SearchRequest;
use serde_json::{Value, json};

/// The tenant `seed-demo` provisions.
const TENANT: &str = "demo";

/// A token that appears ONLY in the items this run creates, so the search
/// step proves full-text indexing end to end (the seed fixture never uses
/// it).
const SEARCH_TOKEN: &str = "fluxcapacitor";

struct Step {
    n: u32,
    name: &'static str,
}

impl Step {
    fn start(n: u32, name: &'static str) -> Step {
        Step { n, name }
    }

    fn ok(&self, detail: impl AsRef<str>) {
        println!(
            "[e2e] step {} ({}) ... ok — {}",
            self.n,
            self.name,
            detail.as_ref()
        );
    }

    fn fail(&self, cause: impl std::fmt::Display) -> ! {
        eprintln!("[e2e] step {} ({}) FAILED: {cause}", self.n, self.name);
        std::process::exit(1);
    }
}

/// `expect`-like unwrap that attributes the failure to its step.
fn or_fail<T, E: std::fmt::Display>(step: &Step, result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => step.fail(e),
    }
}

fn require(step: &Step, condition: bool, cause: &str) {
    if !condition {
        step.fail(cause);
    }
}

/// Extract the JSON-RPC message from a streamable-HTTP response body:
/// plain JSON or SSE `data:` framing (same contract as tests/mcp.rs).
fn rpc_message(step: &Step, body: &str) -> Value {
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
    step.fail(format!("no JSON-RPC message in response body: {body:?}"))
}

/// One JSON-RPC POST to `/mcp` (streamable HTTP, X-Tenant header). Returns
/// `(status, mcp-session-id header, body)`.
async fn mcp_post(
    step: &Step,
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    session: Option<&str>,
    body: Value,
) -> (u16, Option<String>, String) {
    let mut request = http
        .post(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("x-tenant", TENANT)
        .json(&body);
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = or_fail(step, request.send().await);
    let status = response.status().as_u16();
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = or_fail(step, response.text().await);
    (status, session_id, body)
}

/// An established MCP session's wiring (client, endpoint, credentials).
struct McpSession<'a> {
    http: &'a reqwest::Client,
    base_url: &'a str,
    token: &'a str,
    session_id: String,
}

impl McpSession<'_> {
    /// Call one MCP tool; returns the CallToolResult text, failing the
    /// step on HTTP, protocol, or tool errors.
    async fn tool(&self, step: &Step, id: i64, tool: &str, arguments: Value) -> String {
        let (status, _, body) = mcp_post(
            step,
            self.http,
            self.base_url,
            self.token,
            Some(&self.session_id),
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }),
        )
        .await;
        require(
            step,
            status == 200,
            &format!("{tool} HTTP {status}: {body}"),
        );
        let message = rpc_message(step, &body);
        require(
            step,
            message.get("error").is_none(),
            &format!("{tool} protocol error: {message}"),
        );
        let result = &message["result"];
        require(
            step,
            !result["isError"].as_bool().unwrap_or(false),
            &format!("{tool} tool error: {result}"),
        );
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| step.fail(format!("{tool} returned no text content: {result}")))
            .to_string()
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let base_url = std::env::var("KAIROS_E2E_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8188".to_string());
    let issuer = std::env::var("KAIROS_E2E_ISSUER")
        .unwrap_or_else(|_| "http://localhost:5558/dex".to_string());
    let http = reqwest::Client::new();

    println!("[e2e] golden path against {base_url} (tenant {TENANT:?}, issuer {issuer})");

    // --- 1: healthz -----------------------------------------------------------
    let step = Step::start(1, "healthz");
    let response = or_fail(&step, http.get(format!("{base_url}/healthz")).send().await);
    require(
        &step,
        response.status().is_success(),
        &format!("GET /healthz returned {}", response.status()),
    );
    step.ok("server reachable");

    // --- 2: real Dex token for alice -------------------------------------------
    let step = Step::start(2, "dex password grant (alice)");
    let response = or_fail(
        &step,
        http.post(format!("{issuer}/token"))
            .form(&[
                ("grant_type", "password"),
                ("username", "alice@kairos.test"),
                ("password", "alice-password"),
                ("scope", "openid email profile"),
                ("client_id", "kairos-cli"),
            ])
            .send()
            .await,
    );
    require(
        &step,
        response.status().is_success(),
        &format!("token endpoint returned {}", response.status()),
    );
    let token_body: Value = or_fail(&step, response.json().await);
    let token = token_body["access_token"]
        .as_str()
        .unwrap_or_else(|| step.fail("no access_token in Dex response"))
        .to_string();
    step.ok("access token issued");

    // --- 3: REST whoami ----------------------------------------------------------
    let step = Step::start(3, "REST whoami");
    let client = KairosClient::with_static_token(&base_url, &token).with_tenant(TENANT);
    let who = or_fail(&step, client.whoami().await);
    require(
        &step,
        who.user.email == "alice@kairos.test",
        &format!("whoami returned {:?}", who.user.email),
    );
    step.ok(format!("alice authenticated against tenant {TENANT}"));

    // --- 4: board discovery ---------------------------------------------------
    let step = Step::start(4, "board discovery");
    let boards = or_fail(
        &step,
        client
            .list_boards(Pagination {
                limit: Some(50),
                offset: None,
            })
            .await,
    );
    let board_id = |slug: &str| -> String {
        boards
            .items
            .iter()
            .find(|b| b.slug == slug)
            .unwrap_or_else(|| step.fail(format!("seeded board {slug:?} not found")))
            .id
            .clone()
    };
    let strategy_board = board_id("strategy");
    let initiative_board = board_id("initiatives");
    let delivery_board = board_id("platform-delivery");
    let delivery = or_fail(&step, client.get_board(&delivery_board).await);
    let todo_column = delivery
        .columns
        .iter()
        .find(|c| c.name == "Todo")
        .unwrap_or_else(|| step.fail("platform-delivery has no Todo column"))
        .id
        .clone();
    step.ok("strategy, initiatives, platform-delivery boards resolved");

    // --- 5: create strategy -----------------------------------------------------
    let step = Step::start(5, "create strategy");
    let strategy = or_fail(
        &step,
        client
            .create_strategy(&CreateStrategyRequest {
                board_id: strategy_board,
                column_id: None,
                title: "E2E: zero-downtime deploys".to_string(),
                content: format!("Golden-path strategy ({SEARCH_TOKEN})."),
                hypothesis: Some("Deploys stop being events.".to_string()),
            })
            .await,
    );
    step.ok(format!("created {}", strategy.short_code));

    // --- 6: create initiative + parent edge --------------------------------------
    let step = Step::start(6, "create initiative + parent edge");
    let initiative = or_fail(
        &step,
        client
            .create_initiative(&CreateInitiativeRequest {
                board_id: initiative_board,
                column_id: None,
                title: "E2E: blue-green rollout".to_string(),
                content: format!("Golden-path initiative ({SEARCH_TOKEN})."),
                complexity: Some("m".to_string()),
                bucket_type: None,
            })
            .await,
    );
    or_fail(
        &step,
        client
            .create_relationship(&CreateRelationshipRequest {
                source_short_code: strategy.short_code.clone(),
                target_short_code: initiative.short_code.clone(),
                relationship: "parent".to_string(),
            })
            .await,
    );
    step.ok(format!(
        "created {} (parent {})",
        initiative.short_code, strategy.short_code
    ));

    // --- 7: decompose into two tasks with a blocks edge ---------------------------
    let step = Step::start(7, "decompose: 2 tasks + parent edges + blocks edge");
    let mut tasks = Vec::new();
    for title in ["E2E: dual-stack router", "E2E: traffic cutover"] {
        let task = or_fail(
            &step,
            client
                .create_task(&CreateTaskRequest {
                    board_id: delivery_board.clone(),
                    column_id: None,
                    title: title.to_string(),
                    content: format!("Golden-path task ({SEARCH_TOKEN})."),
                    task_type: Some("task".to_string()),
                    team_id: None,
                })
                .await,
        );
        or_fail(
            &step,
            client
                .create_relationship(&CreateRelationshipRequest {
                    source_short_code: initiative.short_code.clone(),
                    target_short_code: task.short_code.clone(),
                    relationship: "parent".to_string(),
                })
                .await,
        );
        tasks.push(task);
    }
    or_fail(
        &step,
        client
            .create_relationship(&CreateRelationshipRequest {
                source_short_code: tasks[0].short_code.clone(),
                target_short_code: tasks[1].short_code.clone(),
                relationship: "blocks".to_string(),
            })
            .await,
    );
    step.ok(format!(
        "created {} blocks-> {}",
        tasks[0].short_code, tasks[1].short_code
    ));

    // --- 8: transition -----------------------------------------------------------
    let step = Step::start(8, "transition task Backlog -> Todo");
    let moved = or_fail(
        &step,
        client
            .transition_task(&tasks[0].short_code, &todo_column)
            .await,
    );
    require(
        &step,
        moved.column_id == todo_column,
        "transition did not land in Todo",
    );
    step.ok(format!("{} now in Todo", tasks[0].short_code));

    // --- 9: search finds all of it -------------------------------------------------
    let step = Step::start(9, "search finds the created items");
    let found = or_fail(
        &step,
        client
            .search(&SearchRequest {
                q: Some(SEARCH_TOKEN.to_string()),
                filter: None,
                traverse: None,
                sort: None,
                limit: Some(50),
                offset: None,
            })
            .await,
    );
    let initiative_hits: Vec<&str> = found
        .results
        .initiatives
        .iter()
        .map(|i| i.short_code.as_str())
        .collect();
    let task_hits: Vec<&str> = found
        .results
        .tasks
        .iter()
        .map(|t| t.short_code.as_str())
        .collect();
    require(
        &step,
        initiative_hits.contains(&initiative.short_code.as_str()),
        &format!(
            "search missed {}: {initiative_hits:?}",
            initiative.short_code
        ),
    );
    for task in &tasks {
        require(
            &step,
            task_hits.contains(&task.short_code.as_str()),
            &format!("search missed {}: {task_hits:?}", task.short_code),
        );
    }
    step.ok(format!(
        "q={SEARCH_TOKEN:?} found the initiative and both tasks (total {})",
        found.total
    ));

    // --- 10: MCP session smoke -------------------------------------------------------
    let step = Step::start(10, "MCP session smoke (initialize/whoami/board_items)");
    let (status, session_id, body) = mcp_post(
        &step,
        &http,
        &base_url,
        &token,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "kairos-e2e", "version": "0.0.0"},
            },
        }),
    )
    .await;
    require(
        &step,
        status == 200,
        &format!("initialize HTTP {status}: {body}"),
    );
    let session_id =
        session_id.unwrap_or_else(|| step.fail("initialize returned no Mcp-Session-Id"));
    let init = rpc_message(&step, &body);
    require(
        &step,
        init["result"]["serverInfo"]["name"] == "kairos",
        &format!("unexpected serverInfo: {init}"),
    );
    let (status, _, body) = mcp_post(
        &step,
        &http,
        &base_url,
        &token,
        Some(&session_id),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    require(
        &step,
        status == 202,
        &format!("initialized notification HTTP {status}: {body}"),
    );
    let mcp = McpSession {
        http: &http,
        base_url: &base_url,
        token: &token,
        session_id,
    };
    let text = mcp.tool(&step, 1, "whoami", json!({})).await;
    require(
        &step,
        text.contains("alice@kairos.test"),
        &format!("MCP whoami does not name alice: {text}"),
    );
    let text = mcp
        .tool(
            &step,
            2,
            "board_items",
            json!({"board": "platform-delivery"}),
        )
        .await;
    require(
        &step,
        text.contains(&tasks[0].short_code),
        &format!(
            "MCP board_items does not show {}: {text}",
            tasks[0].short_code
        ),
    );
    step.ok("initialize + whoami + board_items over streamable HTTP");

    println!(
        "[e2e] GUI leg: runs next as a separate `angreal test e2e` phase \
         (KAIROS-T-0045, Playwright over the Leptos GUI)"
    );
    println!("[e2e] golden path PASSED (10/10 steps)");
    ExitCode::SUCCESS
}
