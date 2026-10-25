//! The base code index in Kairos (COLLIERY-T-1853, COLLIERY-I-0264 "The
//! flow" and "Who builds the base index").
//!
//! The 5 scenarios of the task:
//!
//! 1. Upload, then download, an index.
//! 2. The nearest indexed commit.
//! 3. A push updates the index.
//! 4. The pool is shared.
//! 5. A user with no right cannot upload.
//!
//! The first build (KAIROS-T-0318): a repository with no index gets a first
//! build; a repository with `code_index_build = off` gets none; the setting
//! refuses an unknown value; a first build does not stop the updates.
//!
//! Each index is a real `kairos-index` file of a small Python repository
//! made by the test, built with the fake summarizer and the deterministic
//! vectors (no model). The repository has no Rust, so no SCIP run.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch databases `kairos_code_index_t1853_*`, one for each scenario.
//!
//! Cast: `svc` org admin; `bob` member of `platform`, the owner team of
//! the repositories; `alice` member of `web` (no right on them).

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::Error;
use kairos_client::types::Pagination;
use kairos_client::types_auth::Secret;
use kairos_client::types_code_index::{
    PutCodeIndexSettings, PutSummaryProvider, PutVectorProvider,
};
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_repositories::{
    CodeIndexBuild, CodeIndexSummaries, CreateRepositoryRequest, DefaultBranchSource,
    UpdateRepositoryRequest,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_embed::{DeterministicProvider, EmbeddingProvider};
use kairos_index::{FakeSummarizer, Index, Level, UpdateOptions};
use kairos_server::app;
use kairos_server::code_index::{
    ACTIVE_RUN, BREAKER_TRIPPED, CodeIndexService, FakeSummarizers, MIN_REBUILD, OPTED_OUT,
    SummarizerSource, end_stale_runs, first_builds, job_key, requested_builds, run_builder, sweep,
};
use kairos_server::middleware::auth::Authenticator;

fn user_id(conn: &mut PgConnection, email: &str) -> Uuid {
    users::table
        .filter(users::email.eq(email))
        .select(users::id)
        .first(conn)
        .unwrap_or_else(|e| panic!("user {email} not provisioned: {e}"))
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
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const STATS_A: &str = "\
def total(xs):
    \"\"\"The sum of the values.\"\"\"
    return sum(xs)


def mean(xs):
    \"\"\"The mean of the values.\"\"\"
    return total(xs) / len(xs)


def spread(xs):
    return max(xs) - min(xs)
";

const STATS_B: &str = "\
def total(xs):
    \"\"\"The sum of the values.\"\"\"
    return sum(xs)


def mean(xs):
    \"\"\"The mean of the values, 0 for no values.\"\"\"
    if not xs:
        return 0
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

const LOADER_A: &str = "\
def load(path):
    with open(path) as f:
        return [float(line) for line in f]
";

const LOADER_C: &str = "\
def load(path):
    with open(path) as f:
        return [float(line) for line in f if line.strip()]
";

/// A git repository with commit A on `main`. Returns its folder.
fn repository_at_a(dir: &Path) -> String {
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, "pkg/__init__.py", "");
    write(dir, "pkg/stats.py", STATS_A);
    write(dir, "pkg/report.py", REPORT);
    write(dir, "pkg/loader.py", LOADER_A);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "A"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Commit B: one function in one file changes.
fn commit_b(dir: &Path) -> String {
    write(dir, "pkg/stats.py", STATS_B);
    git(dir, &["commit", "-q", "-am", "B"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Commit C: one function in another file changes.
fn commit_c(dir: &Path) -> String {
    write(dir, "pkg/loader.py", LOADER_C);
    git(dir, &["commit", "-q", "-am", "C"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Build the index of `commit` of the repository at `repo`, as a checkout
/// does: the tree of the commit, the fake summarizer, the deterministic
/// vectors.
fn index_of(repo: &Path, commit: &str, out: &Path) -> Vec<u8> {
    let tree = tempfile::tempdir().unwrap();
    let archive = Command::new("git")
        .args(["archive", "--format=tar", commit])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(archive.status.success());
    let mut untar = Command::new("tar")
        .args(["-x", "-C"])
        .arg(tree.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    untar
        .stdin
        .take()
        .unwrap()
        .write_all(&archive.stdout)
        .unwrap();
    assert!(untar.wait().unwrap().success());
    let mut summarizer = FakeSummarizer::default();
    kairos_index::update(
        tree.path(),
        out,
        &mut summarizer,
        &DeterministicProvider::default(),
        &UpdateOptions::default(),
    )
    .expect("the index of the commit builds");
    std::fs::read(out).unwrap()
}

/// What an index says: its files, symbols and edges, and each summary that
/// its structure uses.
#[derive(Debug, PartialEq)]
struct Content {
    files: Vec<String>,
    symbols: Vec<String>,
    edges: Vec<String>,
    summaries: Vec<(String, String, Option<Vec<f32>>)>,
}

fn content_of(bytes: &[u8]) -> Content {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    std::fs::write(&path, bytes).unwrap();
    let keys = kairos_index::store::keys_of(&path).expect("an index file");
    let index = Index::open(&path).unwrap();
    Content {
        files: index
            .files()
            .unwrap()
            .into_iter()
            .map(|f| format!("{} {} {}", f.path, f.decision, f.content_hash))
            .collect(),
        symbols: index
            .symbols()
            .unwrap()
            .into_iter()
            .map(|s| format!("{s:?}"))
            .collect(),
        edges: index
            .edges()
            .unwrap()
            .into_iter()
            .map(|e| format!("{e:?}"))
            .collect(),
        summaries: keys
            .iter()
            .map(|k| {
                let s = index
                    .summary(k)
                    .unwrap()
                    .unwrap_or_else(|| panic!("summary {k}"));
                (s.key, s.summary, s.vector)
            })
            .collect(),
    }
}

fn keys_in(bytes: &[u8]) -> BTreeSet<String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    std::fs::write(&path, bytes).unwrap();
    kairos_index::store::keys_of(&path)
        .unwrap()
        .into_iter()
        .collect()
}

fn repository(slug: &str) -> CreateRepositoryRequest {
    CreateRepositoryRequest {
        slug: Some(slug.into()),
        forge: "github".into(),
        repo_full_name: format!("acme/{slug}"),
        repo_url: format!("https://github.com/acme/{slug}"),
        default_branch: None,
        team: "platform".into(),
        description: None,
    }
}

/// A tenant with the 2 teams and the cast, a server with code indexes on,
/// and one registered repository `payments-api` whose git repository has
/// commit A on `main`. Each scenario has a scratch database of its own.
struct World {
    conn: PgConnection,
    state: app::AppState,
    service: Arc<CodeIndexService>,
    /// The organization admin.
    svc: kairos_client::KairosClient,
    alice: kairos_client::KairosClient,
    bob: kairos_client::KairosClient,
    repo_id: Uuid,
    /// The git repository of `payments-api`.
    git: PathBuf,
    /// Commit A.
    a: String,
    /// Index files made by the scenario.
    files: tempfile::TempDir,
    /// The git server with basic auth of a private repository
    /// (COLLIERY-T-3105), when the world has one.
    git_http: Option<common::git_http::GitHttp>,
    _work: tempfile::TempDir,
    /// The scratch database: dropped last, with the world (KAIROS-T-0445).
    _scratch: common::ScratchConn,
}

/// The user and the token that the git server of a private repository
/// takes (COLLIERY-T-3105).
const TOKEN_USER: &str = "x-access-token";
const TOKEN: &str = "github_pat_t3105_SECRET_value";

impl World {
    async fn new(scratch_db: &str) -> World {
        Self::build(scratch_db, false).await
    }

    /// A world whose repository is private (COLLIERY-T-3105): the server
    /// fetches it over HTTP from a git server that needs [`TOKEN`], and the
    /// deployment has a `KAIROS_SECRETS_KEY`.
    async fn private(scratch_db: &str) -> World {
        Self::build(scratch_db, true).await
    }

    async fn build(scratch_db: &str, private: bool) -> World {
        let admin_conn = recreate_scratch_db(scratch_db);
        let scratch_url = with_database(&common::admin_database_url(), scratch_db);
        let mut conn =
            PgConnection::establish(&scratch_url).expect("connecting to scratch database");
        run_public_migrations(&mut conn).expect("running public migrations");
        provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
        let org_id: Uuid = organizations::table
            .filter(organizations::slug.eq("acme"))
            .select(organizations::id)
            .first(&mut conn)
            .expect("acme org row");

        // The git repository of payments-api, and the folder of the server
        // for its clones. The server fetches from the folder in place of
        // the repo_url.
        let work = tempfile::tempdir().unwrap();
        let git = work.path().join("payments-api");
        std::fs::create_dir_all(&git).unwrap();
        let a = repository_at_a(&git);
        let key = kairos_server::secrets::SecretsKey::from_bytes([42u8; 32]);
        let git_http = if private {
            Some(common::git_http::serve(work.path().to_path_buf(), TOKEN_USER, TOKEN).await)
        } else {
            None
        };
        // Each repository is fetched from the folder of its slug in `work`
        // (KAIROS-T-0318 has 2 repositories).
        let http_base = git_http.as_ref().map(|server| server.base_url.clone());
        let root = work.path().to_path_buf();
        let service = Arc::new(
            CodeIndexService::new(
                work.path().join("clones"),
                Arc::new(
                    move |repo: &kairos_db::models::repositories::Repository| match &http_base {
                        Some(base) => format!("{base}/{}/.git", repo.slug),
                        None => root.join(&repo.slug).display().to_string(),
                    },
                ),
            )
            // The key of the deployment: the read token of a private
            // repository, and the secret of a hosted provider (KAIROS-T-0341).
            .with_secrets_key(Some(key.clone())),
        );
        // KAIROS-T-0495: a 429 waits for no time in the tests.
        service.set_summary_backoff(kairos_index::rate::Backoff {
            sleep: Arc::new(|_, _: &dyn Fn()| ()),
            ..Default::default()
        });

        let http = reqwest::Client::new();
        let svc_token = user_token(&http, "svc").await;
        let alice_token = user_token(&http, "alice").await;
        let bob_token = user_token(&http, "bob").await;
        let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
        let auth = Arc::new(
            Authenticator::discover(ISSUER, AUDIENCE)
                .await
                .expect("OIDC discovery against live Dex"),
        );
        let mut config = base_config(&scratch_url);
        config.secrets_key = Some(key.clone());
        let mut state = app::state_with(config, pool, auth);
        state.code_index = Some(service.clone());
        let server = spawn_server(app::router(state.clone())).await;
        let svc = server.client(&svc_token, "acme");
        let alice = server.client(&alice_token, "acme");
        let bob = server.client(&bob_token, "acme");
        for client in [&svc, &alice, &bob] {
            let _ = client.whoami().await;
        }
        for (email, role) in [
            ("svc@kairos.test", OrgRole::Admin),
            ("alice@kairos.test", OrgRole::Member),
            ("bob@kairos.test", OrgRole::Member),
        ] {
            let user_id = user_id(&mut conn, email);
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id,
                    role,
                })
                .execute(&mut conn)
                .expect("granting membership");
        }
        let platform = svc
            .create_team(&CreateTeamRequest {
                name: "Platform".into(),
                slug: "platform".into(),
                code_prefix: "PLATFORM".into(),
                team_type: None,
            })
            .await
            .expect("platform");
        let web = svc
            .create_team(&CreateTeamRequest {
                name: "Web".into(),
                slug: "web".into(),
                code_prefix: "WEB".into(),
                team_type: None,
            })
            .await
            .expect("web");
        for (team, email) in [
            (&platform.id, "bob@kairos.test"),
            (&web.id, "alice@kairos.test"),
        ] {
            svc.add_team_member(
                team,
                &AddTeamMemberRequest {
                    user_id: user_id(&mut conn, email).to_string(),
                },
            )
            .await
            .expect("team member");
        }
        let repo = bob
            .create_repository(&repository("payments-api"))
            .await
            .expect("payments-api");
        diesel::sql_query("SET search_path TO org_acme, public")
            .execute(&mut conn)
            .unwrap();
        World {
            _scratch: admin_conn,
            conn,
            state,
            service,
            svc,
            alice,
            bob,
            repo_id: repo.id.parse().unwrap(),
            git,
            a,
            files: tempfile::tempdir().unwrap(),
            git_http,
            _work: work,
        }
    }

    /// The index file of `commit`, as a checkout builds it.
    fn index(&self, commit: &str) -> Vec<u8> {
        index_of(
            &self.git,
            commit,
            &self.files.path().join(format!("{commit}.db")),
        )
    }

    async fn sweep(&self) -> Vec<kairos_server::code_index::BuildOutcome> {
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
        sweep(
            &self.state.blocking,
            &self.service,
            Some(Arc::new(FakeSummarizers)),
            embedder,
        )
        .await
    }

    /// One pass of the update lane with `summarizers`.
    async fn sweep_with(
        &self,
        summarizers: Arc<dyn SummarizerSource>,
    ) -> Vec<kairos_server::code_index::BuildOutcome> {
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
        sweep(
            &self.state.blocking,
            &self.service,
            Some(summarizers),
            embedder,
        )
        .await
    }

    /// One pass of the first-build lane (KAIROS-T-0318).
    async fn first_builds(&self) -> Vec<kairos_server::code_index::BuildOutcome> {
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
        first_builds(
            &self.state.blocking,
            &self.service,
            Some(Arc::new(FakeSummarizers)),
            embedder,
        )
        .await
    }

    /// The builds that a person asked for (KAIROS-T-0332), one pass.
    async fn requested_builds(&self) -> Vec<kairos_server::code_index::BuildOutcome> {
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
        requested_builds(
            &self.state.blocking,
            &self.service,
            Some(Arc::new(FakeSummarizers)),
            embedder,
        )
        .await
    }

    /// The folder of the git repositories of the world.
    fn work(&self) -> &Path {
        self._work.path()
    }
}

// ===========================================================================
// Scenario: Upload, then download, an index
// ===========================================================================
#[tokio::test]
async fn upload_then_download_an_index() {
    // Given a repository registered in Kairos
    let w = World::new("kairos_code_index_t1853_upload").await;
    let c = commit_c(&w.git);
    let index_c = w.index(&c);

    // When I upload the index of commit C
    let uploaded = w
        .bob
        .upload_code_index("payments-api", &c, Some("main"), index_c.clone())
        .await
        .expect("bob uploads the index of C");
    assert_eq!(uploaded.index.commit, c);
    assert_eq!(uploaded.index.r#ref.as_deref(), Some("main"));
    assert_eq!(uploaded.index.source, "upload");

    // And I download the index of commit C
    let downloaded = w
        .alice
        .download_code_index("payments-api", &c)
        .await
        .expect("each member can download");

    // Then the downloaded index is the same as the uploaded one
    assert_eq!(content_of(&downloaded), content_of(&index_c));
    let by_id = w
        .alice
        .download_code_index(&w.repo_id.to_string(), &c)
        .await
        .expect("by UUID");
    assert_eq!(content_of(&by_id), content_of(&index_c));
    let err = w
        .alice
        .download_code_index("payments-api", &w.a)
        .await
        .expect_err("A has no index");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
}

// ===========================================================================
// Scenario: The nearest indexed commit
// ===========================================================================
#[tokio::test]
async fn the_nearest_indexed_commit() {
    // Given indexes for commits A and C of main, where C comes after A
    let w = World::new("kairos_code_index_t1853_nearest").await;
    let a = w.a.clone();
    let b = commit_b(&w.git);
    let c = commit_c(&w.git);
    for commit in [&a, &c] {
        w.bob
            .upload_code_index("payments-api", commit, Some("main"), w.index(commit))
            .await
            .expect("upload");
    }

    // When I ask for the nearest indexed commit below commit B, between A and C
    let nearest = w
        .alice
        .nearest_code_index("payments-api", &b)
        .await
        .expect("nearest below B");

    // Then the answer is A
    assert_eq!(nearest.index.commit, a);
    assert_eq!(nearest.from, b);
    assert_eq!(nearest.distance, 1);

    let at_c = w
        .alice
        .nearest_code_index("payments-api", &c)
        .await
        .expect("nearest at C");
    assert_eq!((at_c.index.commit.as_str(), at_c.distance), (c.as_str(), 0));
    let err = w
        .alice
        .nearest_code_index("payments-api", &"0".repeat(40))
        .await
        .expect_err("a commit that the repository does not have");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
    let listed = w.alice.list_code_indexes("payments-api").await.unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|i| i.commit.as_str())
            .collect::<BTreeSet<_>>(),
        [a.as_str(), c.as_str()].into()
    );
}

// ===========================================================================
// Scenario: The indexed commits, in pages (KAIROS-T-0518)
// ===========================================================================
#[tokio::test]
async fn the_indexed_commits_in_pages() {
    // Given indexes for commits A and C of main, C written after A
    let w = World::new("kairos_code_index_t0518_pages").await;
    let a = w.a.clone();
    let c = commit_c(&w.git);
    for commit in [&a, &c] {
        let uploaded = w
            .bob
            .upload_code_index("payments-api", commit, Some("main"), w.index(commit))
            .await
            .expect("upload");
        assert_eq!(
            uploaded.index.schema_version,
            Some(kairos_index::SCHEMA_VERSION)
        );
    }

    // When a member of another team asks for pages of one row
    let page = |offset| Pagination {
        limit: Some(1),
        offset: Some(offset),
    };
    let first = w
        .alice
        .code_indexes_page("payments-api", page(0))
        .await
        .expect("the first page");
    let second = w
        .alice
        .code_indexes_page("payments-api", page(1))
        .await
        .expect("the second page");

    // Then the newest write comes first, and the total is exact
    assert_eq!((first.total, first.limit, first.offset), (2, 1, 0));
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].commit, c);
    assert_eq!(first.items[0].r#ref.as_deref(), Some("main"));
    assert_eq!(
        first.items[0].schema_version,
        Some(kairos_index::SCHEMA_VERSION)
    );
    assert!(first.items[0].structure_bytes > 0);
    assert_eq!(second.total, 2);
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].commit, a);

    // With no limit the page has each row; past the end it has none
    let whole = w
        .alice
        .code_indexes_page("payments-api", Pagination::default())
        .await
        .unwrap();
    assert_eq!(
        whole
            .items
            .iter()
            .map(|i| i.commit.as_str())
            .collect::<Vec<_>>(),
        [c.as_str(), a.as_str()]
    );
    assert_eq!(whole.limit, 50);
    let past = w
        .alice
        .code_indexes_page("payments-api", page(5))
        .await
        .unwrap();
    assert!(past.items.is_empty());
    assert_eq!(past.total, 2);

    // The whole list and the nearest commit give the schema version too
    let listed = w.alice.list_code_indexes("payments-api").await.unwrap();
    assert!(
        listed
            .iter()
            .all(|i| i.schema_version == Some(kairos_index::SCHEMA_VERSION)),
        "{listed:?}"
    );
    let nearest = w
        .alice
        .nearest_code_index("payments-api", &c)
        .await
        .unwrap();
    assert_eq!(
        nearest.index.schema_version,
        Some(kairos_index::SCHEMA_VERSION)
    );

    // An unknown repository is a 404
    let err = w
        .alice
        .code_indexes_page("no-such-repo", page(0))
        .await
        .expect_err("an unknown repository");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
}

// ===========================================================================
// Scenario: A push updates the index
// ===========================================================================
#[tokio::test]
async fn a_push_updates_the_index() {
    // Given an index of commit A of main
    let w = World::new("kairos_code_index_t1853_push").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    let outcomes = w.sweep().await;
    assert!(
        outcomes.iter().all(|o| o.report.is_none()),
        "no push, no build: {outcomes:?}"
    );

    // When commit B, which changes one file, is pushed to main
    let b = commit_b(&w.git);
    let outcomes = w.sweep().await;

    // Then Kairos has an index of commit B
    let built: Vec<_> = outcomes.iter().filter(|o| o.report.is_some()).collect();
    assert_eq!(built.len(), 1, "{outcomes:?}");
    let outcome = built[0];
    assert_eq!(outcome.repository, "payments-api");
    assert_eq!(outcome.commit, b);
    assert_eq!(outcome.base.as_deref(), Some(a.as_str()));
    let listed = w.alice.list_code_indexes("payments-api").await.unwrap();
    let index_b = listed
        .iter()
        .find(|i| i.commit == b)
        .expect("Kairos has an index of commit B");
    assert_eq!(index_b.source, "build");
    assert_eq!(index_b.r#ref.as_deref(), Some("main"));

    // And the summarizer ran only for the changed symbols
    let report = outcome.report.as_ref().unwrap();
    let symbols: Vec<&str> = report
        .summary
        .calls
        .iter()
        .filter(|call| call.level == Level::Symbol)
        .map(|call| call.name.as_str())
        .collect();
    assert_eq!(symbols, ["pkg/stats.py:mean"]);

    // The index of B is the index that a checkout of B builds.
    let built_b = w
        .alice
        .download_code_index("payments-api", &b)
        .await
        .expect("the index of B downloads");
    assert_eq!(content_of(&built_b), content_of(&w.index(&b)));

    // A second sweep has nothing to do.
    let outcomes = w.sweep().await;
    assert!(outcomes.iter().all(|o| o.report.is_none()), "{outcomes:?}");
}

// ===========================================================================
// Scenario: The pool is shared
// ===========================================================================
#[tokio::test]
async fn the_pool_is_shared() {
    // Given indexes of 2 commits that share most of their code
    let mut w = World::new("kairos_code_index_t1853_pool").await;
    let a = w.a.clone();
    let c = commit_c(&w.git);
    let (index_a, index_c) = (w.index(&a), w.index(&c));
    let first = w
        .bob
        .upload_code_index("payments-api", &a, None, index_a.clone())
        .await
        .expect("A");
    let second = w
        .bob
        .upload_code_index("payments-api", &c, None, index_c.clone())
        .await
        .expect("C");
    let (keys_a, keys_c) = (keys_in(&index_a), keys_in(&index_c));
    let shared = keys_a.intersection(&keys_c).count();
    assert!(
        shared * 2 > keys_a.len(),
        "A and C share most of their summaries: {shared} of {}",
        keys_a.len()
    );

    // Then the shared summaries are stored one time
    assert_eq!(first.new_summaries, keys_a.len());
    assert_eq!(second.new_summaries, keys_c.difference(&keys_a).count());
    let union = keys_a.union(&keys_c).count();
    assert_eq!(second.pool_size as usize, union);
    let rows: i64 = {
        use kairos_db::schema::code_index_summaries::dsl;
        dsl::code_index_summaries
            .filter(dsl::repository_id.eq(w.repo_id))
            .count()
            .get_result(&mut w.conn)
            .unwrap()
    };
    assert_eq!(rows as usize, union);
    assert!(union < keys_a.len() + keys_c.len());
}

// ===========================================================================
// Scenario: A user with no right cannot upload
// ===========================================================================
#[tokio::test]
async fn a_user_with_no_right_cannot_upload() {
    // Given a user with no right on the repository
    let w = World::new("kairos_code_index_t1853_right").await;
    let a = w.a.clone();

    // When the user uploads an index
    let err = w
        .alice
        .upload_code_index("payments-api", &a, None, w.index(&a))
        .await
        .expect_err("alice is not in the owner team");

    // Then the upload is refused
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    assert!(
        w.alice
            .list_code_indexes("payments-api")
            .await
            .unwrap()
            .is_empty(),
        "the refused upload stored nothing"
    );
}

// ===========================================================================
// The rule of the inputs: unknown input is refused and named
// ===========================================================================
#[tokio::test]
async fn unknown_input_is_refused_and_named() {
    let w = World::new("kairos_code_index_t1853_input").await;
    let a = w.a.clone();
    let (status, body) = w
        .bob
        .raw_request(
            reqwest::Method::GET,
            &format!("/api/repositories/payments-api/code-indexes/nearest?commit={a}&depth=3"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(status, 400, "{body}");
    assert!(body.to_string().contains("depth"), "{body}");
    let (status, body) = w
        .bob
        .raw_request(
            reqwest::Method::GET,
            &format!("/api/repositories/payments-api/code-indexes/{a}?format=zip"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(status, 400, "{body}");
    assert!(body.to_string().contains("format"), "{body}");
    let err = w
        .bob
        .upload_code_index("payments-api", "not-a-commit", None, w.index(&a))
        .await
        .expect_err("a bad commit");
    assert!(
        matches!(&err, Error::Validation { field: Some(f), .. } if f == "commit"),
        "{err}"
    );
    let err = w
        .bob
        .upload_code_index("payments-api", &a, Some("my branch"), w.index(&a))
        .await
        .expect_err("a bad ref");
    assert!(
        matches!(&err, Error::Validation { field: Some(f), .. } if f == "ref"),
        "{err}"
    );
    let err = w
        .bob
        .upload_code_index("payments-api", &a, None, b"not an index".to_vec())
        .await
        .expect_err("a body that is not an index");
    assert!(matches!(err, Error::Validation { .. }), "{err}");
}

/// Whether a file below `dir` has `needle` in it.
fn any_file_contains(dir: &Path, needle: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            any_file_contains(&path, needle)
        } else {
            std::fs::read(&path)
                .is_ok_and(|bytes| bytes.windows(needle.len()).any(|w| w == needle.as_bytes()))
        }
    })
}

// ===========================================================================
// Scenario: The builder fetches a private repository with the stored read
// token (COLLIERY-T-3105)
// ===========================================================================
#[tokio::test]
async fn a_private_repository_is_fetched_with_the_stored_read_token() {
    // Given a private repository with an index of commit A, and a push of B
    let w = World::private("kairos_code_index_t3105_private").await;
    let server = w.git_http.as_ref().expect("a git server");
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    let b = commit_b(&w.git);

    // When the builder runs and the repository has no token
    let started = std::time::Instant::now();
    let outcomes = w.sweep().await;
    let elapsed = started.elapsed();

    // Then the fetch fails at once, with no prompt
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    let note = outcomes[0].note.clone().unwrap_or_default();
    assert!(note.starts_with("failed: git fetch failed"), "{note}");
    assert!(outcomes[0].report.is_none());
    assert!(elapsed < std::time::Duration::from_secs(20), "{elapsed:?}");
    assert!(server.seen().iter().all(|s| s.user.is_none()));
    println!("no token: {note} ({elapsed:?})");

    // When a member of the owner team sets the token
    let status = w
        .bob
        .set_repository_credential(
            "payments-api",
            &kairos_client::types_auth::Secret::new(TOKEN),
        )
        .await
        .expect("the token is set");
    assert!(status.set);

    // Then the builder fetches with it, and indexes B
    let outcomes = w.sweep().await;
    let built: Vec<_> = outcomes.iter().filter(|o| o.report.is_some()).collect();
    assert_eq!(built.len(), 1, "{outcomes:?}");
    assert_eq!(built[0].commit, b);
    println!("with the token: built {}", built[0].commit);

    // And git sent the token in the Authorization header, with the user
    // x-access-token, and never in a URL
    let seen = server.seen();
    assert!(
        seen.iter().any(|s| s.user.as_deref() == Some(TOKEN_USER)),
        "{seen:?}"
    );
    assert!(seen.iter().all(|s| !s.uri.contains(TOKEN)), "{seen:?}");
    // And no file of the clones (the config of the clone included) has it
    assert!(!any_file_contains(&w._work.path().join("clones"), TOKEN));

    // When the token is removed, and C is pushed
    w.bob
        .remove_repository_credential("payments-api")
        .await
        .expect("the token is removed");
    commit_c(&w.git);
    let before = server.seen().len();
    let outcomes = w.sweep().await;

    // Then the next fetch has no token
    let note = outcomes[0].note.clone().unwrap_or_default();
    assert!(note.starts_with("failed: git fetch failed"), "{note}");
    assert!(server.seen()[before..].iter().all(|s| s.user.is_none()));
    assert!(!note.contains(TOKEN));
}

// ===========================================================================
// Scenario: The read token of the organization for the host comes before
// the token of the repository (KAIROS-T-0431)
// ===========================================================================
#[tokio::test]
async fn the_org_token_of_the_host_comes_before_the_token_of_the_repository() {
    // Given a private repository on github.com with an index of commit A, a
    // push of B, and a repository token that the git server refuses
    let w = World::private("kairos_code_index_t0431_org_token").await;
    let server = w.git_http.as_ref().expect("a git server");
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    let b = commit_b(&w.git);
    const WRONG: &str = "fake_repository_token_t0431";
    w.bob
        .set_repository_credential(
            "payments-api",
            &kairos_client::types_auth::Secret::new(WRONG),
        )
        .await
        .expect("the repository token is set");

    // When the builder runs and the host has no token of the organization
    let outcomes = w.sweep().await;

    // Then the fetch uses the token of the repository, and the note says so
    let note = outcomes[0].note.clone().unwrap_or_default();
    println!("repository token: {note}");
    assert!(note.starts_with("failed: git fetch failed"), "{note}");
    assert!(
        note.ends_with("The fetch used the read token of the repository."),
        "{note}"
    );
    assert!(!note.contains(WRONG), "{note}");

    // When a member who is not an organization admin sets a token of the
    // host, the call is refused
    let (status, body) = w
        .bob
        .raw_request(
            reqwest::Method::PUT,
            "/api/org/read-tokens/github.com",
            Some(&serde_json::json!({ "token": TOKEN })),
        )
        .await
        .unwrap();
    assert_eq!(status, 403, "{body}");

    // When the organization admin sets the token of the host (written in
    // another case)
    let set = w
        .svc
        .set_org_read_token("GitHub.com", &kairos_client::types_auth::Secret::new(TOKEN))
        .await
        .expect("the token of the host is set");
    assert_eq!(set.host, "github.com");

    // Then the builder fetches with the token of the host, and indexes B,
    // though the token of the repository is wrong
    let before = server.seen().len();
    let outcomes = w.sweep().await;
    let built: Vec<_> = outcomes.iter().filter(|o| o.report.is_some()).collect();
    assert_eq!(built.len(), 1, "{outcomes:?}");
    assert_eq!(built[0].commit, b);
    let seen = server.seen();
    assert!(
        seen[before..]
            .iter()
            .any(|s| s.user.as_deref() == Some(TOKEN_USER)),
        "{seen:?}"
    );
    assert!(seen.iter().all(|s| !s.uri.contains(TOKEN)), "{seen:?}");
    assert!(!any_file_contains(&w._work.path().join("clones"), TOKEN));

    // When the token of the host is removed, and C is pushed
    let left = w
        .svc
        .remove_org_read_token("github.com")
        .await
        .expect("the token of the host is removed");
    assert!(left.is_empty());
    commit_c(&w.git);
    let outcomes = w.sweep().await;

    // Then the next fetch uses the token of the repository again
    let note = outcomes[0].note.clone().unwrap_or_default();
    assert!(
        note.ends_with("The fetch used the read token of the repository."),
        "{note}"
    );

    // When the repository token is removed too, the fetch has no token
    w.bob
        .remove_repository_credential("payments-api")
        .await
        .expect("the repository token is removed");
    let outcomes = w.sweep().await;
    let note = outcomes[0].note.clone().unwrap_or_default();
    assert!(note.ends_with("The fetch used no read token."), "{note}");
}

// ===========================================================================
// KAIROS-T-0318: the first index of a repository that has no index
// ===========================================================================

/// The `.db` files below `dir`.
fn databases_below(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                databases_below(&path)
            } else if path.extension().is_some_and(|e| e == "db") {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}

// Scenario: A repository with no index gets a first build (criteria 1, 4)
#[tokio::test]
async fn a_repository_with_no_index_gets_a_first_build() {
    // Given a registered repository with commit A on main, and no index
    let w = World::new("kairos_code_index_t0318_first").await;
    let a = w.a.clone();
    assert!(
        w.alice
            .list_code_indexes("payments-api")
            .await
            .unwrap()
            .is_empty()
    );
    // The update lane does not read it: it has no index.
    assert!(w.sweep().await.is_empty());

    // When the first-build lane runs
    let outcomes = w.first_builds().await;

    // Then Kairos has an index of the head of main, with no upload
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    let outcome = &outcomes[0];
    assert_eq!(outcome.repository, "payments-api");
    assert_eq!(outcome.commit, a);
    assert_eq!(outcome.base, None, "a first build has no base");
    let report = outcome.report.as_ref().expect("a first build");
    assert!(report.summary.symbols.summarized > 0, "{report:?}");
    println!(
        "first build of {}: {} files, {} symbols, {} symbol summaries in {:?}",
        outcome.commit,
        report.build.files,
        report.build.symbols,
        report.summary.symbols.summarized,
        outcome.elapsed
    );
    let listed = w.alice.list_code_indexes("payments-api").await.unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].commit, a);
    assert_eq!(listed[0].source, "build");
    assert_eq!(listed[0].r#ref.as_deref(), Some("main"));

    // And the index is the index that a checkout of A builds
    let built = w
        .alice
        .download_code_index("payments-api", &a)
        .await
        .expect("the index of A downloads");
    assert_eq!(content_of(&built), content_of(&w.index(&a)));

    // And the work database is removed after the store
    let left = databases_below(&w.work().join("clones").join("first-builds"));
    assert!(left.is_empty(), "{left:?}");

    // And the next pass has nothing to do; the update lane follows the
    // repository now
    assert!(w.first_builds().await.is_empty());
    let outcomes = w.sweep().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[0].note.as_deref(), Some("the head has an index"));
}

// Scenario: A repository that opted out gets no first build (criteria 3, 4)
#[tokio::test]
async fn a_repository_that_opted_out_gets_no_first_build() {
    // Given a registered repository with no index
    let w = World::new("kairos_code_index_t0318_opt_out").await;
    let before = w.bob.get_repository("payments-api").await.unwrap();
    assert_eq!(before.repository.code_index_build, CodeIndexBuild::On);

    // When a member of the owner team sets code_index_build to off
    let repo = w
        .bob
        .update_repository(
            "payments-api",
            &UpdateRepositoryRequest {
                code_index_build: Some(CodeIndexBuild::Off),
                ..Default::default()
            },
        )
        .await
        .expect("bob opts the repository out");
    assert_eq!(repo.code_index_build, CodeIndexBuild::Off);
    let read = w.alice.get_repository("payments-api").await.unwrap();
    assert_eq!(read.repository.code_index_build, CodeIndexBuild::Off);

    // Then the first-build lane does not build it, and says why
    let outcomes = w.first_builds().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(outcomes[0].report.is_none());
    assert_eq!(outcomes[0].note.as_deref(), Some(OPTED_OUT));
    assert!(
        w.alice
            .list_code_indexes("payments-api")
            .await
            .unwrap()
            .is_empty()
    );
    // And no clone is made
    assert!(!w.work().join("clones").join("clones").exists());

    // And with an uploaded index, the update lane does not update it
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("an upload still works");
    commit_b(&w.git);
    let outcomes = w.sweep().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[0].note.as_deref(), Some(OPTED_OUT));

    // When the setting is on again, the update lane updates the index
    w.bob
        .update_repository(
            "payments-api",
            &UpdateRepositoryRequest {
                code_index_build: Some(CodeIndexBuild::On),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let outcomes = w.sweep().await;
    assert!(outcomes[0].report.is_some(), "{outcomes:?}");
}

// Scenario: The setting refuses an unknown value and names it
#[tokio::test]
async fn the_opt_out_setting_refuses_an_unknown_value() {
    let w = World::new("kairos_code_index_t0318_value").await;
    // A text that is not on or off: 422, with the field and the value.
    for value in ["maybe", "OFF", ""] {
        let (status, body) = w
            .bob
            .raw_request(
                reqwest::Method::PATCH,
                "/api/repositories/payments-api",
                Some(&serde_json::json!({ "code_index_build": value })),
            )
            .await
            .unwrap();
        let text = body.to_string();
        println!("code_index_build = {value:?}: {status} {text}");
        assert_eq!(status, 422, "{text}");
        assert!(text.contains("code_index_build"), "{text}");
        assert!(text.contains(&format!("`{value}`")), "{text}");
    }
    // A value that is not a text: refused, with the field. (serde_json
    // classes this error as a syntax error, so the status is 400.)
    let (status, body) = w
        .bob
        .raw_request(
            reqwest::Method::PATCH,
            "/api/repositories/payments-api",
            Some(&serde_json::json!({ "code_index_build": true })),
        )
        .await
        .unwrap();
    println!("code_index_build = true: {status} {body}");
    assert_eq!(status, 400, "{body}");
    assert!(body.to_string().contains("code_index_build"), "{body}");
    let repo = w.bob.get_repository("payments-api").await.unwrap();
    assert_eq!(repo.repository.code_index_build, CodeIndexBuild::On);
}

/// The fake summarizer, with a delay for each request of a path that
/// starts with `slow`, and a count of those requests. It makes a first
/// build that takes some seconds.
#[derive(Clone)]
struct SlowSummarizers {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    delay: std::time::Duration,
}

struct SlowSummarizer {
    inner: FakeSummarizer,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    delay: std::time::Duration,
}

impl kairos_index::Summarizer for SlowSummarizer {
    fn model(&self) -> String {
        self.inner.model()
    }

    fn summarize(&mut self, request: &kairos_index::SummaryRequest) -> Result<String, String> {
        if request.path.starts_with("slow") {
            std::thread::sleep(self.delay);
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        self.inner.summarize(request)
    }
}

impl SummarizerSource for SlowSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(
            &mut dyn kairos_index::Summarizer,
        )
            -> Result<kairos_index::UpdateReport, kairos_index::IndexError>,
    ) -> Result<kairos_index::UpdateReport, kairos_index::IndexError> {
        job(&mut SlowSummarizer {
            inner: FakeSummarizer::default(),
            calls: Arc::clone(&self.calls),
            delay: self.delay,
        })
    }

    fn model_name(&self) -> String {
        FakeSummarizer::default().model
    }
}

/// Wait for `check`, for `limit` at most.
async fn wait_for<F, Fut>(limit: std::time::Duration, what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = std::time::Instant::now();
    while !check().await {
        assert!(started.elapsed() < limit, "no {what} after {limit:?}");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

// Scenario: A first build does not stop the updates (criterion 2)
#[tokio::test]
async fn a_first_build_does_not_stop_the_updates() {
    // Given payments-api with an index of A, and big-lib with no index,
    // whose first build takes some seconds
    let w = World::new("kairos_code_index_t0318_lanes").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    let big = w.work().join("big-lib");
    std::fs::create_dir_all(&big).unwrap();
    git(&big, &["init", "-q", "-b", "main"]);
    const FUNCTIONS: usize = 60;
    let code: String = (0..FUNCTIONS)
        .map(|i| format!("def f{i}(x):\n    return x + {i}\n\n\n"))
        .collect();
    write(&big, "slow/__init__.py", "");
    write(&big, "slow/lib.py", &code);
    git(&big, &["add", "-A"]);
    git(&big, &["commit", "-q", "-m", "big"]);
    w.bob
        .create_repository(&repository("big-lib"))
        .await
        .expect("big-lib");

    // When the builder runs, with an interval of half a second
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let summarizers = SlowSummarizers {
        calls: Arc::clone(&calls),
        delay: std::time::Duration::from_millis(100),
    };
    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    // KAIROS-T-0374: the builder adds the jobs, and the job worker of the
    // server runs them, one build with the embedded model at a time.
    let worker = Arc::new(kairos_server::jobs::Worker::new(
        w.state.blocking.clone(),
        w.service.instance(),
        kairos_server::jobs::JobSettings {
            poll: std::time::Duration::from_millis(200),
            ..Default::default()
        },
    ));
    let jobs = tokio::spawn(Arc::clone(&worker).run());
    let builder = tokio::spawn(run_builder(
        Arc::clone(&worker),
        w.state.blocking.clone(),
        Arc::clone(&w.service),
        Some(Arc::new(summarizers)),
        embedder,
        std::time::Duration::from_millis(500),
    ));
    let count = || calls.load(std::sync::atomic::Ordering::SeqCst);
    wait_for(
        std::time::Duration::from_secs(60),
        "start of the first build of big-lib",
        || async { count() > 0 },
    )
    .await;

    // And commit B is pushed to payments-api while the first build runs
    let b = commit_b(&w.git);
    let pushed = std::time::Instant::now();

    // Then the update lane indexes B before the first build ends
    wait_for(std::time::Duration::from_secs(60), "index of B", || async {
        w.alice
            .list_code_indexes("payments-api")
            .await
            .unwrap()
            .iter()
            .any(|i| i.commit == b)
    })
    .await;
    let done_at_b = count();
    let big_at_b = w.alice.list_code_indexes("big-lib").await.unwrap();
    println!(
        "B indexed {:?} after the push; the first build of big-lib had {done_at_b} of about {} summaries",
        pushed.elapsed(),
        FUNCTIONS + 2
    );
    assert!(big_at_b.is_empty(), "the first build ended first");
    assert!(done_at_b < FUNCTIONS, "{done_at_b}");

    // And the first build of big-lib ends, with an index of its head
    wait_for(
        std::time::Duration::from_secs(120),
        "index of big-lib",
        || async {
            !w.alice
                .list_code_indexes("big-lib")
                .await
                .unwrap()
                .is_empty()
        },
    )
    .await;
    let big_index = w.alice.list_code_indexes("big-lib").await.unwrap();
    assert_eq!(big_index[0].source, "build");
    assert!(count() >= FUNCTIONS);
    builder.abort();
    jobs.abort();
}

// ===========================================================================
// KAIROS-T-0331: each run of the builder leaves a record
// ===========================================================================

/// The runs of `payments-api`, newest first.
async fn runs_of(w: &World) -> Vec<kairos_client::types_code_index::CodeIndexBuild> {
    w.alice
        .list_code_index_builds("payments-api", None)
        .await
        .expect("the runs list")
        .items
}

#[tokio::test]
async fn a_push_records_an_ok_run_and_an_upload_is_a_run() {
    // Given an index of commit A, sent by bob
    let w = World::new("kairos_code_index_t0331_push").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].trigger, "upload");
    assert_eq!(runs[0].outcome, "ok");
    assert_eq!(runs[0].commit.as_deref(), Some(a.as_str()));
    assert_eq!(runs[0].r#ref.as_deref(), Some("main"));
    assert!(runs[0].requested_by.is_some(), "the uploader is recorded");
    assert!(runs[0].finished_at.is_some());

    // A pass with nothing to do leaves no row
    w.sweep().await;
    assert_eq!(runs_of(&w).await.len(), 1);

    // When commit B is pushed and the builder runs
    let b = commit_b(&w.git);
    let outcomes = w.sweep().await;
    assert!(outcomes.iter().any(|o| o.report.is_some()), "{outcomes:?}");

    // Then the newest run is an ok push run of commit B with the counts
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 2, "{runs:?}");
    let run = &runs[0];
    assert_eq!(run.trigger, "push");
    assert_eq!(run.outcome, "ok");
    assert_eq!(run.commit.as_deref(), Some(b.as_str()));
    assert_eq!(run.r#ref.as_deref(), Some("main"));
    assert!(run.files.is_some_and(|n| n > 0), "{run:?}");
    assert!(run.symbols.is_some_and(|n| n > 0), "{run:?}");
    assert!(run.summaries_made.is_some_and(|n| n > 0), "{run:?}");
    assert_eq!(run.error, None);
    assert_eq!(run.requested_by, None, "the builder has no user");
    assert!(run.finished_at.is_some());
    assert_eq!(runs[1].trigger, "upload");
}

#[tokio::test]
async fn a_first_build_records_a_run() {
    let w = World::new("kairos_code_index_t0331_first").await;
    let outcomes = w.first_builds().await;
    assert!(outcomes[0].report.is_some(), "{outcomes:?}");
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].trigger, "first");
    assert_eq!(runs[0].outcome, "ok");
    assert_eq!(runs[0].commit.as_deref(), Some(w.a.as_str()));
    assert!(runs[0].symbols.is_some_and(|n| n > 0));
}

#[tokio::test]
async fn a_failed_fetch_records_one_failed_run() {
    // Given an index of commit A, and a repository that the server cannot
    // fetch any more
    let w = World::new("kairos_code_index_t0331_failed").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    std::fs::remove_dir_all(&w.git).expect("remove the git repository");

    // When the builder runs 2 times
    let outcomes = w.sweep().await;
    let note = outcomes[0].note.clone().expect("a note");
    assert!(note.starts_with("failed"), "{note}");
    w.sweep().await;

    // Then one failed run has the text of the failure, and the server is
    // still up
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 2, "the upload and one failure: {runs:?}");
    let run = &runs[0];
    assert_eq!(run.trigger, "push");
    assert_eq!(run.outcome, "failed");
    assert_eq!(run.error.as_deref(), Some(note.as_str()));
    assert_eq!(run.commit, None, "the fetch gave no commit");
    assert!(run.finished_at.is_some());
    assert!(w.alice.list_code_indexes("payments-api").await.is_ok());
}

#[tokio::test]
async fn a_run_that_the_server_did_not_end_is_failed_at_the_next_run() {
    // Given a run that is still `running` from a server that stopped
    let mut w = World::new("kairos_code_index_t0331_stale").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    let stale =
        kairos_db::code_index_builds::start(&mut w.conn, w.repo_id, "push", None, None, None)
            .expect("a running row");

    // When the next run of the repository starts
    commit_b(&w.git);
    let outcomes = w.sweep().await;
    assert!(outcomes.iter().any(|o| o.report.is_some()), "{outcomes:?}");

    // Then the stale run is failed with the text, and the new run is ok
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 3, "{runs:?}");
    let old = runs
        .iter()
        .find(|r| r.id == stale.id.to_string())
        .expect("the stale run is listed");
    assert_eq!(old.outcome, "failed");
    assert_eq!(
        old.error.as_deref(),
        Some(kairos_db::code_index_builds::SERVER_STOPPED)
    );
    assert_eq!(runs[0].outcome, "ok");
    assert_eq!(runs[0].trigger, "push");
}

// KAIROS-T-0354, KAIROS-T-0374: the update lane does not add a second job
// of a repository whose requested build waits, and the requested run keeps
// its row and ends ok.
#[tokio::test]
async fn the_update_lane_does_not_start_a_run_while_a_requested_run_is_active() {
    // Given an index of commit A, and a requested run that waits for the
    // builder
    let w = World::new("kairos_code_index_t0354_active").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    let requested = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    let b = commit_b(&w.git);

    // When the update lane runs
    let outcomes = w.sweep().await;

    // Then it adds no job of the repository, and says why
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
    let note = outcomes[0].note.clone().expect("a note");
    assert!(outcomes[0].report.is_none(), "{outcomes:?}");
    assert!(note.starts_with(ACTIVE_RUN), "{note}");
    assert!(note.contains(&requested.id), "{note}");

    // And the requested run is the one build, and ends ok with the index of
    // the head
    assert!(outcomes[1].report.is_some(), "{outcomes:?}");
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 2, "the upload and the request: {runs:?}");
    assert_eq!(runs[0].id, requested.id);
    assert_eq!(runs[0].outcome, "ok", "{runs:?}");
    assert_eq!(runs[0].commit.as_deref(), Some(b.as_str()));
    let outcomes = w.sweep().await;
    assert_eq!(outcomes[0].note.as_deref(), Some("the head has an index"));
}

// KAIROS-T-0354, KAIROS-T-0374: the stale-run rule ends only the runs that
// no open job names; the run of a job that runs, or waits for a server,
// stays.
#[tokio::test]
async fn only_the_runs_of_a_server_that_stopped_are_ended() {
    use kairos_db::code_index_builds as runs;
    let mut w = World::new("kairos_code_index_t0354_stale").await;
    // Given a running run of a server that stopped, a running run from
    // before the servers recorded their start, and a running run of a
    // build job that another server holds
    let stopped = runs::start(
        &mut w.conn,
        w.repo_id,
        "push",
        None,
        None,
        Some(Uuid::new_v4()),
    )
    .expect("a run of a server that stopped");
    let old = runs::start(&mut w.conn, w.repo_id, "push", None, None, None)
        .expect("a run with no server");
    let other = Uuid::new_v4();
    let ours = runs::start(&mut w.conn, w.repo_id, "first", None, None, Some(other))
        .expect("a run of a job");
    let job = kairos_db::jobs::enqueue(
        &mut w.conn,
        &kairos_db::jobs::NewJob {
            tenant: Some("acme".into()),
            payload: serde_json::json!({
                "repository_id": w.repo_id, "trigger": "first", "run": ours.id
            }),
            dedupe_key: Some(job_key("acme", w.repo_id)),
            ..kairos_db::jobs::NewJob::new("embedded_build")
        },
    )
    .unwrap()
    .expect("the job");
    kairos_db::jobs::take(&mut w.conn, other, &["embedded_build".to_string()], 600.0)
        .unwrap()
        .expect("the other server holds the job");

    // When the builder of this server starts
    end_stale_runs(&w.state.blocking).await;

    // Then the runs that no job names are ended, and the run of the job is
    // not
    let runs = runs_of(&w).await;
    let of = |id: Uuid| {
        runs.iter()
            .find(|r| r.id == id.to_string())
            .unwrap_or_else(|| panic!("run {id}"))
    };
    for stale in [stopped.id, old.id] {
        assert_eq!(of(stale).outcome, "failed", "{runs:?}");
        assert_eq!(of(stale).error.as_deref(), Some(runs::SERVER_STOPPED));
    }
    assert_eq!(of(ours.id).outcome, "running", "{runs:?}");
    assert_eq!(of(ours.id).error, None);

    // And when the job ended without its run, the next start ends it
    assert!(kairos_db::jobs::fail(&mut w.conn, job.id, other, "gone").unwrap());
    end_stale_runs(&w.state.blocking).await;
    let runs = runs_of(&w).await;
    let ended = runs
        .iter()
        .find(|r| r.id == ours.id.to_string())
        .expect("the run");
    assert_eq!(ended.outcome, "failed");
    assert_eq!(ended.error.as_deref(), Some(runs::SERVER_STOPPED));
}

#[tokio::test]
async fn the_runs_list_clamps_its_limit() {
    let w = World::new("kairos_code_index_t0331_limit").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");
    let list = w
        .alice
        .list_code_index_builds("payments-api", None)
        .await
        .unwrap();
    assert_eq!(list.limit, 20);
    assert_eq!(list.total, 1);
    let list = w
        .alice
        .list_code_index_builds("payments-api", Some(0))
        .await
        .unwrap();
    assert_eq!(list.limit, 1);
    let list = w
        .alice
        .list_code_index_builds("payments-api", Some(500))
        .await
        .unwrap();
    assert_eq!(list.limit, 100);
    assert_eq!(list.items.len(), 1);
    // An unknown repository
    let err = w
        .alice
        .list_code_index_builds("no-such-repo", None)
        .await
        .expect_err("refused");
    assert!(matches!(err, Error::NotFound { .. }), "{err}");
}

// ===========================================================================
// KAIROS-T-0332: a person asks for a rebuild
// ===========================================================================

#[tokio::test]
async fn a_member_of_the_owner_team_asks_for_a_rebuild() {
    // Given an index of commit A, sent by bob
    let w = World::new("kairos_code_index_t0332_rebuild").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the first index");

    // A member with no right on the repository is refused
    let err = w
        .alice
        .rebuild_code_index("payments-api")
        .await
        .expect_err("alice has no right");
    assert!(matches!(err, Error::Forbidden { .. }), "{err}");
    assert_eq!(runs_of(&w).await.len(), 1, "a refusal writes no run");

    // When bob asks
    let run = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    assert_eq!(run.trigger, "request");
    assert_eq!(run.outcome, "running");
    assert!(run.requested_by.is_some());
    assert_eq!(run.finished_at, None);

    // A second request while the first runs is refused and names the run
    let err = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect_err("one run at a time");
    match &err {
        Error::Conflict { code, details, .. } => {
            assert_eq!(code, "CODE_INDEX_BUILD_RUNNING");
            assert_eq!(details["run"], run.id);
        }
        other => panic!("{other}"),
    }

    // Then the builder makes the index on its next pass
    let outcomes = w.requested_builds().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    let outcome = &outcomes[0];
    assert_eq!(outcome.commit, a);
    let report = outcome.report.as_ref().expect("a full build");
    assert!(report.build.symbols > 0);

    // And the run is ok, with the counts, and the index of A is there
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs[0].id, run.id);
    assert_eq!(runs[0].outcome, "ok");
    assert_eq!(runs[0].trigger, "request");
    assert_eq!(runs[0].commit.as_deref(), Some(a.as_str()));
    assert!(runs[0].symbols.is_some_and(|n| n > 0));
    let built = w
        .alice
        .download_code_index("payments-api", &a)
        .await
        .expect("the index of A downloads");
    assert_eq!(content_of(&built), content_of(&w.index(&a)));
    let listed = w.alice.list_code_indexes("payments-api").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source, "build", "the rebuild replaced the upload");

    // And a new request is possible again
    w.bob
        .rebuild_code_index("payments-api")
        .await
        .expect("the run ended, so a new request goes");
    let mut conn = PgConnection::establish(&with_database(
        &common::admin_database_url(),
        "kairos_code_index_t0332_rebuild",
    ))
    .unwrap();
    let open = kairos_db::jobs::open_by_key(&mut conn, &job_key("acme", w.repo_id))
        .unwrap()
        .expect("the request is a job");
    assert_eq!(
        open.priority,
        kairos_server::jobs::priority::REQUESTED_BUILD
    );
}

#[tokio::test]
async fn a_repository_with_the_builder_off_is_not_rebuilt_on_request() {
    let w = World::new("kairos_code_index_t0332_off").await;
    w.bob
        .update_repository(
            "payments-api",
            &UpdateRepositoryRequest {
                code_index_build: Some(CodeIndexBuild::Off),
                ..Default::default()
            },
        )
        .await
        .expect("off");
    let err = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect_err("the builder is off");
    match &err {
        Error::Other {
            status,
            code,
            message,
            ..
        } => {
            assert_eq!(*status, 422);
            assert_eq!(code, "CODE_INDEX_BUILD_OFF");
            assert!(message.contains("--code-index-build on"), "{message}");
        }
        other => panic!("{other}"),
    }
    assert!(runs_of(&w).await.is_empty(), "a refusal writes no run");
}

#[tokio::test]
async fn a_requested_build_that_fails_ends_its_run_as_failed() {
    let w = World::new("kairos_code_index_t0332_failed").await;
    std::fs::remove_dir_all(&w.git).expect("remove the git repository");
    let run = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    let outcomes = w.requested_builds().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    let note = outcomes[0].note.clone().expect("a note");
    assert!(note.starts_with("failed"), "{note}");
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].id, run.id);
    assert_eq!(runs[0].outcome, "failed");
    assert_eq!(runs[0].error.as_deref(), Some(note.as_str()));
    // The person can ask again: a requested build has no memory of a
    // failed commit.
    w.bob
        .rebuild_code_index("payments-api")
        .await
        .expect("a new request after a failure");
}

// ===========================================================================
// KAIROS-T-0398: the code index builds of a repository can be stopped
// ===========================================================================

/// Set `code_index_build` of `payments-api` with the REST call
/// `PATCH /api/repositories/{slug}`, as bob.
async fn set_build(w: &World, value: CodeIndexBuild) {
    let repo = w
        .bob
        .update_repository(
            "payments-api",
            &UpdateRepositoryRequest {
                code_index_build: Some(value),
                ..Default::default()
            },
        )
        .await
        .expect("bob sets code_index_build");
    assert_eq!(repo.code_index_build, value);
}

/// The build kinds, for a lease by hand.
fn build_kinds() -> Vec<String> {
    vec![
        kairos_server::jobs::kinds::EMBEDDED_BUILD.to_string(),
        kairos_server::jobs::kinds::HOSTED_BUILD.to_string(),
    ]
}

// Scenario: off cancels the build jobs that wait, and the sweep adds none
#[tokio::test]
async fn turning_the_builder_off_cancels_the_waiting_builds() {
    // Given a requested build that waits for a server
    let mut w = World::new("kairos_code_index_t0398_cancel").await;
    let key = job_key("acme", w.repo_id);
    let run = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    let job = kairos_db::jobs::open_by_key(&mut w.conn, &key)
        .unwrap()
        .expect("the request is a job");
    assert_eq!(job.state, "queued");

    // When bob sets code_index_build to off with the REST call
    set_build(&w, CodeIndexBuild::Off).await;

    // Then the job is cancelled, with no more tries
    let cancelled = kairos_db::jobs::get(&mut w.conn, job.id).unwrap().unwrap();
    assert_eq!(cancelled.state, "failed");
    assert_eq!(
        cancelled.last_error.as_deref(),
        Some(kairos_server::code_index::TURNED_OFF)
    );
    assert!(
        kairos_db::jobs::open_by_key(&mut w.conn, &key)
            .unwrap()
            .is_none()
    );
    // And its run ends as failed, with the reason
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].id, run.id);
    assert_eq!(runs[0].outcome, "failed");
    assert_eq!(
        runs[0].error.as_deref(),
        Some(kairos_server::code_index::TURNED_OFF)
    );
    // And no server runs it
    assert!(w.requested_builds().await.is_empty());

    // And the sweep adds no first build and no update
    let outcomes = w.first_builds().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[0].note.as_deref(), Some(OPTED_OUT));
    assert!(
        kairos_db::jobs::open_by_key(&mut w.conn, &key)
            .unwrap()
            .is_none()
    );
    let outcomes = w.sweep().await;
    assert!(
        outcomes
            .iter()
            .all(|o| o.report.is_none() && o.note.as_deref() == Some(OPTED_OUT)),
        "{outcomes:?}"
    );
    assert!(
        kairos_db::jobs::open_by_key(&mut w.conn, &key)
            .unwrap()
            .is_none()
    );
    assert_eq!(runs_of(&w).await.len(), 1, "no new run");

    // When the builder is on again, a job whose server stopped (its lease
    // ran out) waits to be taken again: off cancels it too
    set_build(&w, CodeIndexBuild::On).await;
    w.bob
        .rebuild_code_index("payments-api")
        .await
        .expect("a new request");
    let leased = kairos_db::jobs::take(&mut w.conn, Uuid::new_v4(), &build_kinds(), -1.0)
        .unwrap()
        .expect("a lease that ran out");
    set_build(&w, CodeIndexBuild::Off).await;
    let cancelled = kairos_db::jobs::get(&mut w.conn, leased.id)
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.state, "failed");
    assert_eq!(runs_of(&w).await[0].outcome, "failed");
}

// Scenario: off does not kill a build that runs, but no further try starts
#[tokio::test]
async fn turning_the_builder_off_lets_a_running_build_go_on_with_no_new_try() {
    // Given a requested build that a server runs now
    let mut w = World::new("kairos_code_index_t0398_running").await;
    let key = job_key("acme", w.repo_id);
    let run = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    let server = Uuid::new_v4();
    let leased = kairos_db::jobs::take(&mut w.conn, server, &build_kinds(), 600.0)
        .unwrap()
        .expect("the server leases the job");

    // When bob sets code_index_build to off
    set_build(&w, CodeIndexBuild::Off).await;

    // Then the build goes on: its job and its run are not changed
    let job = kairos_db::jobs::get(&mut w.conn, leased.id)
        .unwrap()
        .unwrap();
    assert_eq!(job.state, "leased");
    assert_eq!(job.leased_by, Some(server));
    assert_eq!(runs_of(&w).await[0].outcome, "running");

    // When the try fails, and the job is queued for a further try
    let released = kairos_db::jobs::release(&mut w.conn, leased.id, server, "the try failed", 0.0)
        .unwrap()
        .expect("the server held the lease");
    assert_eq!(released.state, "queued");

    // Then the further try builds nothing, and ends the run
    let outcomes = w.requested_builds().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(outcomes[0].report.is_none(), "{outcomes:?}");
    assert_eq!(outcomes[0].note.as_deref(), Some(OPTED_OUT));
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].id, run.id);
    assert_eq!(runs[0].outcome, "failed");
    assert_eq!(runs[0].error.as_deref(), Some(OPTED_OUT));
    assert!(
        w.alice
            .list_code_indexes("payments-api")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        kairos_db::jobs::open_by_key(&mut w.conn, &key)
            .unwrap()
            .is_none()
    );
}

// ===========================================================================
// KAIROS-T-0399: a build that stalls or stops its server trips the breaker
// of its repository
// ===========================================================================

// Scenario: a stalled update trips the breaker; the sweep and a waiting
// job start no build; a request resets it
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stalled_build_trips_the_breaker_until_a_request() {
    // Given an index of A, and a push of B whose update stalls at its first
    // summary
    const DB: &str = "kairos_code_index_t0399_stall";
    let mut w = World::new(DB).await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    let b = commit_extra(&w.git);
    let key = job_key("acme", w.repo_id);
    let frozen = FrozenSummarizers {
        keys: Arc::new(Mutex::new(Vec::new())),
        limit: 0,
    };
    // A pool of its own: the stalled step keeps its connection.
    let url = with_database(&common::admin_database_url(), DB);
    let (worker, outcomes) = build_worker(
        kairos_server::blocking::BlockingTenantPool::new(&url, 4),
        &w.service,
        Arc::new(frozen),
        quick_jobs(),
    );
    kairos_server::code_index::enqueue_updates(&w.state.blocking, &w.service, &worker).await;
    let job = kairos_db::jobs::open_by_key(&mut w.conn, &key)
        .unwrap()
        .expect("the update job");

    // When the worker runs it, and the job makes no progress. A runtime of
    // its own: the stalled step parks its thread for ever, and a runtime
    // waits for its threads when it stops.
    let stalled = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    stalled.spawn(Arc::clone(&worker).run());
    wait_for_breaker(&w, DB, &key).await;
    stalled.shutdown_background();

    // Then the watchdog killed it, it failed on its first try, and the
    // breaker tripped with the reason
    let job = kairos_db::jobs::get(&mut w.conn, job.id).unwrap().unwrap();
    assert_eq!(job.state, "failed", "{job:?}");
    assert_eq!(job.tries, 1);
    let reason = job.last_error.clone().unwrap();
    assert!(reason.contains("made no progress"), "{reason}");
    assert_eq!(reports(&outcomes), 0);
    let breaker = kairos_db::code_index_breakers::get(&mut w.conn, w.repo_id)
        .unwrap()
        .expect("tripped");
    assert_eq!(breaker.reason, reason);
    assert_eq!(breaker.job_id, Some(job.id));

    // And the REST repository shows it
    let shown = w
        .svc
        .get_repository("payments-api")
        .await
        .unwrap()
        .repository
        .code_index_breaker;
    assert!(shown.tripped);
    assert_eq!(shown.reason.as_deref(), Some(reason.as_str()));
    assert_eq!(
        shown.tripped_at.as_deref(),
        Some(breaker.tripped_at.to_rfc3339().as_str())
    );

    // And the sweep adds no update while it is tripped
    let notes = w.sweep().await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].report.is_none(), "{notes:?}");
    assert!(
        notes[0]
            .note
            .as_deref()
            .is_some_and(|n| n.starts_with(BREAKER_TRIPPED)),
        "{notes:?}"
    );
    assert!(
        kairos_db::jobs::open_by_key(&mut w.conn, &key)
            .unwrap()
            .is_none()
    );

    // And a build job that waits (an update queued again) does not build
    let waiting = kairos_db::jobs::enqueue(
        &mut w.conn,
        &kairos_db::jobs::NewJob {
            tenant: Some("acme".to_string()),
            payload: serde_json::json!({ "repository_id": w.repo_id, "trigger": "push" }),
            dedupe_key: Some(key.clone()),
            ..kairos_db::jobs::NewJob::new(kairos_server::jobs::kinds::EMBEDDED_BUILD)
        },
    )
    .unwrap()
    .expect("a waiting update");
    let outcomes = w.requested_builds().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(outcomes[0].report.is_none(), "{outcomes:?}");
    assert!(
        outcomes[0]
            .note
            .as_deref()
            .is_some_and(|n| n.starts_with(BREAKER_TRIPPED)),
        "{outcomes:?}"
    );
    let waiting = kairos_db::jobs::get(&mut w.conn, waiting.id)
        .unwrap()
        .unwrap();
    assert_eq!(waiting.state, "done", "{waiting:?}");
    assert!(
        w.alice
            .download_code_index("payments-api", &b)
            .await
            .is_err(),
        "no index of B"
    );

    // When bob asks for a build
    w.bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");

    // Then the breaker is reset, and the requested build makes the index of B
    assert!(
        kairos_db::code_index_breakers::get(&mut w.conn, w.repo_id)
            .unwrap()
            .is_none()
    );
    assert!(
        !w.svc
            .get_repository("payments-api")
            .await
            .unwrap()
            .repository
            .code_index_breaker
            .tripped
    );
    let outcomes = w.requested_builds().await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[0].commit, b);
    assert!(outcomes[0].report.is_some(), "{outcomes:?}");
    // And the sweep works on the repository again
    let notes = w.sweep().await;
    assert!(
        notes
            .iter()
            .all(|n| !n.note.as_deref().unwrap_or("").starts_with(BREAKER_TRIPPED)),
        "{notes:?}"
    );
}

// ===========================================================================
// KAIROS-T-0341: the builder chooses the summarizer of each build, and a
// hosted summarizer sends N requests at a time with retries
// ===========================================================================

/// The settings of the tenant: Ollama Cloud at the fake endpoint.
fn hosted_settings(base_url: &str, key: &str, concurrency: i32) -> PutCodeIndexSettings {
    PutCodeIndexSettings {
        summary: Some(PutSummaryProvider {
            provider: "ollama-cloud".into(),
            base_url: Some(base_url.into()),
            model: Some("fake-model".into()),
            region: None,
            secret: Some(Secret::new(key)),
        }),
        vectors: Some(PutVectorProvider {
            provider: "embedded".into(),
            ..Default::default()
        }),
        concurrency: Some(concurrency),
        default_summaries: None,
        version: None,
    }
}

async fn opt_in(w: &World, slug: &str) {
    w.bob
        .update_repository(
            slug,
            &UpdateRepositoryRequest {
                code_index_summaries: Some(CodeIndexSummaries::Hosted),
                ..Default::default()
            },
        )
        .await
        .expect("the opt-in");
}

/// The newest run of a repository.
async fn newest_run(w: &World, slug: &str) -> kairos_client::types_code_index::CodeIndexBuild {
    w.alice
        .list_code_index_builds(slug, Some(1))
        .await
        .expect("the runs")
        .items
        .into_iter()
        .next()
        .expect("a run")
}

#[tokio::test]
async fn a_hosted_repository_is_summarized_by_the_endpoint() {
    // Given a tenant on Ollama Cloud (the fake endpoint, 4 at a time, a 429
    // on the first request), one repository opted in and one not
    let w = World::new("kairos_code_index_t0341_hosted").await;
    let chat = common::chat_http::serve("ollama_t0341_key", 1).await;
    w.svc
        .put_code_index_settings(&hosted_settings(&chat.base_url, "ollama_t0341_key", 4))
        .await
        .expect("the provider");
    let infra = w.work().join("platform-infra");
    std::fs::create_dir_all(&infra).expect("the folder of platform-infra");
    repository_at_a(&infra);
    w.bob
        .create_repository(&repository("platform-infra"))
        .await
        .expect("platform-infra");
    opt_in(&w, "payments-api").await;

    // When the first-build lane runs
    let outcomes = w.first_builds().await;
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
    let hosted = outcomes
        .iter()
        .find(|o| o.repository == "payments-api")
        .expect("payments-api");
    let embedded = outcomes
        .iter()
        .find(|o| o.repository == "platform-infra")
        .expect("platform-infra");

    // Then the opted-in repository is summarized through the endpoint, 4
    // requests at a time, and the 429 was tried again
    let report = hosted
        .report
        .as_ref()
        .unwrap_or_else(|| panic!("{hosted:?}"));
    assert!(report.summary.symbols.summarized > 0, "{report:?}");
    assert!(
        report
            .summary
            .calls
            .iter()
            .all(|c| c.summary.starts_with("A hosted summary of")),
        "{:?}",
        report.summary.calls
    );
    assert_eq!(
        chat.requests(),
        report.summary.calls.len() + 1,
        "each summary is one request, and the 429 one more"
    );
    assert!(
        chat.max_in_flight() >= 2,
        "in flight: {}",
        chat.max_in_flight()
    );
    assert_eq!(
        newest_run(&w, "payments-api").await.model.as_deref(),
        Some("ollama-cloud/fake-model")
    );

    // And the other repository used the embedded model
    let report = embedded
        .report
        .as_ref()
        .unwrap_or_else(|| panic!("{embedded:?}"));
    assert!(
        report
            .summary
            .calls
            .iter()
            .all(|c| !c.summary.starts_with("A hosted summary of")),
        "{:?}",
        report.summary.calls
    );
    assert_eq!(
        newest_run(&w, "platform-infra").await.model.as_deref(),
        Some("fake/fixed")
    );
}

// KAIROS-T-0358: the default summarizer of the organization.
#[tokio::test]
async fn a_repository_that_follows_the_organization_gets_its_default() {
    // Given a tenant on Ollama Cloud with the default hosted, a repository
    // that follows the organization (the default of a new one) and a
    // repository with embedded set by hand
    let w = World::new("kairos_code_index_t0358_default").await;
    let chat = common::chat_http::serve("ollama_t0358_key", 0).await;
    let mut settings = hosted_settings(&chat.base_url, "ollama_t0358_key", 4);
    settings.default_summaries = Some(CodeIndexSummaries::Hosted);
    w.svc
        .put_code_index_settings(&settings)
        .await
        .expect("the provider and the default");
    let infra = w.work().join("platform-infra");
    std::fs::create_dir_all(&infra).expect("the folder of platform-infra");
    repository_at_a(&infra);
    w.bob
        .create_repository(&repository("platform-infra"))
        .await
        .expect("platform-infra");
    w.bob
        .update_repository(
            "platform-infra",
            &UpdateRepositoryRequest {
                code_index_summaries: Some(CodeIndexSummaries::Embedded),
                ..Default::default()
            },
        )
        .await
        .expect("embedded by hand");

    // When the first-build lane runs
    let outcomes = w.first_builds().await;
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");

    // Then the repository that follows the organization is summarized
    // through the endpoint, and the other one by the embedded model
    assert_eq!(
        newest_run(&w, "payments-api").await.model.as_deref(),
        Some("ollama-cloud/fake-model")
    );
    assert!(chat.requests() > 0, "the endpoint got no request");
    assert_eq!(
        newest_run(&w, "platform-infra").await.model.as_deref(),
        Some("fake/fixed")
    );
}

#[tokio::test]
async fn a_hosted_endpoint_that_refuses_the_key_fails_the_run_and_the_next_repository_builds() {
    let w = World::new("kairos_code_index_t0341_refused").await;
    let chat = common::chat_http::serve("the-real-key", 0).await;
    w.svc
        .put_code_index_settings(&hosted_settings(&chat.base_url, "a-wrong-key", 2))
        .await
        .expect("the provider");
    let infra = w.work().join("platform-infra");
    std::fs::create_dir_all(&infra).expect("the folder of platform-infra");
    repository_at_a(&infra);
    w.bob
        .create_repository(&repository("platform-infra"))
        .await
        .expect("platform-infra");
    opt_in(&w, "payments-api").await;

    let outcomes = w.first_builds().await;
    let hosted = outcomes
        .iter()
        .find(|o| o.repository == "payments-api")
        .expect("payments-api");
    let note = hosted.note.clone().expect("a failed run");
    assert!(note.contains("401"), "{note}");
    let run = newest_run(&w, "payments-api").await;
    assert_eq!(run.outcome, "failed");
    assert!(
        run.error.as_deref().is_some_and(|e| e.contains("401")),
        "{run:?}"
    );
    // The embedded repository of the same pass built.
    let embedded = outcomes
        .iter()
        .find(|o| o.repository == "platform-infra")
        .expect("platform-infra");
    assert!(embedded.report.is_some(), "{embedded:?}");
}

// ===========================================================================
// KAIROS-T-0495: a 429 waits, a missing summary does not fail the build, and
// one cap of requests for each endpoint
// ===========================================================================

/// A back-off that records its waits and does not sleep.
fn recorded_backoff() -> (
    kairos_index::rate::Backoff,
    Arc<Mutex<Vec<std::time::Duration>>>,
) {
    let waits = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&waits);
    let backoff = kairos_index::rate::Backoff {
        sleep: Arc::new(move |d, _: &dyn Fn()| seen.lock().unwrap().push(d)),
        ..Default::default()
    };
    (backoff, waits)
}

#[tokio::test]
async fn a_429_with_and_without_retry_after_waits_and_the_build_ends_ok() {
    // Given an endpoint that answers 429 with Retry-After 3, then 429 with
    // none, then a summary, one request at a time
    let w = World::new("kairos_code_index_t0495_backoff").await;
    let chat =
        common::chat_http::serve_refusing("ollama_t0495_key", vec![Some("3"), None], false).await;
    w.svc
        .put_code_index_settings(&hosted_settings(&chat.base_url, "ollama_t0495_key", 1))
        .await
        .expect("the provider");
    opt_in(&w, "payments-api").await;
    let (backoff, waits) = recorded_backoff();
    w.service.set_summary_backoff(backoff);

    // When the first build runs
    let outcomes = w.first_builds().await;
    let hosted = &outcomes[0];
    let report = hosted
        .report
        .as_ref()
        .unwrap_or_else(|| panic!("{hosted:?}"));

    // Then it waited 3 s (the Retry-After) and 4 s (the 2nd refusal), and
    // each summary was made
    assert_eq!(
        *waits.lock().unwrap(),
        [3, 4].map(std::time::Duration::from_secs)
    );
    assert_eq!(chat.requests(), report.summary.calls.len() + 2);
    let run = newest_run(&w, "payments-api").await;
    assert_eq!(run.outcome, "ok", "{run:?}");
    assert_eq!(run.summaries_missing, Some(0));
    assert!(run.summaries_made.unwrap_or(0) > 0, "{run:?}");
}

#[tokio::test]
async fn an_endpoint_that_always_answers_429_gives_an_ok_build_with_missing_summaries() {
    // Given an endpoint that answers 429 to each request
    let w = World::new("kairos_code_index_t0495_always").await;
    let chat = common::chat_http::serve_refusing("ollama_t0495_key", Vec::new(), true).await;
    w.svc
        .put_code_index_settings(&hosted_settings(&chat.base_url, "ollama_t0495_key", 2))
        .await
        .expect("the provider");
    opt_in(&w, "payments-api").await;

    // When the first build runs
    let outcomes = w.first_builds().await;
    let hosted = &outcomes[0];
    let report = hosted
        .report
        .as_ref()
        .unwrap_or_else(|| panic!("{hosted:?}"));

    // Then the build ends ok with the structure and no summary, and the run
    // says how many are missing; each request was tried 6 times
    assert!(report.summary.calls.is_empty(), "{report:?}");
    let missing = report.summary.missing();
    assert!(missing > 0, "{report:?}");
    let failed =
        report.summary.symbols.failed + report.summary.files.failed + report.summary.modules.failed;
    assert_eq!(chat.requests(), failed * 6);
    let run = newest_run(&w, "payments-api").await;
    assert_eq!(run.outcome, "ok", "{run:?}");
    assert_eq!(run.summaries_made, Some(0));
    assert_eq!(run.summaries_missing, Some(missing as i32));
    assert!(
        run.symbols.unwrap_or(0) > 0,
        "the structure is stored: {run:?}"
    );

    // When the endpoint answers again and a person asks for a build
    chat.stop_refusing();
    w.bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    let outcomes = w.requested_builds().await;
    assert!(outcomes[0].report.is_some(), "{outcomes:?}");

    // Then the next build makes the missing summaries
    let run = newest_run(&w, "payments-api").await;
    assert_eq!(run.outcome, "ok", "{run:?}");
    assert_eq!(run.summaries_missing, Some(0), "{run:?}");
    assert_eq!(run.summaries_made, Some(missing as i32), "{run:?}");
}

#[tokio::test]
async fn a_back_off_longer_than_the_lease_does_not_stall_the_build() {
    // Given an index of A, a push of B, and an endpoint whose first answer
    // is a 429 with Retry-After 4: longer than the lease of 3 s
    const DB: &str = "kairos_code_index_t0495_lease";
    let w = World::new(DB).await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    commit_extra(&w.git);
    let chat = common::chat_http::serve_refusing("ollama_t0495_key", vec![Some("4")], false).await;
    let mut config = kairos_index::hosted::HostedConfig::new(
        "ollama-cloud",
        &chat.base_url,
        "fake-model-lease",
        1,
    );
    config.api_key = Some("ollama_t0495_key".into());
    let url = with_database(&common::admin_database_url(), DB);
    let (worker, outcomes) = build_worker(
        kairos_server::blocking::BlockingTenantPool::new(&url, 4),
        &w.service,
        Arc::new(kairos_server::code_index::HostedSummarizers { config }),
        quick_jobs(),
    );

    // When the worker runs the update, which waits 4 s (the real sleep)
    kairos_server::code_index::enqueue_updates(&w.state.blocking, &w.service, &worker).await;
    worker.drain().await;

    // Then the wait reported progress: the build ended ok on its first try
    assert_eq!(reports(&outcomes), 1);
    let run = newest_run(&w, "payments-api").await;
    assert_eq!(run.outcome, "ok", "{run:?}");
    assert_eq!(run.summaries_missing, Some(0), "{run:?}");
}

#[tokio::test]
async fn two_builds_on_one_endpoint_share_its_cap() {
    // Given 2 repositories on one endpoint with a concurrency of 1 (the
    // worker runs the 2 builds at the same time)
    let w = World::new("kairos_code_index_t0495_cap").await;
    let chat = common::chat_http::serve("ollama_t0495_key", 0).await;
    w.svc
        .put_code_index_settings(&hosted_settings(&chat.base_url, "ollama_t0495_key", 1))
        .await
        .expect("the provider");
    let infra = w.work().join("platform-infra");
    std::fs::create_dir_all(&infra).expect("the folder of platform-infra");
    repository_at_a(&infra);
    w.bob
        .create_repository(&repository("platform-infra"))
        .await
        .expect("platform-infra");
    opt_in(&w, "payments-api").await;
    opt_in(&w, "platform-infra").await;

    // When both first builds run
    let outcomes = w.first_builds().await;
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
    assert!(outcomes.iter().all(|o| o.report.is_some()), "{outcomes:?}");

    // Then one request was in flight at a time, for both builds together
    assert_eq!(chat.max_in_flight(), 1);
}

#[tokio::test]
async fn an_embedded_repository_on_a_server_with_no_model_gets_a_failed_run() {
    let w = World::new("kairos_code_index_t0341_nomodel").await;
    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    let outcomes = first_builds(&w.state.blocking, &w.service, None, embedder).await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    let note = outcomes[0].note.clone().expect("a failed run");
    assert!(note.contains("no embedded model"), "{note}");
    let run = newest_run(&w, "payments-api").await;
    assert_eq!(run.outcome, "failed");
    assert!(
        run.error
            .as_deref()
            .is_some_and(|e| e.contains("hosted provider")),
        "{run:?}"
    );
}

// ===========================================================================
// KAIROS-T-0342: the Bedrock summarizer
// ===========================================================================

/// The settings of the tenant: Bedrock at the fake endpoint (the region is
/// the endpoint override of the tests).
fn bedrock_settings(base_url: &str, credentials: &str) -> PutCodeIndexSettings {
    PutCodeIndexSettings {
        summary: Some(PutSummaryProvider {
            provider: "bedrock".into(),
            base_url: None,
            model: Some("anthropic.claude-3-5-haiku-20241022-v1:0".into()),
            region: Some(base_url.into()),
            secret: Some(Secret::new(credentials)),
        }),
        vectors: Some(PutVectorProvider {
            provider: "embedded".into(),
            ..Default::default()
        }),
        concurrency: Some(3),
        default_summaries: None,
        version: None,
    }
}

#[tokio::test]
async fn a_bedrock_repository_is_summarized_through_the_converse_api() {
    // Given a tenant on Bedrock (the fake endpoint, a throttle on the first
    // request) and an opted-in repository
    let w = World::new("kairos_code_index_t0342_bedrock").await;
    let bedrock = common::bedrock_http::serve("AKIAT0342EXAMPLE", 1).await;
    w.svc
        .put_code_index_settings(&bedrock_settings(
            &bedrock.base_url,
            "AKIAT0342EXAMPLE:secretkey0342:sessiontoken0342",
        ))
        .await
        .expect("the provider");
    opt_in(&w, "payments-api").await;

    // When the first-build lane runs
    let outcomes = w.first_builds().await;
    let hosted = outcomes
        .iter()
        .find(|o| o.repository == "payments-api")
        .expect("payments-api");

    // Then the summaries come from Bedrock, signed, with the model id
    // encoded in the path, and the throttle was tried again
    let report = hosted
        .report
        .as_ref()
        .unwrap_or_else(|| panic!("{hosted:?}"));
    assert!(report.summary.symbols.summarized > 0, "{report:?}");
    assert!(
        report
            .summary
            .calls
            .iter()
            .all(|c| c.summary.starts_with("A Bedrock summary of")),
        "{:?}",
        report.summary.calls
    );
    let authorizations = bedrock.authorizations();
    assert_eq!(authorizations.len(), report.summary.calls.len() + 1);
    assert!(
        authorizations
            .iter()
            .all(|a| a.contains("/bedrock/aws4_request,")
                && a.contains("SignedHeaders=content-type;host;x-amz-date;x-amz-security-token,")),
        "{authorizations:?}"
    );
    assert!(
        bedrock
            .model_ids()
            .iter()
            .all(|id| id == "anthropic.claude-3-5-haiku-20241022-v1:0"),
        "{:?}",
        bedrock.model_ids()
    );
    assert_eq!(
        newest_run(&w, "payments-api").await.model.as_deref(),
        Some("bedrock/anthropic.claude-3-5-haiku-20241022-v1:0")
    );
}

#[tokio::test]
async fn bedrock_credentials_of_one_part_are_refused_before_a_build() {
    let w = World::new("kairos_code_index_t0342_credentials").await;
    let err = w
        .svc
        .put_code_index_settings(&bedrock_settings("https://127.0.0.1:1", "AKIA-only"))
        .await
        .expect_err("the credentials have 2 or 3 parts");
    match &err {
        Error::Validation { field, .. } => assert_eq!(field.as_deref(), Some("summary.secret")),
        other => panic!("{other}"),
    }
}

// ===========================================================================
// Scenario: An index of another schema version gets a full build
// (KAIROS-T-0347)
// ===========================================================================
#[tokio::test]
async fn an_index_of_another_schema_version_gets_a_full_build() {
    // Given Kairos holds an index of commit A, the head, with schema
    // version 5, as a server upgrade leaves behind. It goes into the store
    // directly: an upload refuses such a file.
    let mut w = World::new("kairos_code_index_t0347_schema").await;
    let a = w.a.clone();
    let mut old = w.index(&a);
    old[60..64].copy_from_slice(&5u32.to_be_bytes());
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &old).unwrap();
    let structure = encoder.finish().unwrap();
    kairos_db::code_indexes::put(
        &mut w.conn,
        &kairos_db::code_indexes::NewCodeIndex {
            repository_id: w.repo_id,
            commit_sha: a.clone(),
            ref_name: Some("main".into()),
            source: "upload",
            structure,
            structure_bytes: old.len() as i64,
            summary_keys: 0,
            vector_model: None,
            created_by: None,
            citations: Vec::new(),
        },
        &[],
        &[],
    )
    .expect("the old index is stored");

    // When the builder runs
    let outcomes = w.sweep().await;

    // Then the head gets a full build, in place of a failed update
    let built: Vec<_> = outcomes.iter().filter(|o| o.report.is_some()).collect();
    assert_eq!(built.len(), 1, "{outcomes:?}");
    assert_eq!(built[0].commit, a);
    assert_eq!(built[0].base, None, "a full build has no base");
    let index_a = w
        .alice
        .download_code_index("payments-api", &a)
        .await
        .expect("the index of A downloads");
    assert_eq!(content_of(&index_a), content_of(&w.index(&a)));

    // And the run row is an ok push run of commit A
    let runs = runs_of(&w).await;
    assert_eq!(runs[0].trigger, "push", "{runs:?}");
    assert_eq!(runs[0].outcome, "ok", "{runs:?}");
    assert_eq!(runs[0].commit.as_deref(), Some(a.as_str()));
    assert!(runs[0].files.is_some_and(|n| n > 0), "{runs:?}");

    // And the next pass has nothing to do
    let outcomes = w.sweep().await;
    assert!(outcomes.iter().all(|o| o.report.is_none()), "{outcomes:?}");
}

// ===========================================================================
// Scenario: An index of a commit that is not in the history gets a full
// build (KAIROS-T-0356)
// ===========================================================================
#[tokio::test]
async fn an_index_of_a_commit_not_in_the_history_gets_a_full_build() {
    // Given Kairos holds an index of a commit that is not an ancestor of
    // the head, as a rewritten history leaves behind
    let mut w = World::new("kairos_code_index_t0356_history").await;
    let a = w.a.clone();
    let gone = "0f37ee0a688e2ccea185b44530802349a3824a2a".to_string();
    let file = w.index(&a);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &file).unwrap();
    let structure = encoder.finish().unwrap();
    kairos_db::code_indexes::put(
        &mut w.conn,
        &kairos_db::code_indexes::NewCodeIndex {
            repository_id: w.repo_id,
            commit_sha: gone.clone(),
            ref_name: Some("main".into()),
            source: "upload",
            structure,
            structure_bytes: file.len() as i64,
            summary_keys: 0,
            vector_model: None,
            created_by: None,
            citations: Vec::new(),
        },
        &[],
        &[],
    )
    .expect("the index of the gone commit is stored");

    // When the builder runs
    let outcomes = w.sweep().await;

    // Then the head gets a full build, in place of a failed update
    let built: Vec<_> = outcomes.iter().filter(|o| o.report.is_some()).collect();
    assert_eq!(built.len(), 1, "{outcomes:?}");
    assert_eq!(built[0].commit, a);
    assert_eq!(built[0].base, None, "a full build has no base");

    // And the run row is an ok push run of the head
    let runs = runs_of(&w).await;
    assert_eq!(runs[0].trigger, "push", "{runs:?}");
    assert_eq!(runs[0].outcome, "ok", "{runs:?}");
    assert_eq!(runs[0].commit.as_deref(), Some(a.as_str()));

    // And the next pass has nothing to do
    let outcomes = w.sweep().await;
    assert!(outcomes.iter().all(|o| o.report.is_none()), "{outcomes:?}");
}

// ===========================================================================
// KAIROS-T-0334: the update lane serves the repository that has been stale
// longest first
// ===========================================================================

/// The fake summarizer, which calls `hook` before each summary.
#[derive(Clone)]
struct HookSummarizers {
    hook: Arc<dyn Fn(&kairos_index::SummaryRequest) + Send + Sync>,
}

struct HookSummarizer {
    inner: FakeSummarizer,
    hook: Arc<dyn Fn(&kairos_index::SummaryRequest) + Send + Sync>,
}

impl kairos_index::Summarizer for HookSummarizer {
    fn model(&self) -> String {
        self.inner.model()
    }

    fn summarize(&mut self, request: &kairos_index::SummaryRequest) -> Result<String, String> {
        (self.hook)(request);
        self.inner.summarize(request)
    }
}

impl SummarizerSource for HookSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(
            &mut dyn kairos_index::Summarizer,
        )
            -> Result<kairos_index::UpdateReport, kairos_index::IndexError>,
    ) -> Result<kairos_index::UpdateReport, kairos_index::IndexError> {
        job(&mut HookSummarizer {
            inner: FakeSummarizer::default(),
            hook: Arc::clone(&self.hook),
        })
    }

    fn model_name(&self) -> String {
        FakeSummarizer::default().model
    }
}

/// A second repository `ledger`, registered after `payments-api`, with an
/// index of its first commit, sent now. Its folder.
async fn add_ledger(w: &World) -> PathBuf {
    let dir = w.work().join("ledger");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    write(&dir, "ledger/__init__.py", "");
    write(
        &dir,
        "ledger/book.py",
        "def post(entries, entry):\n    return entries + [entry]\n\n\n\
         def balance(entries):\n    return sum(e[1] for e in entries)\n",
    );
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    w.bob
        .create_repository(&repository("ledger"))
        .await
        .expect("ledger");
    let bytes = index_of(&dir, &head, &w.files.path().join("ledger.db"));
    w.bob
        .upload_code_index("ledger", &head, Some("main"), bytes)
        .await
        .expect("the index of ledger");
    dir
}

/// Push a commit that adds one function in `folder` of the git repository
/// at `dir`. Its sha.
fn push_function(dir: &Path, folder: &str) -> String {
    let n = git(dir, &["rev-list", "--count", "HEAD"]);
    write(
        dir,
        &format!("{folder}/added_{n}.py"),
        &format!("def added_{n}(x):\n    return x * {n}\n"),
    );
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", &format!("added {n}")]);
    git(dir, &["rev-parse", "HEAD"])
}

/// The repositories that a pass built, in order.
fn built(outcomes: &[kairos_server::code_index::BuildOutcome]) -> Vec<&str> {
    outcomes
        .iter()
        .filter(|o| o.report.is_some())
        .map(|o| o.repository.as_str())
        .collect()
}

// Criterion 1: with 2 repositories stale, the one that has been stale
// longest is built first, even when the other gets a new push during the
// build.
#[tokio::test]
async fn the_repository_that_has_been_stale_longest_is_built_first() {
    // Given payments-api with the older index, and ledger, which comes first
    // in the list (by slug) and gets its push first
    let w = World::new("kairos_code_index_t0334_order").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    let ledger = add_ledger(&w).await;
    push_function(&ledger, "ledger");
    let b = commit_b(&w.git);

    // When the update lane runs, and ledger gets a push during the build
    // of payments-api
    let pushed = Arc::new(Mutex::new(None::<String>));
    let hook = {
        let (ledger, pushed) = (ledger.clone(), Arc::clone(&pushed));
        move |request: &kairos_index::SummaryRequest| {
            let mut pushed = pushed.lock().unwrap();
            if request.path.starts_with("pkg/") && pushed.is_none() {
                *pushed = Some(push_function(&ledger, "ledger"));
            }
        }
    };
    let outcomes = w
        .sweep_with(Arc::new(HookSummarizers {
            hook: Arc::new(hook),
        }))
        .await;

    // Then payments-api is built first, and ledger after it, at its new head
    assert_eq!(built(&outcomes), ["payments-api", "ledger"], "{outcomes:?}");
    assert_eq!(outcomes[0].commit, b);
    let pushed = pushed
        .lock()
        .unwrap()
        .clone()
        .expect("the push during the build");
    assert_eq!(outcomes[1].commit, pushed);
}

// Criterion 2: a repository that gets a push during each build of another
// repository is not built twice in a row while the other is stale. And
// KAIROS_CODE_INDEX_MIN_REBUILD_SECS keeps a repository that was updated a
// short time ago for a later pass.
#[tokio::test]
async fn a_busy_repository_is_not_built_twice_in_a_row() {
    // Given 2 repositories, each stale
    let w = World::new("kairos_code_index_t0334_busy").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    let ledger = add_ledger(&w).await;
    commit_b(&w.git);
    push_function(&ledger, "ledger");

    // When each build of one repository gets a push to the other
    let last = Arc::new(Mutex::new(String::new()));
    let hook = {
        let (payments, ledger, last) = (w.git.clone(), ledger.clone(), Arc::clone(&last));
        move |request: &kairos_index::SummaryRequest| {
            let folder = request
                .path
                .split('/')
                .next()
                .unwrap_or_default()
                .to_string();
            let mut last = last.lock().unwrap();
            if *last == folder {
                return;
            }
            *last = folder.clone();
            match folder.as_str() {
                "pkg" => push_function(&ledger, "ledger"),
                _ => push_function(&payments, "pkg"),
            };
        }
    };
    let summarizers: Arc<dyn SummarizerSource> = Arc::new(HookSummarizers {
        hook: Arc::new(hook),
    });
    // A pass fetches each repository when it adds the jobs (KAIROS-T-0374),
    // so a push during the pass waits for the next pass: up to 8 passes for
    // 6 builds.
    let mut sequence = Vec::new();
    for _ in 0..8 {
        let outcomes = w.sweep_with(Arc::clone(&summarizers)).await;
        sequence.extend(built(&outcomes).into_iter().map(str::to_string));
        if sequence.len() >= 6 {
            break;
        }
    }

    // Then the builds alternate: no repository gets 2 builds in a row
    println!("builds: {sequence:?}");
    assert_eq!(sequence.len(), 6, "{sequence:?}");
    assert!(
        sequence.windows(2).all(|pair| pair[0] != pair[1]),
        "{sequence:?}"
    );

    // And with a least time of 1 hour between 2 updates, the 2 stale
    // repositories wait, and the notes say why
    push_function(&ledger, "ledger");
    w.service.set_min_rebuild_secs(3600);
    let outcomes = w.sweep_with(Arc::clone(&summarizers)).await;
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
    for outcome in &outcomes {
        let note = outcome.note.as_deref().unwrap_or_default();
        assert!(outcome.report.is_none(), "{outcome:?}");
        assert!(note.starts_with(MIN_REBUILD), "{note}");
        assert!(
            note.contains("KAIROS_CODE_INDEX_MIN_REBUILD_SECS is 3600"),
            "{note}"
        );
    }
    w.service.set_min_rebuild_secs(0);
    let outcomes = w.sweep_with(summarizers).await;
    assert_eq!(built(&outcomes).len(), 2, "{outcomes:?}");
}

// ===========================================================================
// KAIROS-T-0335: an update keeps the summaries that it has made
// ===========================================================================

/// The fake summarizer, which records the key of each summary and stops
/// the build (a panic of its thread, as when the server stops) at the call
/// after `limit` summaries.
#[derive(Clone)]
struct StoppingSummarizers {
    keys: Arc<Mutex<Vec<String>>>,
    limit: usize,
}

struct StoppingSummarizer {
    inner: FakeSummarizer,
    keys: Arc<Mutex<Vec<String>>>,
    limit: usize,
}

impl kairos_index::Summarizer for StoppingSummarizer {
    fn model(&self) -> String {
        self.inner.model()
    }

    fn summarize(&mut self, request: &kairos_index::SummaryRequest) -> Result<String, String> {
        let mut keys = self.keys.lock().unwrap();
        if keys.len() == self.limit {
            drop(keys);
            panic!("the server stops during the update");
        }
        keys.push(request.key.clone());
        self.inner.summarize(request)
    }
}

impl SummarizerSource for StoppingSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(
            &mut dyn kairos_index::Summarizer,
        )
            -> Result<kairos_index::UpdateReport, kairos_index::IndexError>,
    ) -> Result<kairos_index::UpdateReport, kairos_index::IndexError> {
        job(&mut StoppingSummarizer {
            inner: FakeSummarizer::default(),
            keys: Arc::clone(&self.keys),
            limit: self.limit,
        })
    }

    fn model_name(&self) -> String {
        FakeSummarizer::default().model
    }
}

/// A commit of payments-api that adds a file of 6 functions.
fn commit_extra(dir: &Path) -> String {
    let code: String = (0..6)
        .map(|i| format!("def extra_{i}(x):\n    return x - {i}\n\n\n"))
        .collect();
    write(dir, "pkg/extra.py", &code);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "extra"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// The keys of the summary pool of payments-api in Kairos.
fn pool_keys(w: &mut World) -> BTreeSet<String> {
    use kairos_db::schema::code_index_summaries::dsl;
    dsl::code_index_summaries
        .filter(dsl::repository_id.eq(w.repo_id))
        .select(dsl::key)
        .load::<String>(&mut w.conn)
        .unwrap()
        .into_iter()
        .collect()
}

/// An update stopped after N summaries: the next update makes only the
/// summaries that were not made, and its run row counts the reused ones.
#[tokio::test]
async fn the_next_update_makes_only_the_summaries_that_a_stopped_update_did_not_make() {
    // Given an index of A, and a commit B with new code
    let w = World::new("kairos_code_index_t0335_resume").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    let b = commit_extra(&w.git);
    let new_keys: BTreeSet<String> = keys_in(&w.index(&b))
        .difference(&keys_in(&w.index(&a)))
        .cloned()
        .collect();
    assert!(new_keys.len() > 4, "{new_keys:?}");

    // When the server stops during the update of B, after 3 summaries
    const MADE: usize = 3;
    let made = Arc::new(Mutex::new(Vec::new()));
    let outcomes = w
        .sweep_with(Arc::new(StoppingSummarizers {
            keys: Arc::clone(&made),
            limit: MADE,
        }))
        .await;
    assert!(outcomes[0].report.is_none(), "{outcomes:?}");
    let made: BTreeSet<String> = made.lock().unwrap().iter().cloned().collect();
    assert_eq!(made.len(), MADE);
    let work = databases_below(&w.work().join("clones").join("updates"));
    assert_eq!(work.len(), 1, "the work database stays: {work:?}");

    // And the next update runs
    let outcomes = w.sweep().await;

    // Then it makes only the summaries that were not made
    let report = outcomes[0].report.as_ref().expect("the update of B");
    assert_eq!(outcomes[0].commit, b);
    let again: BTreeSet<String> = report.summary.calls.iter().map(|c| c.key.clone()).collect();
    assert!(again.is_disjoint(&made), "made again: {again:?}");
    let made_now = report.summary.symbols.summarized
        + report.summary.files.summarized
        + report.summary.modules.summarized;
    let reused =
        report.summary.symbols.reused + report.summary.files.reused + report.summary.modules.reused;
    println!(
        "{} new keys: {MADE} made before the stop, {made_now} made after it, {reused} reused",
        new_keys.len()
    );
    assert_eq!(made_now, new_keys.len() - MADE);
    let union: BTreeSet<String> = again.union(&made).cloned().collect();
    assert_eq!(union, new_keys);

    // And the run row counts the summaries that it made and reused
    let runs = runs_of(&w).await;
    assert_eq!(runs[0].outcome, "ok", "{runs:?}");
    assert_eq!(runs[0].summaries_made, Some(made_now as i32));
    assert_eq!(runs[0].summaries_reused, Some(reused as i32));
    assert!(reused >= MADE, "{reused}");
    assert_eq!(runs[1].outcome, "failed", "the stopped run: {runs:?}");

    // And the index of B is the index that a checkout of B builds, and the
    // work database is removed
    let built = w
        .alice
        .download_code_index("payments-api", &b)
        .await
        .expect("the index of B downloads");
    assert_eq!(content_of(&built), content_of(&w.index(&b)));
    let work = databases_below(&w.work().join("clones").join("updates"));
    assert!(work.is_empty(), "{work:?}");
}

/// A summary of an update that did not end does not appear in an index
/// that does not need its key.
#[tokio::test]
async fn a_summary_of_a_stopped_update_stays_out_of_an_index_that_does_not_need_it() {
    // Given an update of B that stopped after 3 summaries
    let mut w = World::new("kairos_code_index_t0335_unused").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    commit_extra(&w.git);
    let made = Arc::new(Mutex::new(Vec::new()));
    w.sweep_with(Arc::new(StoppingSummarizers {
        keys: Arc::clone(&made),
        limit: 3,
    }))
    .await;
    let made: BTreeSet<String> = made.lock().unwrap().iter().cloned().collect();
    assert_eq!(made.len(), 3);

    // When the code of B goes away before the next update, and the head
    // is C
    git(&w.git, &["rm", "-q", "pkg/extra.py"]);
    git(&w.git, &["commit", "-q", "-m", "no extra"]);
    let c = commit_c(&w.git);
    let outcomes = w.sweep().await;
    assert!(outcomes[0].report.is_some(), "{outcomes:?}");
    assert_eq!(outcomes[0].commit, c);

    // Then the index of C does not have them
    let index_c = w
        .alice
        .download_code_index("payments-api", &c)
        .await
        .expect("the index of C downloads");
    assert!(keys_in(&index_c).is_disjoint(&made));
    assert_eq!(content_of(&index_c), content_of(&w.index(&c)));

    // And the pool keeps them as rows that no index used (KAIROS-T-0378),
    // until the retention pass removes them; the rows of the indexes stay
    let unused = unused_pool_keys(&mut w);
    assert_eq!(unused, made, "the made summaries are in the pool, unused");
    let used: BTreeSet<String> = pool_keys(&mut w).difference(&made).cloned().collect();
    assert!(keys_in(&index_c).is_subset(&used));
    let config = kairos_core::retention::RetentionConfig {
        code_index_pool_days: 30,
        ..Default::default()
    };
    let later = chrono::Utc::now() + chrono::Duration::days(29);
    diesel::sql_query("SET search_path TO public")
        .execute(&mut w.conn)
        .unwrap();
    let kept = kairos_db::retention::delete_unused_summaries(&mut w.conn, &config, later).unwrap();
    assert_eq!(kept, 0, "a summary younger than the setting stays");
    let later = chrono::Utc::now() + chrono::Duration::days(31);
    let deleted =
        kairos_db::retention::delete_unused_summaries(&mut w.conn, &config, later).unwrap();
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut w.conn)
        .unwrap();
    assert_eq!(deleted as usize, made.len());
    assert_eq!(pool_keys(&mut w), used, "the rows of the indexes stay");
    let again = w
        .alice
        .download_code_index("payments-api", &c)
        .await
        .expect("the index of C downloads");
    assert_eq!(content_of(&again), content_of(&w.index(&c)));
}

/// The keys of the pool of payments-api that no stored index used
/// (KAIROS-T-0378).
fn unused_pool_keys(w: &mut World) -> BTreeSet<String> {
    use kairos_db::schema::code_index_summaries::dsl;
    dsl::code_index_summaries
        .filter(dsl::repository_id.eq(w.repo_id))
        .filter(dsl::last_used_at.is_null())
        .select(dsl::key)
        .load::<String>(&mut w.conn)
        .unwrap()
        .into_iter()
        .collect()
}

// ===========================================================================
// KAIROS-T-0374: the builds run as jobs; 2 servers share them
// ===========================================================================

/// Short job times, so that a lease runs out in a few seconds. Not 1 s: a
/// build reports no progress while it fetches and writes its run row, and
/// those steps can take most of a second on a loaded machine
/// (KAIROS-T-0378).
fn quick_jobs() -> kairos_server::jobs::JobSettings {
    kairos_server::jobs::JobSettings {
        heartbeat: std::time::Duration::from_millis(250),
        lease: std::time::Duration::from_secs(3),
        poll: std::time::Duration::from_millis(100),
        retry_after: std::time::Duration::ZERO,
        ..Default::default()
    }
}

/// A second server of the world: a service with the clones in `dir`, and
/// its own start id.
fn second_service(w: &World, dir: &Path) -> Arc<CodeIndexService> {
    let root = w.work().to_path_buf();
    Arc::new(
        CodeIndexService::new(
            dir.to_path_buf(),
            Arc::new(move |repo: &kairos_db::models::repositories::Repository| {
                root.join(&repo.slug).display().to_string()
            }),
        )
        .with_secrets_key(w.service.secrets_key().cloned()),
    )
}

/// A job worker of the server of `service` that runs the builds with
/// `summarizers`, and the outcomes of its builds.
fn build_worker(
    blocking: kairos_server::blocking::BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    summarizers: Arc<dyn SummarizerSource>,
    settings: kairos_server::jobs::JobSettings,
) -> (
    Arc<kairos_server::jobs::Worker>,
    Arc<Mutex<Vec<kairos_server::code_index::BuildOutcome>>>,
) {
    let worker = Arc::new(kairos_server::jobs::Worker::new(
        blocking.clone(),
        service.instance(),
        settings,
    ));
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    kairos_server::code_index::BuildHandler::new(
        blocking,
        Arc::clone(service),
        Some(summarizers),
        embedder,
    )
    .with_outcomes(Arc::clone(&outcomes))
    .register(&worker);
    (worker, outcomes)
}

fn reports(outcomes: &Mutex<Vec<kairos_server::code_index::BuildOutcome>>) -> usize {
    outcomes
        .lock()
        .unwrap()
        .iter()
        .filter(|o| o.report.is_some())
        .count()
}

fn scratch_conn(db: &str) -> PgConnection {
    let mut conn =
        PgConnection::establish(&with_database(&common::admin_database_url(), db)).unwrap();
    diesel::sql_query("SET search_path TO org_acme, public")
        .execute(&mut conn)
        .unwrap();
    conn
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_servers_build_a_pushed_repository_once() {
    // Given an index of commit A, and commit B pushed
    const DB: &str = "kairos_code_index_t0374_two";
    let w = World::new(DB).await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    let b = commit_b(&w.git);

    // And 2 servers, each with its job worker
    let other = second_service(&w, &w.work().join("clones-2"));
    assert_ne!(other.instance(), w.service.instance());
    let fake: Arc<dyn SummarizerSource> = Arc::new(FakeSummarizers);
    let (one, one_out) = build_worker(
        w.state.blocking.clone(),
        &w.service,
        Arc::clone(&fake),
        quick_jobs(),
    );
    let (two, two_out) = build_worker(
        kairos_server::blocking::BlockingTenantPool::new(
            &with_database(&common::admin_database_url(), DB),
            4,
        ),
        &other,
        fake,
        quick_jobs(),
    );
    let loops = [
        tokio::spawn(Arc::clone(&one).run()),
        tokio::spawn(Arc::clone(&two).run()),
    ];

    // When the sweep of each server adds its jobs at the same time, twice
    for _ in 0..2 {
        tokio::join!(
            enqueue_updates_of(&w, &w.service, &one),
            enqueue_updates_of(&w, &other, &two),
        );
    }

    // Then the index of B is built once, by one server
    let key = job_key("acme", w.repo_id);
    wait_for(
        std::time::Duration::from_secs(60),
        "the end of the job",
        || {
            let key = key.clone();
            async move {
                kairos_db::jobs::open_by_key(&mut scratch_conn(DB), &key)
                    .unwrap()
                    .is_none()
            }
        },
    )
    .await;
    for l in loops {
        l.abort();
    }
    assert_eq!(
        reports(&one_out) + reports(&two_out),
        1,
        "one build: {:?} {:?}",
        one_out.lock().unwrap(),
        two_out.lock().unwrap()
    );
    let runs = runs_of(&w).await;
    let pushes: Vec<_> = runs.iter().filter(|r| r.trigger == "push").collect();
    assert_eq!(pushes.len(), 1, "{runs:?}");
    assert_eq!(pushes[0].outcome, "ok");
    assert_eq!(pushes[0].commit.as_deref(), Some(b.as_str()));
    let jobs = kairos_db::jobs::list(&mut scratch_conn(DB), "embedded_build").unwrap();
    assert_eq!(
        jobs.len(),
        1,
        "the second sweep and the other server added no job"
    );
    assert_eq!(jobs[0].state, "done");
    assert_eq!(jobs[0].tries, 1);
}

async fn enqueue_updates_of(
    w: &World,
    service: &Arc<CodeIndexService>,
    worker: &kairos_server::jobs::Worker,
) {
    kairos_server::code_index::enqueue_updates(&w.state.blocking, service, worker).await;
}

// KAIROS-T-0450: the sweep of a server reads the repositories with no
// index, then adds a first build for each. When the first build of another
// server ends between the read and the add, the dedupe key no longer
// blocks: the add must see the index.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_first_build_is_not_added_again_after_the_first_job_ends() {
    // Given a repository with no index, and 2 servers
    const DB: &str = "kairos_code_index_t0450_first";
    let w = World::new(DB).await;
    let other = second_service(&w, &w.work().join("clones-2"));
    let fake: Arc<dyn SummarizerSource> = Arc::new(FakeSummarizers);
    let (one, one_out) = build_worker(
        w.state.blocking.clone(),
        &w.service,
        Arc::clone(&fake),
        quick_jobs(),
    );
    let (two, _) = build_worker(w.state.blocking.clone(), &other, fake, quick_jobs());

    // And the sweep of server two read the list before the first build
    // ended: the repository has no index
    let listed = {
        let mut conn = scratch_conn(DB);
        assert_eq!(
            kairos_db::code_indexes::unindexed_repositories(&mut conn).unwrap(),
            vec![w.repo_id]
        );
        kairos_db::repositories::load(&mut conn, w.repo_id).unwrap()
    };

    // And server one added the first build
    let notes =
        kairos_server::code_index::enqueue_first_builds(&w.state.blocking, &w.service, &one).await;
    assert!(notes.is_empty(), "{notes:?}");

    // When server two adds its first build while the job is open
    let note = kairos_server::code_index::enqueue_first_build(
        &w.state.blocking,
        &other,
        &two,
        "acme",
        &listed,
    )
    .await
    .and_then(|n| n.note)
    .unwrap_or_default();

    // Then it adds none
    assert!(note.starts_with(ACTIVE_RUN), "{note}");

    // When the first build ends on server one
    let key = job_key("acme", w.repo_id);
    let worker = tokio::spawn(Arc::clone(&one).run());
    wait_for(
        std::time::Duration::from_secs(60),
        "the end of the first build",
        || {
            let key = key.clone();
            async move {
                kairos_db::jobs::open_by_key(&mut scratch_conn(DB), &key)
                    .unwrap()
                    .is_none()
            }
        },
    )
    .await;
    worker.abort();
    assert_eq!(reports(&one_out), 1, "{:?}", one_out.lock().unwrap());

    // And server two adds its first build from the list it read before
    let note = kairos_server::code_index::enqueue_first_build(
        &w.state.blocking,
        &other,
        &two,
        "acme",
        &listed,
    )
    .await
    .and_then(|n| n.note)
    .unwrap_or_default();

    // Then it adds none: the repository has an index
    assert_eq!(note, kairos_server::code_index::HAS_INDEX);
    let jobs = kairos_db::jobs::list(&mut scratch_conn(DB), "embedded_build").unwrap();
    assert_eq!(jobs.len(), 1, "one first build: {jobs:?}");
    assert_eq!(jobs[0].state, "done");
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].trigger, "first");
    assert_eq!(runs[0].outcome, "ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_requested_rebuild_is_not_started_twice() {
    // Given a repository with no index, and a request
    const DB: &str = "kairos_code_index_t0374_request";
    let w = World::new(DB).await;
    let run = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");

    // And 2 servers, whose first-build passes and second requests come
    // while the request waits
    let other = second_service(&w, &w.work().join("clones-2"));
    let fake: Arc<dyn SummarizerSource> = Arc::new(FakeSummarizers);
    let (one, one_out) = build_worker(
        w.state.blocking.clone(),
        &w.service,
        Arc::clone(&fake),
        quick_jobs(),
    );
    let (two, two_out) = build_worker(w.state.blocking.clone(), &other, fake, quick_jobs());
    for (service, worker) in [(&w.service, &one), (&other, &two)] {
        let notes =
            kairos_server::code_index::enqueue_first_builds(&w.state.blocking, service, worker)
                .await;
        let note = notes[0].note.clone().unwrap_or_default();
        assert!(note.starts_with(ACTIVE_RUN), "{note}");
        assert!(note.contains(&run.id), "the note names the run: {note}");
    }
    let err = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect_err("one run at a time");
    assert!(matches!(err, Error::Conflict { .. }), "{err}");

    // When both workers run
    let loops = [
        tokio::spawn(Arc::clone(&one).run()),
        tokio::spawn(Arc::clone(&two).run()),
    ];
    wait_for(
        std::time::Duration::from_secs(60),
        "the end of the run",
        || async { runs_of(&w).await[0].outcome != "running" },
    )
    .await;
    for l in loops {
        l.abort();
    }

    // Then the request was built once, and its run ended ok
    assert_eq!(reports(&one_out) + reports(&two_out), 1);
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].id, run.id);
    assert_eq!(runs[0].outcome, "ok");
    assert_eq!(runs[0].trigger, "request");
    let jobs = kairos_db::jobs::list(&mut scratch_conn(DB), "embedded_build").unwrap();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0].state, "done");
    assert_eq!(jobs[0].tries, 1);
}

/// The fake summarizer, which records the key of each summary, and blocks
/// for ever at the call after `limit` summaries: the server is frozen.
#[derive(Clone)]
struct FrozenSummarizers {
    keys: Arc<Mutex<Vec<String>>>,
    limit: usize,
}

struct FrozenSummarizer {
    inner: FakeSummarizer,
    keys: Arc<Mutex<Vec<String>>>,
    limit: usize,
}

impl kairos_index::Summarizer for FrozenSummarizer {
    fn model(&self) -> String {
        self.inner.model()
    }

    fn summarize(&mut self, request: &kairos_index::SummaryRequest) -> Result<String, String> {
        let mut keys = self.keys.lock().unwrap();
        if keys.len() == self.limit {
            drop(keys);
            loop {
                std::thread::park();
            }
        }
        keys.push(request.key.clone());
        self.inner.summarize(request)
    }
}

impl SummarizerSource for FrozenSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(
            &mut dyn kairos_index::Summarizer,
        )
            -> Result<kairos_index::UpdateReport, kairos_index::IndexError>,
    ) -> Result<kairos_index::UpdateReport, kairos_index::IndexError> {
        job(&mut FrozenSummarizer {
            inner: FakeSummarizer::default(),
            keys: Arc::clone(&self.keys),
            limit: self.limit,
        })
    }

    fn model_name(&self) -> String {
        FakeSummarizer::default().model
    }
}

// KAIROS-T-0399 (GitHub #34): a build whose server stopped is not taken
// again; the breaker of its repository trips. A request resets it, and the
// requested build keeps the summaries of the stopped server (KAIROS-T-0378).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_killed_servers_build_trips_the_breaker_and_a_request_ends_ok_with_its_summaries() {
    // Given a repository with no index, whose first build starts on a
    // server that stops after 3 summaries
    const DB: &str = "kairos_code_index_t0374_killed";
    const MADE: usize = 3;
    let w = World::new(DB).await;
    let url = with_database(&common::admin_database_url(), DB);
    let frozen = FrozenSummarizers {
        keys: Arc::new(Mutex::new(Vec::new())),
        limit: MADE,
    };
    let dead = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let dead_service = second_service(&w, &w.work().join("clones"));
    let (dead_worker, _) = build_worker(
        kairos_server::blocking::BlockingTenantPool::new(&url, 4),
        &dead_service,
        Arc::new(frozen.clone()),
        quick_jobs(),
    );
    {
        let (blocking, service, worker) = (
            w.state.blocking.clone(),
            Arc::clone(&dead_service),
            Arc::clone(&dead_worker),
        );
        dead.spawn(async move {
            kairos_server::code_index::enqueue_first_builds(&blocking, &service, &worker).await;
            worker.run().await;
        });
    }
    wait_for(
        std::time::Duration::from_secs(60),
        "3 summaries",
        || async { frozen.keys.lock().unwrap().len() == MADE },
    )
    .await;
    // The server stops: no heartbeat, no end of its job.
    dead.shutdown_background();
    let made: BTreeSet<String> = frozen.keys.lock().unwrap().iter().cloned().collect();
    let key = job_key("acme", w.repo_id);
    let leased = kairos_db::jobs::open_by_key(&mut scratch_conn(DB), &key)
        .unwrap()
        .expect("the job of the stopped server");
    assert_eq!(leased.leased_by, Some(dead_service.instance()));

    // When another server's worker runs, after the lease ran out
    let keys = Arc::new(Mutex::new(Vec::new()));
    let alive = StoppingSummarizers {
        keys: Arc::clone(&keys),
        limit: usize::MAX,
    };
    let (worker, outcomes) = build_worker(
        w.state.blocking.clone(),
        &w.service,
        Arc::new(alive),
        quick_jobs(),
    );
    let jobs = tokio::spawn(Arc::clone(&worker).run());
    wait_for_breaker(&w, DB, &key).await;

    // Then the job is not taken again: it failed on its first try, and the
    // breaker of the repository tripped with the reason
    let job = kairos_db::jobs::get(&mut scratch_conn(DB), leased.id)
        .unwrap()
        .unwrap();
    assert_eq!(job.state, "failed", "{job:?}");
    assert_eq!(job.tries, 1);
    let reason = job.last_error.clone().unwrap();
    assert!(
        reason.contains("The lease ran out on try 1 of 3"),
        "{reason}"
    );
    assert_eq!(reports(&outcomes), 0, "{:?}", outcomes.lock().unwrap());
    assert!(keys.lock().unwrap().is_empty(), "no summary made again");
    let breaker = w
        .bob
        .get_repository("payments-api")
        .await
        .unwrap()
        .repository
        .code_index_breaker;
    assert!(breaker.tripped, "{breaker:?}");
    assert_eq!(breaker.reason.as_deref(), Some(reason.as_str()));
    assert!(breaker.tripped_at.is_some());
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].trigger, "first");
    assert_eq!(runs[0].outcome, "failed", "{runs:?}");

    // And the sweep adds no first build while it is tripped
    let notes =
        kairos_server::code_index::enqueue_first_builds(&w.state.blocking, &w.service, &worker)
            .await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0]
            .note
            .as_deref()
            .is_some_and(|n| n.starts_with(BREAKER_TRIPPED)),
        "{notes:?}"
    );
    assert!(
        kairos_db::jobs::open_by_key(&mut scratch_conn(DB), &key)
            .unwrap()
            .is_none()
    );

    // When bob asks for a build
    let requested = w
        .bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    assert!(
        !w.bob
            .get_repository("payments-api")
            .await
            .unwrap()
            .repository
            .code_index_breaker
            .tripped,
        "the request resets the breaker"
    );
    worker.wake();
    wait_for(
        std::time::Duration::from_secs(60),
        "the end of the requested build",
        || {
            let key = key.clone();
            async move {
                kairos_db::jobs::open_by_key(&mut scratch_conn(DB), &key)
                    .unwrap()
                    .is_none()
            }
        },
    )
    .await;
    jobs.abort();

    // Then the requested build ends ok, with the summaries of the stopped
    // server kept
    assert_eq!(reports(&outcomes), 1, "{:?}", outcomes.lock().unwrap());
    let again: BTreeSet<String> = keys.lock().unwrap().iter().cloned().collect();
    assert!(again.is_disjoint(&made), "made again: {again:?}");
    let runs = runs_of(&w).await;
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs[0].id, requested.id);
    assert_eq!(runs[0].outcome, "ok", "{runs:?}");
    assert!(
        runs[0].summaries_reused.is_some_and(|n| n as usize >= MADE),
        "{runs:?}"
    );
    let row = kairos_db::code_index_builds::get(&mut scratch_conn(DB), runs[0].id.parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(row.server_instance, Some(w.service.instance()));
    let built = w
        .alice
        .download_code_index("payments-api", &w.a)
        .await
        .expect("the index of A");
    assert_eq!(content_of(&built), content_of(&w.index(&w.a)));
}

/// Wait until the build job with `key` is not open, its run has ended, and
/// the breaker of payments-api has tripped (KAIROS-T-0399).
async fn wait_for_breaker(w: &World, db: &str, key: &str) {
    let repo = w.repo_id;
    wait_for(
        std::time::Duration::from_secs(60),
        "the breaker trips and the run ends",
        || {
            let (key, db) = (key.to_string(), db.to_string());
            async move {
                let mut conn = scratch_conn(&db);
                kairos_db::jobs::open_by_key(&mut conn, &key)
                    .unwrap()
                    .is_none()
                    && kairos_db::code_index_breakers::get(&mut conn, repo)
                        .unwrap()
                        .is_some()
                    && kairos_db::code_index_builds::running(&mut conn, repo)
                        .unwrap()
                        .is_none()
            }
        },
    )
    .await;
}

// ===========================================================================
// KAIROS-T-0378: the summaries are kept in Postgres as they are made, so a
// build that stops on one server finishes on another server that has an
// empty KAIROS_CODE_INDEX_DIR
// ===========================================================================

/// The rows of the pool of payments-api with these keys that a stored index
/// used.
fn used_pool_keys(w: &mut World) -> BTreeSet<String> {
    use kairos_db::schema::code_index_summaries::dsl;
    dsl::code_index_summaries
        .filter(dsl::repository_id.eq(w.repo_id))
        .filter(dsl::last_used_at.is_not_null())
        .select(dsl::key)
        .load::<String>(&mut w.conn)
        .unwrap()
        .into_iter()
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_build_stopped_on_one_server_finishes_on_another_with_an_empty_folder() {
    // Given a repository with no index, whose first build starts on server A
    // and stops after 3 summaries: the server is killed
    const DB: &str = "kairos_code_index_t0378_empty";
    const MADE: usize = 3;
    let mut w = World::new(DB).await;
    let url = with_database(&common::admin_database_url(), DB);
    let frozen = FrozenSummarizers {
        keys: Arc::new(Mutex::new(Vec::new())),
        limit: MADE,
    };
    let dead = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let dead_service = second_service(&w, &w.work().join("server-a"));
    let (dead_worker, _) = build_worker(
        kairos_server::blocking::BlockingTenantPool::new(&url, 4),
        &dead_service,
        Arc::new(frozen.clone()),
        quick_jobs(),
    );
    {
        let (blocking, service, worker) = (
            w.state.blocking.clone(),
            Arc::clone(&dead_service),
            Arc::clone(&dead_worker),
        );
        dead.spawn(async move {
            kairos_server::code_index::enqueue_first_builds(&blocking, &service, &worker).await;
            worker.run().await;
        });
    }
    wait_for(
        std::time::Duration::from_secs(60),
        "3 summaries",
        || async { frozen.keys.lock().unwrap().len() == MADE },
    )
    .await;
    dead.shutdown_background();
    let made: BTreeSet<String> = frozen.keys.lock().unwrap().iter().cloned().collect();

    // Then the pool in Postgres has the 3 summaries already, used by no
    // index
    wait_for(
        std::time::Duration::from_secs(10),
        "the 3 summaries in the pool",
        || {
            let made = made.clone();
            async move {
                let mut conn = scratch_conn(DB);
                use kairos_db::schema::code_index_summaries::dsl;
                let keys: BTreeSet<String> = dsl::code_index_summaries
                    .filter(dsl::last_used_at.is_null())
                    .select(dsl::key)
                    .load::<String>(&mut conn)
                    .unwrap()
                    .into_iter()
                    .collect();
                keys == made
            }
        },
    )
    .await;

    // When server B, with an empty folder of its own, runs after the lease
    // ran out: the job is not taken again, and the breaker trips
    // (KAIROS-T-0399). Then bob asks for a build, and server B runs it
    let folder_b = w.work().join("server-b");
    assert!(!folder_b.exists(), "server B has an empty folder");
    let server_b = second_service(&w, &folder_b);
    let keys = Arc::new(Mutex::new(Vec::new()));
    let (worker, outcomes) = build_worker(
        w.state.blocking.clone(),
        &server_b,
        Arc::new(StoppingSummarizers {
            keys: Arc::clone(&keys),
            limit: usize::MAX,
        }),
        quick_jobs(),
    );
    let jobs = tokio::spawn(Arc::clone(&worker).run());
    let key = job_key("acme", w.repo_id);
    wait_for_breaker(&w, DB, &key).await;
    assert!(keys.lock().unwrap().is_empty(), "no summary made again");
    w.bob
        .rebuild_code_index("payments-api")
        .await
        .expect("202 with the run");
    worker.wake();
    wait_for(
        std::time::Duration::from_secs(60),
        "the end of the requested build",
        || {
            let key = key.clone();
            async move {
                kairos_db::jobs::open_by_key(&mut scratch_conn(DB), &key)
                    .unwrap()
                    .is_none()
            }
        },
    )
    .await;
    jobs.abort();

    // Then the build ends ok on server B, and makes only the rest
    assert_eq!(reports(&outcomes), 1, "{:?}", outcomes.lock().unwrap());
    let all = keys_in(&w.index(&w.a));
    let again: BTreeSet<String> = keys.lock().unwrap().iter().cloned().collect();
    assert!(again.is_disjoint(&made), "made again: {again:?}");
    assert_eq!(again.len(), all.len() - MADE, "only the rest");
    let union: BTreeSet<String> = again.union(&made).cloned().collect();
    assert_eq!(union, all);
    let runs = runs_of(&w).await;
    assert_eq!(
        runs.len(),
        2,
        "the first build failed, the request: {runs:?}"
    );
    assert_eq!(runs[1].outcome, "failed", "{runs:?}");
    assert_eq!(runs[0].trigger, "request", "{runs:?}");
    assert_eq!(runs[0].outcome, "ok", "{runs:?}");
    assert_eq!(runs[0].summaries_made, Some((all.len() - MADE) as i32));
    let row = kairos_db::code_index_builds::get(&mut scratch_conn(DB), runs[0].id.parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(row.server_instance, Some(server_b.instance()));

    // And the index is the index of a checkout, and each summary of the pool
    // is used by it now
    let built = w
        .alice
        .download_code_index("payments-api", &w.a)
        .await
        .expect("the index of A");
    assert_eq!(content_of(&built), content_of(&w.index(&w.a)));
    assert_eq!(used_pool_keys(&mut w), all);
    assert!(unused_pool_keys(&mut w).is_empty());
}

#[tokio::test]
async fn an_update_stopped_on_one_server_finishes_on_another_with_an_empty_folder() {
    // Given an index of A, and a commit B with new code, whose update stops
    // after 3 summaries on the server of the world
    let mut w = World::new("kairos_code_index_t0378_update").await;
    let a = w.a.clone();
    w.bob
        .upload_code_index("payments-api", &a, Some("main"), w.index(&a))
        .await
        .expect("the index of A");
    let b = commit_extra(&w.git);
    let new_keys: BTreeSet<String> = keys_in(&w.index(&b))
        .difference(&keys_in(&w.index(&a)))
        .cloned()
        .collect();
    const MADE: usize = 3;
    let made = Arc::new(Mutex::new(Vec::new()));
    let outcomes = w
        .sweep_with(Arc::new(StoppingSummarizers {
            keys: Arc::clone(&made),
            limit: MADE,
        }))
        .await;
    assert!(outcomes[0].report.is_none(), "{outcomes:?}");
    let made: BTreeSet<String> = made.lock().unwrap().iter().cloned().collect();
    assert_eq!(
        unused_pool_keys(&mut w),
        made,
        "in Postgres as they were made"
    );

    // When the next update runs on another server, with an empty folder
    let other = second_service(&w, &w.work().join("server-b"));
    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicProvider::default());
    let outcomes = sweep(
        &w.state.blocking,
        &other,
        Some(Arc::new(FakeSummarizers)),
        embedder,
    )
    .await;

    // Then it makes only the summaries that were not made
    let report = outcomes[0].report.as_ref().expect("the update of B");
    assert_eq!(outcomes[0].commit, b);
    let again: BTreeSet<String> = report.summary.calls.iter().map(|c| c.key.clone()).collect();
    assert!(again.is_disjoint(&made), "made again: {again:?}");
    assert_eq!(again.len(), new_keys.len() - MADE);
    let union: BTreeSet<String> = again.union(&made).cloned().collect();
    assert_eq!(union, new_keys);
    let built = w
        .alice
        .download_code_index("payments-api", &b)
        .await
        .expect("the index of B downloads");
    assert_eq!(content_of(&built), content_of(&w.index(&b)));
    assert!(unused_pool_keys(&mut w).is_empty(), "each row is used now");
}

// ===========================================================================
// KAIROS-T-0430: the builder indexes the default branch of the forge, read
// at each fetch; a branch set by hand wins
// ===========================================================================

/// The outcome of `slug` in `outcomes`.
fn outcome_of<'a>(
    outcomes: &'a [kairos_server::code_index::BuildOutcome],
    slug: &str,
) -> &'a kairos_server::code_index::BuildOutcome {
    outcomes
        .iter()
        .find(|o| o.repository == slug)
        .unwrap_or_else(|| panic!("no outcome of {slug}: {outcomes:?}"))
}

async fn set_branch(
    w: &World,
    request: UpdateRepositoryRequest,
) -> kairos_client::types_repositories::Repository {
    w.bob
        .update_repository("ledger", &request)
        .await
        .expect("bob changes the branch of ledger")
}

// Scenario: a repository registered with no branch follows the HEAD of a
// bare repository; a branch set by hand wins; clearing it goes back to the
// forge (criteria 1, 2, 3, 4)
#[tokio::test]
async fn the_builder_follows_the_head_of_the_forge_and_a_branch_set_by_hand_wins() {
    let w = World::new("kairos_code_index_t0430_forge_branch").await;
    let a = w.a.clone();
    // Only ledger is built in this scenario.
    set_build(&w, CodeIndexBuild::Off).await;

    // Given a bare repository `ledger` with commit A on main, and HEAD on
    // main, registered with no branch
    let bare = w.work().join("ledger");
    git(
        w.work(),
        &["clone", "-q", "--bare", "payments-api", "ledger"],
    );
    let created = w
        .bob
        .create_repository(&repository("ledger"))
        .await
        .expect("ledger");
    assert_eq!(created.default_branch, "main");
    assert_eq!(created.default_branch_source, DefaultBranchSource::Forge);
    assert_eq!(created.forge_branch, None);

    // When the first build runs
    let outcomes = w.first_builds().await;

    // Then it indexes the head of the branch that HEAD names
    assert_eq!(outcome_of(&outcomes, "ledger").commit, a);
    let repo = w.bob.get_repository("ledger").await.unwrap().repository;
    assert_eq!(repo.default_branch, "main");
    assert_eq!(repo.default_branch_source, DefaultBranchSource::Forge);
    assert_eq!(repo.forge_branch.as_deref(), Some("main"));
    assert_eq!(repo.default_branch_summary(), "from the forge");

    // When the forge gets a new default branch `development` with commit B
    git(&w.git, &["checkout", "-q", "-b", "development"]);
    let b = commit_b(&w.git);
    git(
        &w.git,
        &["push", "-q", &bare.display().to_string(), "development"],
    );
    git(&bare, &["symbolic-ref", "HEAD", "refs/heads/development"]);
    let outcomes = w.sweep().await;

    // Then the next fetch picks it up, and the index of B is on development
    let outcome = outcome_of(&outcomes, "ledger");
    assert_eq!(outcome.commit, b, "{outcome:?}");
    assert_eq!(outcome.base.as_deref(), Some(a.as_str()), "{outcome:?}");
    let repo = w.bob.get_repository("ledger").await.unwrap().repository;
    assert_eq!(repo.default_branch, "development");
    assert_eq!(repo.default_branch_source, DefaultBranchSource::Forge);
    assert_eq!(repo.forge_branch.as_deref(), Some("development"));
    let listed = w.alice.list_code_indexes("ledger").await.unwrap();
    let index_b = listed
        .iter()
        .find(|i| i.commit == b)
        .expect("an index of B");
    assert_eq!(index_b.r#ref.as_deref(), Some("development"));

    // When a person sets main by hand
    let repo = set_branch(
        &w,
        UpdateRepositoryRequest {
            default_branch: Some("main".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(repo.default_branch, "main");
    assert_eq!(repo.default_branch_source, DefaultBranchSource::Hand);
    assert_eq!(
        repo.default_branch_summary(),
        "set by hand; the forge gives development"
    );

    // Then the builder indexes main, whatever the forge says
    let outcomes = w.sweep().await;
    let outcome = outcome_of(&outcomes, "ledger");
    assert_eq!(outcome.commit, a, "{outcome:?}");
    assert_eq!(outcome.note.as_deref(), Some("the head has an index"));
    let repo = w.bob.get_repository("ledger").await.unwrap().repository;
    assert_eq!(repo.default_branch, "main");
    assert_eq!(repo.default_branch_source, DefaultBranchSource::Hand);
    assert_eq!(repo.forge_branch.as_deref(), Some("development"));

    // And main set by hand again changes nothing
    let before = repo.updated_at.clone();
    let repo = set_branch(
        &w,
        UpdateRepositoryRequest {
            default_branch: Some("main".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(repo.updated_at, before);

    // And a branch with forge is refused, and the error names the field
    let refused = w
        .bob
        .update_repository(
            "ledger",
            &UpdateRepositoryRequest {
                default_branch: Some("main".into()),
                default_branch_source: Some(DefaultBranchSource::Forge),
                ..Default::default()
            },
        )
        .await
        .expect_err("a branch set by hand cannot follow the forge");
    assert!(
        matches!(&refused, Error::Validation { field: Some(f), .. } if f == "default_branch_source"),
        "{refused}"
    );

    // When the person clears the branch set by hand
    let repo = set_branch(
        &w,
        UpdateRepositoryRequest {
            default_branch_source: Some(DefaultBranchSource::Forge),
            ..Default::default()
        },
    )
    .await;

    // Then the repository follows the forge again, at once with the branch
    // of the last fetch
    assert_eq!(repo.default_branch, "development");
    assert_eq!(repo.default_branch_source, DefaultBranchSource::Forge);
    let outcomes = w.sweep().await;
    let outcome = outcome_of(&outcomes, "ledger");
    assert_eq!(outcome.commit, b, "{outcome:?}");
    assert_eq!(outcome.note.as_deref(), Some("the head has an index"));

    // And a repository registered with a branch has it by hand
    let mut pinned = repository("ledger-pinned");
    pinned.default_branch = Some("trunk".into());
    let pinned = w
        .bob
        .create_repository(&pinned)
        .await
        .expect("ledger-pinned");
    assert_eq!(pinned.default_branch, "trunk");
    assert_eq!(pinned.default_branch_source, DefaultBranchSource::Hand);
}

// KAIROS-T-0478: a language with no SCIP index is a warning on the run, and
// the list of the runs gives an organization admin the disk use of the
// builder.
#[tokio::test]
async fn a_run_records_its_scip_warnings_and_an_admin_sees_the_disk_use() {
    // Given payments-api, a Python repository, a builder that did not
    // prepare its tools (so the Python indexer is looked for on PATH, and
    // the test has none there), and a Go tool folder
    let w = World::new("kairos_code_index_t0478_warnings").await;
    std::fs::create_dir_all(w.service.dir().join("tools/go-0.0.0")).unwrap();
    std::fs::write(
        w.service
            .dir()
            .join("tools/go-0.0.0/.kairos-index-unpacked"),
        "x",
    )
    .unwrap();
    std::fs::write(w.service.dir().join("tools/go-0.0.0/go"), vec![0u8; 4096]).unwrap();

    // When the first-build lane runs
    let outcomes = w.first_builds().await;
    let report = outcomes
        .iter()
        .find(|o| o.repository == "payments-api")
        .and_then(|o| o.report.as_ref())
        .unwrap_or_else(|| panic!("{outcomes:?}"));

    // Then the run is ok and has the warning of Python, with its reason
    let python: Vec<_> = report
        .build
        .scip_warnings
        .iter()
        .filter(|w| w.language == "python")
        .collect();
    assert_eq!(python.len(), 1, "{:?}", report.build.scip_warnings);
    let run = newest_run(&w, "payments-api").await;
    assert_eq!(run.outcome, "ok");
    assert_eq!(run.scip_warnings.len(), 1, "{run:?}");
    assert_eq!(run.scip_warnings[0].language, "python");
    assert_eq!(run.scip_warnings[0].reason, python[0].reason);

    // And the organization admin sees the disk use of the tools by language
    let admin = w
        .svc
        .list_code_index_builds("payments-api", Some(1))
        .await
        .expect("the runs");
    let disk = admin.disk.expect("the disk use, for an admin");
    let go = disk
        .tools
        .iter()
        .find(|t| t.name == "go-0.0.0")
        .unwrap_or_else(|| panic!("{disk:?}"));
    assert_eq!(go.language.as_deref(), Some("go"));
    assert!(go.bytes >= 4096, "{go:?}");
    assert_eq!(disk.caches_bytes, 0);
    assert!(disk.caches_cap_bytes > 0);

    // And a member does not
    let member = w
        .bob
        .list_code_index_builds("payments-api", Some(1))
        .await
        .expect("the runs");
    assert!(member.disk.is_none());
    assert_eq!(member.items[0].scip_warnings, run.scip_warnings);
}
