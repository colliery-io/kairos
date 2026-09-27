//! COLLIERY-T-0213: `kairos login --email` end-to-end, through the real
//! binary, against the production router booted in-process on an ephemeral
//! port over a scratch database.
//!
//! The server has `KAIROS_LOCAL_AUTH` on and a local account, the same
//! fixture that `kairos-server/tests/local_login.rs` uses. Most cases run on
//! a deployment with NO issuer (KAIROS-T-0208), which is the deployment that
//! could not use the CLI at all before this task.
//!
//! Every run of the binary has `KAIROS_CONFIG_DIR`, `XDG_CONFIG_HOME` and
//! `HOME` pointed at a temporary directory. No test reads or writes the
//! credential cache of the person who runs the suite.
//!
//! The password goes to the binary on standard input. One test drives the
//! terminal prompt through a pseudo-terminal; it needs `python3`, and it
//! says so and does nothing when there is none.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use tokio::io::AsyncWriteExt;

use kairos_client::{Error, KairosClient};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::config::{AppConfig, LogFormat};
use kairos_server::local_auth::hash_password;
use kairos_server::middleware::auth::Authenticator;

/// The live dev/test issuer (`.angreal/dex/config.yaml`).
const ISSUER: &str = "http://localhost:41558/dex";
const AUDIENCE: &str = "kairos-cli";
const TENANT: &str = "acme";
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

const EMAIL: &str = "ada@example.test";
/// A fixture password. It has a space in it, and no character that a shell
/// or JSON changes, so an assertion that it is absent from the output means
/// what it says.
const PASSWORD: &str = "correct horse battery staple 0213";
const WRONG_PASSWORD: &str = "not the password at all 0213";
const UNKNOWN_EMAIL: &str = "nobody@example.test";

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("database url has a path");
    format!("{base}/{db_name}")
}

/// What one run of the binary printed.
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn all(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

/// Run the `kairos` binary with its config directory in `config_dir`, and
/// `stdin` (when there is one) written to its standard input.
async fn run_cli(config_dir: &Path, args: &[&str], stdin: Option<&str>) -> Run {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(args)
        .env("KAIROS_CONFIG_DIR", config_dir)
        // The two fallbacks of the config directory. With these set, a
        // defect in the resolution order still cannot reach the real home.
        .env("XDG_CONFIG_HOME", config_dir)
        .env("HOME", config_dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("running the kairos binary");
    if let Some(input) = stdin {
        let mut pipe = child.stdin.take().expect("stdin piped");
        // The binary can exit before it reads (a usage error), which closes
        // the pipe. That is not a failure of the test.
        let _ = pipe.write_all(input.as_bytes()).await;
        drop(pipe);
    }
    let output = tokio::time::timeout(std::time::Duration::from_secs(60), child.wait_with_output())
        .await
        .expect("the kairos binary finished in time")
        .expect("the kairos binary ran");
    Run {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A config directory by its path, with the `path()` of a `TempDir`.
struct ConfigDir(std::path::PathBuf);

impl ConfigDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

/// How the fixture deployment lets people log in.
#[derive(Clone, Copy)]
struct Shape {
    issuer: bool,
    local_auth: bool,
    max_failures: u32,
}

const LOCAL_ONLY: Shape = Shape {
    issuer: false,
    local_auth: true,
    max_failures: 100,
};

/// A booted deployment over its own scratch database.
struct Deployment {
    base_url: String,
    scratch_db: String,
}

impl Deployment {
    /// Scratch database, tenant `acme`, the account [`EMAIL`] with
    /// [`PASSWORD`] as an organization admin, and the production router on an
    /// ephemeral port.
    async fn boot(scratch_db: &str, shape: Shape) -> Self {
        let admin_url = admin_database_url();
        let mut admin = PgConnection::establish(&admin_url)
            .expect("connecting to compose postgres (is the stack up? `angreal services up`)");
        sql_query(format!("DROP DATABASE IF EXISTS {scratch_db} WITH (FORCE)"))
            .execute(&mut admin)
            .expect("dropping scratch db");
        sql_query(format!("CREATE DATABASE {scratch_db}"))
            .execute(&mut admin)
            .expect("creating scratch db");
        let scratch_url = with_database(&admin_url, scratch_db);

        let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch db");
        run_public_migrations(&mut conn).expect("public migrations");
        provision_tenant(&mut conn, TENANT, "Acme Inc").expect("provisioning tenant");
        let org_id: uuid::Uuid = organizations::table
            .filter(organizations::slug.eq(TENANT))
            .select(organizations::id)
            .first(&mut conn)
            .expect("org row");
        let hash = hash_password(PASSWORD).expect("hash");
        let (user, _) = kairos_db::local_auth::upsert_local_user(&mut conn, EMAIL, "Ada", &hash)
            .expect("local account");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: user.id,
                role: OrgRole::Admin,
            })
            .execute(&mut conn)
            .expect("membership");
        drop(conn);

        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(if shape.issuer {
            Authenticator::with_static_keys(ISSUER, AUDIENCE, [])
        } else {
            // What `build_state` makes when the config names no issuer.
            Authenticator::disabled()
        });
        let config = AppConfig {
            database_url: scratch_url.clone(),
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            oidc_issuer_url: shape.issuer.then(|| ISSUER.to_string()),
            oidc_audience: shape.issuer.then(|| AUDIENCE.to_string()),
            base_domain: Some("kairos.test".to_string()),
            single_tenant: None,
            // The first-boot admin of a deployment is a deployment admin
            // under this name (`local:<email>`); the fixture does the same.
            deployment_admins: vec![kairos_db::local_auth::local_external_id(EMAIL)],
            log_level: "info".to_string(),
            log_format: LogFormat::Json,
            embed_refresh_secs: 0,
            dev_ui: false,
            web_dist: None,
            web_client_id: "kairos-web".to_string(),
            api_bearer: kairos_server::config::ApiBearer::AccessToken,
            web_client_secret: None,
            public_url: None,
            webhook_signing_key: None,
            otel_endpoint: None,
            otel_sample_ratio: 1.0,
            auth_max_failures: shape.max_failures,
            auth_failure_window_secs: 300,
            auth_lockout_secs: 60,
            trusted_proxy: false,
            local_auth: shape.local_auth,
            session_ttl_secs: 14 * 24 * 60 * 60,
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
        Self {
            base_url,
            scratch_db: scratch_db.to_string(),
        }
    }

    fn teardown(self) {
        let mut admin = PgConnection::establish(&admin_database_url()).expect("admin");
        sql_query(format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.scratch_db
        ))
        .execute(&mut admin)
        .expect("dropping scratch db after test");
    }
}

/// `kairos login --url <URL> --email <EMAIL> --tenant acme`, password on
/// standard input.
async fn login(config_dir: &Path, base_url: &str, email: &str, password: &str) -> Run {
    run_cli(
        config_dir,
        &[
            "login", "--url", base_url, "--email", email, "--tenant", TENANT,
        ],
        Some(&format!("{password}\n")),
    )
    .await
}

fn read_cache(config_dir: &Path) -> serde_json::Value {
    let path = config_dir.join("credentials.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the cache file"))
        .expect("cache JSON")
}

/// No output of any run holds the password, the bearer, or anything with the
/// shape of a session bearer (case i).
fn assert_no_secret(runs: &[(&str, &Run)], bearer: Option<&str>) {
    for (label, run) in runs {
        let text = run.all();
        for secret in [PASSWORD, WRONG_PASSWORD] {
            assert!(
                !text.contains(secret),
                "{label}: the output holds a password:\n{text}"
            );
        }
        assert!(
            !text.contains("kairos_ss_"),
            "{label}: the output holds a session bearer:\n{text}"
        );
        if let Some(bearer) = bearer {
            assert!(
                !text.contains(bearer),
                "{label}: the output holds the bearer:\n{text}"
            );
        }
    }
}

/// Cases a, b, h and i on a deployment with local accounts and no issuer:
/// log in, use the session, look at the cache, log out.
#[tokio::test]
async fn login_with_a_password_then_whoami_then_logout() {
    let deployment = Deployment::boot("kairos_cli_t0213_golden_test", LOCAL_ONLY).await;
    let base_url = deployment.base_url.clone();
    // A config directory that does not exist yet, so that the CLI makes it.
    let scratch = tempfile::tempdir().expect("scratch dir");
    let config_dir = ConfigDir(scratch.path().join("kairos"));

    // --- a: log in with the password on standard input ----------------------
    let logged_in = login(config_dir.path(), &base_url, EMAIL, PASSWORD).await;
    assert_eq!(logged_in.code, 0, "login failed: {}", logged_in.all());
    assert!(
        logged_in
            .stdout
            .contains(&format!("Logged in to {base_url} as {EMAIL}.")),
        "{}",
        logged_in.stdout
    );

    // --- b: the cache holds the bearer and its expiry, mode 0600 ------------
    let credentials_path = config_dir.path().join("credentials.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&credentials_path)
            .expect("the cache file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "credentials.json must be 0600");
        let dir_mode = std::fs::metadata(config_dir.path())
            .expect("the config dir")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700, "the config dir is owner-only");
    }
    let cache = read_cache(config_dir.path());
    let entry = &cache["deployments"][&base_url];
    let bearer = entry["access_token"]
        .as_str()
        .expect("the cached bearer")
        .to_string();
    assert!(bearer.starts_with("kairos_ss_"), "a session bearer");
    assert_eq!(bearer.len(), "kairos_ss_".len() + 64);
    assert_eq!(entry["kind"], "local_session", "{entry:#}");
    assert_eq!(entry["email"], EMAIL, "{entry:#}");
    assert_eq!(entry["tenant"], TENANT, "{entry:#}");
    assert!(entry.get("refresh_token").is_none(), "no refresh token");
    assert_eq!(entry["issuer"], "", "a local session has no issuer");
    let expires_at = entry["expires_at"].as_u64().expect("expires_at");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let fortnight = now + 14 * 24 * 60 * 60;
    assert!(
        expires_at.abs_diff(fortnight) < 300,
        "the expiry is the server's, a fortnight from now: {expires_at} vs {fortnight}"
    );

    // --- a: the session works for the other commands, unchanged ------------
    let whoami = run_cli(config_dir.path(), &["whoami"], None).await;
    assert_eq!(whoami.code, 0, "whoami failed: {}", whoami.all());
    assert!(
        whoami.stdout.contains(&format!("user:   Ada <{EMAIL}>")),
        "{}",
        whoami.stdout
    );
    assert!(
        whoami.stdout.contains("org:    acme (role: admin)"),
        "{}",
        whoami.stdout
    );

    let whoami_json = run_cli(config_dir.path(), &["whoami", "--json"], None).await;
    assert_eq!(whoami_json.code, 0, "{}", whoami_json.all());
    let identity: serde_json::Value =
        serde_json::from_str(&whoami_json.stdout).expect("whoami --json prints JSON");
    assert_eq!(identity["user"]["email"], EMAIL);

    let boards = run_cli(config_dir.path(), &["boards", "list", "--json"], None).await;
    assert_eq!(boards.code, 0, "boards list failed: {}", boards.all());

    // --- h: logout ends the session on the server and removes the entry -----
    let logout = run_cli(config_dir.path(), &["logout"], None).await;
    assert_eq!(logout.code, 0, "logout failed: {}", logout.all());
    assert!(
        logout
            .stdout
            .contains(&format!("Logged out of {base_url}.")),
        "{}",
        logout.stdout
    );
    assert!(
        logout
            .stdout
            .contains("The session is ended on the server."),
        "{}",
        logout.stdout
    );
    let cache = read_cache(config_dir.path());
    assert!(
        cache["deployments"]
            .as_object()
            .expect("deployments")
            .is_empty(),
        "logout must remove the entry: {cache:#}"
    );
    // The bearer that was in the cache is no longer a credential.
    let client = KairosClient::with_static_token(&base_url, &bearer).with_tenant(TENANT);
    match client.whoami().await {
        Err(Error::Unauthorized { .. }) => {}
        other => panic!("the bearer must not work after logout, got {other:?}"),
    }
    let after = run_cli(config_dir.path(), &["whoami"], None).await;
    assert_eq!(after.code, 2, "whoami after logout is an auth error");

    // --- i: no secret in any output -----------------------------------------
    assert_no_secret(
        &[
            ("login", &logged_in),
            ("whoami", &whoami),
            ("whoami --json", &whoami_json),
            ("boards list --json", &boards),
            ("logout", &logout),
            ("whoami after logout", &after),
        ],
        Some(&bearer),
    );

    deployment.teardown();
}

/// The procedure of the how-to "Use local accounts": the first admin logs in
/// with no tenant, because there is none yet, and makes the organization
/// with the CLI.
#[tokio::test]
async fn the_first_admin_makes_an_organization_with_the_cli() {
    let deployment = Deployment::boot("kairos_cli_t0213_firstorg_test", LOCAL_ONLY).await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");

    let logged_in = run_cli(
        config_dir.path(),
        &["login", "--url", &deployment.base_url, "--email", EMAIL],
        Some(PASSWORD),
    )
    .await;
    assert_eq!(logged_in.code, 0, "{}", logged_in.all());
    let cache = read_cache(config_dir.path());
    assert!(
        cache["deployments"][&deployment.base_url]
            .get("tenant")
            .is_none(),
        "no tenant was given, so none is cached: {cache:#}"
    );

    let created = run_cli(
        config_dir.path(),
        &[
            "admin", "tenants", "create", "--slug", "newco", "--name", "New Co",
        ],
        None,
    )
    .await;
    assert_eq!(created.code, 0, "{}", created.all());

    let whoami = run_cli(config_dir.path(), &["whoami", "--tenant", "newco"], None).await;
    assert_eq!(whoami.code, 0, "{}", whoami.all());
    assert!(
        whoami.stdout.contains("org:    newco (role: admin)"),
        "{}",
        whoami.stdout
    );

    assert_no_secret(
        &[
            ("login", &logged_in),
            ("admin tenants create", &created),
            ("whoami", &whoami),
        ],
        None,
    );
    deployment.teardown();
}

/// Case c: there is no `--password`. The run fails as a usage error, sends
/// nothing, and writes no cache.
#[tokio::test]
async fn the_password_is_not_an_argument() {
    let config_dir = tempfile::tempdir().expect("scratch config dir");
    let run = run_cli(
        config_dir.path(),
        &[
            "login",
            "--url",
            // Nothing listens here. A usage error comes before any request.
            "http://127.0.0.1:9",
            "--email",
            EMAIL,
            "--password",
            "x",
        ],
        None,
    )
    .await;
    assert_eq!(run.code, 2, "clap's usage error: {}", run.all());
    assert!(
        run.stderr.contains("unexpected argument '--password'"),
        "{}",
        run.stderr
    );
    assert!(!config_dir.path().join("credentials.json").exists());

    let help = run_cli(config_dir.path(), &["login", "--help"], None).await;
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("--email <EMAIL>"), "{}", help.stdout);
    assert!(
        !help.stdout.contains("--password"),
        "the help must not offer --password:\n{}",
        help.stdout
    );

    // --email with an option of the OAuth login is a usage error too.
    for (flag, value) in [
        ("--issuer", ISSUER),
        ("--client-id", "kairos-cli"),
        ("--bearer", "id_token"),
    ] {
        let run = run_cli(
            config_dir.path(),
            &[
                "login",
                "--url",
                "http://127.0.0.1:9",
                "--email",
                EMAIL,
                flag,
                value,
            ],
            None,
        )
        .await;
        assert_eq!(run.code, 2, "{flag}: {}", run.all());
        assert!(run.stderr.contains("cannot be used with"), "{}", run.stderr);
    }
}

/// Case d: no `--email`, on a deployment with no issuer. The message names
/// `--email` and says that the deployment uses local accounts.
#[tokio::test]
async fn login_without_email_on_a_deployment_with_no_issuer_names_email() {
    let deployment = Deployment::boot("kairos_cli_t0213_noissuer_test", LOCAL_ONLY).await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");

    let run = run_cli(
        config_dir.path(),
        &["login", "--url", &deployment.base_url],
        None,
    )
    .await;
    assert_eq!(run.code, 1, "{}", run.all());
    assert!(run.stderr.contains("--email"), "{}", run.stderr);
    assert!(run.stderr.contains("local accounts"), "{}", run.stderr);
    assert!(
        run.stderr.contains(&format!(
            "kairos login --url {} --email <EMAIL>",
            deployment.base_url
        )),
        "the message gives the command: {}",
        run.stderr
    );
    assert!(!config_dir.path().join("credentials.json").exists());

    deployment.teardown();
}

/// Case e: a wrong password and an unknown account give the same exit code
/// and the same message, and write nothing.
#[tokio::test]
async fn a_failed_login_does_not_say_which_part_was_wrong() {
    let deployment = Deployment::boot("kairos_cli_t0213_wrong_test", LOCAL_ONLY).await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");

    let wrong_password = login(
        config_dir.path(),
        &deployment.base_url,
        EMAIL,
        WRONG_PASSWORD,
    )
    .await;
    let unknown_account = login(
        config_dir.path(),
        &deployment.base_url,
        UNKNOWN_EMAIL,
        PASSWORD,
    )
    .await;

    assert_eq!(wrong_password.code, 2, "{}", wrong_password.all());
    assert_eq!(unknown_account.code, 2, "{}", unknown_account.all());
    assert_eq!(
        wrong_password.stderr, unknown_account.stderr,
        "the two failures must print the same text"
    );
    assert_eq!(wrong_password.stdout, unknown_account.stdout);
    assert!(
        wrong_password
            .stderr
            .contains("the deployment did not accept the email and password"),
        "{}",
        wrong_password.stderr
    );
    // The message names neither address: a name in one of them is a
    // difference between them.
    for run in [&wrong_password, &unknown_account] {
        assert!(!run.stderr.contains(EMAIL), "{}", run.stderr);
        assert!(!run.stderr.contains(UNKNOWN_EMAIL), "{}", run.stderr);
    }
    assert!(
        !config_dir.path().join("credentials.json").exists(),
        "a failed login writes no cache"
    );

    // An empty password is refused before any request.
    let empty = login(config_dir.path(), &deployment.base_url, EMAIL, "").await;
    assert_eq!(empty.code, 1, "{}", empty.all());
    assert!(empty.stderr.contains("no password"), "{}", empty.stderr);

    assert_no_secret(
        &[
            ("wrong password", &wrong_password),
            ("unknown account", &unknown_account),
            ("empty password", &empty),
        ],
        None,
    );
    deployment.teardown();
}

/// The throttle (KAIROS-T-0202): after too many failures the server answers
/// 429, and the CLI says how long to wait.
#[tokio::test]
async fn a_throttled_login_says_to_wait() {
    let deployment = Deployment::boot(
        "kairos_cli_t0213_throttle_test",
        Shape {
            max_failures: 2,
            ..LOCAL_ONLY
        },
    )
    .await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");

    for _ in 0..2 {
        let run = login(
            config_dir.path(),
            &deployment.base_url,
            EMAIL,
            WRONG_PASSWORD,
        )
        .await;
        assert_eq!(run.code, 2, "{}", run.all());
    }
    // Locked out now: the correct password is refused too.
    let run = login(config_dir.path(), &deployment.base_url, EMAIL, PASSWORD).await;
    assert_eq!(run.code, 1, "{}", run.all());
    assert!(
        run.stderr.contains("too many failed login attempts"),
        "{}",
        run.stderr
    );
    let wait = run
        .stderr
        .split("Wait ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("the message gives the wait: {}", run.stderr));
    assert!((1..=61).contains(&wait), "the wait is the server's: {wait}");
    assert!(!config_dir.path().join("credentials.json").exists());
    assert_no_secret(&[("throttled", &run)], None);

    deployment.teardown();
}

/// Case f: `--email` on a deployment with local accounts off.
#[tokio::test]
async fn login_with_email_on_a_deployment_with_local_accounts_off() {
    let deployment = Deployment::boot(
        "kairos_cli_t0213_off_test",
        Shape {
            issuer: true,
            local_auth: false,
            max_failures: 100,
        },
    )
    .await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");

    let run = login(config_dir.path(), &deployment.base_url, EMAIL, PASSWORD).await;
    assert_eq!(run.code, 1, "{}", run.all());
    assert!(
        run.stderr
            .contains("local accounts are off on this deployment"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains(&format!("`kairos login --url {}`", deployment.base_url))
            && run.stderr.contains("issuer"),
        "the message says that login without --email uses the issuer: {}",
        run.stderr
    );
    assert!(!config_dir.path().join("credentials.json").exists());
    assert_no_secret(&[("local accounts off", &run)], None);

    deployment.teardown();
}

/// A deployment with BOTH an issuer and local accounts: `--email` is the
/// password login, and no `--email` is still the device flow (case j for
/// this shape; the issuer-only shape is `cli_live.rs`, unchanged).
#[tokio::test]
async fn a_deployment_with_both_offers_both() {
    let deployment = Deployment::boot(
        "kairos_cli_t0213_both_test",
        Shape {
            issuer: true,
            local_auth: true,
            max_failures: 100,
        },
    )
    .await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");

    let run = login(config_dir.path(), &deployment.base_url, EMAIL, PASSWORD).await;
    assert_eq!(run.code, 0, "{}", run.all());
    let cache = read_cache(config_dir.path());
    assert_eq!(
        cache["deployments"][&deployment.base_url]["kind"],
        "local_session"
    );
    let whoami = run_cli(config_dir.path(), &["whoami"], None).await;
    assert_eq!(whoami.code, 0, "{}", whoami.all());

    // No --email: the device flow starts. Read until it asks for approval,
    // then stop it; nobody approves the grant here.
    use tokio::io::{AsyncBufReadExt, BufReader};
    let device_dir = tempfile::tempdir().expect("scratch config dir");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_kairos"))
        .args(["login", "--url", &deployment.base_url])
        .env("KAIROS_CONFIG_DIR", device_dir.path())
        .env("XDG_CONFIG_HOME", device_dir.path())
        .env("HOME", device_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawning kairos login");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
    let mut seen = Vec::new();
    let started = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await.expect("reading login output") {
            let done = line.contains("user_code=") || line.contains("enter code");
            seen.push(line);
            if done {
                return true;
            }
        }
        false
    })
    .await
    .expect("the device flow started in time");
    child.kill().await.expect("stopping the device flow");
    assert!(started, "the device flow must start: {seen:#?}");
    assert!(
        seen.iter()
            .any(|line| line == &format!("Discovered OIDC issuer: {ISSUER}")),
        "{seen:#?}"
    );

    assert_no_secret(&[("login", &run), ("whoami", &whoami)], None);
    deployment.teardown();
}

/// A listener that counts the connections it gets, and answers none.
async fn counting_listener() -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral port");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = connections.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            drop(socket);
        }
    });
    (url, connections)
}

/// Write a cache that holds one local session for `url`.
fn write_local_session(config_dir: &Path, url: &str, bearer: &str, expires_at: u64) {
    let cache = serde_json::json!({
        "version": 1,
        "deployments": {
            url: {
                "access_token": bearer,
                "expires_at": expires_at,
                "issuer": "",
                "client_id": "",
                "tenant": TENANT,
                "kind": "local_session",
                "email": EMAIL,
            }
        }
    });
    std::fs::write(
        config_dir.join("credentials.json"),
        serde_json::to_string_pretty(&cache).expect("json"),
    )
    .expect("writing the cache");
}

/// Case g: an expired local session. The CLI says so, gives the command to
/// log in again, and sends NO request: the listener sees no connection. It
/// does not try an OAuth refresh, which has no issuer to go to.
#[tokio::test]
async fn an_expired_session_names_the_login_command_and_sends_nothing() {
    let (url, connections) = counting_listener().await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");
    let bearer = format!("kairos_ss_{}", "e".repeat(64));
    write_local_session(config_dir.path(), &url, &bearer, 1);

    let mut runs = Vec::new();
    for args in [
        vec!["whoami"],
        vec!["whoami", "--json"],
        vec!["boards", "list"],
        vec!["tasks", "list", "--json"],
    ] {
        let run = run_cli(config_dir.path(), &args, None).await;
        assert_eq!(run.code, 2, "{args:?}: {}", run.all());
        assert!(
            run.stderr
                .contains(&format!("the session for {url} has expired")),
            "{args:?}: {}",
            run.stderr
        );
        assert!(
            run.stderr.contains(&format!(
                "Run `kairos login --url {url} --email {EMAIL} --tenant {TENANT}`"
            )),
            "{args:?}: the message gives the exact command: {}",
            run.stderr
        );
        assert!(
            !run.stderr.contains("refresh"),
            "{args:?}: a local session has no refresh: {}",
            run.stderr
        );
        assert!(run.stdout.is_empty(), "{args:?}: {}", run.stdout);
        runs.push(run);
    }
    assert_eq!(
        connections.load(Ordering::SeqCst),
        0,
        "no request goes out with an expired bearer"
    );
    // The entry stays: only a login or a logout changes the cache.
    assert_eq!(
        read_cache(config_dir.path())["deployments"][&url]["kind"],
        "local_session"
    );
    let labelled: Vec<(&str, &Run)> = runs.iter().map(|run| ("expired", run)).collect();
    assert_no_secret(&labelled, Some(&bearer));
}

/// Logout when the server cannot be reached: the local entry is removed all
/// the same, and the person is told that the session is not ended.
#[tokio::test]
async fn logout_removes_the_entry_when_the_server_does_not_answer() {
    let config_dir = tempfile::tempdir().expect("scratch config dir");
    let bearer = format!("kairos_ss_{}", "f".repeat(64));
    // Port 9 (discard): nothing listens, the connection is refused.
    let url = "http://127.0.0.1:9";
    write_local_session(config_dir.path(), url, &bearer, 4_000_000_000);

    let run = run_cli(config_dir.path(), &["logout"], None).await;
    assert_eq!(run.code, 1, "{}", run.all());
    assert!(
        run.stderr
            .contains("the deployment did not end the session"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("The local entry is removed from the cache"),
        "{}",
        run.stderr
    );
    let cache = read_cache(config_dir.path());
    assert!(
        cache["deployments"]
            .as_object()
            .expect("deployments")
            .is_empty(),
        "the entry is removed: {cache:#}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(config_dir.path().join("credentials.json"))
            .expect("the cache file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert_no_secret(&[("logout", &run)], Some(&bearer));
}

/// Criterion 1, the terminal path: with standard input a terminal, the CLI
/// asks for the password and does not echo it.
///
/// The workspace has no pseudo-terminal crate, so the test drives one
/// through `python3` (its `pty` module is in the standard library). With no
/// `python3` the test says so and checks nothing.
#[cfg(unix)]
#[tokio::test]
async fn the_terminal_prompt_does_not_echo_the_password() {
    const DRIVER: &str = r#"
import os, pty, select, sys, time

binary, url, email, tenant, password = sys.argv[1:6]
pid, fd = pty.fork()
if pid == 0:
    os.execv(binary, [binary, "login", "--url", url, "--email", email, "--tenant", tenant])

seen = b""
sent = False
deadline = time.time() + 60
status = None
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.2)
    if ready:
        try:
            chunk = os.read(fd, 4096)
        except OSError:
            chunk = b""
        if not chunk:
            break
        seen += chunk
    if not sent and b"Password" in seen:
        # Give the CLI time to turn the echo off, as a person would.
        time.sleep(0.5)
        os.write(fd, password.encode() + b"\n")
        sent = True
else:
    # The CLI did not finish. Stop it, so that the test fails and does not
    # wait for ever.
    os.kill(pid, 9)
_, status = os.waitpid(pid, 0)
sys.stdout.write(seen.decode(errors="replace"))
sys.stdout.write("\nSENT=%s EXIT=%d\n" % (sent, os.waitstatus_to_exitcode(status)))
"#;

    let python = ["python3", "/usr/bin/python3"]
        .into_iter()
        .find(|candidate| {
            std::process::Command::new(candidate)
                .args(["-c", "import pty, os; os.waitstatus_to_exitcode"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        });
    let Some(python) = python else {
        eprintln!(
            "SKIPPED the_terminal_prompt_does_not_echo_the_password: no python3 (3.9 or later) \
             to drive a pseudo-terminal"
        );
        return;
    };

    let deployment = Deployment::boot("kairos_cli_t0213_tty_test", LOCAL_ONLY).await;
    let config_dir = tempfile::tempdir().expect("scratch config dir");

    let output = tokio::process::Command::new(python)
        .args([
            "-c",
            DRIVER,
            env!("CARGO_BIN_EXE_kairos"),
            &deployment.base_url,
            EMAIL,
            TENANT,
            PASSWORD,
        ])
        .env("KAIROS_CONFIG_DIR", config_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .env("HOME", config_dir.path())
        .stdin(Stdio::null())
        .output()
        .await
        .expect("running the pseudo-terminal driver");
    let terminal = String::from_utf8_lossy(&output.stdout).into_owned();
    let driver_errors = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        terminal.contains(&format!("Password for {EMAIL}: ")),
        "the CLI asks for the password on the terminal:\n{terminal}\n{driver_errors}"
    );
    assert!(
        terminal.contains("SENT=True EXIT=0"),
        "the login succeeds:\n{terminal}\n{driver_errors}"
    );
    assert!(
        terminal.contains(&format!("Logged in to {} as {EMAIL}.", deployment.base_url)),
        "{terminal}"
    );
    // The point of the test. Everything that the terminal showed is in
    // `terminal`, the echo of the typed characters included, if there is one.
    assert!(
        !terminal.contains(PASSWORD),
        "the terminal must not show the password:\n{terminal}"
    );
    assert!(!terminal.contains("kairos_ss_"), "{terminal}");

    let cache = read_cache(config_dir.path());
    assert_eq!(
        cache["deployments"][&deployment.base_url]["kind"],
        "local_session"
    );
    deployment.teardown();
}
