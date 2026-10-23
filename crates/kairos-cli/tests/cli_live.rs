//! KAIROS-T-0036: the `kairos` binary end-to-end against the LIVE compose
//! stack — Dex (device grant driven HEADLESSLY through its HTML forms) and
//! the production router booted in-process on an ephemeral port over the
//! uniquely named scratch database `kairos_cli_m4_test`.
//!
//! Covered, in one flow (KAIROS-A-0015 exit codes asserted throughout):
//!
//! | phase | behavior                                                        |
//! |-------|-----------------------------------------------------------------|
//! | login | RFC 9728 issuer discovery → device grant → cache 0600, keyed    |
//! | whoami| user/org/role/teams via kairos-client, `--json` and human form  |
//! | refresh| expired `expires_at` → transparent refresh grant, cache rewritten |
//! | refresh failure | bad refresh token → exit 2 with re-login instruction |
//! | corrupted cache | garbage credentials.json → exit 2, actionable       |
//! | logout| entry cleared; whoami afterwards → exit 2 "not logged in"       |
//! | unreachable | login against a dead port → exit 1, actionable          |
//!
//! The headless approval mirrors a browser against Dex: POST the user code
//! to `/device/auth/verify_code` (redirects — followed as GET like a
//! browser — to the local-connector login form), then POST alice's
//! credentials to the form's URL, landing on `/device/callback`.

#[path = "../../kairos-db/tests/common/scratch.rs"]
mod scratch;

use std::process::Stdio;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use tokio::io::{AsyncBufReadExt, BufReader};

use kairos_client::types::CreateTaskRequest;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::{Error, KairosClient};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::config::{AppConfig, LogFormat};
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary (shared-services
/// discipline: own the database, nothing else).
static SCRATCH_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_cli_m4_test");
/// The live dev/test issuer (`.angreal/dex/config.yaml`).
const ISSUER: &str = "http://localhost:41558/dex";
/// Tokens are minted through the `kairos-cli` public client, so `aud` is
/// `kairos-cli` (KAIROS-T-0017).
const AUDIENCE: &str = "kairos-cli";
const TENANT: &str = "cli_m4";
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("database url has a path");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

/// Run the `kairos` binary with a key in `var` and `KAIROS_URL`, and no other
/// key in its environment; returns (exit_code, stdout, stderr).
async fn run_cli_with_key(
    config_dir: &std::path::Path,
    url: &str,
    var: &str,
    key: &str,
    args: &[&str],
) -> (i32, String, String) {
    run_cli_with_key_env(config_dir, url, var, key, args, &[]).await
}

/// [`run_cli_with_key`] with the variables of `env` set (KAIROS-T-0594).
async fn run_cli_with_key_env(
    config_dir: &std::path::Path,
    url: &str,
    var: &str,
    key: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> (i32, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(args)
        .env("KAIROS_CONFIG_DIR", config_dir)
        .env_remove("KAIROS_KEY")
        .env_remove("KAIROS_MCP_KEY")
        .env_remove("KAIROS_QUIET")
        .env("KAIROS_URL", url)
        .env(var, key)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .output()
        .await
        .expect("running the kairos binary");
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Run the `kairos` binary with `KAIROS_CONFIG_DIR` pointed at the test's
/// scratch config dir; returns (exit_code, stdout, stderr).
async fn run_cli(config_dir: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(args)
        .env("KAIROS_CONFIG_DIR", config_dir)
        // The key of a person's own shell must not stand in for the login
        // under test (KAIROS_KEY wins over the cache).
        .env_remove("KAIROS_KEY")
        .env_remove("KAIROS_MCP_KEY")
        .env_remove("KAIROS_URL")
        .env_remove("KAIROS_QUIET")
        .stdin(Stdio::null())
        .output()
        .await
        .expect("running the kairos binary");
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Approve a pending device grant headlessly: submit the user code, then
/// alice's credentials, exactly as a browser would.
async fn approve_device_grant(user_code: &str) {
    // Redirect-following reqwest client (302/303 continue as GET, like a
    // browser); cookies on in case Dex sets any.
    let http = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .expect("building the approval client");

    // 1. Submit the user code; land (via redirects) on the login form.
    let response = http
        .post(format!("{ISSUER}/device/auth/verify_code"))
        .form(&[("user_code", user_code)])
        .send()
        .await
        .expect("submitting the user code");
    assert!(
        response.status().is_success(),
        "verify_code failed: {}",
        response.status()
    );
    let login_url = response.url().clone();
    let body = response.text().await.expect("login form body");
    assert!(
        login_url.path().contains("/auth"),
        "expected to land on the connector login form, got {login_url}\n{body}"
    );

    // 2. Submit alice's credentials to the login form URL.
    let response = http
        .post(login_url)
        .form(&[
            ("login", "alice@kairos.test"),
            ("password", "alice-password"),
        ])
        .send()
        .await
        .expect("submitting credentials");
    let final_url = response.url().clone();
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    assert!(
        status.is_success() && final_url.path().contains("/device/callback"),
        "device approval did not complete: {status} at {final_url}\n{body}"
    );
}

/// A real alice token via the password grant (JIT provisioning + fixture
/// setup only; the CLI itself earns its tokens through the device grant).
async fn alice_password_token() -> String {
    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("{ISSUER}/token"))
        .form(&[
            ("grant_type", "password"),
            ("username", "alice@kairos.test"),
            ("password", "alice-password"),
            ("scope", "openid email profile"),
            ("client_id", AUDIENCE),
        ])
        .send()
        .await
        .expect("reaching live Dex (is the stack up? `angreal services up`)")
        .error_for_status()
        .expect("password grant")
        .json()
        .await
        .expect("token JSON");
    response["access_token"]
        .as_str()
        .expect("access_token")
        .to_string()
}

#[tokio::test]
async fn cli_login_whoami_refresh_logout_live() {
    // --- scratch database + tenant -----------------------------------------
    let admin_url = admin_database_url();
    let mut admin_conn = PgConnection::establish(&admin_url)
        .expect("connecting to compose postgres (is the stack up? `angreal services up`)");
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db");
    sql_query(format!("CREATE DATABASE {SCRATCH_DB}"))
        .execute(&mut admin_conn)
        .expect("creating scratch db");
    let _scratch_guard = scratch::ScratchGuard::new(&SCRATCH_DB);
    let scratch_url = with_database(&admin_url, &SCRATCH_DB);

    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch db");
    run_public_migrations(&mut conn).expect("public migrations");
    provision_tenant(&mut conn, TENANT, "CLI M4 Test Org").expect("provisioning tenant");
    let org_id: uuid::Uuid = organizations::table
        .filter(organizations::slug.eq(TENANT))
        .select(organizations::id)
        .first(&mut conn)
        .expect("org row");

    // --- the production router on an ephemeral port -------------------------
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = std::sync::Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let config = AppConfig {
        database_url: scratch_url.clone(),
        pool_size: 16,
        pool_min_idle: 0,
        job_pool_size: 6,
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        oidc_issuer_url: Some(ISSUER.to_string()),
        oidc_audience: Some(AUDIENCE.to_string()),
        base_domain: Some("kairos.test".to_string()),
        single_tenant: None,
        single_tenant_name: None,
        auto_join_domains: vec![],
        deployment_admins: vec![],
        log_level: "info".to_string(),
        log_format: LogFormat::Json,
        // No background embedding sweep in a CLI test fixture.
        embed_refresh_secs: 0,
        code_index_dir: None,
        role: Default::default(),
        code_index_poll_secs: 0,
        code_index_threads: 4,
        code_index_min_rebuild_secs: 0,
        code_index_deps_cap_mb: 10240,
        code_index_min_free_mb: 5120,
        code_index_deps_env: Vec::new(),
        code_index_tool_downloads: false,
        dev_ui: false,
        // GUI serving fields (KAIROS-T-0039): irrelevant to the CLI suite.
        web_dist: None,
        web_client_id: "kairos-web".to_string(),
        api_bearer: kairos_server::config::ApiBearer::AccessToken,
        web_client_secret: None,
        public_url: None,
        webhook_signing_key: None,
        secrets_key: None,

        otel_endpoint: None,

        otel_sample_ratio: 1.0,
        auth_max_failures: 5,
        auth_failure_window_secs: 300,
        auth_lockout_secs: 60,
        auth_cache_secs: 60,
        request_timeout_secs: 30,
        trusted_proxy: false,
        local_auth: false,
        session_ttl_secs: 14 * 24 * 60 * 60,
        api_key_ttl_days: 90,
        stale_claim_days: 14,
        api_key_rotation_grace_secs: 24 * 60 * 60,
        jobs: Default::default(),
        webhooks: Default::default(),
        writeback: Default::default(),
        cli_download_url: None,
        pages: Default::default(),
        bootstrap_admin: None,
        bootstrap_password: None,
        bootstrap_password_hash: None,
    };
    let router = app::router(app::state_with(config, pool, auth));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral port");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("test server");
    });

    // --- fixture: alice JIT-provisioned, org admin, on one team -------------
    let alice_token = alice_password_token().await;
    let alice = KairosClient::with_static_token(&base_url, &alice_token).with_tenant(TENANT);
    match alice.whoami().await {
        Err(Error::Forbidden { code, .. }) => assert_eq!(code, "MEMBERSHIP_REQUIRED"),
        other => panic!("expected MEMBERSHIP_REQUIRED before the grant, got {other:?}"),
    }
    let alice_id: uuid::Uuid = users::table
        .filter(users::email.eq("alice@kairos.test"))
        .select(users::id)
        .first(&mut conn)
        .expect("alice JIT-provisioned");
    diesel::insert_into(organization_members::table)
        .values(NewOrganizationMember {
            organization_id: org_id,
            user_id: alice_id,
            role: OrgRole::Admin,
        })
        .execute(&mut conn)
        .expect("granting membership");
    let team = alice
        .create_team(&CreateTeamRequest {
            name: "Platform".into(),
            slug: "platform".into(),
            code_prefix: "PLATFORM".into(),
            team_type: None,
        })
        .await
        .expect("creating the platform team");
    alice
        .add_team_member(
            &team.id,
            &AddTeamMemberRequest {
                user_id: alice_id.to_string(),
            },
        )
        .await
        .expect("adding alice to the team");

    // --- phase A: kairos login (headless device grant) ----------------------
    let config_dir = tempfile::tempdir().expect("temp config dir");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(["login", "--url", &base_url, "--tenant", TENANT])
        .env("KAIROS_CONFIG_DIR", config_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning kairos login");

    let stdout = child.stdout.take().expect("stdout piped");
    let mut lines = BufReader::new(stdout).lines();
    let mut login_output = Vec::new();
    let mut user_code = None;
    while let Some(line) = lines.next_line().await.expect("reading login output") {
        // "To sign in, open: http://…/device?user_code=XXXX-XXXX"
        if let Some((_, code)) = line.split_once("user_code=") {
            user_code = Some(code.trim().to_string());
        }
        let done = user_code.is_some();
        login_output.push(line);
        if done {
            break;
        }
    }
    let user_code = user_code
        .unwrap_or_else(|| panic!("login never printed a verification URL: {login_output:#?}"));
    assert!(
        login_output
            .iter()
            .any(|l| l.contains(&format!("The OIDC issuer of the deployment is: {ISSUER}"))),
        "RFC 9728 discovery not reported: {login_output:#?}"
    );

    approve_device_grant(&user_code).await;

    // Drain the rest of stdout and wait for exit 0.
    while let Some(line) = lines.next_line().await.expect("draining login output") {
        login_output.push(line);
    }
    let status = tokio::time::timeout(Duration::from_secs(120), child.wait())
        .await
        .expect("login finished in time")
        .expect("login exit status");
    assert_eq!(status.code(), Some(0), "login failed: {login_output:#?}");
    let login_text = login_output.join("\n");
    assert!(login_text.contains("enter code:"), "{login_text}");
    assert!(
        login_text.contains(&format!("You are logged in to {base_url}")),
        "{login_text}"
    );

    // The cache: 0600, keyed by deployment URL, full entry.
    let credentials_path = config_dir.path().join("credentials.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&credentials_path)
            .expect("credentials.json written")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "credentials.json must be chmod 600");
    }
    let cache: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&credentials_path).expect("cache"))
            .expect("cache is JSON");
    let entry = &cache["deployments"][&base_url];
    assert_eq!(entry["issuer"], ISSUER, "cache: {cache:#}");
    assert_eq!(entry["client_id"], "kairos-cli");
    assert_eq!(entry["tenant"], TENANT);
    assert!(entry["access_token"].is_string());
    assert!(
        entry["refresh_token"].is_string(),
        "offline_access must yield a refresh token: {cache:#}"
    );
    let first_expiry = entry["expires_at"].as_u64().expect("expires_at");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    assert!(first_expiry > now, "expiry must be in the future");

    // --- phase B: whoami (--json and human) ---------------------------------
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["whoami", "--json"]).await;
    assert_eq!(code, 0, "whoami --json failed: {stderr}");
    let identity: serde_json::Value = serde_json::from_str(&stdout).expect("whoami JSON");
    assert_eq!(identity["user"]["email"], "alice@kairos.test");
    assert_eq!(identity["organization"]["slug"], TENANT);
    assert_eq!(identity["organization"]["role"], "admin");
    assert_eq!(identity["teams"][0]["slug"], "platform");

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["whoami"]).await;
    assert_eq!(code, 0, "whoami failed: {stderr}");
    assert!(stdout.contains("alice@kairos.test"), "{stdout}");
    assert!(
        stdout.contains(&format!("{TENANT} (role: admin)")),
        "{stdout}"
    );
    assert!(stdout.contains("Platform"), "{stdout}");
    // KAIROS-T-0594: the source of the credential.
    assert!(
        stdout.contains(&format!("credential: login cached for {base_url}")),
        "{stdout}"
    );

    // --- phase B1: KAIROS-T-0630, a note and `get` -------------------------
    let task = alice
        .create_task(&CreateTaskRequest {
            board_id: Some("platform-delivery".into()),
            column_id: None,
            title: "Notes in the CLI".into(),
            content: "# Notes\n\nThe body.\n".into(),
            task_type: None,
            work_class: None,
            team_id: None,
            repository: None,
            from_team: None,
            parent: None,
        })
        .await
        .expect("creating a task");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "note",
            &task.short_code,
            "--text",
            "Found it.\n\n## Findings\n\nA heading in a note.",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks note failed: {stderr}");
    assert_eq!(
        stdout.trim(),
        format!(
            "Kairos appended note n-1 to {}. The content stays at version 1.",
            task.short_code
        )
    );
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["tasks", "get", &task.short_code]).await;
    assert_eq!(code, 0, "tasks get failed: {stderr}");
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    assert!(
        stdout.contains(&format!(
            "# Notes\n\nThe body.\n\n## Note n-1 ({today}, alice"
        )) && stdout.contains("Found it.\n\n## Findings\n\nA heading in a note.\n"),
        "the note follows the content: {stdout}"
    );
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "get", &task.short_code, "--json"],
    )
    .await;
    assert_eq!(code, 0, "tasks get --json failed: {stderr}");
    let got: serde_json::Value = serde_json::from_str(&stdout).expect("task JSON");
    assert_eq!(got["content"], "# Notes\n\nThe body.\n", "{got}");
    assert_eq!(got["notes"][0]["id"], "n-1", "{got}");
    assert_eq!(
        got["notes"][0]["text"], "Found it.\n\n## Findings\n\nA heading in a note.",
        "{got}"
    );

    // --- phase B1b: KAIROS-T-0670, the interval of the issue import -------------
    alice
        .create_repository(
            &kairos_client::types_repositories::CreateRepositoryRequest {
                slug: Some("cadence-app".into()),
                forge: "github".into(),
                repo_full_name: "acme/cadence-app".into(),
                repo_url: "https://github.com/acme/cadence-app".into(),
                default_branch: None,
                team: team.id.clone(),
                description: None,
            },
        )
        .await
        .expect("a repository");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "repos",
            "update",
            "cadence-app",
            "--import-interval",
            "often",
        ],
    )
    .await;
    assert_ne!(code, 0, "a bad interval is refused: {stdout}");
    assert!(
        stderr.contains("issue_import_interval") || stderr.contains("\"often\""),
        "the refusal names the value: {stderr}"
    );
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["repos", "update", "cadence-app", "--import-interval", "30s"],
    )
    .await;
    assert_ne!(code, 0);
    assert!(stderr.contains("shortest"), "{stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "update", "cadence-app", "--import-interval", "15m"],
    )
    .await;
    assert_eq!(code, 0, "repos update failed: {stderr}");
    assert!(
        stdout.contains("changed the repository cadence-app"),
        "{stdout}"
    );
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["repos", "get", "cadence-app"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("issue import:   every 15m;"), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "import-status", "cadence-app"],
    )
    .await;
    assert_eq!(code, 0, "import-status failed: {stderr}");
    for needle in [
        "no import job has run yet",
        "Interval: 15m",
        "last complete run: none",
        "next run:",
        "issues in the record: 0",
    ] {
        assert!(stdout.contains(needle), "{needle}: {stdout}");
    }
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "update", "cadence-app", "--import-interval", "off"],
    )
    .await;
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("changed the repository"), "{stdout}");
    let (code, stdout, _) = run_cli(
        config_dir.path(),
        &["repos", "get", "cadence-app", "--json"],
    )
    .await;
    assert_eq!(code, 0);
    let got: serde_json::Value = serde_json::from_str(&stdout).expect("repository JSON");
    assert_eq!(got["issue_import_interval"], "off", "{got}");

    // --- phase B2: an agent key in KAIROS_KEY stands in for the login ------
    // A config dir with no cache: the key and KAIROS_URL are all there is.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["keys", "create", "--name", "cli-live-key", "--json"],
    )
    .await;
    assert_eq!(code, 0, "keys create failed: {stderr}");
    let created: serde_json::Value = serde_json::from_str(&stdout).expect("key JSON");
    let key = created["key"].as_str().expect("the raw key").to_string();
    let empty_dir = tempfile::tempdir().expect("empty config dir");
    let (code, stdout, stderr) = run_cli_with_key(
        empty_dir.path(),
        &base_url,
        "KAIROS_KEY",
        &key,
        &["whoami", "--json"],
    )
    .await;
    assert_eq!(code, 0, "whoami with KAIROS_KEY failed: {stderr}");
    let identity: serde_json::Value = serde_json::from_str(&stdout).expect("whoami JSON");
    assert_eq!(identity["user"]["email"], "alice@kairos.test");
    assert!(
        !empty_dir.path().join("credentials.json").exists(),
        "a key must not write the cache"
    );
    // KAIROS-T-0594: a key that hides the login of the same deployment.
    // whoami names the key, and stderr has one warning line.
    let (code, stdout, stderr) = run_cli_with_key(
        config_dir.path(),
        &base_url,
        "KAIROS_MCP_KEY",
        &key,
        &["whoami"],
    )
    .await;
    assert_eq!(code, 0, "whoami with a key and a login failed: {stderr}");
    assert!(
        stdout.contains("credential: key from KAIROS_MCP_KEY"),
        "{stdout}"
    );
    let warning = format!(
        "warning: KAIROS_MCP_KEY is set, so the CLI sends that key and not the login for \
         {base_url}. Set KAIROS_QUIET=1 to hide this warning."
    );
    assert_eq!(
        stderr.lines().filter(|line| *line == warning).count(),
        1,
        "{stderr}"
    );
    assert!(!stderr.contains(&key), "{stderr}");

    // --- phase B2b: a [[project]] of config.toml names the deployment ------
    // #270: no KAIROS_URL and no file in the folder. The project of the
    // folder in the config of the person gives the deployment, to a command
    // and to `kairos mcp-proxy`, the `kairos` MCP server of the plugin.
    project_config_and_mcp_proxy(&base_url, &key).await;

    let (code, _, stderr) = run_cli_with_key_env(
        config_dir.path(),
        &base_url,
        "KAIROS_MCP_KEY",
        &key,
        &["whoami"],
        &[("KAIROS_QUIET", "1")],
    )
    .await;
    assert_eq!(code, 0, "whoami with KAIROS_QUIET failed: {stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
    // No login in the config dir: no warning.
    let (code, _, stderr) = run_cli_with_key(
        empty_dir.path(),
        &base_url,
        "KAIROS_MCP_KEY",
        &key,
        &["whoami"],
    )
    .await;
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");

    // A key that the deployment refuses names its variable, not `kairos login`.
    let (code, _, stderr) = run_cli_with_key(
        empty_dir.path(),
        &base_url,
        "KAIROS_MCP_KEY",
        "kairos_sk_not_a_key",
        &["whoami"],
    )
    .await;
    assert_eq!(code, 2, "a refused key is an auth error: {stderr}");
    assert!(stderr.contains("KAIROS_MCP_KEY"), "{stderr}");
    assert!(!stderr.contains("kairos_sk_not_a_key"), "{stderr}");

    // --- phase C: automatic refresh on expiry --------------------------------
    let mut cache: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&credentials_path).expect("cache"))
            .expect("cache JSON");
    let old_access = cache["deployments"][&base_url]["access_token"]
        .as_str()
        .expect("access token")
        .to_string();
    cache["deployments"][&base_url]["expires_at"] = serde_json::json!(1);
    std::fs::write(&credentials_path, cache.to_string()).expect("forcing expiry");

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["whoami", "--json"]).await;
    assert_eq!(code, 0, "whoami after forced expiry must refresh: {stderr}");
    let identity: serde_json::Value = serde_json::from_str(&stdout).expect("whoami JSON");
    assert_eq!(identity["user"]["email"], "alice@kairos.test");

    let refreshed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&credentials_path).expect("cache"))
            .expect("cache JSON");
    let refreshed_entry = &refreshed["deployments"][&base_url];
    assert_ne!(
        refreshed_entry["access_token"].as_str().expect("token"),
        old_access,
        "the refresh grant must mint a new access token"
    );
    assert!(
        refreshed_entry["expires_at"].as_u64().expect("expires_at") > now,
        "the rewritten cache must carry the new expiry"
    );
    assert!(refreshed_entry["refresh_token"].is_string());

    // --- phase D: refresh failure → exit 2 with a re-login instruction ------
    let mut cache = refreshed;
    cache["deployments"][&base_url]["expires_at"] = serde_json::json!(1);
    cache["deployments"][&base_url]["refresh_token"] = serde_json::json!("garbage-refresh");
    std::fs::write(&credentials_path, cache.to_string()).expect("breaking the refresh token");
    let valid_backup = std::fs::read_to_string(&credentials_path).expect("backup");

    let (code, _, stderr) = run_cli(config_dir.path(), &["whoami"]).await;
    assert_eq!(code, 2, "a failed refresh is an auth error: {stderr}");
    assert!(stderr.contains("could not refresh it"), "{stderr}");
    assert!(stderr.contains("kairos login --url"), "{stderr}");

    // --- phase E: corrupted cache → exit 2, actionable -----------------------
    std::fs::write(&credentials_path, "{this is not json").expect("corrupting cache");
    let (code, _, stderr) = run_cli(config_dir.path(), &["whoami"]).await;
    assert_eq!(code, 2, "corrupted cache is an auth error: {stderr}");
    assert!(stderr.contains("corrupted"), "{stderr}");
    assert!(stderr.contains("kairos login"), "{stderr}");

    // --- phase F: logout ------------------------------------------------------
    std::fs::write(&credentials_path, valid_backup).expect("restoring cache");
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["logout"]).await;
    assert_eq!(code, 0, "logout failed: {stderr}");
    assert!(
        stdout.contains(&format!("You are logged out of {base_url}")),
        "{stdout}"
    );
    let cache: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&credentials_path).expect("cache"))
            .expect("cache JSON");
    assert!(
        cache["deployments"]
            .as_object()
            .expect("deployments")
            .is_empty(),
        "logout must clear the entry: {cache:#}"
    );

    let (code, _, stderr) = run_cli(config_dir.path(), &["whoami"]).await;
    assert_eq!(code, 2, "whoami after logout is an auth error");
    assert!(stderr.contains("kairos login"), "{stderr}");

    // --- phase G: unreachable deployment → exit 1, actionable ----------------
    let (code, _, stderr) =
        run_cli(config_dir.path(), &["login", "--url", "http://127.0.0.1:9"]).await;
    assert_eq!(code, 1, "unreachable deployment is an API-level failure");
    assert!(stderr.contains("cannot reach the deployment"), "{stderr}");

    // --- teardown -------------------------------------------------------------
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db after test");
}

/// #270: a folder bound to the deployment by a `[[project]]` of
/// `config.toml`, with a key and no KAIROS_URL. `kairos whoami` in the
/// folder finds the deployment; `kairos mcp-proxy` forwards MCP over stdio
/// to `/mcp` of it: initialize, the initialized notification, tools/list
/// and a call of `whoami`.
async fn project_config_and_mcp_proxy(base_url: &str, key: &str) {
    let config = tempfile::tempdir().expect("a config dir");
    let project = tempfile::tempdir().expect("a project folder");
    let project = project.path().canonicalize().expect("the project folder");
    std::fs::write(
        config.path().join("config.toml"),
        format!(
            "[default]\ndeployment = \"http://127.0.0.1:9\"\n\n\
             [[project]]\npath = {:?}\ndeployment = {base_url:?}\n",
            project.display().to_string()
        ),
    )
    .expect("write config.toml");
    let kairos = |args: &[&str]| {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"));
        command
            .args(args)
            .current_dir(&project)
            .env("KAIROS_CONFIG_DIR", config.path())
            .env_remove("KAIROS_URL")
            .env_remove("KAIROS_KEY")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env("KAIROS_QUIET", "1")
            .env("KAIROS_UPDATE_CHECK", "0")
            .env("KAIROS_MCP_KEY", key);
        command
    };

    let shown = kairos(&["config", "show", "--json"])
        .output()
        .await
        .expect("kairos config show");
    let shown: serde_json::Value = serde_json::from_slice(&shown.stdout).expect("config JSON");
    assert_eq!(shown["bound"], true, "{shown}");
    assert_eq!(shown["values"]["deployment"]["value"], base_url, "{shown}");
    assert_eq!(shown["values"]["deployment"]["from"], "project", "{shown}");

    let whoami = kairos(&["whoami", "--json"])
        .output()
        .await
        .expect("kairos whoami");
    assert!(
        whoami.status.success(),
        "whoami with the project config failed: {}",
        String::from_utf8_lossy(&whoami.stderr)
    );
    let identity: serde_json::Value = serde_json::from_slice(&whoami.stdout).expect("whoami JSON");
    assert_eq!(identity["user"]["email"], "alice@kairos.test");

    let mut proxy = kairos(&["mcp-proxy"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("start kairos mcp-proxy");
    let mut stdin = proxy.stdin.take().expect("stdin");
    let mut lines = BufReader::new(proxy.stdout.take().expect("stdout")).lines();
    let mut send = async |message: serde_json::Value| {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .expect("write to the proxy");
    };
    let mut next = async || -> serde_json::Value {
        let line = tokio::time::timeout(Duration::from_secs(30), lines.next_line())
            .await
            .expect("an answer of the proxy in time")
            .expect("read the proxy")
            .expect("a line");
        serde_json::from_str(&line).expect("one JSON-RPC message per line")
    };

    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "cli-live", "version": "0"}},
    }))
    .await;
    let init = next().await;
    assert_eq!(init["id"], 1, "{init}");
    assert!(init["result"]["serverInfo"].is_object(), "{init}");
    send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
    send(serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let tools = next().await;
    assert_eq!(tools["id"], 2, "{tools}");
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"whoami"), "{names:?}");
    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "whoami", "arguments": {}},
    }))
    .await;
    let called = next().await;
    assert_eq!(called["id"], 3, "{called}");
    let text = called["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(text.contains("alice@kairos.test"), "{called}");
}
