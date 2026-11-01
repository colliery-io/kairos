//! `kairos mcp-proxy` (#270): the `kairos` MCP server of the Claude Code
//! plugin. It speaks MCP over stdio to the client and forwards each message
//! to `/mcp` of the deployment of the project, over the Streamable HTTP
//! transport.
//!
//! The plugin could only name one URL, `${KAIROS_URL}/mcp`, fixed for the
//! whole session, so two repositories could not use two deployments. The
//! proxy finds the deployment as each command does: `--url`, then
//! `KAIROS_URL`, then the `[[project]]` of the folder of the session
//! (`CLAUDE_PROJECT_DIR`), then `[default]`, then the only deployment of a
//! login. The credential: `KAIROS_KEY` or
//! `KAIROS_MCP_KEY`, else the token of `kairos login`, refreshed as it
//! expires. The tenant of a login: `--tenant`, then the binding, then the
//! tenant of the login, sent as `X-Tenant` as the CLI sends it.
//!
//! Each line on stdin is one JSON-RPC message. The proxy posts it and
//! writes each JSON-RPC message of the answer (a JSON body, or the `data:`
//! of a `text/event-stream`) as one line on stdout. It keeps the
//! `Mcp-Session-Id` of the server. A request that fails before the server
//! answers gets a JSON-RPC error with the reason, so the client shows it
//! and does not wait.
//!
//! KAIROS-T-0680: `--profile admin|all` forwards to `/mcp?profile=admin` or
//! `/mcp?profile=all`, so a second server of the proxy gives the
//! administration tools with the same credential (also a `kairos login`).
//!
//! KAIROS-T-0682: a deployment that is down (an upgrade) answered a raw
//! HTML 503 page, or reset the connection, or did not answer, and the agent
//! read "refused" for a write that the server had applied. A 502, 503 or
//! 504, a timeout and a reset now give one plain message, with
//! `retryable: true` in the error data. A read is safe to send again, so the
//! proxy retries it with a backoff. A write with no `request_key` is not
//! retried: a second send can apply it twice.
//!
//! KAIROS-T-0687: a write that carries a `request_key` (KAIROS-T-0681,
//! KAIROS-A-0065) is retried with the backoff of a read. The server gives
//! the first answer to a second call with the same key and writes nothing,
//! so the retry is a replay. The error data says `"retry": "replay"` and
//! names the key.
//!
//! KAIROS-T-0693: the reads and the tools that take a `request_key` come
//! from the table `kairos_client::mcp_tools`, which a test of the server
//! checks against `tools/list`.
//!
//! KAIROS-T-0699: a write that the table marks idempotent (a set, a patch,
//! a replace: `set_state`, `set_metadata`) is retried with the backoff of
//! a read. A second call gives the same state. The error data says
//! `"retry": "idempotent"`.
//!
//! KAIROS-T-0706: when the proxy cannot find its deployment or its
//! credential (no login, a key with no URL, logins for two deployments and
//! no binding, a `config.toml` that does not parse), it does not exit. An
//! exit before the handshake is "failed to connect" in Claude Code, and the
//! agent does not see why. The proxy serves MCP with no tools: the
//! instructions of the server give the reason and the commands to run, as
//! the fallback of `plugin/bin/kairos-code` does. Stderr has the same text.

use std::sync::Arc;
use std::time::Duration;

use kairos_client::TokenProvider;
// KAIROS-T-0693: the reads and the keyed writes come from one table.
use kairos_client::mcp_tools;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc};

use crate::auth::{self, Source};
use crate::credentials;
use crate::error::CliError;
use crate::project_config;
use crate::provider::CachedTokenProvider;

#[derive(clap::Args, Debug)]
pub struct McpProxyArgs {
    /// Deployment base URL (defaults to KAIROS_URL, then the project config, then the only cached deployment)
    #[arg(long)]
    pub url: Option<String>,
    /// Tenant slug override (defaults to the project config, then the tenant cached at login)
    #[arg(long)]
    pub tenant: Option<String>,
    /// The tool profile of the server: work (the default), admin (the administration tools) or all (each tool)
    #[arg(long, value_enum, default_value_t = Profile::Work)]
    pub profile: Profile,
}

/// The tool profiles of `/mcp` (KAIROS-T-0454).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// The tools of the daily work: `/mcp`.
    Work,
    /// The administration tools: `/mcp?profile=admin`.
    Admin,
    /// Each tool: `/mcp?profile=all`.
    All,
}

/// The MCP endpoint of a deployment for a profile.
fn endpoint(deployment: &str, profile: Profile) -> String {
    let base = format!("{}/mcp", credentials::normalize_url(deployment));
    match profile {
        Profile::Work => base,
        Profile::Admin => format!("{base}?profile=admin"),
        Profile::All => format!("{base}?profile=all"),
    }
}

/// The header of the session of the Streamable HTTP transport.
const SESSION_HEADER: &str = "mcp-session-id";

/// How the proxy authenticates.
enum Credential {
    Key(String),
    Login(Arc<CachedTokenProvider>),
}

/// The upstream server and the state of the session.
struct Upstream {
    http: reqwest::Client,
    endpoint: String,
    credential: Credential,
    tenant: Option<String>,
    session: Mutex<Option<String>>,
    /// The waits before each retry of a read; its length is the number of
    /// retries.
    backoff: Vec<Duration>,
}

/// The waits before the retries of a read, of a write with a
/// `request_key` and of an idempotent write: two retries.
const READ_BACKOFF: [Duration; 2] = [Duration::from_millis(250), Duration::from_secs(1)];

/// The message of a write when the deployment is unavailable.
const UNAVAILABLE_WRITE: &str = "The deployment is unavailable or did not answer (maybe an \
     upgrade). A write may or may not have been applied: read the item before you retry.";

/// The message of a write that did not get to the deployment.
const UNAVAILABLE_NOT_SENT: &str = "The deployment is unavailable (maybe an upgrade). The \
     request did not get to it, so nothing was applied. Retry the request later.";

/// The methods of MCP that only read.
const READ_METHODS: [&str; 9] = [
    "initialize",
    "ping",
    "tools/list",
    "resources/list",
    "resources/templates/list",
    "resources/read",
    "prompts/list",
    "prompts/get",
    "completion/complete",
];

/// Why a post failed.
#[derive(Debug, PartialEq)]
enum Failure {
    /// The deployment is unavailable: a 502, 503 or 504, a timeout, a reset,
    /// or no connection. The request is safe to send again only when it is
    /// a read, or when it did not get to the deployment (`sent: false`).
    Unavailable {
        cause: &'static str,
        status: Option<u16>,
        sent: bool,
    },
    /// Any other failure: the text says why.
    Other(String),
}

impl Failure {
    /// The failure of a request that got no answer.
    fn of_send(e: &reqwest::Error) -> Self {
        let (cause, sent) = if e.is_connect() {
            ("connect", false)
        } else if e.is_timeout() {
            ("timeout", true)
        } else {
            ("reset", true)
        };
        Failure::Unavailable {
            cause,
            status: None,
            sent,
        }
    }
}

/// True when a message only reads: a read method, a `tools/call` of a read
/// tool, or a batch of reads.
fn is_read(message: &Value) -> bool {
    if let Value::Array(batch) = message {
        return !batch.is_empty() && batch.iter().all(is_read);
    }
    match message.get("method").and_then(Value::as_str) {
        Some("tools/call") => message
            .pointer("/params/name")
            .and_then(Value::as_str)
            .is_some_and(mcp_tools::is_read),
        Some(method) => READ_METHODS.contains(&method),
        None => false,
    }
}

/// The `request_key` of a `tools/call` of a tool that takes one, when its
/// arguments carry one.
fn request_key(message: &Value) -> Option<&str> {
    if message.get("method").and_then(Value::as_str) != Some("tools/call") {
        return None;
    }
    let name = message.pointer("/params/name").and_then(Value::as_str)?;
    if !mcp_tools::takes_request_key(name) {
        return None;
    }
    message
        .pointer("/params/arguments/request_key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
}

/// True for a `tools/call` of a write that the table marks idempotent
/// (KAIROS-T-0699).
fn is_idempotent(message: &Value) -> bool {
    message.get("method").and_then(Value::as_str) == Some("tools/call")
        && message
            .pointer("/params/name")
            .and_then(Value::as_str)
            .is_some_and(mcp_tools::is_idempotent)
}

/// How the proxy may send a message again when the deployment is
/// unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    /// A read: safe to send again.
    Read,
    /// A write with a `request_key`: a second send is a replay, and writes
    /// nothing new (KAIROS-A-0065).
    Replay,
    /// An idempotent write: a second send gives the same state
    /// (KAIROS-T-0699).
    Idempotent,
    /// A write with no key: never sent again.
    None,
}

impl Retry {
    fn of(message: &Value) -> Self {
        if is_read(message) {
            return Retry::Read;
        }
        let safe = |m: &Value| is_read(m) || request_key(m).is_some() || is_idempotent(m);
        let batch: Vec<&Value> = match message {
            Value::Array(batch) => batch.iter().collect(),
            message => vec![message],
        };
        if batch.is_empty() || !batch.iter().all(|m| safe(m)) {
            return Retry::None;
        }
        if batch.iter().any(|m| request_key(m).is_some()) {
            Retry::Replay
        } else {
            Retry::Idempotent
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Retry::Read => "read",
            Retry::Replay => "replay",
            Retry::Idempotent => "idempotent",
            Retry::None => "none",
        }
    }
}

impl McpProxyArgs {
    pub async fn run(self) -> Result<(), CliError> {
        match connect(self) {
            Ok(upstream) => forward_stdio(Arc::new(upstream)).await,
            Err(off) => {
                eprintln!("{off}");
                serve_no_tools(off).await
            }
        }
    }
}

/// The MCP server with no tools of KAIROS-T-0706: its instructions say why
/// the tools are off and what to run.
#[derive(Clone)]
struct NoTools {
    instructions: String,
}

impl rmcp::ServerHandler for NoTools {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::new(
            "kairos",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(self.instructions.clone())
    }
}

/// Serve [`NoTools`] on stdin and stdout until the client closes stdin.
async fn serve_no_tools(instructions: String) -> Result<(), CliError> {
    use rmcp::ServiceExt;
    let server = NoTools { instructions }
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| CliError::Failure(format!("The MCP server with no tools failed: {e}.")))?;
    server
        .waiting()
        .await
        .map_err(|e| CliError::Failure(format!("The MCP server with no tools failed: {e}.")))?;
    Ok(())
}

/// What to run when the binding of the folder is not correct.
fn config_fix(folder: &str) -> String {
    format!(
        "Correct the file, then run `kairos config show {folder}` to see the binding of the \
         folder of the session. Then reconnect the `kairos` server with /mcp, or start the \
         session again."
    )
}

/// What to run when the proxy finds no deployment or no credential.
fn login_fix(folder: &str) -> String {
    format!(
        "Do one of these, then reconnect the `kairos` server with /mcp, or start the session \
         again:\n\
         - Run `kairos login --url <deployment>`.\n\
         - With logins for more than one deployment: run `kairos config project \
         --deployment <url>` in the checkout ({folder}), or set {url} in the environment of \
         Claude Code.\n\
         - Set {url} and {key} (an agent key) in the environment of Claude Code.\n\
         `kairos config show` gives the binding of the folder.",
        url = auth::URL_VAR,
        key = auth::KEY_VARS.join(" or "),
    )
}

/// The instructions of the server with no tools: the reason, then the fix.
fn tools_off(reason: &CliError, fix: &str) -> String {
    format!(
        "The Kairos tools are off: `kairos mcp-proxy` cannot reach a deployment.\n{}\n{fix}",
        reason.to_string().trim_end()
    )
}

/// Forward MCP between stdio and the deployment until stdin closes.
async fn forward_stdio(upstream: Arc<Upstream>) -> Result<(), CliError> {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    // One writer: the answers of requests that run at the same time do
    // not mix their lines.
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = rx.recv().await {
            if stdout.write_all(line.as_bytes()).await.is_err()
                || stdout.write_all(b"\n").await.is_err()
                || stdout.flush().await.is_err()
            {
                break;
            }
        }
    });
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut tasks = Vec::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let upstream = upstream.clone();
        let tx = tx.clone();
        tasks.push(tokio::spawn(async move {
            for out in upstream.forward(&line).await {
                let _ = tx.send(out);
            }
        }));
    }
    for task in tasks {
        let _ = task.await;
    }
    upstream.end().await;
    drop(tx);
    let _ = writer.await;
    Ok(())
}

/// The deployment, the credential and the tenant of the proxy. The error is
/// the instructions of the server with no tools (KAIROS-T-0706).
fn connect(args: McpProxyArgs) -> Result<Upstream, String> {
    let folder = project_config::folder_here()
        .map(|f| f.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    let binding =
        project_config::binding_here().map_err(|e| tools_off(&e, &config_fix(&folder)))?;
    let off = |e: CliError| tools_off(&e, &login_fix(&folder));
    let env = auth::Env::from_process();
    let url = args.url.or_else(|| {
        env.url
            .is_none()
            .then(|| binding.get("deployment").map(str::to_string))
            .flatten()
    });
    let path = credentials::credentials_path().map_err(off)?;
    let source = auth::resolve(url.as_deref(), &env, || credentials::load(&path)).map_err(off)?;
    let (deployment, credential, tenant) = match source {
        Source::Key {
            deployment, key, ..
        } => (deployment, Credential::Key(key), args.tenant),
        Source::Cache { deployment, store } => {
            let entry = credentials::entry_for(&store, &deployment).map_err(off)?;
            let same = binding.get("deployment").is_none_or(|bound| {
                credentials::normalize_url(bound) == credentials::normalize_url(&deployment)
            });
            let tenant = args
                .tenant
                .or_else(|| binding.get("tenant").filter(|_| same).map(str::to_string))
                .or(entry.tenant);
            let provider = Arc::new(CachedTokenProvider::new(path, deployment.clone()));
            (deployment, Credential::Login(provider), tenant)
        }
    };
    let timeout = kairos_client::timeout_from_env()
        .map_err(|e| {
            tools_off(
                &e.into(),
                "Correct the variable, then start the session again.",
            )
        })?
        .unwrap_or(kairos_client::DEFAULT_TIMEOUT);
    Upstream::new(
        endpoint(&deployment, args.profile),
        credential,
        tenant,
        timeout,
    )
    .map_err(|e| tools_off(&e, "Start the session again."))
}

impl Upstream {
    fn new(
        endpoint: String,
        credential: Credential,
        tenant: Option<String>,
        timeout: Duration,
    ) -> Result<Self, CliError> {
        // The timeouts of kairos-client (KAIROS-T-0607): before, the proxy
        // waited with no limit for a deployment that did not answer.
        let http = reqwest::Client::builder()
            .connect_timeout(kairos_client::DEFAULT_CONNECT_TIMEOUT.min(timeout))
            .timeout(timeout)
            .build()
            .map_err(|e| CliError::Failure(format!("The CLI cannot make an HTTP client: {e}.")))?;
        Ok(Upstream {
            http,
            endpoint,
            credential,
            tenant,
            session: Mutex::new(None),
            backoff: READ_BACKOFF.to_vec(),
        })
    }
}

impl Upstream {
    /// The bearer token of the next request.
    async fn token(&self) -> Result<String, String> {
        match &self.credential {
            Credential::Key(key) => Ok(key.clone()),
            Credential::Login(provider) => provider.bearer_token().await.map_err(|e| e.to_string()),
        }
    }

    /// Forward one message. The answer: the lines to write.
    async fn forward(&self, line: &str) -> Vec<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(_) => {
                return vec![error_line(
                    Value::Null,
                    -32700,
                    "Parse error: the proxy got a line that is not JSON.".into(),
                )];
            }
        };
        let id = message.get("id").cloned();
        let retry = Retry::of(&message);
        let key = request_key(&message).map(str::to_string);
        let read = retry == Retry::Read;
        let mut attempts = 1;
        // True when an attempt before the last got to the deployment.
        let mut sent_before = false;
        let failure = loop {
            match self.post(line).await {
                Ok(lines) => return lines,
                Err(Failure::Unavailable { sent, .. })
                    if retry != Retry::None && attempts <= self.backoff.len() =>
                {
                    sent_before |= sent;
                    tokio::time::sleep(self.backoff[attempts - 1]).await;
                    attempts += 1;
                }
                Err(failure) => break failure,
            }
        };
        // A notification gets no answer, also when it fails.
        let Some(id) = id else {
            return Vec::new();
        };
        let (message, data) = match failure {
            Failure::Other(why) => (why, None),
            Failure::Unavailable {
                cause,
                status,
                sent,
            } => {
                let sent_any = sent || sent_before;
                let message = match retry {
                    Retry::Read => format!(
                        "The deployment is unavailable or did not answer (maybe an upgrade). \
                         The proxy sent this read {attempts} times. Retry it later."
                    ),
                    Retry::Replay => format!(
                        "The deployment is unavailable or did not answer (maybe an upgrade). \
                         The proxy sent this write {attempts} times with the same \
                         request_key. Send it again later with the same request_key: if the \
                         server applied the write, it gives the first answer and writes \
                         nothing new."
                    ),
                    Retry::Idempotent => format!(
                        "The deployment is unavailable or did not answer (maybe an upgrade). \
                         The proxy sent this write {attempts} times. The tool is idempotent: \
                         send it again later. If the server applied the write, a second \
                         call gives the same state."
                    ),
                    Retry::None if sent => UNAVAILABLE_WRITE.to_string(),
                    Retry::None => UNAVAILABLE_NOT_SENT.to_string(),
                };
                let applied = if read || !sent_any { "no" } else { "unknown" };
                let mut data = json!({
                    "retryable": true,
                    "cause": cause,
                    "status": status,
                    "attempts": attempts,
                    "write_applied": applied,
                    "retry": retry.as_str(),
                });
                if let Some(key) = key {
                    data["request_key"] = json!(key);
                }
                (message, Some(data))
            }
        };
        vec![error_line_with(id, -32603, message, data)]
    }

    async fn post(&self, body: &str) -> Result<Vec<String>, Failure> {
        let token = self.token().await.map_err(Failure::Other)?;
        let mut request = self
            .http
            .post(&self.endpoint)
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .body(body.to_string());
        if let Some(tenant) = &self.tenant {
            request = request.header("X-Tenant", tenant);
        }
        if let Some(session) = self.session.lock().await.clone() {
            request = request.header(SESSION_HEADER, session);
        }
        let response = request.send().await.map_err(|e| Failure::of_send(&e))?;
        if let Some(session) = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
        {
            *self.session.lock().await = Some(session.to_string());
        }
        let status = response.status();
        let event_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/event-stream"));
        if unavailable(status) {
            // The body is often the HTML page of a proxy in front of the
            // server: it is not read, and not shown.
            return Err(Failure::Unavailable {
                cause: "status",
                status: Some(status.as_u16()),
                sent: true,
            });
        }
        // A reset or a timeout in the body: the request got to the server.
        let text = response.text().await.map_err(|e| {
            let cause = if e.is_timeout() { "timeout" } else { "reset" };
            Failure::Unavailable {
                cause,
                status: None,
                sent: true,
            }
        })?;
        if !status.is_success() {
            return Err(Failure::Other(refusal(status, &self.endpoint, &text)));
        }
        Ok(if event_stream {
            sse_messages(&text)
        } else {
            json_messages(&text)
        })
    }

    /// End the session of the server, when it gave one.
    async fn end(&self) {
        let Some(session) = self.session.lock().await.clone() else {
            return;
        };
        let Ok(token) = self.token().await else {
            return;
        };
        let _ = self
            .http
            .delete(&self.endpoint)
            .bearer_auth(token)
            .header(SESSION_HEADER, session)
            .send()
            .await;
    }
}

/// True for the statuses of a deployment that is down or that is not
/// ready: 502, 503 and 504.
fn unavailable(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 502..=504)
}

/// The text of a refused request: the status, and what to do for 401. An
/// HTML body is not shown.
fn refusal(status: reqwest::StatusCode, endpoint: &str, body: &str) -> String {
    let detail = body.trim();
    let detail = if detail.is_empty() || detail.starts_with('<') {
        String::new()
    } else {
        format!(": {}", detail.chars().take(300).collect::<String>())
    };
    let mut out =
        format!("The Kairos deployment at {endpoint} refused the request with {status}{detail}.");
    if status == reqwest::StatusCode::UNAUTHORIZED {
        out.push_str(&format!(
            " Run `kairos login`, or set {} to an agent key.",
            auth::KEY_VARS.join(" or ")
        ));
    }
    out
}

/// A JSON-RPC error as one line.
fn error_line(id: Value, code: i64, message: String) -> String {
    error_line_with(id, code, message, None)
}

/// A JSON-RPC error as one line, with its `data` when there is one.
fn error_line_with(id: Value, code: i64, message: String, data: Option<Value>) -> String {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error}).to_string()
}

/// The messages of a JSON body: one message, or a batch (an array). An
/// empty body (202 for a notification) gives none.
fn json_messages(body: &str) -> Vec<String> {
    match serde_json::from_str::<Value>(body.trim()) {
        Ok(Value::Array(batch)) => batch.iter().map(Value::to_string).collect(),
        Ok(message) => vec![message.to_string()],
        Err(_) => Vec::new(),
    }
}

/// The messages of a `text/event-stream` body: the `data:` lines of each
/// event, joined, when they are JSON.
fn sse_messages(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for event in body.replace("\r\n", "\n").split("\n\n") {
        let data: Vec<&str> = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|d| d.strip_prefix(' ').unwrap_or(d))
            .collect();
        if data.is_empty() {
            continue;
        }
        out.extend(json_messages(&data.join("\n")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_stream_gives_each_json_message_as_one_line() {
        let body = "event: message\r\nid: 1\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\r\ndata: \"result\":{}}\r\n\r\n: a comment\n\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n";
        assert_eq!(
            sse_messages(body),
            [
                r#"{"id":1,"jsonrpc":"2.0","result":{}}"#,
                r#"{"jsonrpc":"2.0","method":"notifications/progress"}"#,
            ]
        );
    }

    #[test]
    fn a_json_body_is_one_message_or_a_batch_and_an_empty_body_is_none() {
        assert_eq!(json_messages(r#"{"id":1}"#), [r#"{"id":1}"#]);
        assert_eq!(
            json_messages(r#"[{"id":1},{"id":2}]"#),
            [r#"{"id":1}"#, r#"{"id":2}"#]
        );
        assert!(json_messages("").is_empty());
    }

    #[test]
    fn a_401_says_how_to_sign_in() {
        let text = refusal(reqwest::StatusCode::UNAUTHORIZED, "https://k/mcp", "");
        assert!(
            text.starts_with(
                "The Kairos deployment at https://k/mcp refused the request with 401 Unauthorized."
            ),
            "{text}"
        );
        assert!(
            text.contains("Run `kairos login`, or set KAIROS_KEY or KAIROS_MCP_KEY"),
            "{text}"
        );
    }

    #[test]
    fn a_failed_request_is_an_error_with_its_id() {
        assert_eq!(
            error_line(json!(7), -32603, "no".into()),
            r#"{"error":{"code":-32603,"message":"no"},"id":7,"jsonrpc":"2.0"}"#
        );
    }

    #[test]
    fn the_profile_chooses_the_endpoint() {
        assert_eq!(
            endpoint("https://k.example/", Profile::Work),
            "https://k.example/mcp"
        );
        assert_eq!(
            endpoint("https://k.example", Profile::Admin),
            "https://k.example/mcp?profile=admin"
        );
        assert_eq!(
            endpoint("https://k.example", Profile::All),
            "https://k.example/mcp?profile=all"
        );
    }

    #[test]
    fn an_html_body_of_a_refusal_is_not_shown() {
        let text = refusal(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "https://k/mcp",
            "<html><body>oops</body></html>",
        );
        assert_eq!(
            text,
            "The Kairos deployment at https://k/mcp refused the request with 500 Internal Server Error."
        );
    }

    #[test]
    fn the_profile_flag_takes_work_admin_and_all() {
        use clap::Parser;
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            args: McpProxyArgs,
        }
        let parse = |argv: &[&str]| Cli::try_parse_from(argv).map(|c| c.args.profile);
        assert_eq!(parse(&["kairos"]).unwrap(), Profile::Work);
        assert_eq!(
            parse(&["kairos", "--profile", "admin"]).unwrap(),
            Profile::Admin
        );
        assert_eq!(
            parse(&["kairos", "--profile", "all"]).unwrap(),
            Profile::All
        );
        let refused = parse(&["kairos", "--profile", "every"])
            .unwrap_err()
            .to_string();
        assert!(refused.contains("work, admin, all"), "{refused}");
    }

    #[test]
    fn reads_are_the_read_methods_and_the_calls_of_read_tools() {
        let call = |name: &str| json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name}});
        assert!(is_read(&call("get_item")));
        assert!(is_read(&call("search")));
        assert!(!is_read(&call("create_item")));
        assert!(!is_read(&call("append_note")));
        assert!(!is_read(&call("a_tool_that_is_not_known")));
        assert!(is_read(&json!({"id": 1, "method": "tools/list"})));
        assert!(is_read(&json!({"id": 1, "method": "initialize"})));
        assert!(!is_read(&json!({"method": "notifications/initialized"})));
        assert!(is_read(&json!([call("get_item"), call("whoami")])));
        assert!(!is_read(&json!([
            call("get_item"),
            call("transition_item")
        ])));
        assert!(!is_read(&json!([])));
    }

    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};

    /// What the stub server does with each request.
    #[derive(Clone, Copy)]
    enum Stub {
        /// 503 with an HTML page, as a proxy in front of the server.
        Html503,
        /// Close the connection after it reads the request.
        Reset,
        /// Read the request and give no answer.
        Hang,
        /// Close the first connection, then answer 200.
        ResetThenOk,
    }

    /// Read one HTTP request: the head, then the body of Content-Length.
    async fn read_request(socket: &mut TcpStream) {
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let Ok(n) = socket.read(&mut buf).await else {
                return;
            };
            if n == 0 {
                return;
            }
            data.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&data).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                if data.len() >= end + 4 + length {
                    return;
                }
            }
        }
    }

    /// A stub deployment on a local port: its endpoint and its count of
    /// requests.
    async fn stub(kind: Stub) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let count = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let n = count.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    read_request(&mut socket).await;
                    let answer = match (kind, n) {
                        (Stub::Html503, _) => {
                            let page = "<html><body><h1>503 Service Temporarily Unavailable</h1></body></html>";
                            format!(
                                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                                page.len()
                            )
                        }
                        (Stub::Reset, _) | (Stub::ResetThenOk, 0) => return,
                        (Stub::Hang, _) => {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                            return;
                        }
                        (Stub::ResetThenOk, _) => {
                            let body = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            )
                        }
                    };
                    let _ = socket.write_all(answer.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (format!("http://{addr}/mcp"), hits)
    }

    /// A proxy to `endpoint` with short waits.
    fn proxy(endpoint: String) -> Upstream {
        let mut upstream = Upstream::new(
            endpoint,
            Credential::Key("test-key".into()),
            None,
            Duration::from_millis(500),
        )
        .unwrap();
        upstream.backoff = vec![Duration::from_millis(1); 2];
        upstream
    }

    fn call(name: &str) -> String {
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": {}}}).to_string()
    }

    /// The one answer line, as JSON.
    fn answer(lines: Vec<String>) -> Value {
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(!lines[0].contains("<html"), "{}", lines[0]);
        serde_json::from_str(&lines[0]).unwrap()
    }

    fn keyed_call(name: &str, key: &str) -> String {
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": {"short_code": "ACME-T-0001", "request_key": key}}}).to_string()
    }

    #[test]
    fn a_write_with_a_request_key_is_a_replay() {
        let call = |name: &str| json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": {}}});
        let keyed: Value = serde_json::from_str(&keyed_call("create_item", "key-0001")).unwrap();
        assert_eq!(Retry::of(&call("get_item")), Retry::Read);
        assert_eq!(Retry::of(&keyed), Retry::Replay);
        assert_eq!(request_key(&keyed), Some("key-0001"));
        assert_eq!(Retry::of(&call("create_item")), Retry::None);
        let empty: Value = serde_json::from_str(&keyed_call("create_item", "")).unwrap();
        assert_eq!(Retry::of(&empty), Retry::None, "an empty key is no key");
        let not_a_string = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "create_item", "arguments": {"request_key": 7}}});
        assert_eq!(Retry::of(&not_a_string), Retry::None);
        assert_eq!(
            Retry::of(&json!([call("get_item"), keyed.clone()])),
            Retry::Replay
        );
        assert_eq!(
            Retry::of(&json!([keyed, call("append_note")])),
            Retry::None,
            "a batch with one write with no key is not sent again"
        );
        assert_eq!(Retry::of(&json!([])), Retry::None);
        let unkeyed: Value = serde_json::from_str(&keyed_call("claim_item", "key-0001")).unwrap();
        assert_eq!(
            Retry::of(&unkeyed),
            Retry::None,
            "a tool that takes no request_key and is not idempotent is not sent again"
        );
    }

    #[test]
    fn an_idempotent_write_is_sent_again() {
        let call = |name: &str| json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": {}}});
        let keyed: Value = serde_json::from_str(&keyed_call("create_item", "key-0001")).unwrap();
        assert_eq!(Retry::of(&call("set_state")), Retry::Idempotent);
        assert_eq!(Retry::of(&call("set_metadata")), Retry::Idempotent);
        assert_eq!(Retry::of(&call("mark_events_read")), Retry::Idempotent);
        assert_eq!(Retry::of(&call("update_item")), Retry::None);
        assert_eq!(Retry::of(&call("claim_item")), Retry::None);
        assert_eq!(
            Retry::of(&json!([call("get_item"), call("set_state")])),
            Retry::Idempotent
        );
        assert_eq!(Retry::of(&json!([call("set_state"), keyed])), Retry::Replay);
        assert_eq!(
            Retry::of(&json!([call("set_state"), call("append_note")])),
            Retry::None,
            "a batch with one write with no key is not sent again"
        );
        // A method that is not tools/call is not an idempotent write.
        assert_eq!(
            Retry::of(&json!({"id": 1, "method": "set_state"})),
            Retry::None
        );
        // Each tool that the table marks idempotent is sent again.
        for (name, kind) in mcp_tools::TOOLS {
            if *kind == mcp_tools::ToolKind::Idempotent {
                assert_eq!(Retry::of(&call(name)), Retry::Idempotent, "{name}");
            }
        }
    }

    #[tokio::test]
    async fn an_idempotent_write_is_retried_after_a_reset() {
        let (endpoint, hits) = stub(Stub::ResetThenOk).await;
        let out = answer(proxy(endpoint).forward(&call("set_state")).await);
        assert_eq!(out["result"]["ok"], true, "{out}");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_idempotent_write_is_sent_three_times_and_the_data_says_idempotent() {
        let (endpoint, hits) = stub(Stub::Html503).await;
        let out = answer(proxy(endpoint).forward(&call("set_metadata")).await);
        let message = out["error"]["message"].as_str().unwrap();
        assert!(message.contains("sent this write 3 times"), "{message}");
        assert!(message.contains("The tool is idempotent"), "{message}");
        let data = &out["error"]["data"];
        assert_eq!(data["retry"], "idempotent");
        assert!(data.get("request_key").is_none());
        assert_eq!(data["attempts"], 3);
        assert_eq!(data["write_applied"], "unknown");
        assert_eq!(data["retryable"], true);
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn an_idempotent_write_with_no_connection_was_not_applied() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let out = answer(
            proxy(format!("http://{addr}/mcp"))
                .forward(&call("set_state"))
                .await,
        );
        assert_eq!(out["error"]["data"]["retry"], "idempotent");
        assert_eq!(out["error"]["data"]["attempts"], 3);
        assert_eq!(out["error"]["data"]["write_applied"], "no");
    }

    #[tokio::test]
    async fn a_write_that_is_not_idempotent_and_has_no_key_is_sent_once() {
        let (endpoint, hits) = stub(Stub::Html503).await;
        let out = answer(proxy(endpoint).forward(&call("claim_item")).await);
        assert_eq!(out["error"]["data"]["retry"], "none");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_write_with_a_request_key_is_retried_after_a_reset() {
        let (endpoint, hits) = stub(Stub::ResetThenOk).await;
        let out = answer(
            proxy(endpoint)
                .forward(&keyed_call("create_item", "key-reset-01"))
                .await,
        );
        assert_eq!(out["result"]["ok"], true, "{out}");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_write_with_a_request_key_is_sent_three_times_and_the_data_says_replay() {
        let (endpoint, hits) = stub(Stub::Html503).await;
        let out = answer(
            proxy(endpoint)
                .forward(&keyed_call("append_note", "key-503-0001"))
                .await,
        );
        let message = out["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("sent this write 3 times with the same request_key"),
            "{message}"
        );
        assert!(message.contains("writes nothing new"), "{message}");
        let data = &out["error"]["data"];
        assert_eq!(data["retry"], "replay");
        assert_eq!(data["request_key"], "key-503-0001");
        assert_eq!(data["attempts"], 3);
        assert_eq!(data["write_applied"], "unknown");
        assert_eq!(data["retryable"], true);
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_write_with_a_request_key_and_no_connection_was_not_applied() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let out = answer(
            proxy(format!("http://{addr}/mcp"))
                .forward(&keyed_call("create_item", "key-conn-001"))
                .await,
        );
        assert_eq!(out["error"]["data"]["retry"], "replay");
        assert_eq!(out["error"]["data"]["attempts"], 3);
        assert_eq!(out["error"]["data"]["write_applied"], "no");
    }

    #[tokio::test]
    async fn a_503_html_page_on_a_write_is_a_plain_message_and_is_not_retried() {
        let (endpoint, hits) = stub(Stub::Html503).await;
        let out = answer(proxy(endpoint).forward(&call("create_item")).await);
        assert_eq!(out["error"]["message"], UNAVAILABLE_WRITE);
        assert_eq!(out["error"]["data"]["retry"], "none");
        assert!(out["error"]["data"].get("request_key").is_none());
        assert_eq!(out["error"]["data"]["retryable"], true);
        assert_eq!(out["error"]["data"]["status"], 503);
        assert_eq!(out["error"]["data"]["write_applied"], "unknown");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_reset_on_a_write_says_the_write_may_be_applied() {
        let (endpoint, hits) = stub(Stub::Reset).await;
        let out = answer(proxy(endpoint).forward(&call("append_note")).await);
        assert_eq!(out["error"]["message"], UNAVAILABLE_WRITE);
        assert_eq!(out["error"]["data"]["cause"], "reset");
        assert_eq!(out["error"]["data"]["retryable"], true);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_timeout_on_a_write_says_the_write_may_be_applied() {
        let (endpoint, hits) = stub(Stub::Hang).await;
        let out = answer(proxy(endpoint).forward(&call("transition_item")).await);
        assert_eq!(out["error"]["message"], UNAVAILABLE_WRITE);
        assert_eq!(out["error"]["data"]["cause"], "timeout");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_write_with_no_connection_says_nothing_was_applied() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let out = answer(
            proxy(format!("http://{addr}/mcp"))
                .forward(&call("create_item"))
                .await,
        );
        assert_eq!(out["error"]["message"], UNAVAILABLE_NOT_SENT);
        assert_eq!(out["error"]["data"]["cause"], "connect");
        assert_eq!(out["error"]["data"]["write_applied"], "no");
    }

    #[tokio::test]
    async fn a_read_is_retried_after_a_reset() {
        let (endpoint, hits) = stub(Stub::ResetThenOk).await;
        let out = answer(proxy(endpoint).forward(&call("get_item")).await);
        assert_eq!(out["result"]["ok"], true, "{out}");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_read_is_sent_three_times_then_fails_plainly() {
        let (endpoint, hits) = stub(Stub::Html503).await;
        let out = answer(proxy(endpoint).forward(&call("search")).await);
        let message = out["error"]["message"].as_str().unwrap();
        assert!(message.contains("sent this read 3 times"), "{message}");
        assert_eq!(out["error"]["data"]["retry"], "read");
        assert_eq!(out["error"]["data"]["retryable"], true);
        assert_eq!(out["error"]["data"]["write_applied"], "no");
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }
}
