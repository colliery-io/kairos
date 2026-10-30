//! The scenarios of the base index (COLLIERY-T-1854): a checkout starts
//! from the nearest index in Kairos, a far branch is told to rebase, and
//! with no Kairos the local index is updated.
//!
//! A scenario with Kairos runs the production router in the test, over a
//! scratch database of the dev stack (`angreal services up`), as
//! `kairos-server/tests/code_index.rs` does. The repository `payments-api`
//! is a small Python git repository (no Rust, so no SCIP run), and the
//! server fetches it from its folder. The checkout is a clone of it. The
//! `kairos` binary connects with a service-account key, as the plugin does.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use cucumber::{given, then, when};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use kairos_client::KairosClient;
use kairos_client::types_org::CreateTeamRequest;
use kairos_client::types_repositories::CreateRepositoryRequest;
use kairos_db::api_keys::{self, NewApiKey};
use kairos_db::models::{NewOrganizationMember, NewServiceAccountUser, OrgRole, User};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_embed::DeterministicProvider;
use kairos_index::{FakeSummarizer, Index, SUMMARIZED_KINDS, UpdateOptions};
use kairos_server::app;
use kairos_server::code_index::CodeIndexService;
use kairos_server::config::{AppConfig, LogFormat};
use kairos_server::middleware::auth::Authenticator;
use kairos_server::service_accounts::auth::{generate_key, hash_key};
use tempfile::TempDir;

use super::{CliWorld, KAIROS, clean_env, index_db};

/// Same default as `.angreal/task_db.py`.
const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

/// The slug of the repository in Kairos.
const REPOSITORY: &str = "payments-api";

/// A port that no server listens on: a connection to it is refused.
const NO_KAIROS: &str = "http://127.0.0.1:1";

const STATS: &str = "\
def total(xs):
    \"\"\"The sum of the values.\"\"\"
    return sum(xs)


def mean(xs):
    \"\"\"The mean of the values.\"\"\"
    return total(xs) / len(xs)


def spread(xs):
    return max(xs) - min(xs)
";

const REPORT: &str = "\
from pkg.stats import mean, spread


def render(rows):
    return f\"mean {mean(rows)}, spread {spread(rows)}\"


def header(title):
    return title.upper()
";

const LOADER: &str = "\
def load(path):
    with open(path) as f:
        return [float(line) for line in f]


def count_lines(path):
    return len(load(path))
";

/// The 3 changes of the branch: one function in each file. Each pair is
/// (file, the text to replace, its replacement, the changed function).
const BRANCH_CHANGES: &[(&str, &str, &str, &str)] = &[
    (
        "pkg/stats.py",
        "return max(xs) - min(xs)",
        "return max(xs) - min(xs) if xs else 0",
        "spread",
    ),
    (
        "pkg/report.py",
        "return title.upper()",
        "return title.strip().upper()",
        "header",
    ),
    (
        "pkg/loader.py",
        "return [float(line) for line in f]",
        "return [float(line) for line in f if line.strip()]",
        "load",
    ),
];

fn admin_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_database(url: &str, db_name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a database in DATABASE_URL");
    format!("{base}/{db_name}")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@kairos.test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@kairos.test")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().expect("a folder")).expect("make the folder");
    fs::write(path, text).expect("write the file");
}

/// A git repository with commit A on `main`. Returns commit A.
fn repository_at_a(dir: &Path) -> String {
    fs::create_dir_all(dir).expect("make the repository");
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, "pkg/__init__.py", "");
    write(dir, "pkg/stats.py", STATS);
    write(dir, "pkg/report.py", REPORT);
    write(dir, "pkg/loader.py", LOADER);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "A"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// The index of `commit` of the repository at `repo`, as a checkout builds
/// it: the tree of the commit, the fake summarizer, the deterministic
/// vectors.
///
/// `options` choose the summaries: a part of them for a scenario.
fn index_with(repo: &Path, commit: &str, out: &Path, options: &UpdateOptions) -> Vec<u8> {
    let tree = TempDir::new().expect("a folder for the tree");
    let archive = Command::new("git")
        .args(["archive", "--format=tar", commit])
        .current_dir(repo)
        .output()
        .expect("git archive");
    assert!(archive.status.success());
    let mut untar = Command::new("tar")
        .args(["-x", "-C"])
        .arg(tree.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("tar");
    use std::io::Write;
    untar
        .stdin
        .take()
        .expect("the stdin of tar")
        .write_all(&archive.stdout)
        .expect("write to tar");
    assert!(untar.wait().expect("tar").success());
    kairos_index::update(
        tree.path(),
        out,
        &mut FakeSummarizer::default(),
        &DeterministicProvider::default(),
        options,
    )
    .expect("the index of the commit builds");
    fs::read(out).expect("read the index")
}

/// A Kairos server in the test, with a scratch database, the repository
/// `payments-api` and a service-account key that is an organization admin.
pub struct Kairos {
    pub url: String,
    pub key: String,
    scratch_db: String,
    /// The git repository of `payments-api`.
    pub remote: PathBuf,
    /// Commit A of `main`.
    pub a: String,
    /// The folders of the remote, the clones of the server and the index
    /// files.
    work: TempDir,
}

impl std::fmt::Debug for Kairos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kairos").field("url", &self.url).finish()
    }
}

impl Drop for Kairos {
    fn drop(&mut self) {
        if let Ok(mut admin) = PgConnection::establish(&admin_database_url()) {
            let _ = sql_query(format!(
                "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                self.scratch_db
            ))
            .execute(&mut admin);
        }
    }
}

impl Kairos {
    async fn start() -> Kairos {
        let scratch_db = format!("ktest_cli_t1854_{}", uuid::Uuid::new_v4().simple());
        let admin_url = admin_database_url();
        let mut admin = PgConnection::establish(&admin_url)
            .expect("connect to the dev stack (is it up? `angreal services up`)");
        sql_query(format!("CREATE DATABASE {scratch_db}"))
            .execute(&mut admin)
            .expect("create the scratch database");
        let scratch_url = with_database(&admin_url, &scratch_db);
        let mut conn = PgConnection::establish(&scratch_url).expect("connect to the scratch db");
        run_public_migrations(&mut conn).expect("public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provision acme");
        sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .expect("pin the tenant");
        let org_id: uuid::Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("the acme row");
        let sa: User = diesel::insert_into(users::table)
            .values(NewServiceAccountUser::new(
                "svc:claude-code".to_string(),
                "claude-code@svc.acme.kairos".to_string(),
                "claude-code",
            ))
            .returning(User::as_returning())
            .get_result(&mut conn)
            .expect("the service account");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: sa.id,
                role: OrgRole::Admin,
            })
            .execute(&mut conn)
            .expect("the membership");
        let key = generate_key("acme");
        api_keys::create_key(
            &mut conn,
            NewApiKey {
                user_id: sa.id,
                name: "t1854".to_string(),
                token_hash: hash_key(&key),
                prefix: "t1854".to_string(),
                created_by: sa.id,
                expires_at: None,
            },
        )
        .expect("the key");
        drop(conn);

        let work = TempDir::new().expect("a work folder");
        let remote = work.path().join(REPOSITORY);
        let a = repository_at_a(&remote);
        let from = remote.display().to_string();
        let service = Arc::new(CodeIndexService::new(
            work.path().join("server"),
            Arc::new(move |_: &kairos_db::models::repositories::Repository| from.clone()),
        ));
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(Authenticator::with_static_keys(
            "http://localhost:41558/dex",
            "kairos-cli",
            [],
        ));
        let mut state = app::state_with(config(&scratch_url), pool, auth);
        state.code_index = Some(service);
        let router = app::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("the test server");
        });

        let kairos = Kairos {
            url,
            key,
            scratch_db,
            remote,
            a,
            work,
        };
        let client = kairos.client();
        client
            .create_team(&CreateTeamRequest {
                name: "Platform".into(),
                slug: "platform".into(),
                code_prefix: "PLATFORM".into(),
                team_type: None,
            })
            .await
            .expect("the team");
        client
            .create_repository(&CreateRepositoryRequest {
                slug: Some(REPOSITORY.into()),
                forge: "github".into(),
                repo_full_name: format!("acme/{REPOSITORY}"),
                repo_url: format!("https://github.com/acme/{REPOSITORY}"),
                default_branch: None,
                team: "platform".into(),
                description: None,
            })
            .await
            .expect("the repository");
        kairos
    }

    fn client(&self) -> KairosClient {
        KairosClient::with_static_token(&self.url, &self.key)
    }

    /// Build the summarized index of `commit` and send it to Kairos.
    async fn upload(&self, commit: &str) -> Vec<u8> {
        self.upload_with(commit, &UpdateOptions::default()).await
    }

    /// [`Self::upload`] with these options.
    async fn upload_with(&self, commit: &str, options: &UpdateOptions) -> Vec<u8> {
        let file = self.work.path().join(format!("{commit}.db"));
        let _ = fs::remove_file(&file);
        let bytes = index_with(&self.remote, commit, &file, options);
        self.client()
            .upload_code_index(REPOSITORY, commit, Some("main"), bytes.clone())
            .await
            .expect("the upload");
        bytes
    }
}

fn config(scratch_url: &str) -> AppConfig {
    AppConfig {
        database_url: scratch_url.to_string(),
        pool_size: 16,
        pool_min_idle: 0,
        job_pool_size: 6,
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        oidc_issuer_url: None,
        oidc_audience: None,
        base_domain: Some("kairos.test".to_string()),
        single_tenant: None,
        single_tenant_name: None,
        auto_join_domains: vec![],
        deployment_admins: vec![],
        log_level: "info".to_string(),
        log_format: LogFormat::Json,
        dev_ui: false,
        web_dist: None,
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
        web_client_id: "kairos-web".to_string(),
        api_bearer: kairos_server::config::ApiBearer::AccessToken,
        web_client_secret: None,
        public_url: None,
        webhook_signing_key: None,
        // KAIROS-T-0343: the hosted provider of a scenario has a secret.
        secrets_key: Some(kairos_server::secrets::SecretsKey::from_bytes([3u8; 32])),
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
    }
}

/// What the scenarios of the base index keep.
#[derive(Debug, Default)]
pub struct BaseWorld {
    pub kairos: Option<Kairos>,
    /// The checkout: a clone of the remote, or a repository of its own.
    pub checkout: Option<TempDir>,
    /// The index file of A that Kairos has.
    pub index_a: Vec<u8>,
    /// The URL and the key that the CLI gets.
    pub url: String,
    pub key: String,
    /// The content hash of the changed file before the change.
    pub hash_before: String,
    /// The last run of `kairos index update`.
    pub run: Option<Output>,
    /// KAIROS-T-0348: the folder the server started in, and its config
    /// folder, kept for the scenario.
    pub elsewhere: Option<(TempDir, TempDir)>,
    /// KAIROS-T-0516: commit C of main (250 new files after A), and commit
    /// B after it (1 change).
    pub c: String,
    pub b: String,
    /// KAIROS-T-0646: a worktree of the checkout, in a folder of its own.
    /// When it is set, it is the root of the scenario.
    pub worktree: Option<(TempDir, PathBuf)>,
    /// KAIROS-T-0646: a board folder with clones and a worktree.
    pub board: Option<TempDir>,
}

impl BaseWorld {
    fn root(&self) -> &Path {
        if let Some((_, worktree)) = &self.worktree {
            return worktree;
        }
        self.checkout.as_ref().expect("no checkout").path()
    }

    fn kairos(&self) -> &Kairos {
        self.kairos.as_ref().expect("no Kairos")
    }

    fn index(&self) -> Index {
        Index::open(&index_db(self.root())).expect("open the index")
    }

    /// A clone of the remote, on `main`.
    fn clone_remote(&mut self) {
        let checkout = TempDir::new().expect("a folder for the checkout");
        let remote = self.kairos().remote.display().to_string();
        git(checkout.path(), &["clone", "-q", &remote, "."]);
        self.url = self.kairos().url.clone();
        self.key = self.kairos().key.clone();
        self.checkout = Some(checkout);
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.run.as_ref().expect("no run").stdout).into_owned()
    }

    /// The stdout and the stderr of the last run.
    fn output(&self) -> String {
        let run = self.run.as_ref().expect("no run");
        format!(
            "{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        )
    }

    /// `kairos index update` in the checkout, with the URL and the key of
    /// the scenario and no other Kairos setting of the machine.
    fn update(&mut self) {
        self.update_with(&[]);
    }

    /// [`Self::update`] with these environment variables added.
    fn update_with(&mut self, env: &[(&str, &str)]) {
        self.update_args(&[], env);
    }

    /// [`Self::update`] with these flags and environment variables added.
    fn update_args(&mut self, flags: &[&str], env: &[(&str, &str)]) {
        let root = self.root().to_path_buf();
        let config = TempDir::new().expect("a config folder");
        let mut command = Command::new(KAIROS);
        clean_env(&mut command);
        command
            .args(["index", "update", "--repository", REPOSITORY])
            .args(flags)
            .arg("--root")
            .arg(&root)
            .env_remove("KAIROS_MCP_KEY")
            .env("KAIROS_CONFIG_DIR", config.path())
            .env("KAIROS_URL", &self.url)
            .env("KAIROS_KEY", &self.key);
        for (name, value) in env {
            command.env(name, value);
        }
        let run = command.output().expect("run kairos");
        self.run = Some(run);
    }

    /// `kairos index upload` in the checkout (KAIROS-T-0592), with the URL
    /// and the key of the scenario and no other Kairos setting.
    fn upload_cli(&mut self) {
        let root = self.root().to_path_buf();
        let config = TempDir::new().expect("a config folder");
        let run = clean_env(&mut Command::new(KAIROS))
            .args(["index", "upload", "--repo", REPOSITORY, "--root"])
            .arg(&root)
            .env_remove("KAIROS_MCP_KEY")
            .env("KAIROS_CONFIG_DIR", config.path())
            .env("KAIROS_URL", &self.url)
            .env("KAIROS_KEY", &self.key)
            .output()
            .expect("run kairos");
        self.run = Some(run);
    }

    /// The summarizable symbols of the index, test code too (KAIROS-T-0421),
    /// as `file:name`, split by whether they have a summary.
    fn summarized(&self) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut with = BTreeSet::new();
        let mut without = BTreeSet::new();
        for s in self.index().symbols().expect("symbols") {
            if !SUMMARIZED_KINDS.contains(&s.kind.as_str()) {
                continue;
            }
            let name = format!("{}:{}", s.file, s.name);
            if s.summary_key.is_some() {
                with.insert(name);
            } else {
                without.insert(name);
            }
        }
        (with, without)
    }
}

fn base(world: &mut CliWorld) -> &mut BaseWorld {
    &mut world.base
}

// --- Given --------------------------------------------------------------------

#[given(regex = r"^Kairos has (?:an|a summarized) index of commit A of main$")]
async fn kairos_has_a(world: &mut CliWorld) {
    let w = base(world);
    let kairos = Kairos::start().await;
    let a = kairos.a.clone();
    w.index_a = kairos.upload(&a).await;
    w.kairos = Some(kairos);
}

/// KAIROS-T-0355: Kairos has an index of A with the summaries of 2
/// symbols only.
#[given("Kairos has a partly summarized index of commit A of main")]
async fn kairos_has_a_part_of_a(world: &mut CliWorld) {
    let w = base(world);
    let kairos = Kairos::start().await;
    let a = kairos.a.clone();
    let mut options = UpdateOptions::default();
    options.summarize.max_new_symbols = Some(2);
    w.index_a = kairos.upload_with(&a, &options).await;
    w.kairos = Some(kairos);
}

/// KAIROS-T-0355: a clone of A, and its local index from the base of
/// Kairos, with the summaries that the base has.
#[given("a checkout of A whose local index links the summaries of that index")]
fn a_checkout_with_the_partial_index(world: &mut CliWorld) {
    let w = base(world);
    w.clone_remote();
    w.update_args(&["--link-only"], &[]);
    let out = w.output();
    assert!(w.run.as_ref().expect("a run").status.success(), "{out}");
    assert!(
        w.stdout().contains("Kairos: downloaded the index of"),
        "{out}"
    );
    let (with, without) = w.summarized();
    assert_eq!(with.len(), 2, "{out}");
    assert!(!without.is_empty(), "{out}");
}

/// KAIROS-T-0355: the builder of Kairos makes the other summaries of A, as
/// a push run does: the index of A in Kairos now has each summary.
#[given("Kairos then gets a summary of each symbol of A")]
async fn kairos_gets_each_summary(world: &mut CliWorld) {
    let w = base(world);
    let a = w.kairos().a.clone();
    w.index_a = w.kairos().upload(&a).await;
}

#[then("the update says that it took the summaries from Kairos")]
fn update_says_taken(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    assert!(w.run.as_ref().expect("a run").status.success(), "{out}");
    let stdout = w.stdout();
    assert!(
        stdout.contains("the local index is nearer to the tree than the index of"),
        "{out}"
    );
    assert!(
        stdout.contains("The update took the summaries from Kairos and keeps the local structure."),
        "{out}"
    );
    assert!(!stdout.contains("Kairos: downloaded the index of"), "{out}");
    assert!(!stdout.contains("Not made:"), "{out}");
}

#[then("the server says that it took the summaries from Kairos")]
fn server_says_taken(world: &mut CliWorld) {
    let r = world.result.as_ref().expect("no result");
    assert!(!r.is_error, "{}", r.text);
    assert!(
        r.text
            .contains("the local index is nearer to the tree than the index of"),
        "{}",
        r.text
    );
    assert!(
        r.text
            .contains("The update took the summaries from Kairos and keeps the local structure."),
        "{}",
        r.text
    );
    assert!(r.text.contains(", 0 with none."), "{}", r.text);
}

/// KAIROS-T-0343: the organization has a hosted provider, and the
/// repository is opted in. The endpoint is never called: the CLI makes no
/// summary on such a repository.
#[given("the repository is on the hosted provider of the organization")]
async fn the_repository_is_hosted(world: &mut CliWorld) {
    let client = base(world).kairos().client();
    client
        .put_code_index_settings(&kairos_client::types_code_index::PutCodeIndexSettings {
            summary: Some(kairos_client::types_code_index::PutSummaryProvider {
                provider: "ollama-cloud".into(),
                base_url: Some("https://ollama.invalid/v1".into()),
                model: Some("gemma4:31b".into()),
                region: None,
                secret: Some(kairos_client::types_auth::Secret::new("a-key-t0343")),
            }),
            vectors: Some(kairos_client::types_code_index::PutVectorProvider {
                provider: "embedded".into(),
                ..Default::default()
            }),
            concurrency: None,
            default_summaries: None,
            version: None,
        })
        .await
        .expect("the hosted provider of the organization");
    client
        .update_repository(
            REPOSITORY,
            &kairos_client::types_repositories::UpdateRepositoryRequest {
                code_index_summaries: Some(
                    kairos_client::types_repositories::CodeIndexSummaries::Hosted,
                ),
                ..Default::default()
            },
        )
        .await
        .expect("the opt-in of the repository");
}

#[given(
    regex = r"^a checkout of a branch from A with (\d+) changed files(?: and no local index)?$"
)]
fn a_branch(world: &mut CliWorld, changed: usize) {
    let w = base(world);
    w.clone_remote();
    let root = w.root().to_path_buf();
    git(&root, &["checkout", "-q", "-b", "feature"]);
    if changed == BRANCH_CHANGES.len() {
        for (file, from, to, _) in BRANCH_CHANGES {
            let path = root.join(file);
            let text = fs::read_to_string(&path).expect("read");
            assert!(text.contains(from), "{file} has no {from:?}");
            fs::write(&path, text.replace(from, to)).expect("write");
        }
    } else {
        for i in 0..changed {
            write(
                &root,
                &format!("pkg/generated/part_{i:03}.py"),
                &format!("def part_{i}(x):\n    return x + {i}\n"),
            );
        }
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "the branch"]);
    assert!(!index_db(&root).exists());
}

/// KAIROS-T-0646: a clone whose `.claude/kairos.local.md` names the
/// deployment and the repository, and a worktree of it on a branch with the
/// 3 changes. The `origin` of the clone is a folder, so no repository of
/// Kairos has it: only the file of the clone names the repository.
#[given("a worktree of a wired clone, on a branch from A with 3 changed files")]
fn a_worktree(world: &mut CliWorld) {
    let w = base(world);
    w.clone_remote();
    let clone = w.root().to_path_buf();
    write(
        &clone,
        ".claude/kairos.local.md",
        &format!(
            "---\ndeployment_url: {}\nrepository: {REPOSITORY}\n---\n",
            w.kairos().url
        ),
    );
    let folder = TempDir::new().expect("a folder for the worktree");
    let worktree = folder.path().join("wt-feature");
    let path = worktree.display().to_string();
    git(&clone, &["worktree", "add", "-q", "-b", "feature", &path]);
    for (file, from, to, _) in BRANCH_CHANGES {
        let path = worktree.join(file);
        let text = fs::read_to_string(&path).expect("read");
        fs::write(&path, text.replace(from, to)).expect("write");
    }
    git(&worktree, &["commit", "-q", "-am", "the branch"]);
    assert!(!worktree.join(".claude").exists());
    w.worktree = Some((folder, worktree));
}

/// KAIROS-T-0646: a board folder. Its wiring names the deployment. It
/// holds a clone of `payments-api` 1 commit behind origin/main, a worktree
/// of that clone, and a clone of a repository that Kairos does not have.
#[given(
    "a board folder with a clone 1 commit behind, a worktree of it and a clone of another repository"
)]
fn a_board_folder(world: &mut CliWorld) {
    let w = base(world);
    let board = TempDir::new().expect("a board folder");
    let remote = w.kairos().remote.display().to_string();
    write(
        board.path(),
        ".claude/kairos.local.md",
        &format!("---\ndeployment_url: {}\n---\n", w.kairos().url),
    );
    git(board.path(), &["clone", "-q", &remote, REPOSITORY]);
    let clone = board.path().join(REPOSITORY);
    let source = w.kairos().remote.clone();
    write(&source, "pkg/extra.py", "def extra():\n    return 1\n");
    git(&source, &["add", "-A"]);
    git(&source, &["commit", "-q", "-m", "D"]);
    git(&clone, &["fetch", "-q"]);
    let url = format!("git@github.com:acme/{REPOSITORY}.git");
    git(&clone, &["remote", "set-url", "origin", &url]);
    let worktree = board.path().join("wt-payments").display().to_string();
    git(
        &clone,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "work",
            &worktree,
            "origin/main",
        ],
    );
    let other = board.path().join("other");
    fs::create_dir_all(&other).expect("make the other clone");
    git(&other, &["init", "-q", "-b", "main"]);
    git(
        &other,
        &["remote", "add", "origin", "https://github.com/acme/other"],
    );
    w.board = Some(board);
}

/// KAIROS-T-0516: main gets commit C with `files` new files, then commit B
/// with 1 change.
#[given(regex = r"^main then has commit C with (\d+) new files, then commit B with 1 change$")]
fn main_has_c_and_b(world: &mut CliWorld, files: usize) {
    let w = base(world);
    let remote = w.kairos().remote.clone();
    for i in 0..files {
        write(
            &remote,
            &format!("pkg/generated/part_{i:03}.py"),
            &format!("def part_{i}(x):\n    return x + {i}\n"),
        );
    }
    git(&remote, &["add", "-A"]);
    git(&remote, &["commit", "-q", "-m", "C"]);
    w.c = git(&remote, &["rev-parse", "HEAD"]);
    let (file, from, to, _) = BRANCH_CHANGES[0];
    let path = remote.join(file);
    let text = fs::read_to_string(&path).expect("read");
    fs::write(&path, text.replace(from, to)).expect("write");
    git(&remote, &["commit", "-q", "-am", "B"]);
    w.b = git(&remote, &["rev-parse", "HEAD"]);
}

#[given("Kairos has an index of commit B")]
async fn kairos_has_b(world: &mut CliWorld) {
    let w = base(world);
    let b = w.b.clone();
    w.kairos().upload(&b).await;
}

/// KAIROS-T-0516: a clone whose main is at C: HEAD is an ancestor of
/// origin/main, and the nearest index below HEAD is the index of A.
#[given("a checkout of main at C, behind origin/main at B")]
fn a_checkout_behind(world: &mut CliWorld) {
    let w = base(world);
    w.clone_remote();
    let root = w.root().to_path_buf();
    git(&root, &["reset", "-q", "--hard", &w.c]);
    assert_eq!(git(&root, &["rev-parse", "origin/main"]), w.b);
    assert!(!index_db(&root).exists());
}

/// KAIROS-T-0348: the server starts in an empty folder, as a session of a
/// board folder does, with the deployment of the scenario and no other
/// Kairos setting of the machine.
#[given("a kairos-code server started in a folder that is not a checkout")]
fn a_server_outside_a_checkout(world: &mut CliWorld) {
    let elsewhere = TempDir::new().expect("a folder that is not a checkout");
    let config = TempDir::new().expect("a config folder");
    let url = world.base.url.clone();
    let key = world.base.key.clone();
    let agent = super::Agent::start_with_env(
        elsewhere.path(),
        &[
            ("KAIROS_CONFIG_DIR", config.path().to_str().expect("utf-8")),
            ("KAIROS_URL", &url),
            ("KAIROS_KEY", &key),
            ("KAIROS_MCP_KEY", ""),
        ],
    );
    world.agent = Some(agent);
    world.base.elsewhere = Some((elsewhere, config));
}

#[when("the agent calls code_search")]
fn calls_code_search(world: &mut CliWorld) {
    let result = world.agent.as_mut().expect("no server").call(
        "code_search",
        serde_json::json!({"query": "the mean of the values"}),
    );
    world.result = Some(result);
}

#[then("the server refuses: no checkout is open, call fetch_index")]
fn refuses_no_checkout(world: &mut CliWorld) {
    let r = world.result.as_ref().expect("no result");
    assert!(r.is_error, "{}", r.text);
    assert!(r.text.starts_with("NO_CHECKOUT: "), "{}", r.text);
    assert!(r.text.contains("`fetch_index`"), "{}", r.text);
    assert!(r.text.contains("`root`"), "{}", r.text);
    assert!(!r.text.contains("kairos index build"), "{}", r.text);
}

#[when("the agent calls fetch_index with the path of the checkout")]
fn calls_fetch_index(world: &mut CliWorld) {
    // A folder inside the checkout, not its root: the server finds the root.
    let inside = world.base.root().join("pkg");
    let result = world.agent.as_mut().expect("no server").call(
        "fetch_index",
        serde_json::json!({"root": inside, "repository": REPOSITORY}),
    );
    world.result = Some(result);
}

/// KAIROS-T-0515: `fetch_index` takes `max_changed`.
#[when("the agent calls fetch_index with the path of the checkout and max_changed 10")]
fn calls_fetch_index_with_a_limit(world: &mut CliWorld) {
    let root = world.base.root().to_path_buf();
    let result = world.agent.as_mut().expect("no server").call(
        "fetch_index",
        serde_json::json!({"root": root, "repository": REPOSITORY, "max_changed": 10}),
    );
    world.result = Some(result);
}

/// KAIROS-T-0517: the refusal of the tool names `max_changed`.
#[then("the server refuses: 250 files changed, the limit is 10, give max_changed 250")]
fn refuses_with_the_limit(world: &mut CliWorld) {
    let r = world.result.as_ref().expect("no result");
    assert!(r.is_error, "{}", r.text);
    assert!(
        r.text.starts_with("INDEX: 250 files changed since "),
        "{}",
        r.text
    );
    assert!(
        r.text
            .contains("The limit is 10, so the server did not change the index."),
        "{}",
        r.text
    );
    assert!(r.text.contains("Rebase on main"), "{}", r.text);
    assert!(
        r.text
            .contains("Or give `max_changed` 250 to update from this index."),
        "{}",
        r.text
    );
    assert!(!index_db(world.base.root()).exists(), "{}", r.text);
}

/// KAIROS-T-0517: a failed fetch opened nothing, and says so.
#[then("the server says that no checkout is open")]
fn says_no_checkout_is_open(world: &mut CliWorld) {
    let r = world.result.as_ref().expect("no result");
    assert!(
        r.text.contains(
            "The server has no open checkout, so the other tools refuse until `fetch_index` \
             succeeds."
        ),
        "{}",
        r.text
    );
}

#[then(regex = r"^the server says that the index of A is open, with (\d+) changed files$")]
fn says_open(world: &mut CliWorld, changed: usize) {
    let r = world.result.as_ref().expect("no result");
    assert!(!r.is_error, "{}", r.text);
    let a = &world.base.kairos().a[..12];
    assert!(
        r.text.starts_with("The server opened the index of "),
        "{}",
        r.text
    );
    assert!(
        r.text.contains(&format!(
            "the index of commit {a} from Kairos, {changed} files changed since it."
        )),
        "{}",
        r.text
    );
    assert!(
        r.text.contains("The tools answer for this checkout now."),
        "{}",
        r.text
    );
}

#[then("module_map answers for the checkout")]
fn module_map_answers(world: &mut CliWorld) {
    let result = world
        .agent
        .as_mut()
        .expect("no server")
        .call("module_map", serde_json::json!({}));
    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.contains("pkg/stats.py"), "{}", result.text);
}

#[then("the checkout has the index of A, updated for its changed files")]
fn checkout_has_the_index(world: &mut CliWorld) {
    let w = &world.base;
    let index = w.index();
    let files = index.files().expect("files");
    let stats = files
        .iter()
        .find(|f| f.path == "pkg/stats.py")
        .expect("pkg/stats.py");
    let in_a = {
        let dir = TempDir::new().expect("a folder");
        let path = dir.path().join("a.db");
        fs::write(&path, &w.index_a).expect("write");
        Index::open(&path)
            .expect("the index of A")
            .files()
            .expect("files")
            .into_iter()
            .find(|f| f.path == "pkg/stats.py")
            .expect("pkg/stats.py in A")
            .content_hash
    };
    assert_ne!(
        stats.content_hash, in_a,
        "the index is the index of A, not updated"
    );
    // The summaries of A are linked from the pool: no model ran.
    let (with, _) = w.summarized();
    assert!(
        !with.is_empty(),
        "no summary was linked from the base index"
    );
}

#[given("a kairos binary with no summarizer")]
fn no_summarizer(_world: &mut CliWorld) {
    // A build of one function with an empty pool: a binary with a
    // summarizer and no model file says that the model file is missing.
    let dir = TempDir::new().expect("a folder");
    git(dir.path(), &["init", "-q"]);
    write(dir.path(), "one.py", "def one():\n    return 1\n");
    let out = clean_env(&mut Command::new(KAIROS))
        .args(["index", "build", "--root"])
        .arg(dir.path())
        .output()
        .expect("run kairos");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains("This kairos binary has no summarizer."),
        "this scenario needs a kairos binary with no feature `llama`:\n{stdout}"
    );
}

#[given("a checkout with a local index and no connection to Kairos")]
fn a_local_index(world: &mut CliWorld) {
    let w = base(world);
    let checkout = TempDir::new().expect("a folder for the checkout");
    repository_at_a(checkout.path());
    let db = index_db(checkout.path());
    fs::create_dir_all(db.parent().expect("a folder")).expect("make the index folder");
    kairos_index::update(
        checkout.path(),
        &db,
        &mut FakeSummarizer::default(),
        &DeterministicProvider::default(),
        &UpdateOptions::default(),
    )
    .expect("the local index");
    w.checkout = Some(checkout);
    w.url = NO_KAIROS.to_string();
    w.key = "kairos_acme_not_a_key".to_string();
}

// --- When ---------------------------------------------------------------------

#[when(expr = "I run {string}")]
fn run_update(world: &mut CliWorld, command: String) {
    match command.as_str() {
        "kairos index update" => base(world).update(),
        "kairos index update --link-only" => base(world).update_args(&["--link-only"], &[]),
        "kairos index update --json" => base(world).update_args(&["--json"], &[]),
        "kairos index upload" => base(world).upload_cli(),
        other => panic!("no step runs {other:?}"),
    }
}

/// KAIROS-T-0646: no `KAIROS_URL` and no `--repo`: the deployment and the
/// repository come from the file of the clone. The key is in `KAIROS_KEY`.
#[when("I run \"kairos index update\" in the worktree, with no KAIROS_URL and no --repo")]
fn run_update_in_worktree(world: &mut CliWorld) {
    let w = base(world);
    let config = TempDir::new().expect("a config folder");
    let run = clean_env(&mut Command::new(KAIROS))
        .args(["index", "update", "--root"])
        .arg(w.root())
        .env_remove("KAIROS_MCP_KEY")
        .env_remove("KAIROS_URL")
        .env("KAIROS_CONFIG_DIR", config.path())
        .env("KAIROS_UPDATE_CHECK", "0")
        .env("KAIROS_KEY", &w.key)
        .output()
        .expect("run kairos");
    w.run = Some(run);
}

/// KAIROS-T-0646: the deployment comes from the wiring of the board folder.
#[when("I run \"kairos repos local --json\" on the board folder, with no KAIROS_URL")]
fn run_repos_local(world: &mut CliWorld) {
    let w = base(world);
    let config = TempDir::new().expect("a config folder");
    let board = w.board.as_ref().expect("no board folder").path();
    let run = clean_env(&mut Command::new(KAIROS))
        .args(["repos", "local", "--json"])
        .arg(board)
        .env_remove("KAIROS_MCP_KEY")
        .env_remove("KAIROS_URL")
        .env("KAIROS_CONFIG_DIR", config.path())
        .env("KAIROS_UPDATE_CHECK", "0")
        .env("KAIROS_KEY", &w.kairos().key)
        .output()
        .expect("run kairos");
    w.run = Some(run);
}

#[then(
    regex = r"^the list has (\S+): repository (\S+), (clone|worktree of \S+), (\d+) behind origin/main$"
)]
fn local_lists(
    world: &mut CliWorld,
    folder: String,
    repository: String,
    kind: String,
    behind: u64,
) {
    let entry = local_entry(base(world), &folder);
    assert_eq!(entry["repository"], repository.as_str(), "{entry}");
    assert_eq!(entry["default_branch"], "main", "{entry}");
    assert_eq!(entry["behind"], behind, "{entry}");
    match kind.strip_prefix("worktree of ") {
        Some(clone) => {
            assert_eq!(entry["worktree"], true, "{entry}");
            assert_eq!(entry["clone"], clone, "{entry}");
        }
        None => assert_eq!(entry["worktree"], false, "{entry}"),
    }
}

#[then(regex = r"^the list has (\S+) with no repository and no count$")]
fn local_lists_unknown(world: &mut CliWorld, folder: String) {
    let entry = local_entry(base(world), &folder);
    assert!(entry["repository"].is_null(), "{entry}");
    assert!(entry["behind"].is_null(), "{entry}");
    assert_eq!(entry["remote"], "github.com/acme/other", "{entry}");
}

#[then("the list has no other entry")]
fn local_lists_no_other(world: &mut CliWorld) {
    let list = local_list(base(world));
    assert_eq!(list["total"], 3, "{list}");
}

/// The `--json` output of `kairos repos local`.
fn local_list(w: &BaseWorld) -> serde_json::Value {
    let out = w.output();
    assert!(w.run.as_ref().expect("no run").status.success(), "{out}");
    serde_json::from_str(&w.stdout()).unwrap_or_else(|e| panic!("{e}: {out}"))
}

fn local_entry(w: &BaseWorld, folder: &str) -> serde_json::Value {
    let list = local_list(w);
    list["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["folder"] == folder)
        .unwrap_or_else(|| panic!("no {folder} in {list}"))
        .clone()
}

#[when(expr = "I run {string} with KAIROS_INDEX_SUMMARIZE=1")]
fn run_update_with_summarize(world: &mut CliWorld, command: String) {
    assert_eq!(command, "kairos index update");
    base(world).update_with(&[("KAIROS_INDEX_SUMMARIZE", "1")]);
}

#[then("the CLI says that this repository makes its summaries on Kairos")]
fn says_hosted(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    assert!(
        w.stdout()
            .contains("This repository makes its summaries on Kairos"),
        "{out}"
    );
    assert!(!w.stdout().contains("has no summarizer"), "{out}");
}

#[then("the symbols of A keep their summaries, and the changed symbols have none")]
fn linked_not_made(world: &mut CliWorld) {
    let w = base(world);
    let (with, without) = w.summarized();
    assert!(!with.is_empty(), "no symbol kept a summary: {without:?}");
    assert!(!without.is_empty(), "each symbol has a summary: {with:?}");
}

#[when(expr = "I run {string} on a checkout of A")]
fn run_update_on_a(world: &mut CliWorld, command: String) {
    assert_eq!(command, "kairos index update");
    let w = base(world);
    w.clone_remote();
    w.update();
}

#[when(expr = "I change a file and run {string}")]
fn change_and_update(world: &mut CliWorld, command: String) {
    assert_eq!(command, "kairos index update");
    let w = base(world);
    w.hash_before = w
        .index()
        .files()
        .expect("files")
        .into_iter()
        .find(|f| f.path == "pkg/stats.py")
        .expect("pkg/stats.py is in the index")
        .content_hash;
    let path = w.root().join("pkg/stats.py");
    let mut text = fs::read_to_string(&path).expect("read");
    text.push_str("\n\ndef median(xs):\n    return sorted(xs)[len(xs) // 2]\n");
    fs::write(&path, text).expect("write");
    w.update();
}

/// KAIROS-T-0592: a change in the tree that is not committed.
#[when("a file of the checkout changes, with no commit")]
fn a_file_changes(world: &mut CliWorld) {
    let w = base(world);
    let path = w.root().join("pkg/stats.py");
    let mut text = fs::read_to_string(&path).expect("read");
    text.push_str("\n\ndef spread(xs):\n    return max(xs) - min(xs)\n");
    fs::write(&path, text).expect("write");
}

// --- Then ---------------------------------------------------------------------

/// KAIROS-T-0592: the upload stored the index of HEAD, with the branch as
/// its ref, and the index is the index of the checkout.
#[then("Kairos has an index of the HEAD commit of the checkout, for the ref feature")]
async fn kairos_has_head(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    assert!(w.run.as_ref().expect("no run").status.success(), "{out}");
    let head = git(w.root(), &["rev-parse", "HEAD"]);
    assert!(
        w.stdout().starts_with(&format!(
            "Kairos keeps the index of {} (feature) for the repository {REPOSITORY}.",
            &head[..12]
        )),
        "{out}"
    );
    let indexes = w
        .kairos()
        .client()
        .list_code_indexes(REPOSITORY)
        .await
        .expect("the indexes");
    let stored = indexes
        .iter()
        .find(|index| index.commit == head)
        .unwrap_or_else(|| panic!("no index of {head}: {indexes:?}"));
    assert_eq!(stored.r#ref.as_deref(), Some("feature"));
    assert_eq!(stored.source, "upload");
    let local = fs::read(index_db(w.root())).expect("the local index");
    let downloaded = w
        .kairos()
        .client()
        .download_code_index(REPOSITORY, &head)
        .await
        .expect("the download");
    let dir = TempDir::new().expect("a folder");
    let (a, b) = (dir.path().join("local.db"), dir.path().join("stored.db"));
    fs::write(&a, local).expect("write");
    fs::write(&b, downloaded).expect("write");
    let files = |path: &Path| -> Vec<(String, String)> {
        Index::open(path)
            .expect("an index")
            .files()
            .expect("files")
            .into_iter()
            .map(|f| (f.path, f.content_hash))
            .collect()
    };
    assert_eq!(files(&a), files(&b), "the stored index is the local index");
}

/// KAIROS-T-0592: the refusal names a changed file, and Kairos stores
/// nothing for HEAD.
#[then(expr = "the CLI refuses the upload, because {word} is not committed")]
async fn upload_refused(world: &mut CliWorld, file: String) {
    let w = base(world);
    let out = w.output();
    let run = w.run.as_ref().expect("no run");
    assert_eq!(run.status.code(), Some(1), "{out}");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains(&format!(
            "1 changed file that is not committed, for example {file}."
        )),
        "{out}"
    );
    let head = git(w.root(), &["rev-parse", "HEAD"]);
    let indexes = w
        .kairos()
        .client()
        .list_code_indexes(REPOSITORY)
        .await
        .expect("the indexes");
    assert!(
        indexes.iter().all(|index| index.commit != head),
        "{indexes:?}"
    );
}

#[then("the CLI downloads the index of A")]
fn downloads_a(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    let run = w.run.as_ref().expect("no run");
    assert!(run.status.success(), "{out}");
    let a = &w.kairos().a;
    assert!(
        w.stdout()
            .contains(&format!("Kairos: downloaded the index of {}", &a[..12])),
        "{out}"
    );
    assert!(w.stdout().contains("3 files changed since it"), "{out}");
    // The structure is that of the checkout: the changed code is in it.
    let index = w.index();
    let files = index.files().expect("files");
    let stats = files
        .iter()
        .find(|f| f.path == "pkg/stats.py")
        .expect("pkg/stats.py");
    let in_a = {
        let dir = TempDir::new().expect("a folder");
        let path = dir.path().join("a.db");
        fs::write(&path, &w.index_a).expect("write");
        Index::open(&path)
            .expect("the index of A")
            .files()
            .expect("files")
            .into_iter()
            .find(|f| f.path == "pkg/stats.py")
            .expect("pkg/stats.py in A")
            .content_hash
    };
    assert_ne!(stats.content_hash, in_a, "the index is the index of A");
}

/// KAIROS-T-0516: the base is the index of the tip of origin/main.
#[then("the CLI downloads the index of B, 1 file changed since it")]
fn downloads_b(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    assert!(w.run.as_ref().expect("no run").status.success(), "{out}");
    assert!(
        w.stdout().contains(&format!(
            "Kairos: downloaded the index of {}, 1 files changed since it.",
            &w.b[..12]
        )),
        "{out}"
    );
}

/// KAIROS-T-0515: the base of a far branch, with no limit.
#[then(regex = r"^the CLI downloads the index of A, with (\d+) changed files$")]
fn downloads_a_far(world: &mut CliWorld, changed: usize) {
    let w = base(world);
    let out = w.output();
    assert!(w.run.as_ref().expect("no run").status.success(), "{out}");
    let a = &w.kairos().a;
    assert!(
        w.stdout().contains(&format!(
            "Kairos: downloaded the index of {}, {changed} files changed since it.",
            &a[..12]
        )),
        "{out}"
    );
}

/// KAIROS-T-0515: the generated files of the branch are in the index.
#[then(regex = r"^the index of the checkout has the (\d+) new files$")]
fn has_the_new_files(world: &mut CliWorld, count: usize) {
    let w = &world.base;
    let files = w.index().files().expect("files");
    let new = files
        .iter()
        .filter(|f| f.path.starts_with("pkg/generated/"))
        .count();
    assert_eq!(new, count);
    // The files of A are there too, with the summaries of the base.
    assert!(files.iter().any(|f| f.path == "pkg/stats.py"));
    let (with, _) = w.summarized();
    assert!(with.contains("pkg/stats.py:mean"), "{with:?}");
}

/// KAIROS-T-0654: one JSON object on stdout says where the update started.
#[then(
    regex = r"^the output is one JSON object: the update started from the index of A, with (\d+) changed files$"
)]
fn json_start(world: &mut CliWorld, changed: usize) {
    let w = base(world);
    let out = w.output();
    assert!(w.run.as_ref().expect("no run").status.success(), "{out}");
    let v: serde_json::Value = serde_json::from_str(&w.stdout())
        .unwrap_or_else(|e| panic!("stdout is not one JSON value ({e}):\n{out}"));
    assert_eq!(v["command"], "update", "{out}");
    assert_eq!(v["start"]["from"], "base", "{out}");
    assert_eq!(v["start"]["base"]["commit"], w.kairos().a.as_str(), "{out}");
    assert_eq!(v["start"]["base"]["changed"], changed, "{out}");
    assert_eq!(v["start"]["base"]["repository"], REPOSITORY, "{out}");
    assert_eq!(
        v["start"]["local_changed"],
        serde_json::Value::Null,
        "{out}"
    );
    assert!(
        v["structure"]["files"].as_u64().is_some_and(|n| n > 0),
        "{out}"
    );
    assert_eq!(v["summaries"]["result"], "not_made", "{out}");
    assert_eq!(v["summaries"]["left"]["symbols"], changed, "{out}");
}

/// KAIROS-T-0654: a refusal is the error envelope on stderr, with its text.
#[then(
    regex = r"^stdout is empty, and stderr has the error TOO_FAR: (\d+) changed files, the limit is (\d+)$"
)]
fn json_refusal(world: &mut CliWorld, changed: usize, limit: usize) {
    let w = base(world);
    let out = w.output();
    let run = w.run.as_ref().expect("no run");
    assert_eq!(run.status.code(), Some(1), "{out}");
    assert_eq!(w.stdout(), "", "{out}");
    assert!(!index_db(w.root()).exists(), "{out}");
    let stderr = String::from_utf8_lossy(&run.stderr);
    let v: serde_json::Value = serde_json::from_str(&stderr)
        .unwrap_or_else(|e| panic!("stderr is not one JSON value ({e}):\n{out}"));
    let error = &v["error"];
    assert_eq!(error["code"], "TOO_FAR", "{out}");
    assert_eq!(error["details"]["changed"], changed, "{out}");
    assert_eq!(error["details"]["limit"], limit, "{out}");
    assert_eq!(error["details"]["local"], false, "{out}");
    assert_eq!(error["details"]["branch"], "main", "{out}");
    let message = error["message"].as_str().expect("a message");
    assert!(
        message.starts_with(&format!("{changed} files changed since"))
            && message.contains(&format!("Or give `--max-changed {changed}`")),
        "{out}"
    );
}

#[then("it summarizes only the changed symbols")]
fn only_the_changed_symbols(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    let (with, without) = w.summarized();
    let changed: BTreeSet<String> = BRANCH_CHANGES
        .iter()
        .map(|(file, _, _, name)| format!("{file}:{name}"))
        .collect();
    // Each other symbol has its summary from the base index. This binary
    // has no summarizer, so the changed symbols are what a summarizer
    // gets, and the CLI names them.
    assert_eq!(without, changed, "{out}");
    assert!(!with.is_empty(), "{out}");
    assert!(w.stdout().contains("Not made: 3 symbols,"), "{out}");
}

#[then("the CLI builds nothing")]
fn builds_nothing(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    let run = w.run.as_ref().expect("no run");
    assert_eq!(run.status.code(), Some(1), "{out}");
    assert!(!index_db(w.root()).exists(), "{out}");
    assert!(
        out.contains("250 files changed since") && out.contains("The limit is 200"),
        "{out}"
    );
}

#[then(expr = "it tells me to rebase on main, or to run {string}")]
fn tells_to_rebase(world: &mut CliWorld, command: String) {
    let out = base(world).output();
    assert!(out.contains("Rebase on main"), "{out}");
    assert!(out.contains(&format!("run `{command}`")), "{out}");
    // KAIROS-T-0517: and the limit that lets the update start.
    assert!(
        out.contains("Or give `--max-changed 250` to update from this index."),
        "{out}"
    );
}

#[then("it gives the time of a full build")]
fn gives_the_time(world: &mut CliWorld) {
    let out = base(world).output();
    assert!(out.contains("A full build takes about "), "{out}");
}

#[then("each symbol of A has its summary from the base index")]
fn each_symbol_has_its_summary(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    assert!(w.run.as_ref().expect("no run").status.success(), "{out}");
    let dir = TempDir::new().expect("a folder");
    let path = dir.path().join("a.db");
    fs::write(&path, &w.index_a).expect("write");
    let a = Index::open(&path).expect("the index of A");
    let key_of = |index: &Index| {
        index
            .symbols()
            .expect("symbols")
            .into_iter()
            .filter(|s| SUMMARIZED_KINDS.contains(&s.kind.as_str()))
            .map(|s| (format!("{}:{}", s.file, s.name), s.summary_key))
            .collect::<Vec<_>>()
    };
    let index = w.index();
    let in_a = key_of(&a);
    assert!(!in_a.is_empty() && in_a.iter().all(|(_, k)| k.is_some()));
    assert_eq!(key_of(&index), in_a, "{out}");
    for (_, key) in &in_a {
        let key = key.as_deref().expect("a key");
        assert_eq!(
            index.summary(key).expect("read").map(|s| s.summary),
            a.summary(key).expect("read").map(|s| s.summary)
        );
    }
    // The files and the modules too.
    assert_eq!(
        index.file_summary_keys().expect("file keys"),
        a.file_summary_keys().expect("file keys"),
        "{out}"
    );
    assert_eq!(
        index.modules().expect("modules"),
        a.modules().expect("modules")
    );
}

#[then("no model ran")]
fn no_model_ran(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    assert!(w.stdout().contains("No model ran."), "{out}");
    assert!(!w.stdout().contains("Not made:"), "{out}");
}

#[then("the local index describes the changed file")]
fn describes_the_change(world: &mut CliWorld) {
    let w = base(world);
    let out = w.output();
    assert!(w.run.as_ref().expect("no run").status.success(), "{out}");
    let index = w.index();
    let stats = index
        .files()
        .expect("files")
        .into_iter()
        .find(|f| f.path == "pkg/stats.py")
        .expect("pkg/stats.py");
    assert_ne!(stats.content_hash, w.hash_before, "{out}");
    assert!(
        index
            .symbols()
            .expect("symbols")
            .iter()
            .any(|s| s.file == "pkg/stats.py" && s.name == "median"),
        "{out}"
    );
}

#[then("the CLI says that it could not reach Kairos")]
fn could_not_reach(world: &mut CliWorld) {
    let out = base(world).output();
    assert!(out.contains("could not reach Kairos"), "{out}");
}
