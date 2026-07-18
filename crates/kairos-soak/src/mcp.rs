//! MCP session driver over streamable HTTP (KAIROS-S-0006 transport,
//! mirroring `kairos-server/examples/e2e_golden_path.rs`): JSON-RPC over
//! POST `/mcp` with `Accept: application/json, text/event-stream`, the
//! `Mcp-Session-Id` header from `initialize`, SSE-framed responses, and a
//! best-effort DELETE at the end so hours-scale runs do not accumulate
//! server-side sessions.

use serde_json::{Value, json};

/// Extract the JSON-RPC message from a streamable-HTTP body (plain JSON
/// or SSE `data:` framing).
fn rpc_message(body: &str) -> Result<Value, String> {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("jsonrpc").is_some()
    {
        return Ok(value);
    }
    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data:")
            && let Ok(value) = serde_json::from_str::<Value>(data.trim())
            && value.get("jsonrpc").is_some()
        {
            return Ok(value);
        }
    }
    Err(format!("no JSON-RPC message in response body: {body:?}"))
}

/// One JSON-RPC POST to `/mcp`; returns `(status, session header, body)`.
async fn post(
    http: &reqwest::Client,
    base_url: &str,
    tenant: &str,
    token: &str,
    session: Option<&str>,
    body: Value,
) -> Result<(u16, Option<String>, String), String> {
    let mut request = http
        .post(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("x-tenant", tenant)
        .json(&body);
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("POST /mcp: {e}"))?;
    let status = response.status().as_u16();
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = response
        .text()
        .await
        .map_err(|e| format!("/mcp body read: {e}"))?;
    Ok((status, session_id, body))
}

/// Call one tool inside an open session; errors carry the tool name.
#[allow(clippy::too_many_arguments)] // transport plumbing, called from one place
async fn call_tool(
    http: &reqwest::Client,
    base_url: &str,
    tenant: &str,
    token: &str,
    session: &str,
    id: i64,
    tool: &str,
    arguments: Value,
) -> Result<(), String> {
    let (status, _, body) = post(
        http,
        base_url,
        tenant,
        token,
        Some(session),
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }),
    )
    .await?;
    if status != 200 {
        return Err(format!("{tool}: HTTP {status}: {body}"));
    }
    let message = rpc_message(&body).map_err(|e| format!("{tool}: {e}"))?;
    if message.get("error").is_some() {
        return Err(format!("{tool}: protocol error: {message}"));
    }
    if message["result"]["isError"].as_bool().unwrap_or(false) {
        return Err(format!("{tool}: tool error: {}", message["result"]));
    }
    Ok(())
}

/// Run one complete MCP session: initialize → notifications/initialized →
/// `whoami` → `board_items` → `get_item` → best-effort session DELETE.
pub async fn run_session(
    http: &reqwest::Client,
    base_url: &str,
    tenant: &str,
    token: &str,
    board_slug: &str,
    item_short_code: &str,
) -> Result<(), String> {
    let (status, session, body) = post(
        http,
        base_url,
        tenant,
        token,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "kairos-soak", "version": env!("CARGO_PKG_VERSION")},
            },
        }),
    )
    .await?;
    if status != 200 {
        return Err(format!("initialize: HTTP {status}: {body}"));
    }
    rpc_message(&body).map_err(|e| format!("initialize: {e}"))?;
    let session = session.ok_or("initialize returned no Mcp-Session-Id")?;

    let (status, _, body) = post(
        http,
        base_url,
        tenant,
        token,
        Some(&session),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await?;
    if status != 202 {
        return Err(format!("notifications/initialized: HTTP {status}: {body}"));
    }

    call_tool(
        http,
        base_url,
        tenant,
        token,
        &session,
        1,
        "whoami",
        json!({}),
    )
    .await?;
    call_tool(
        http,
        base_url,
        tenant,
        token,
        &session,
        2,
        "board_items",
        json!({"board": board_slug}),
    )
    .await?;
    call_tool(
        http,
        base_url,
        tenant,
        token,
        &session,
        3,
        "get_item",
        json!({"short_code": item_short_code}),
    )
    .await?;

    // Best-effort session teardown (rmcp streamable HTTP supports DELETE);
    // an error here never fails the op.
    let _ = http
        .delete(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .header("x-tenant", tenant)
        .header("mcp-session-id", &session)
        .send()
        .await;
    Ok(())
}
