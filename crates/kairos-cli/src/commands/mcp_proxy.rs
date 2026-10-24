//! `kairos mcp-proxy` (#270): the `kairos` MCP server of the Claude Code
//! plugin. It speaks MCP over stdio to the client and forwards each message
//! to `/mcp` of the deployment of the project, over the Streamable HTTP
//! transport.
//!
//! The plugin could only name one URL, `${KAIROS_URL}/mcp`, fixed for the
//! whole session, so two repositories could not use two deployments. The
//! proxy finds the deployment as each command does: `--url`, then
//! `KAIROS_URL`, then the `[[project]]` of the folder of the session
//! (`CLAUDE_PROJECT_DIR`), then `.claude/kairos.local.md`, then `[default]`,
//! then the only deployment of a login. The credential: `KAIROS_KEY` or
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

use std::sync::Arc;

use kairos_client::TokenProvider;
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
}

impl McpProxyArgs {
    pub async fn run(self) -> Result<(), CliError> {
        let upstream = Arc::new(connect(self)?);
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
}

/// The deployment, the credential and the tenant of the proxy.
fn connect(args: McpProxyArgs) -> Result<Upstream, CliError> {
    let binding = project_config::binding_here();
    let env = auth::Env::from_process();
    let url = args.url.or_else(|| {
        env.url
            .is_none()
            .then(|| binding.get("deployment").map(str::to_string))
            .flatten()
    });
    let path = credentials::credentials_path()?;
    let source = auth::resolve(url.as_deref(), &env, || credentials::load(&path))?;
    let (deployment, credential, tenant) = match source {
        Source::Key {
            deployment, key, ..
        } => (deployment, Credential::Key(key), args.tenant),
        Source::Cache { deployment, store } => {
            let entry = credentials::entry_for(&store, &deployment)?;
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
    let http = reqwest::Client::builder()
        .build()
        .map_err(|e| CliError::Failure(format!("The CLI cannot make an HTTP client: {e}.")))?;
    Ok(Upstream {
        http,
        endpoint: endpoint(&deployment, args.profile),
        credential,
        tenant,
        session: Mutex::new(None),
    })
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
        match self.post(line).await {
            Ok(lines) => lines,
            // A notification gets no answer, also when it fails.
            Err(why) => match id {
                Some(id) => vec![error_line(id, -32603, why)],
                None => Vec::new(),
            },
        }
    }

    async fn post(&self, body: &str) -> Result<Vec<String>, String> {
        let token = self.token().await?;
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
        let response = request.send().await.map_err(|e| {
            format!(
                "The Kairos deployment at {} did not answer: {e}",
                self.endpoint
            )
        })?;
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
        let text = response
            .text()
            .await
            .map_err(|e| format!("The answer of {} could not be read: {e}", self.endpoint))?;
        if !status.is_success() {
            return Err(refusal(status, &self.endpoint, &text));
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

/// The text of a refused request: the status, and what to do for 401.
fn refusal(status: reqwest::StatusCode, endpoint: &str, body: &str) -> String {
    let detail = body.trim();
    let detail = if detail.is_empty() {
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
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
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
}
