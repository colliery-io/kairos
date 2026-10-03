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
use std::sync::Arc;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, recreate_scratch_db, spawn_server, user_token, with_database,
};
use kairos_client::Error;
use kairos_client::types_org::{AddTeamMemberRequest, CreateTeamRequest};
use kairos_client::types_repositories::CreateRepositoryRequest;
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_embed::{DeterministicProvider, EmbeddingProvider};
use kairos_index::{FakeSummarizer, Index, Level, UpdateOptions};
use kairos_server::app;
use kairos_server::code_index::{CodeIndexService, FakeSummarizers, sweep};
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
        let _admin_conn = recreate_scratch_db(scratch_db);
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
        let remote = match &git_http {
            Some(server) => format!("{}/payments-api/.git", server.base_url),
            None => git.display().to_string(),
        };
        let service = Arc::new(
            CodeIndexService::new(
                work.path().join("clones"),
                Arc::new(move |_: &kairos_db::models::repositories::Repository| remote.clone()),
            )
            .with_secrets_key(private.then(|| key.clone())),
        );

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
        config.secrets_key = private.then(|| key.clone());
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
            conn,
            state,
            service,
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
            Arc::new(FakeSummarizers),
            embedder,
        )
        .await
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
