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
const SCRATCH_DB: &str = "kairos_cli_t0037_test";
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
    format!("{base}/{db_name}")
}

/// Run the `kairos` binary with `KAIROS_CONFIG_DIR` pointed at the test's
/// scratch config dir; returns (exit_code, stdout, stderr).
async fn run_cli(config_dir: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(args)
        .env("KAIROS_CONFIG_DIR", config_dir)
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
    let scratch_url = with_database(&admin_url, SCRATCH_DB);

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
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        oidc_issuer_url: ISSUER.to_string(),
        oidc_audience: AUDIENCE.to_string(),
        base_domain: Some("kairos.test".to_string()),
        single_tenant: None,
        deployment_admins: vec![],
        log_level: "info".to_string(),
        log_format: LogFormat::Json,
        dev_ui: false,
        web_dist: None,
        web_client_id: "kairos-web".to_string(),
        api_bearer: kairos_server::config::ApiBearer::AccessToken,
        web_client_secret: None,
        public_url: None,
        webhook_signing_key: None,
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
        stdout.starts_with("Created initiative "),
        "unexpected create rendering: {stdout}"
    );
    let initiative_code = stdout
        .split_whitespace()
        .nth(2)
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
    assert!(stderr.contains("invalid transition (422)"), "{stderr}");
    assert!(stderr.contains("Allowed target columns:"), "{stderr}");
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
        format!("Transitioned task {task_code} to column {todo}")
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
    assert_eq!(
        results["results"]["tasks"][0]["short_code"],
        task_code.as_str()
    );
    assert_eq!(
        results["results"]["initiatives"][0]["short_code"],
        initiative_code.as_str()
    );

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
    assert!(stderr.contains("server-current version is 2"), "{stderr}");
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
        format!("Edited task {task_code}: now version 3")
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
    assert_eq!(stdout.trim(), format!("Deleted {task_code}"));

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
    assert!(stdout.contains("bound to payments-api"), "{stdout}");

    let (code, stdout, stderr) =
        run_cli(config_dir.path(), &["repos", "get", "payments-api"]).await;
    assert_eq!(code, 0, "repos get failed: {stderr}");
    assert!(stdout.contains("cargo test before every PR"), "{stdout}");
    assert!(stdout.contains("How to work here"), "{stdout}");

    // A task created with --repo and no --board routes to the owner's board.
    let (code, stdout, stderr) = run_cli(
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
    assert_eq!(code, 0, "tasks create --repo failed: {stderr}");
    let routed: serde_json::Value = serde_json::from_str(&stdout).expect("task DTO JSON");
    assert_eq!(
        routed["board_id"].as_str(),
        Some(delivery_board_id.as_str())
    );
    assert_eq!(routed["repository"]["slug"], "payments-api");

    let (code, stdout, stderr) = run_cli(config_dir.path(), &["repos", "unbind", &bind_code]).await;
    assert_eq!(code, 0, "repos unbind failed: {stderr}");
    assert!(stdout.contains("unbound"), "{stdout}");

    // --- teardown ----------------------------------------------------------------
    drop(conn);
    sql_query(format!("DROP DATABASE IF EXISTS {SCRATCH_DB} WITH (FORCE)"))
        .execute(&mut admin_conn)
        .expect("dropping scratch db after test");
}
