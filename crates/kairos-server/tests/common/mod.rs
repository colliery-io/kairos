//! Shared integration-test helpers (KAIROS-T-0017/T-0018, restructured for
//! the KAIROS-T-0024 client-backed suite): scratch-database lifecycle
//! against the live compose Postgres, real Dex tokens via the password
//! grant, a real ephemeral-port server for `kairos_client::KairosClient`
//! (the standard way API tests talk to the router), and in-process
//! requests for the protocol-level cases that stay raw (malformed tokens,
//! raw WS handshakes, MCP, OpenAPI probes).
//!
//! Each `tests/*.rs` binary compiles this module independently and uses a
//! different subset, so unused-item lints are expected noise here.
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use kairos_client::KairosClient;
use kairos_server::config::{AppConfig, LogFormat};

/// Same default as `.angreal/task_db.py`'s `DATABASE_URL`.
pub const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The live dev/test issuer (`.angreal/dex/config.yaml`).
pub const ISSUER: &str = "http://localhost:41558/dex";

/// The audience the server accepts = the client id user tokens are minted
/// through: Dex sets `aud` to the requesting OAuth client id, so user
/// tokens are obtained via the `kairos-cli` public client (KAIROS-T-0017).
pub const AUDIENCE: &str = "kairos-cli";

/// The compose Postgres admin URL (`DATABASE_URL` env or the default).
pub fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Replace the database name (final path segment) in a postgres URL.
pub fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL must contain a database path segment");
    format!("{base}/{db_name}")
}

/// Attempts for each transient scratch-DB lifecycle op against the SHARED
/// compose Postgres (KAIROS-T-0048).
///
/// The scratch-DB lifecycle (`connect` / `DROP DATABASE ... FORCE` /
/// `CREATE DATABASE`) runs against a Postgres shared by every integration
/// binary. Under a burst of concurrent provisioning any single step can hit
/// a momentary, self-clearing error — a `too many clients` spike, a connect
/// refused while another binary's `DROP ... FORCE` terminates backends, or
/// server-side create-database contention. Single-shotting these with
/// `.expect(...)` turns such a blip into an instant early-setup panic before
/// any assertion runs (the T-0048 flake signature). Bounded backoff absorbs
/// the blip without touching any product code or weakening any assertion; a
/// genuinely-down stack still fails fast enough (a few hundred ms total).
const DB_SETUP_ATTEMPTS: u32 = 6;

/// Base backoff between scratch-DB retry attempts; doubles each attempt,
/// capped at [`DB_SETUP_BACKOFF_CAP`].
const DB_SETUP_BACKOFF_BASE: std::time::Duration = std::time::Duration::from_millis(50);

/// Ceiling on the per-attempt backoff.
const DB_SETUP_BACKOFF_CAP: std::time::Duration = std::time::Duration::from_secs(2);

/// Run a transient shared-Postgres op with bounded exponential backoff,
/// panicking (with the last error) only after [`DB_SETUP_ATTEMPTS`] failures.
fn retry_db<T, E: std::fmt::Display>(what: &str, mut op: impl FnMut() -> Result<T, E>) -> T {
    let mut backoff = DB_SETUP_BACKOFF_BASE;
    for attempt in 1..=DB_SETUP_ATTEMPTS {
        match op() {
            Ok(value) => return value,
            Err(e) if attempt < DB_SETUP_ATTEMPTS => {
                eprintln!(
                    "[test-harness] {what} failed (attempt {attempt}/{DB_SETUP_ATTEMPTS}): {e}; \
                     retrying in {backoff:?}"
                );
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(DB_SETUP_BACKOFF_CAP);
            }
            Err(e) => panic!("{what} failed after {DB_SETUP_ATTEMPTS} attempts: {e}"),
        }
    }
    unreachable!("loop returns or panics on the final attempt")
}

/// Connect to the shared compose Postgres admin database, retrying transient
/// failures ([`retry_db`]).
fn connect_admin() -> PgConnection {
    let admin_url = admin_database_url();
    retry_db(
        &format!(
            "connect to compose postgres at {admin_url} (is the stack up? `angreal services up`)"
        ),
        || PgConnection::establish(&admin_url),
    )
}

/// Drop (if present) and recreate the uniquely named scratch database on
/// the shared compose server (shared-services discipline: each test binary
/// owns its own database and nothing else). Returns the admin connection
/// for later teardown.
///
/// Every step retries transient shared-Postgres errors with bounded backoff
/// (KAIROS-T-0048): the target database name is unique per binary, so the
/// `DROP`/`CREATE` pair is idempotent under retry and re-running it after a
/// partial failure never clobbers another binary's state.
pub fn recreate_scratch_db(scratch_db: &str) -> PgConnection {
    let mut admin_conn = connect_admin();
    retry_db("dropping scratch database", || {
        sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
            .execute(&mut admin_conn)
    });
    retry_db("creating scratch database", || {
        sql_query(format!("CREATE DATABASE {scratch_db}")).execute(&mut admin_conn)
    });
    admin_conn
}

/// Drop the scratch database after the test (retries transient errors,
/// KAIROS-T-0048).
pub fn drop_scratch_db(admin_conn: &mut PgConnection, scratch_db: &str) {
    retry_db("dropping scratch database after test", || {
        sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)")).execute(admin_conn)
    });
}

/// Obtain a real token from the live Dex via the password grant.
pub async fn dex_token(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: Option<&str>,
    username: &str,
    password: &str,
) -> String {
    let mut form = vec![
        ("grant_type", "password"),
        ("username", username),
        ("password", password),
        ("scope", "openid email profile"),
        ("client_id", client_id),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    let response: Value = http
        .post(format!("{ISSUER}/token"))
        .form(&form)
        .send()
        .await
        .unwrap_or_else(|e| panic!("cannot reach Dex at {ISSUER}: {e} (is the stack up?)"))
        .error_for_status()
        .unwrap_or_else(|e| panic!("password grant for {username} failed: {e}"))
        .json()
        .await
        .expect("token response is JSON");
    response["access_token"]
        .as_str()
        .expect("access_token present")
        .to_string()
}

/// A user token from the seeded `kairos-cli` public client
/// (`.angreal/dex/config.yaml` static passwords are `{user}-password`).
pub async fn user_token(http: &reqwest::Client, user: &str) -> String {
    dex_token(
        http,
        "kairos-cli",
        None,
        &format!("{user}@kairos.test"),
        &format!("{user}-password"),
    )
    .await
}

/// The standard test config: multi-tenant with `kairos.test` as the base
/// domain (tests usually resolve tenants via `X-Tenant`).
pub fn base_config(scratch_url: &str) -> AppConfig {
    AppConfig {
        database_url: scratch_url.to_string(),
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        oidc_issuer_url: Some(ISSUER.to_string()),
        oidc_audience: Some(AUDIENCE.to_string()),
        base_domain: Some("kairos.test".to_string()),
        single_tenant: None,
        deployment_admins: vec![],
        log_level: "info".to_string(),
        log_format: LogFormat::Json,
        // The dev Swagger UI is opt-in (KAIROS-T-0023); tests that need it
        // flip this on a copy.
        dev_ui: false,
        // GUI serving (KAIROS-T-0039): no dist dir by default (tests that
        // exercise the SPA fallback set one on a copy).
        web_dist: None,
        // No background refresher in tests: they assert on exact embedding
        // counts, and a sweep running underneath would make those flaky.
        embed_refresh_secs: 0,
        web_client_id: "kairos-web".to_string(),
        api_bearer: kairos_server::config::ApiBearer::AccessToken,
        web_client_secret: None,
        // Forge integration (KAIROS-T-0097): configured by default so the
        // connection surface is exercisable; tests that need the
        // unconfigured 501 path clear these on a copy.
        public_url: Some("https://kairos.test".to_string()),
        webhook_signing_key: Some("test-webhook-signing-key".to_string()),
        // KAIROS-T-0196: no trace export in tests. Spans are still created — the
        // request span is unconditional — they just go nowhere, which is what an
        // unconfigured deployment does too.
        otel_endpoint: None,
        otel_sample_ratio: 1.0,
        auth_max_failures: 5,
        auth_failure_window_secs: 300,
        auth_lockout_secs: 60,
        trusted_proxy: false,
        local_auth: false,
        session_ttl_secs: 14 * 24 * 60 * 60,
        bootstrap_admin: None,
        bootstrap_password: None,
        bootstrap_password_hash: None,
    }
}

/// One in-process request against the production router: any method/URI,
/// optional bearer token, extra headers, optional JSON body. Returns the
/// status and the parsed JSON body (`Null` for empty bodies).
pub async fn request(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("non-JSON body: {e}"))
    };
    (status, body)
}

/// The `error.code` of an S-0005 error envelope (panics on any other
/// shape).
pub fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or_else(|| {
        panic!("expected S-0005 error envelope, got: {body}");
    })
}

/// A live instance of the production router on an ephemeral local port —
/// what `KairosClient`-backed tests (KAIROS-T-0024) talk to. The serve
/// task lives until the test process exits (tests are one `#[tokio::test]`
/// each, so nothing outlives its runtime).
pub struct TestServer {
    /// The bound address (`127.0.0.1:{port}`).
    pub addr: std::net::SocketAddr,
    /// `http://127.0.0.1:{port}`.
    pub base_url: String,
}

/// Serve `router` on an ephemeral 127.0.0.1 port.
pub async fn spawn_server(router: Router) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("test server");
    });
    TestServer {
        addr,
        base_url: format!("http://{addr}"),
    }
}

impl TestServer {
    /// A typed client for this server: fixed bearer token, tenant via the
    /// `X-Tenant` fallback.
    pub fn client(&self, token: &str, tenant: &str) -> KairosClient {
        KairosClient::with_static_token(&self.base_url, token).with_tenant(tenant)
    }

    /// A typed client WITHOUT tenant resolution (cross-tenant
    /// deployment-admin routes, tenant-failure probes).
    pub fn client_untenanted(&self, token: &str) -> KairosClient {
        KairosClient::with_static_token(&self.base_url, token)
    }
}
