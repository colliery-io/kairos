//! KAIROS-T-0037: the `kairos` command tree end-to-end against the LIVE
//! stack — Dex (device grant approved headlessly, the T-0036 recipe) and
//! the production router booted in-process on an ephemeral port over the
//! uniquely named scratch database `kairos_cli_t0037_test`.
//!
//! The golden path, all through the real binary (exit codes asserted):
//!
//! login → boards list → boards show (items by column) → initiative create
//! → task create → task transition (invalid first: the allowed-targets
//! rendering; then valid) → search finds it (flags and --query-json) →
//! task edit (stale --version → the 409 current-version rendering; then a
//! clean edit) → delete (without --confirm refused; with it, deleted) →
//! members list → orgs show → teams list.

#[path = "../../kairos-db/tests/common/scratch.rs"]
mod scratch;

use std::process::Stdio;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use tokio::io::{AsyncBufReadExt, BufReader};

use kairos_client::types_org::CreateTeamRequest;
use kairos_client::{Error, KairosClient};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::config::{AppConfig, LogFormat};
use kairos_server::middleware::auth::Authenticator;

/// Uniquely named scratch database for this test binary (shared-services
/// discipline: own the database, nothing else).
static SCRATCH_DB: scratch::ScratchName = scratch::ScratchName::new("kairos_cli_t0037_test");
/// The live dev/test issuer (`.angreal/dex/config.yaml`).
const ISSUER: &str = "http://localhost:41558/dex";
const AUDIENCE: &str = "kairos-cli";
const TENANT: &str = "cli_t37";
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("database url has a path");
    format!("{base}/{}", scratch::scratch_db_name(db_name))
}

/// Run the `kairos` binary with `KAIROS_CONFIG_DIR` pointed at the test's
/// scratch config dir; returns (exit_code, stdout, stderr).
async fn run_cli(config_dir: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(args)
        .env("KAIROS_CONFIG_DIR", config_dir)
        // No checkout around the command: the binding of the folder of the
        // test run (KAIROS-T-0707: the top of the checkout) must not apply.
        .current_dir(std::env::temp_dir())
        .env_remove("CLAUDE_PROJECT_DIR")
        // The key of a person's own shell must not stand in for the login
        // under test (KAIROS_KEY wins over the cache).
        .env_remove("KAIROS_KEY")
        .env_remove("KAIROS_MCP_KEY")
        .env_remove("KAIROS_URL")
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

/// [`run_cli`], with `input` on standard input (a pipe, not a terminal).
async fn run_cli_stdin(
    config_dir: &std::path::Path,
    args: &[&str],
    input: &str,
) -> (i32, String, String) {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(args)
        .env("KAIROS_CONFIG_DIR", config_dir)
        // No checkout around the command: the binding of the folder of the
        // test run (KAIROS-T-0707: the top of the checkout) must not apply.
        .current_dir(std::env::temp_dir())
        .env_remove("CLAUDE_PROJECT_DIR")
        // The key of a person's own shell must not stand in for the login
        // under test (KAIROS_KEY wins over the cache).
        .env_remove("KAIROS_KEY")
        .env_remove("KAIROS_MCP_KEY")
        .env_remove("KAIROS_URL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("running the kairos binary");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(input.as_bytes())
        .await
        .expect("stdin write");
    drop(stdin);
    let output = child
        .wait_with_output()
        .await
        .expect("the kairos binary runs");
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Approve a pending device grant headlessly (T-0036's proven recipe):
/// submit the user code, then alice's credentials, exactly as a browser
/// would.
async fn approve_device_grant(user_code: &str) {
    let http = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .expect("building the approval client");

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
    assert!(
        login_url.path().contains("/auth"),
        "expected the connector login form, got {login_url}"
    );

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
    assert!(
        response.status().is_success() && final_url.path().contains("/device/callback"),
        "device approval did not complete at {final_url}"
    );
}

/// A real alice token via the password grant (JIT provisioning + fixture
/// setup only; the CLI earns its own tokens through the device grant).
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

/// Drive `kairos login` with the headless device-grant approval; panics on
/// a non-zero exit.
async fn cli_login(config_dir: &std::path::Path, base_url: &str) {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(["login", "--url", base_url, "--tenant", TENANT])
        .env("KAIROS_CONFIG_DIR", config_dir)
        // No checkout around the command: the binding of the folder of the
        // test run (KAIROS-T-0707: the top of the checkout) must not apply.
        .current_dir(std::env::temp_dir())
        .env_remove("CLAUDE_PROJECT_DIR")
        // The key of a person's own shell must not stand in for the login
        // under test (KAIROS_KEY wins over the cache).
        .env_remove("KAIROS_KEY")
        .env_remove("KAIROS_MCP_KEY")
        .env_remove("KAIROS_URL")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning kairos login");

    let stdout = child.stdout.take().expect("stdout piped");
    let mut lines = BufReader::new(stdout).lines();
    let mut output = Vec::new();
    let mut user_code = None;
    while let Some(line) = lines.next_line().await.expect("reading login output") {
        if let Some((_, code)) = line.split_once("user_code=") {
            user_code = Some(code.trim().to_string());
        }
        let done = user_code.is_some();
        output.push(line);
        if done {
            break;
        }
    }
    let user_code =
        user_code.unwrap_or_else(|| panic!("login never printed a verification URL: {output:#?}"));
    approve_device_grant(&user_code).await;
    while let Some(line) = lines.next_line().await.expect("draining login output") {
        output.push(line);
    }
    let status = tokio::time::timeout(Duration::from_secs(120), child.wait())
        .await
        .expect("login finished in time")
        .expect("login exit status");
    assert_eq!(status.code(), Some(0), "login failed: {output:#?}");
}

#[tokio::test]
async fn cli_command_tree_golden_path_live() {
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
    provision_tenant(&mut conn, TENANT, "CLI T-0037 Test Org").expect("provisioning tenant");
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
        web_dist: None,
        web_client_id: "kairos-web".to_string(),
        api_bearer: kairos_server::config::ApiBearer::AccessToken,
        web_client_secret: None,
        // KAIROS-T-0592: `repos webhook add` needs a signing key and a
        // public URL. The CLI sends a bearer, so the URL changes no login.
        public_url: Some("http://kairos.test".to_string()),
        webhook_signing_key: Some("cli-t0592-webhook-signing-key".to_string()),
        // COLLIERY-T-3105: `repos credential set` needs a key.
        secrets_key: Some(kairos_server::secrets::SecretsKey::from_bytes([9u8; 32])),

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
        notice_limit_per_hour: 20,
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

    // --- fixture: alice JIT-provisioned, org admin, one team ----------------
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
        .expect("creating the platform team (and its delivery board)");
    let delivery_board_id = team.delivery_board_id.expect("delivery board created");

    // Column ids of the seeded delivery board (Backlog → Todo → ... graph;
    // Backlog → Active is NOT an edge, which the invalid transition uses).
    let board = alice
        .get_board(&delivery_board_id)
        .await
        .expect("delivery board detail");
    let column_id = |name: &str| -> String {
        board
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("column {name} on the delivery board"))
            .id
            .clone()
    };
    let backlog = column_id("Backlog");
    let todo = column_id("Todo");
    let active = column_id("Active");

    let initiative_board_id = alice
        .list_boards(Default::default())
        .await
        .expect("boards")
        .items
        .into_iter()
        .find(|b| b.slug == "initiatives")
        .expect("default initiative board")
        .id;

    // --- login (headless device grant) ---------------------------------------
    let config_dir = tempfile::tempdir().expect("temp config dir");
    cli_login(config_dir.path(), &base_url).await;

    // --- boards list / show ---------------------------------------------------
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["boards", "list"]).await;
    assert_eq!(code, 0, "boards list failed: {stderr}");
    for fragment in ["SLUG", "initiatives", "strategy", "platform-delivery"] {
        assert!(stdout.contains(fragment), "boards list: {stdout}");
    }
    // KAIROS-T-0593: one page holds all; no next offset.
    assert!(!stdout.contains("Next:"), "boards list: {stdout}");

    // A page with more rows after it names the next offset; --json prints
    // the envelope of the CLI (KAIROS-T-0644, KAIROS-A-0057).
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["boards", "list", "--limit", "1"]).await;
    assert_eq!(code, 0, "boards list --limit 1 failed: {stderr}");
    assert!(
        stdout.contains("(limit 1, offset 0)\nNext: --offset 1\n"),
        "{stdout}"
    );
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["boards", "list", "--limit", "1", "--json"],
    )
    .await;
    assert_eq!(code, 0, "boards list --json failed: {stderr}");
    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("envelope JSON");
    let mut keys: Vec<&str> = envelope
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["items", "next_offset", "total"], "{stdout}");
    assert_eq!(envelope["next_offset"], 1, "{stdout}");
    assert_eq!(
        envelope["items"].as_array().map(Vec::len),
        Some(1),
        "{stdout}"
    );
    let boards_total = envelope["total"].as_i64().expect("total");

    // The last page has a null next_offset.
    let last = (boards_total - 1).to_string();
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "boards", "list", "--limit", "1", "--offset", &last, "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "boards list of the last page failed: {stderr}");
    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("envelope JSON");
    assert!(envelope["next_offset"].is_null(), "{stdout}");

    // --all reads each page: each board, no next offset.
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["boards", "list", "--all", "--json"]).await;
    assert_eq!(code, 0, "boards list --all failed: {stderr}");
    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("envelope JSON");
    assert_eq!(
        envelope["items"].as_array().map(Vec::len),
        usize::try_from(boards_total).ok(),
        "{stdout}"
    );
    assert!(envelope["next_offset"].is_null(), "{stdout}");
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["boards", "list", "--all"]).await;
    assert_eq!(code, 0, "boards list --all failed: {stderr}");
    assert!(
        stdout.ends_with(&format!("total: {boards_total}\n")),
        "{stdout}"
    );
    // --all with --limit is a usage error.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["boards", "list", "--all", "--limit", "1"],
    )
    .await;
    assert_eq!(code, 64, "--all with --limit: {stderr}");

    // A list that the server does not page has the same envelope.
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["grants", "list", "--json"]).await;
    assert_eq!(code, 0, "grants list --json failed: {stderr}");
    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("envelope JSON");
    assert!(envelope["items"].is_array(), "{stdout}");
    assert_eq!(
        envelope["total"].as_u64(),
        envelope["items"].as_array().map(|a| a.len() as u64),
        "{stdout}"
    );
    assert!(envelope["next_offset"].is_null(), "{stdout}");

    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["boards", "show", &delivery_board_id]).await;
    assert_eq!(code, 0, "boards show failed: {stderr}");
    assert!(stdout.contains("== Backlog"), "{stdout}");
    assert!(stdout.contains("== Todo"), "{stdout}");
    assert!(stdout.contains("level delivery"), "{stdout}");

    // --- initiative create (human rendering) ----------------------------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "initiatives",
            "create",
            "--board",
            &initiative_board_id,
            "--title",
            "CLI golden path initiative",
            "--content",
            "Planned by the T-0037 golden path.",
        ],
    )
    .await;
    assert_eq!(code, 0, "initiatives create failed: {stderr}");
    assert!(
        stdout.starts_with("Kairos made the initiative "),
        "unexpected create rendering: {stdout}"
    );
    let initiative_code = stdout
        .split_whitespace()
        .nth(4)
        .expect("initiative short code")
        .to_string();
    assert!(initiative_code.contains("-I-"), "{initiative_code}");

    // --- task create (--json emits the raw DTO) --------------------------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--board",
            &delivery_board_id,
            "--title",
            "Golden path task",
            "--type",
            "bug",
            "--content",
            "Reproduce, fix, verify.",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create failed: {stderr}");
    let task: serde_json::Value = serde_json::from_str(&stdout).expect("task DTO JSON");
    let task_code = task["short_code"].as_str().expect("short_code").to_string();
    assert_eq!(task["task_type"], "bug");
    assert_eq!(task["column_id"].as_str(), Some(backlog.as_str()));
    assert_eq!(task["version"], 1);

    // The board view now shows the task in Backlog.
    let (code, stdout, _) =
        run_cli(config_dir.path(), &["boards", "show", &delivery_board_id]).await;
    assert_eq!(code, 0);
    let backlog_section = stdout
        .split("== ")
        .find(|s| s.starts_with("Backlog"))
        .expect("Backlog section");
    assert!(backlog_section.contains(&task_code), "{stdout}");

    // --- transition: invalid first (Backlog → Active is not an edge) -----------
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "transition", &task_code, "--to", &active],
    )
    .await;
    assert_eq!(code, 1, "invalid transition must exit 1: {stderr}");
    assert!(
        stderr.contains("The transition is not correct (422)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("The item can move to these columns:"),
        "{stderr}"
    );
    assert!(stderr.contains(&format!("Todo ({todo})")), "{stderr}");

    // ... then valid (Backlog → Todo).
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "transition", &task_code, "--to", &todo],
    )
    .await;
    assert_eq!(code, 0, "valid transition failed: {stderr}");
    assert_eq!(
        stdout.trim(),
        format!("Kairos moved the task {task_code} to the column Todo.")
    );

    // --- KAIROS-T-0557: history prints the versions and the activity --------
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["tasks", "history", &task_code]).await;
    assert_eq!(code, 0, "history failed: {stderr}");
    assert!(
        stdout.starts_with(&format!("# History of {task_code}\n- v1 — ")),
        "{stdout}"
    );
    let activity: Vec<&str> = stdout
        .lines()
        .skip_while(|line| *line != "## Activity")
        .skip(1)
        .collect();
    assert!(
        activity
            .first()
            .is_some_and(|line| line.starts_with("- moved from Backlog to Todo — ")),
        "the newest row first: {stdout}"
    );
    assert!(
        activity
            .last()
            .is_some_and(|line| line.starts_with(&format!("- created as {task_code} — "))),
        "{stdout}"
    );
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "history", &task_code, "--limit", "1", "--json"],
    )
    .await;
    assert_eq!(code, 0, "history --json failed: {stderr}");
    let history: serde_json::Value = serde_json::from_str(&stdout).expect("history JSON");
    assert_eq!(history["versions"]["total"], 1, "{stdout}");
    assert_eq!(history["activity"]["total"], 2, "{stdout}");
    assert_eq!(history["activity"]["next_offset"], 1, "{stdout}");
    assert!(history["versions"]["next_offset"].is_null(), "{stdout}");
    assert_eq!(
        history["activity"]["items"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        history["activity"]["items"][0]["text"], "moved from Backlog to Todo",
        "{stdout}"
    );

    // --- search finds it: flags, --json, and the --query-json escape hatch -----
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["search", "--query", "golden", "--type", "task"],
    )
    .await;
    assert_eq!(code, 0, "search failed: {stderr}");
    assert!(stdout.contains("tasks:"), "{stdout}");
    assert!(stdout.contains(&task_code), "{stdout}");
    assert!(
        !stdout.contains(&initiative_code),
        "--type task must filter: {stdout}"
    );

    let (code, stdout, _) = run_cli(
        config_dir.path(),
        &["search", "--query", "golden", "--json"],
    )
    .await;
    assert_eq!(code, 0);
    let results: serde_json::Value = serde_json::from_str(&stdout).expect("search JSON");
    // KAIROS-A-0057: one flat list of hits; each hit names its type.
    let hits = results["items"].as_array().expect("items");
    let hit = |kind: &str| {
        hits.iter()
            .find(|hit| hit["entity_type"] == kind)
            .map(|hit| hit["short_code"].clone())
    };
    assert_eq!(hit("task"), Some(task_code.as_str().into()), "{stdout}");
    assert_eq!(
        hit("initiative"),
        Some(initiative_code.as_str().into()),
        "{stdout}"
    );
    assert_eq!(results["total"], 2, "{stdout}");
    assert!(results["next_offset"].is_null(), "{stdout}");

    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "search",
            "--query-json",
            r#"{"q": "golden", "filter": {"entity_type": ["task"]}}"#,
        ],
    )
    .await;
    assert_eq!(code, 0, "--query-json search failed: {stderr}");
    assert!(stdout.contains(&task_code), "{stdout}");

    // KAIROS-T-0593: two hits, a page of one: the next offset.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["search", "--query", "golden", "--limit", "1"],
    )
    .await;
    assert_eq!(code, 0, "search --limit 1 failed: {stderr}");
    assert!(stdout.contains("Next: --offset 1"), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["search", "--query", "golden", "--limit", "1", "--json"],
    )
    .await;
    assert_eq!(code, 0, "search --limit 1 --json failed: {stderr}");
    let page: serde_json::Value = serde_json::from_str(&stdout).expect("search JSON");
    assert_eq!(page["next_offset"], 1, "{stdout}");
    // --all reads both pages.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["search", "--query", "golden", "--all", "--json"],
    )
    .await;
    assert_eq!(code, 0, "search --all failed: {stderr}");
    let all: serde_json::Value = serde_json::from_str(&stdout).expect("search JSON");
    assert_eq!(all["items"].as_array().map(Vec::len), Some(2), "{stdout}");
    assert!(all["next_offset"].is_null(), "{stdout}");

    // A search flag beside --query-json is refused, and named; before,
    // --updated-after was dropped without a word.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "search",
            "--query-json",
            r#"{"q": "golden"}"#,
            "--updated-after",
            "2026-01-01T00:00:00Z",
        ],
    )
    .await;
    assert_eq!(code, 1, "a flag beside --query-json is refused: {stdout}");
    assert!(
        stderr.contains("Do not use it with --updated-after."),
        "{stderr}"
    );
    assert!(stdout.is_empty(), "{stdout}");

    // The body goes to the server as it is: the server judges a field
    // that it does not know, and names it.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "search",
            "--query-json",
            r#"{"q": "golden", "no_such_field": 1}"#,
        ],
    )
    .await;
    assert_eq!(code, 1, "the server refuses an unknown field: {stderr}");
    assert!(stderr.contains("no_such_field"), "{stderr}");

    // --- edit: clean bump to version 2, then a stale --version → 409 -----------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "edit",
            &task_code,
            "--content",
            "Fixed and verified.",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks edit failed: {stderr}");
    let edited: serde_json::Value = serde_json::from_str(&stdout).expect("edited DTO");
    assert_eq!(edited["version"], 2);
    assert_eq!(edited["content"], "Fixed and verified.");

    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "edit",
            &task_code,
            "--title",
            "Stale edit",
            "--version",
            "1",
        ],
    )
    .await;
    assert_eq!(code, 1, "stale edit must exit 1: {stderr}");
    assert!(stderr.contains("conflict (409"), "{stderr}");
    assert!(
        stderr.contains("current version on the server is 2"),
        "{stderr}"
    );
    assert!(stderr.contains("--version"), "{stderr}");

    // Without --version the edit re-bases on the fetched current entity.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "edit",
            &task_code,
            "--title",
            "Golden path task (fixed)",
        ],
    )
    .await;
    assert_eq!(code, 0, "re-based edit failed: {stderr}");
    assert_eq!(
        stdout.trim(),
        format!("Kairos changed the task {task_code}. The new version is 3.")
    );

    // --- delete: refused without --confirm, then deleted ------------------------
    let (code, _, stderr) = run_cli(config_dir.path(), &["tasks", "delete", &task_code]).await;
    assert_eq!(code, 1, "delete without --confirm must exit 1");
    assert!(stderr.contains("--confirm"), "{stderr}");

    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "delete", &task_code, "--confirm"],
    )
    .await;
    assert_eq!(code, 0, "confirmed delete failed: {stderr}");
    assert_eq!(stdout.trim(), format!("Kairos archived {task_code}."));

    // An archived task still reads (KAIROS-A-0020, KAIROS-T-0155), and the
    // banner is what stops someone quoting a retired ticket as current.
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["tasks", "get", &task_code]).await;
    assert_eq!(code, 0, "an archived task is still readable: {stderr}");
    assert!(
        stdout.contains("ARCHIVED"),
        "the archived banner is not optional: {stdout}"
    );
    assert!(stdout.contains(&task_code), "{stdout}");

    // …but it is out of the way: gone from the default listing.
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["tasks", "list"]).await;
    assert_eq!(code, 0, "tasks list failed: {stderr}");
    assert!(
        !stdout.contains(&task_code),
        "archived work stays out of default listings: {stdout}"
    );

    // --- members list / orgs show / teams list ----------------------------------
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["members", "list"]).await;
    assert_eq!(code, 0, "members list failed: {stderr}");
    assert!(stdout.contains("alice@kairos.test"), "{stdout}");
    assert!(stdout.contains("admin"), "{stdout}");

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["orgs", "show"]).await;
    assert_eq!(code, 0, "orgs show failed: {stderr}");
    assert!(stdout.contains(&format!("org:   {TENANT}")), "{stdout}");
    assert!(stdout.contains("role:  admin"), "{stdout}");

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["teams", "list"]).await;
    assert_eq!(code, 0, "teams list failed: {stderr}");
    assert!(stdout.contains("Platform"), "{stdout}");

    // --- metadata and templates page (KAIROS-T-0593) ---------------------------
    for slug in ["paging-one", "paging-two"] {
        let (code, _, stderr) = run_cli(
            config_dir.path(),
            &[
                "metadata", "create", "--name", slug, "--slug", slug, "--type", "string",
            ],
        )
        .await;
        assert_eq!(code, 0, "metadata create failed: {stderr}");
        let (code, _, stderr) = run_cli(
            config_dir.path(),
            &["templates", "create", "--name", slug, "--slug", slug],
        )
        .await;
        assert_eq!(code, 0, "templates create failed: {stderr}");
    }
    for noun in ["metadata", "templates"] {
        let (code, stdout, stderr) = run_cli(config_dir.path(), &[noun, "list", "--json"]).await;
        assert_eq!(code, 0, "{noun} list --json failed: {stderr}");
        let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("envelope JSON");
        let total = envelope["total"].as_i64().expect("total");
        assert!(total >= 2, "{stdout}");

        let (code, stdout, stderr) =
            run_cli(config_dir.path(), &[noun, "list", "--limit", "1"]).await;
        assert_eq!(code, 0, "{noun} list --limit 1 failed: {stderr}");
        assert!(
            stdout.contains(&format!(
                "total: {total} (limit 1, offset 0)\nNext: --offset 1\n"
            )),
            "{noun}: {stdout}"
        );
        let last = (total - 1).to_string();
        let (code, stdout, stderr) = run_cli(
            config_dir.path(),
            &[noun, "list", "--limit", "1", "--offset", &last],
        )
        .await;
        assert_eq!(code, 0, "{noun} list of the last page failed: {stderr}");
        assert!(!stdout.contains("Next:"), "{noun}: {stdout}");
    }
    // --all reads each page of a list of the server.
    for noun in ["metadata", "templates"] {
        let (code, stdout, stderr) =
            run_cli(config_dir.path(), &[noun, "list", "--all", "--json"]).await;
        assert_eq!(code, 0, "{noun} list --all failed: {stderr}");
        let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("envelope JSON");
        assert_eq!(
            envelope["items"].as_array().map(|a| a.len() as i64),
            envelope["total"].as_i64(),
            "{noun}: {stdout}"
        );
        assert!(envelope["next_offset"].is_null(), "{noun}: {stdout}");
    }
    // The server finds a definition and a template by the slug
    // (KAIROS-T-0644): update and delete are one call each.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["metadata", "update", "paging-two", "--name", "Paging two"],
    )
    .await;
    assert_eq!(code, 0, "metadata update by slug failed: {stderr}");
    assert!(stdout.contains("paging-two"), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["templates", "update", "paging-two", "--name", "Paging two"],
    )
    .await;
    assert_eq!(code, 0, "templates update by slug failed: {stderr}");
    assert!(stdout.contains("paging-two"), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["templates", "delete", "paging-one", "--confirm"],
    )
    .await;
    assert_eq!(code, 0, "templates delete by slug failed: {stderr}");
    assert!(stdout.contains("paging-one"), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["metadata", "delete", "paging-one", "--confirm"],
    )
    .await;
    assert_eq!(code, 0, "metadata delete by slug failed: {stderr}");
    assert!(stdout.contains("paging-one"), "{stdout}");
    // An unknown slug: exit 1, and the message names it.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["metadata", "update", "no-such-field", "--name", "x"],
    )
    .await;
    assert_eq!(code, 1, "an unknown slug: {stderr}");
    assert!(stderr.contains("no-such-field"), "{stderr}");

    // --- repos create / list / get / bind / unbind (KAIROS-T-0107) ---------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "repos",
            "create",
            "--forge",
            "github",
            "--name",
            "acme/payments-api",
            "--repo-url",
            "https://github.com/acme/payments-api",
            "--team",
            "platform",
            "--slug",
            "payments-api",
            "--description",
            "cargo test before every PR",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "repos create failed: {stderr}");
    let repo: serde_json::Value = serde_json::from_str(&stdout).expect("repo DTO JSON");
    assert_eq!(repo["slug"], "payments-api");
    assert_eq!(repo["team"]["slug"], "platform");
    assert_eq!(
        repo["delivery_board_id"].as_str(),
        Some(delivery_board_id.as_str())
    );

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["repos", "list"]).await;
    assert_eq!(code, 0, "repos list failed: {stderr}");
    assert!(stdout.contains("payments-api"), "{stdout}");
    assert!(stdout.contains("acme/payments-api"), "{stdout}");

    // KAIROS-T-0518: `repos indexes` of a repository with no index is an
    // empty list, in the envelope of each list.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "indexes", "payments-api", "--json"],
    )
    .await;
    assert_eq!(code, 0, "repos indexes --json failed: {stderr}");
    let indexes: serde_json::Value = serde_json::from_str(&stdout).expect("envelope JSON");
    assert_eq!(
        indexes,
        serde_json::json!({ "items": [], "total": 0, "next_offset": null })
    );
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "indexes", "payments-api", "--all"],
    )
    .await;
    assert_eq!(code, 0, "repos indexes --all failed: {stderr}");
    assert_eq!(stdout, "(none)\ntotal: 0\n");
    let (code, _, stderr) = run_cli(config_dir.path(), &["repos", "indexes", "no-such-repo"]).await;
    assert_ne!(code, 0, "an unknown repository must fail");
    assert!(stderr.contains("no-such-repo"), "{stderr}");

    // The golden-path task was deleted above; bind a fresh one.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--board",
            &delivery_board_id,
            "--title",
            "To be bound",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create failed: {stderr}");
    let to_bind: serde_json::Value = serde_json::from_str(&stdout).expect("task DTO JSON");
    let bind_code = to_bind["short_code"]
        .as_str()
        .expect("short_code")
        .to_string();
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "bind", &bind_code, "payments-api"],
    )
    .await;
    assert_eq!(code, 0, "repos bind failed: {stderr}");
    assert!(stdout.contains("to payments-api"), "{stdout}");

    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["repos", "get", "payments-api"]).await;
    assert_eq!(code, 0, "repos get failed: {stderr}");
    assert!(stdout.contains("cargo test before every PR"), "{stdout}");
    assert!(stdout.contains("How to work here"), "{stdout}");
    assert!(stdout.contains("read token:     not set"), "{stdout}");

    // --- repos credential (COLLIERY-T-3105) -----------------------------------
    // The token comes on standard input, never as an argument.
    const TOKEN: &str = "github_pat_cli_t3105_SECRET";
    let (code, _stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "credential", "set", "payments-api", TOKEN],
    )
    .await;
    assert_ne!(code, 0, "the token is not an argument");
    assert!(stderr.contains("unexpected argument"), "{stderr}");
    let (code, stdout, stderr) = run_cli_stdin(
        config_dir.path(),
        &["repos", "credential", "set", "payments-api"],
        &format!("{TOKEN}\n"),
    )
    .await;
    assert_eq!(code, 0, "repos credential set failed: {stderr}");
    println!("credential set: {stdout}");
    assert!(
        stdout.contains("Kairos keeps the read token of the repository payments-api"),
        "{stdout}"
    );
    assert!(!stdout.contains(TOKEN) && !stderr.contains(TOKEN));
    // `show` is `get`: the status, never the token.
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["repos", "show", "payments-api"]).await;
    assert_eq!(code, 0, "repos show failed: {stderr}");
    println!("repos show: {stdout}");
    assert!(stdout.contains("read token:     set by "), "{stdout}");
    assert!(stdout.contains("; not checked"), "{stdout}");
    assert!(!stdout.contains(TOKEN), "{stdout}");
    let (code, stdout, _) = run_cli(
        config_dir.path(),
        &["repos", "get", "payments-api", "--json"],
    )
    .await;
    assert_eq!(code, 0);
    let detail: serde_json::Value = serde_json::from_str(&stdout).expect("detail JSON");
    assert_eq!(detail["credential"]["set"], true, "{detail}");
    assert!(!stdout.contains(TOKEN), "{stdout}");
    // KAIROS-T-0590: refused without --confirm; the token stays.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["repos", "credential", "remove", "payments-api"],
    )
    .await;
    assert_eq!(code, 1, "credential remove without --confirm must exit 1");
    assert!(stderr.contains("--confirm"), "{stderr}");
    let (_, stdout, _) = run_cli(
        config_dir.path(),
        &["repos", "get", "payments-api", "--json"],
    )
    .await;
    let detail: serde_json::Value = serde_json::from_str(&stdout).expect("detail JSON");
    assert_eq!(detail["credential"]["set"], true, "{detail}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "credential", "remove", "payments-api", "--confirm"],
    )
    .await;
    assert_eq!(code, 0, "repos credential remove failed: {stderr}");
    assert!(
        stdout.contains("Kairos removed the read token of the repository payments-api."),
        "{stdout}"
    );
    let (_, stdout, _) = run_cli(config_dir.path(), &["repos", "show", "payments-api"]).await;
    assert!(stdout.contains("read token:     not set"), "{stdout}");

    // COLLIERY-T-0219: a task on the board of a different team links to the
    // repository. That is normal work (COLLIERY-A-0023). `repos get` counts
    // it as open and prints no line about stale tasks; before, it printed
    // `stale tasks: N (...)` for such a task.
    let other_team = alice
        .create_team(&CreateTeamRequest {
            name: "Web".into(),
            slug: "web".into(),
            code_prefix: "WEB".into(),
            team_type: None,
        })
        .await
        .expect("a second team");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--team",
            &other_team.id,
            "--repo",
            "payments-api",
            "--title",
            "Web's work in platform's code",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create on the other team failed: {stderr}");
    let elsewhere: serde_json::Value = serde_json::from_str(&stdout).expect("task DTO JSON");
    assert_eq!(elsewhere["team_id"].as_str(), Some(other_team.id.as_str()));
    assert_ne!(
        elsewhere["board_id"].as_str(),
        Some(delivery_board_id.as_str())
    );
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["repos", "get", "payments-api"]).await;
    assert_eq!(code, 0, "repos get failed: {stderr}");
    assert!(stdout.contains("OWNER_BOARD"), "{stdout}");
    assert!(!stdout.to_lowercase().contains("stale"), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "get", "payments-api", "--json"],
    )
    .await;
    assert_eq!(code, 0, "repos get --json failed: {stderr}");
    let detail: serde_json::Value = serde_json::from_str(&stdout).expect("detail JSON");
    assert_eq!(detail["team"]["slug"], "platform");
    assert_eq!(
        detail["open_tasks"], 2,
        "the bound task and the task on web's board: {detail}"
    );
    assert!(detail.get("stale_tasks").is_none(), "{detail}");

    // --repo without --board and without --team does not choose a board
    // (COLLIERY-T-0217, COLLIERY-A-0023). The CLI refuses before it sends
    // anything, and names the argument that is missing. Until then this
    // routed the task to the board of the repository's owner.
    let (code, _stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--repo",
            "payments-api",
            "--title",
            "Routed by repo",
            "--json",
        ],
    )
    .await;
    assert_ne!(code, 0, "--repo alone must not create a task");
    assert!(stderr.contains("--board"), "{stderr}");

    // --team without --board names the delivery board of that team, and
    // --repo is carried along as the link.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--team",
            &team.id,
            "--repo",
            "payments-api",
            "--title",
            "Routed by team",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create --team --repo failed: {stderr}");
    let routed: serde_json::Value = serde_json::from_str(&stdout).expect("task DTO JSON");
    assert_eq!(
        routed["board_id"].as_str(),
        Some(delivery_board_id.as_str())
    );
    assert_eq!(routed["team_id"].as_str(), Some(team.id.as_str()));
    assert_eq!(routed["repository"]["slug"], "payments-api");

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["repos", "unbind", &bind_code]).await;
    assert_eq!(code, 0, "repos unbind failed: {stderr}");
    assert!(
        stdout.contains("Kairos removed the repository of"),
        "{stdout}"
    );

    // --- KAIROS-T-0321: the teams of an initiative --------------------------------
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["teams", "of", &initiative_code]).await;
    assert_eq!(code, 0, "teams of failed: {stderr}");
    assert!(stdout.contains("has no team"), "{stdout}");

    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["teams", "set", &initiative_code, "platform"],
    )
    .await;
    assert_eq!(code, 0, "teams set failed: {stderr}");
    assert_eq!(
        stdout.trim(),
        format!("Kairos set the team platform on {initiative_code} by hand.")
    );
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["teams", "of", &initiative_code]).await;
    assert_eq!(code, 0, "teams of failed: {stderr}");
    assert!(stdout.contains("platform"), "{stdout}");
    assert!(stdout.contains("set by hand"), "{stdout}");

    // The board filter and the marker.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["boards", "show", "initiatives", "--team", "platform"],
    )
    .await;
    assert_eq!(code, 0, "boards show --team failed: {stderr}");
    assert!(
        stdout.contains(&format!(
            "{initiative_code}  [initiative] CLI golden path initiative  [teams: platform]"
        )),
        "{stdout}"
    );
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["boards", "show", "initiatives", "--no-team"],
    )
    .await;
    assert_eq!(code, 0, "boards show --no-team failed: {stderr}");
    assert!(!stdout.contains(&initiative_code), "{stdout}");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "boards",
            "show",
            "initiatives",
            "--team",
            "platform",
            "--no-team",
        ],
    )
    .await;
    assert_ne!(code, 0, "--team with --no-team must be refused");
    assert!(stderr.contains("cannot be used with"), "{stderr}");

    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["teams", "clear", &initiative_code, "platform"],
    )
    .await;
    assert_eq!(code, 0, "teams clear failed: {stderr}");
    assert!(
        stdout.contains("Kairos cleared the team platform"),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["teams", "clear", &initiative_code, "platform"],
    )
    .await;
    assert_ne!(code, 0, "a second clear must fail");
    assert!(stderr.contains("is not set on"), "{stderr}");
    // A task has no team of its own to set.
    let (code, _, stderr) =
        run_cli(config_dir.path(), &["teams", "set", &bind_code, "platform"]).await;
    assert_ne!(code, 0, "teams set on a task must fail");
    assert!(
        stderr.contains("Only an initiative or a strategy"),
        "{stderr}"
    );

    // --- documents lifecycle (KAIROS-T-0555) -----------------------------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "documents",
            "create",
            "--title",
            "Rollout note",
            "--board",
            &delivery_board_id,
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "documents create failed: {stderr}");
    let document: serde_json::Value = serde_json::from_str(&stdout).expect("document DTO JSON");
    let document_code = document["short_code"].as_str().expect("code").to_string();
    assert_eq!(document["lifecycle"], "draft");
    for state in ["review", "published", "published"] {
        let (code, stdout, stderr) = run_cli(
            config_dir.path(),
            &["documents", "lifecycle", &document_code, "--to", state],
        )
        .await;
        assert_eq!(code, 0, "documents lifecycle --to {state} failed: {stderr}");
        assert_eq!(
            stdout.trim_end(),
            format!("The document {document_code} is {state}.")
        );
    }
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["documents", "get", &document_code, "--json"],
    )
    .await;
    assert_eq!(code, 0, "documents get failed: {stderr}");
    let document: serde_json::Value = serde_json::from_str(&stdout).expect("document DTO JSON");
    assert_eq!(document["lifecycle"], "published");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["documents", "lifecycle", &document_code, "--to", "final"],
    )
    .await;
    assert_ne!(code, 0, "an unknown lifecycle must be refused");
    assert!(
        stderr.contains("final") && stderr.contains("published"),
        "{stderr}"
    );

    // --- KAIROS-T-0589: a name or a slug for each reference -------------------
    let text_dir = tempfile::tempdir().expect("temp text dir");
    let content_path = text_dir.path().join("content.md");
    std::fs::write(&content_path, "Content from a file.").expect("content file");
    let note_path = text_dir.path().join("note.md");
    std::fs::write(&note_path, "A note from a file.").expect("note file");
    let content_file = content_path.to_str().expect("path").to_string();
    let note_file = note_path.to_str().expect("path").to_string();

    // tasks create: --team by slug, --column by name, --kind, --content-file.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--team",
            "platform",
            "--column",
            "backlog",
            "--title",
            "Named references",
            "--kind",
            "bug",
            "--content-file",
            &content_file,
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create by names failed: {stderr}");
    let named: serde_json::Value = serde_json::from_str(&stdout).expect("task DTO JSON");
    let named_code = named["short_code"].as_str().expect("code").to_string();
    // A task starts in the entry column (KAIROS-T-0566).
    assert_eq!(
        named["column_id"].as_str(),
        Some(backlog.as_str()),
        "{stdout}"
    );
    assert_eq!(
        named["team_id"].as_str(),
        Some(team.id.as_str()),
        "{stdout}"
    );
    assert_eq!(named["task_type"], "bug");
    assert_eq!(named["content"], "Content from a file.");
    // An unknown column is refused before the write, and each column is named.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--board",
            &delivery_board_id,
            "--column",
            "Nowhere",
            "--title",
            "x",
        ],
    )
    .await;
    assert_eq!(code, 1, "an unknown column: {stderr}");
    assert!(
        stderr.contains("\"Nowhere\"") && stderr.contains("Backlog"),
        "{stderr}"
    );

    // transition --to a column name; the line names the column.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "transition", &named_code, "--to", "todo"],
    )
    .await;
    assert_eq!(code, 0, "transition by name failed: {stderr}");
    assert_eq!(
        stdout.trim(),
        format!("Kairos moved the task {named_code} to the column Todo.")
    );

    // note --text-file; show, and get as its alias.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "note", &named_code, "--text-file", &note_file],
    )
    .await;
    assert_eq!(code, 0, "note --text-file failed: {stderr}");
    for verb in ["show", "get"] {
        let (code, stdout, stderr) =
            run_cli(config_dir.path(), &["tasks", verb, &named_code]).await;
        assert_eq!(code, 0, "tasks {verb} failed: {stderr}");
        assert!(stdout.contains("A note from a file."), "{stdout}");
    }

    // documents create --template by slug.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "documents",
            "create",
            "--title",
            "From a template",
            "--board",
            &delivery_board_id,
            "--template",
            "paging-two",
            "--json",
        ],
    )
    .await;
    assert_eq!(
        code, 0,
        "documents create --template <slug> failed: {stderr}"
    );
    let templated: serde_json::Value = serde_json::from_str(&stdout).expect("document JSON");
    assert!(templated["template_id"].is_string(), "{stdout}");

    // boards list shows the slug of the team.
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["boards", "list"]).await;
    assert_eq!(code, 0, "boards list failed: {stderr}");
    let delivery_row = stdout
        .lines()
        .find(|line| line.starts_with(&delivery_board_id))
        .expect("the delivery board row");
    assert!(delivery_row.trim_end().ends_with("platform"), "{stdout}");

    // members set-role by email.
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "members",
            "set-role",
            "alice@kairos.test",
            "--role",
            "admin",
        ],
    )
    .await;
    assert_eq!(code, 0, "members set-role by email failed: {stderr}");
    assert!(stdout.contains("has the role admin"), "{stdout}");

    // service accounts and keys by the name of the account.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["service-accounts", "create", "--name", "ci-deploy"],
    )
    .await;
    assert_eq!(code, 0, "service-accounts create failed: {stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["keys", "list", "--service-account", "ci-deploy"],
    )
    .await;
    assert_eq!(code, 0, "keys list by account name failed: {stderr}");
    assert!(stdout.contains("total: 0"), "{stdout}");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["keys", "list", "--service-account", "no-such-account"],
    )
    .await;
    assert_eq!(code, 1, "an unknown account: {stderr}");
    assert!(stderr.contains("no-such-account"), "{stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["service-accounts", "delete", "ci-deploy", "--confirm"],
    )
    .await;
    assert_eq!(code, 0, "service-accounts delete by name failed: {stderr}");
    assert!(stdout.contains("ci-deploy"), "{stdout}");

    // streams by slug, and their teams by slug.
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "streams", "create", "--name", "Payments", "--slug", "payments",
        ],
    )
    .await;
    assert_eq!(code, 0, "streams create failed: {stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["streams", "teams", "add", "payments", "--team", "platform"],
    )
    .await;
    assert_eq!(code, 0, "streams teams add by slugs failed: {stderr}");
    assert!(stdout.contains("platform"), "{stdout}");
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["streams", "teams", "list", "payments"]).await;
    assert_eq!(code, 0, "streams teams list by slug failed: {stderr}");
    assert!(stdout.contains("platform"), "{stdout}");
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["streams", "show", "payments"]).await;
    assert_eq!(code, 0, "streams show by slug failed: {stderr}");
    assert!(stdout.contains("Payments"), "{stdout}");

    // boards flow --until, and the old name --to.
    for flag in ["--until", "--to"] {
        let (code, _, stderr) = run_cli(
            config_dir.path(),
            &["boards", "flow", &delivery_board_id, flag, "2026-10-18"],
        )
        .await;
        assert_eq!(code, 0, "boards flow {flag} failed: {stderr}");
    }

    // --- KAIROS-T-0592: diff, links, archive --preview -------------------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "create",
            "--board",
            "platform-delivery",
            "--title",
            "Diff me",
            "--content",
            "one\ntwo\nthree\n",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create failed: {stderr}");
    let diff_task: serde_json::Value = serde_json::from_str(&stdout).expect("task JSON");
    let diff_code = diff_task["short_code"].as_str().expect("code").to_string();
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "tasks",
            "edit",
            &diff_code,
            "--title",
            "Diff me again",
            "--content",
            "one\n2\nthree\n",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks edit failed: {stderr}");
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["tasks", "diff", &diff_code, "2"]).await;
    assert_eq!(code, 0, "tasks diff failed: {stderr}");
    assert!(
        stdout.starts_with(&format!("--- {diff_code} v1 (")),
        "{stdout}"
    );
    assert!(
        stdout.contains("title: \"Diff me\" -> \"Diff me again\"\n"),
        "{stdout}"
    );
    assert!(stdout.contains("\n-two\n+2\n"), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "diff", &diff_code, "1", "2", "--json"],
    )
    .await;
    assert_eq!(code, 0, "tasks diff --json failed: {stderr}");
    let diff: serde_json::Value = serde_json::from_str(&stdout).expect("diff JSON");
    assert_eq!(diff["from"]["version"], 1, "{stdout}");
    assert_eq!(diff["to"]["version"], 2, "{stdout}");
    assert!(
        diff["diff"].as_str().is_some_and(|d| d.contains("+2\n")),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(config_dir.path(), &["tasks", "diff", &diff_code, "0"]).await;
    assert_eq!(code, 1, "version 0: {stderr}");
    assert!(stderr.contains("The first version is 1"), "{stderr}");

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["tasks", "links", &diff_code]).await;
    assert_eq!(code, 0, "tasks links failed: {stderr}");
    assert_eq!(
        stdout,
        format!("No branch, pull request or issue of a forge links to {diff_code}.\n")
    );
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["tasks", "links", &diff_code, "--json"]).await;
    assert_eq!(code, 0, "tasks links --json failed: {stderr}");
    let links: serde_json::Value = serde_json::from_str(&stdout).expect("links JSON");
    assert_eq!(
        links,
        serde_json::json!({ "items": [], "total": 0, "next_offset": null })
    );

    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "archive", &diff_code, "--preview"],
    )
    .await;
    assert_eq!(code, 0, "archive --preview failed: {stderr}");
    assert!(
        stdout.starts_with(&format!(
            "An archive of {diff_code} takes no other item. Kairos archived nothing."
        )),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(config_dir.path(), &["tasks", "show", &diff_code]).await;
    assert_eq!(code, 0, "the preview archived the task: {stderr}");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["tasks", "archive", &diff_code, "--preview", "--confirm"],
    )
    .await;
    assert_eq!(code, 64, "--preview with --confirm: {stderr}");

    // --- KAIROS-T-0592: activity -------------------------------------------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["activity", "--item", &diff_code, "--json"],
    )
    .await;
    assert_eq!(code, 0, "activity --item failed: {stderr}");
    let feed: serde_json::Value = serde_json::from_str(&stdout).expect("activity JSON");
    let rows = feed["items"].as_array().expect("items");
    assert!(!rows.is_empty(), "{stdout}");
    assert!(
        rows.iter()
            .all(|row| row["entity_short_code"] == diff_code.as_str()),
        "{stdout}"
    );
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["activity", "--item", &diff_code]).await;
    assert_eq!(code, 0, "activity text failed: {stderr}");
    assert!(stdout.starts_with("WHEN"), "{stdout}");
    assert!(stdout.contains(&diff_code), "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["activity", "--actor", "me", "--limit", "1", "--json"],
    )
    .await;
    assert_eq!(code, 0, "activity --actor me failed: {stderr}");
    let feed: serde_json::Value = serde_json::from_str(&stdout).expect("activity JSON");
    assert_eq!(feed["items"].as_array().map(Vec::len), Some(1), "{stdout}");
    assert_eq!(feed["next_offset"], 1, "{stdout}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "activity",
            "--actor",
            "alice@kairos.test",
            "--action",
            "create",
            "--since",
            "2020-01-01",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "activity by email failed: {stderr}");
    let feed: serde_json::Value = serde_json::from_str(&stdout).expect("activity JSON");
    let rows = feed["items"].as_array().expect("items");
    assert!(!rows.is_empty(), "{stdout}");
    assert!(rows.iter().all(|row| row["action"] == "create"), "{stdout}");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["activity", "--actor", "nobody@kairos.test"],
    )
    .await;
    assert_eq!(code, 1, "an unknown actor: {stderr}");
    assert!(stderr.contains("nobody@kairos.test"), "{stderr}");

    // --- KAIROS-T-0592: boards members, show --all, update, delete ---------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["boards", "members", "platform-delivery", "--json"],
    )
    .await;
    assert_eq!(code, 0, "boards members failed: {stderr}");
    let members: serde_json::Value = serde_json::from_str(&stdout).expect("members JSON");
    assert_eq!(
        members["total"].as_u64(),
        members["items"].as_array().map(|a| a.len() as u64),
        "{stdout}"
    );
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["boards", "show", "platform-delivery", "--all"],
    )
    .await;
    assert_eq!(code, 0, "boards show --all failed: {stderr}");
    assert!(stdout.contains(&diff_code), "{stdout}");
    assert!(
        stdout
            .lines()
            .last()
            .is_some_and(|line| line.starts_with("total: ") && !line.contains("limit")),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "boards",
            "show",
            "platform-delivery",
            "--all",
            "--limit",
            "1",
        ],
    )
    .await;
    assert_eq!(code, 64, "--all with --limit: {stderr}");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &[
            "boards",
            "create",
            "--name",
            "Sandbox",
            "--slug",
            "sandbox",
            "--level",
            "strategy",
            "--code-prefix",
            "SANDBOX",
        ],
    )
    .await;
    assert_eq!(code, 0, "boards create failed: {stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["boards", "update", "sandbox", "--name", "Sandbox two"],
    )
    .await;
    assert_eq!(code, 0, "boards update failed: {stderr}");
    assert_eq!(
        stdout,
        "The board sandbox has the name Sandbox two and the slug sandbox.\n"
    );
    let (code, _, stderr) = run_cli(config_dir.path(), &["boards", "update", "sandbox"]).await;
    assert_eq!(code, 1, "an update with nothing to change: {stderr}");
    let (code, _, stderr) = run_cli(config_dir.path(), &["boards", "delete", "sandbox"]).await;
    assert_eq!(code, 1, "a delete with no --confirm: {stderr}");
    assert!(stderr.contains("--confirm"), "{stderr}");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["boards", "delete", "initiatives", "--confirm", "--json"],
    )
    .await;
    assert_eq!(code, 1, "a board with items: {stderr}");
    assert!(stderr.contains("BOARD_NOT_EMPTY"), "{stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["boards", "delete", "sandbox", "--confirm"],
    )
    .await;
    assert_eq!(code, 0, "boards delete failed: {stderr}");
    assert_eq!(stdout, "Kairos deleted the board sandbox.\n");
    let (code, _, _) = run_cli(config_dir.path(), &["boards", "show", "sandbox"]).await;
    assert_ne!(code, 0, "the board is gone");

    // --- KAIROS-T-0592: repos webhook ---------------------------------------------
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "webhook", "add", "payments-api"],
    )
    .await;
    assert_eq!(code, 0, "repos webhook add failed: {stderr}");
    assert!(
        stdout.starts_with("Kairos connected the webhook of the repository payments-api (github)."),
        "{stdout}"
    );
    let secret_line = stdout
        .lines()
        .find(|line| line.trim_start().starts_with("Secret:"))
        .expect("the secret line");
    let secret = secret_line
        .trim_start()
        .trim_start_matches("Secret:")
        .trim()
        .to_string();
    assert!(!secret.is_empty(), "{stdout}");
    assert!(
        stdout.contains("  Payload URL: http://kairos.test/webhooks/github/"),
        "{stdout}"
    );
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["repos", "webhook", "list"]).await;
    assert_eq!(code, 0, "repos webhook list failed: {stderr}");
    assert!(stdout.contains("payments-api"), "{stdout}");
    assert!(
        !stdout.contains(&secret),
        "the list shows no secret: {stdout}"
    );
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "webhook", "list", "payments-api", "--json"],
    )
    .await;
    assert_eq!(code, 0, "repos webhook list --json failed: {stderr}");
    assert!(!stdout.contains(&secret), "{stdout}");
    let hooks: serde_json::Value = serde_json::from_str(&stdout).expect("list JSON");
    assert_eq!(hooks["total"], 1, "{stdout}");
    let first_id = hooks["items"][0]["id"].as_str().expect("id").to_string();
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["repos", "show", "payments-api"]).await;
    assert_eq!(code, 0, "repos show failed: {stderr}");
    assert!(
        stdout.contains(&format!("webhook:        {first_id}")),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["repos", "webhook", "add", "payments-api"],
    )
    .await;
    assert_eq!(code, 1, "a second add: {stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "webhook", "rotate", "payments-api", "--json"],
    )
    .await;
    assert_eq!(code, 0, "repos webhook rotate failed: {stderr}");
    let rotated: serde_json::Value = serde_json::from_str(&stdout).expect("rotate JSON");
    assert_ne!(rotated["id"].as_str(), Some(first_id.as_str()), "{stdout}");
    assert!(
        rotated["webhook_secret"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s != secret),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["repos", "webhook", "remove", "payments-api"],
    )
    .await;
    assert_eq!(code, 1, "a remove with no --confirm: {stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &["repos", "webhook", "remove", "payments-api", "--confirm"],
    )
    .await;
    assert_eq!(code, 0, "repos webhook remove failed: {stderr}");
    assert!(
        stdout.starts_with("Kairos removed the webhook connection"),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["repos", "webhook", "rotate", "payments-api"],
    )
    .await;
    assert_eq!(code, 1, "rotate with no connection: {stderr}");
    assert!(
        stderr.contains("kairos repos webhook add payments-api"),
        "{stderr}"
    );

    // --- KAIROS-T-0592: admin read-tokens ------------------------------------------
    const ORG_TOKEN: &str = "github_pat_cli_t0592_SECRET";
    let (code, stdout, stderr) = run_cli_stdin(
        config_dir.path(),
        &["admin", "read-tokens", "set", "github.com"],
        ORG_TOKEN,
    )
    .await;
    assert_eq!(code, 0, "admin read-tokens set failed: {stderr}");
    assert!(!stdout.contains(ORG_TOKEN), "{stdout}");
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["admin", "read-tokens", "list"]).await;
    assert_eq!(code, 0, "admin read-tokens list failed: {stderr}");
    assert!(stdout.contains("github.com"), "{stdout}");
    assert!(!stdout.contains(ORG_TOKEN), "{stdout}");
    let (code, _, stderr) = run_cli(
        config_dir.path(),
        &["admin", "read-tokens", "remove", "github.com"],
    )
    .await;
    assert_eq!(code, 1, "a remove with no --confirm: {stderr}");
    let (code, stdout, stderr) = run_cli(
        config_dir.path(),
        &[
            "admin",
            "read-tokens",
            "remove",
            "github.com",
            "--confirm",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "admin read-tokens remove failed: {stderr}");
    let left: serde_json::Value = serde_json::from_str(&stdout).expect("tokens JSON");
    assert_eq!(left["total"], 0, "{stdout}");

    // --- KAIROS-T-0592: notifications ----------------------------------------------
    for text in [
        "Robin wrote @alice in a note.",
        "Sam wrote @alice in a note.",
    ] {
        sql_query(format!(
            "INSERT INTO org_{TENANT}.notifications (user_id, kind, text) \
             VALUES ('{alice_id}', 'mention', '{text}')"
        ))
        .execute(&mut conn)
        .expect("a notification");
    }
    let (code, stdout, stderr) = run_cli(config_dir.path(), &["notifications", "list"]).await;
    assert_eq!(code, 0, "notifications list failed: {stderr}");
    assert!(
        stdout.starts_with("You have 2 unread notifications.\n"),
        "{stdout}"
    );
    assert!(stdout.contains("Sam wrote @alice"), "{stdout}");
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["notifications", "list", "--json"]).await;
    assert_eq!(code, 0, "notifications list --json failed: {stderr}");
    let list: serde_json::Value = serde_json::from_str(&stdout).expect("list JSON");
    assert_eq!(list["unread"], 2, "{stdout}");
    let newest = list["items"][0]["id"].as_str().expect("an id").to_string();
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["notifications", "read", &newest]).await;
    assert_eq!(code, 0, "notifications read failed: {stderr}");
    assert_eq!(
        stdout,
        "Kairos marked 1 notification read. You have 1 unread now.\n"
    );
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["notifications", "read", "--all"]).await;
    assert_eq!(code, 0, "notifications read --all failed: {stderr}");
    assert_eq!(
        stdout,
        "Kairos marked 1 notification read. You have 0 unread now.\n"
    );
    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["notifications", "read", "--all"]).await;
    assert_eq!(code, 0, "a second read --all failed: {stderr}");
    assert_eq!(stdout, "You have no unread notification.\n");
    let (code, _, stderr) = run_cli(config_dir.path(), &["notifications", "read"]).await;
    assert_eq!(code, 64, "read with no id and no --all: {stderr}");

    // --- KAIROS-T-0591: link and unlink, --parent, metadata values, show -----
    let cfg = config_dir.path();
    let created_code = |stdout: &str| -> String {
        let item: serde_json::Value = serde_json::from_str(stdout).expect("item DTO JSON");
        item["short_code"].as_str().expect("code").to_string()
    };
    let (code, stdout, stderr) = run_cli(
        cfg,
        &[
            "tasks",
            "create",
            "--board",
            &delivery_board_id,
            "--title",
            "Blocker",
            "--parent",
            &initiative_code,
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create --parent failed: {stderr}");
    let blocker = created_code(&stdout);
    let (code, stdout, stderr) = run_cli(
        cfg,
        &[
            "tasks",
            "create",
            "--board",
            &delivery_board_id,
            "--title",
            "Waiter",
            "--parent",
            &initiative_code,
            "--repo",
            "payments-api",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "tasks create --parent --repo failed: {stderr}");
    let waiter = created_code(&stdout);

    // link: an edge between two items, with its meaning.
    let (code, stdout, stderr) = run_cli(cfg, &["link", &blocker, "blocks", &waiter]).await;
    assert_eq!(code, 0, "link blocks failed: {stderr}");
    assert_eq!(
        stdout,
        format!(
            "Kairos made the link: {blocker} -[blocks]-> {waiter}. {waiter} waits for {blocker}.\n"
        )
    );
    let (code, _, stderr) = run_cli(cfg, &["link", &blocker, "blocks", &waiter]).await;
    assert_eq!(code, 1, "a second blocks edge is refused: {stderr}");
    let (code, _, stderr) = run_cli(cfg, &["link", &waiter, "blocks", &blocker]).await;
    assert_eq!(code, 1, "a cycle of blocks is refused: {stderr}");
    let (code, stdout, stderr) =
        run_cli(cfg, &["link", &blocker, "informs", &waiter, "--json"]).await;
    assert_eq!(code, 0, "link informs --json failed: {stderr}");
    let edge: serde_json::Value = serde_json::from_str(&stdout).expect("edge JSON");
    assert_eq!(edge["relationship"], "informs", "{stdout}");
    assert_eq!(edge["source_short_code"], blocker.as_str(), "{stdout}");
    // --label is for external only; an unknown relationship is usage (64).
    let (code, _, stderr) =
        run_cli(cfg, &["link", &blocker, "blocks", &waiter, "--label", "x"]).await;
    assert_eq!(code, 1, "--label with blocks: {stderr}");
    assert!(stderr.contains("not to blocks"), "{stderr}");
    let (code, _, stderr) = run_cli(cfg, &["link", &blocker, "child", &waiter]).await;
    assert_eq!(code, 64, "an unknown relationship: {stderr}");

    // adrs create --parent: the ADR and its supports edge are one write.
    let adr_total = || async {
        let (code, stdout, stderr) = run_cli(cfg, &["adrs", "list", "--json"]).await;
        assert_eq!(code, 0, "adrs list failed: {stderr}");
        let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
        envelope["total"].as_i64().expect("total")
    };
    let before = adr_total().await;
    let (code, _, stderr) = run_cli(
        cfg,
        &[
            "adrs",
            "create",
            "--board",
            "adrs",
            "--title",
            "Refused parent",
            "--parent",
            &document_code,
        ],
    )
    .await;
    assert_eq!(code, 1, "a document is no parent of an ADR: {stderr}");
    assert_eq!(adr_total().await, before, "a refused parent leaves no ADR");
    let (code, stdout, stderr) = run_cli(
        cfg,
        &[
            "adrs",
            "create",
            "--board",
            "adrs",
            "--title",
            "Queue decision",
            "--parent",
            &waiter,
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "adrs create --parent failed: {stderr}");
    let adr = created_code(&stdout);
    let (code, stdout, stderr) = run_cli(
        cfg,
        &[
            "adrs",
            "create",
            "--board",
            "adrs",
            "--title",
            "Queue decision 2",
            "--json",
        ],
    )
    .await;
    assert_eq!(code, 0, "adrs create failed: {stderr}");
    let adr_two = created_code(&stdout);
    let (code, stdout, stderr) = run_cli(cfg, &["link", &adr_two, "supersedes", &adr]).await;
    assert_eq!(code, 0, "link supersedes failed: {stderr}");
    assert!(
        stdout.ends_with(&format!("{adr_two} replaces {adr}.\n")),
        "{stdout}"
    );
    let (code, stdout, stderr) =
        run_cli(cfg, &["link", &initiative_code, "supports", &document_code]).await;
    assert_eq!(code, 0, "link supports failed: {stderr}");
    assert!(
        stdout.ends_with(&format!("{document_code} supports {initiative_code}.\n")),
        "{stdout}"
    );
    let (code, stdout, stderr) =
        run_cli(cfg, &["link", &document_code, "impacts", "payments-api"]).await;
    assert_eq!(code, 0, "link impacts failed: {stderr}");
    assert!(
        stdout.ends_with(&format!(
            "{document_code} is about the repository payments-api.\n"
        )),
        "{stdout}"
    );
    let (code, _, stderr) = run_cli(
        cfg,
        &[
            "link",
            &waiter,
            "external",
            "https://example.com/rfc",
            "--label",
            "RFC 42",
        ],
    )
    .await;
    assert_eq!(code, 0, "link external failed: {stderr}");

    // metadata set and unset.
    let (code, _, stderr) = run_cli(
        cfg,
        &[
            "metadata",
            "create",
            "--name",
            "Rollout risk",
            "--slug",
            "risk",
            "--type",
            "enum",
            "--option",
            "high",
            "--option",
            "low",
        ],
    )
    .await;
    assert_eq!(code, 0, "metadata create enum failed: {stderr}");
    let (code, stdout, stderr) =
        run_cli(cfg, &["tasks", "metadata", "set", &waiter, "risk=high"]).await;
    assert_eq!(code, 0, "tasks metadata set failed: {stderr}");
    assert_eq!(
        stdout,
        format!("Kairos changed the metadata of {waiter}.\n  metadata:\n    risk: high\n")
    );
    let (code, _, stderr) = run_cli(cfg, &["tasks", "metadata", "set", &waiter, "risk"]).await;
    assert_eq!(code, 64, "a pair without = is usage: {stderr}");
    let (code, _, stderr) =
        run_cli(cfg, &["tasks", "metadata", "set", &waiter, "risk=urgent"]).await;
    assert_eq!(code, 1, "a value outside the enum: {stderr}");
    let (code, _, stderr) = run_cli(
        cfg,
        &["tasks", "metadata", "set", &waiter, "no-such-field=x"],
    )
    .await;
    assert_eq!(code, 1, "an unknown slug: {stderr}");
    assert!(stderr.contains("no-such-field"), "{stderr}");
    let (code, stdout, stderr) = run_cli(
        cfg,
        &["adrs", "metadata", "set", &adr, "risk=low", "--json"],
    )
    .await;
    assert_eq!(code, 0, "adrs metadata set --json failed: {stderr}");
    let values: serde_json::Value = serde_json::from_str(&stdout).expect("metadata JSON");
    assert_eq!(values["values"][0]["value"], "low", "{stdout}");

    // show: repository, metadata, relationships and external references.
    let (code, stdout, stderr) = run_cli(cfg, &["tasks", "show", &waiter]).await;
    assert_eq!(code, 0, "tasks show failed: {stderr}");
    for line in [
        "  repository: payments-api\n".to_string(),
        "  metadata:\n    risk: high\n".to_string(),
        format!("    parent: {initiative_code} — CLI golden path initiative\n"),
        format!("    blocked by: {blocker} — Blocker\n"),
        format!("    informed by: {blocker} — Blocker\n"),
        format!("    supported by: {adr} — Queue decision\n"),
        "  external:\n    RFC 42 <https://example.com/rfc>\n".to_string(),
    ] {
        assert!(
            stdout.contains(&line),
            "tasks show has no {line:?}: {stdout}"
        );
    }
    let (code, stdout, stderr) = run_cli(cfg, &["tasks", "show", &waiter, "--json"]).await;
    assert_eq!(code, 0, "tasks show --json failed: {stderr}");
    let shown: serde_json::Value = serde_json::from_str(&stdout).expect("task DTO JSON");
    assert_eq!(shown["short_code"], waiter.as_str());
    assert!(shown.get("relationships").is_none(), "{stdout}");
    let (code, stdout, stderr) = run_cli(cfg, &["initiatives", "show", &initiative_code]).await;
    assert_eq!(code, 0, "initiatives show failed: {stderr}");
    assert!(
        stdout.contains(&format!("    child: {waiter} — Waiter\n")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "    supported by: {document_code} — Rollout note\n"
        )),
        "{stdout}"
    );
    let (code, stdout, stderr) = run_cli(cfg, &["adrs", "show", &adr]).await;
    assert_eq!(code, 0, "adrs show failed: {stderr}");
    assert!(
        stdout.contains(&format!("    supports: {waiter} — Waiter\n")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "    superseded by: {adr_two} — Queue decision 2\n"
        )),
        "{stdout}"
    );
    assert!(stdout.contains("  external: -\n"), "{stdout}");

    // unlink each relationship; a second unlink fails with exit 1.
    let (code, _, stderr) = run_cli(cfg, &["tasks", "metadata", "unset", &waiter, "risk"]).await;
    assert_eq!(code, 0, "tasks metadata unset failed: {stderr}");
    let (code, stdout, stderr) = run_cli(cfg, &["unlink", &blocker, "blocks", &waiter]).await;
    assert_eq!(code, 0, "unlink blocks failed: {stderr}");
    assert_eq!(
        stdout,
        format!("Kairos removed the link: {blocker} -[blocks]-> {waiter}.\n")
    );
    let (code, _, stderr) = run_cli(cfg, &["unlink", &blocker, "blocks", &waiter]).await;
    assert_eq!(code, 1, "a second unlink: {stderr}");
    assert!(stderr.contains("has no link"), "{stderr}");
    for (source, relationship, target) in [
        (blocker.as_str(), "informs", waiter.as_str()),
        (adr_two.as_str(), "supersedes", adr.as_str()),
        (initiative_code.as_str(), "supports", document_code.as_str()),
        (document_code.as_str(), "impacts", "payments-api"),
        (waiter.as_str(), "external", "https://example.com/rfc"),
        (initiative_code.as_str(), "parent", blocker.as_str()),
    ] {
        let (code, stdout, stderr) = run_cli(cfg, &["unlink", source, relationship, target]).await;
        assert_eq!(code, 0, "unlink {relationship} failed: {stderr}");
        assert!(stdout.starts_with("Kairos removed the link: "), "{stdout}");
    }
    let (code, stdout, stderr) = run_cli(cfg, &["tasks", "show", &waiter]).await;
    assert_eq!(code, 0, "tasks show failed: {stderr}");
    assert!(stdout.contains("  metadata: -\n"), "{stdout}");
    assert!(stdout.contains("  external: -\n"), "{stdout}");
    assert!(!stdout.contains("blocked by:"), "{stdout}");
    // The parent edge comes back with link.
    let (code, stdout, stderr) =
        run_cli(cfg, &["link", &initiative_code, "parent", &blocker]).await;
    assert_eq!(code, 0, "link parent failed: {stderr}");
    assert!(
        stdout.ends_with(&format!("{initiative_code} is the parent of {blocker}.\n")),
        "{stdout}"
    );

    // --- teardown ----------------------------------------------------------------
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db after test");
}
