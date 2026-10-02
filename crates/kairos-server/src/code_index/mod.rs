//! The base code index of each repository (COLLIERY-T-1853, COLLIERY-I-0264
//! "The flow" and "Who builds the base index").
//!
//! # Storage
//!
//! Postgres, in the tenant schema ([`kairos_db::code_indexes`]):
//! - the summary pool of a repository, one row for each summary key, shared
//!   by its commits;
//! - the structure of each indexed commit: the gzip of the SQLite index file
//!   with no summaries ([`kairos_index::store`]).
//!
//! A download assembles an index file from the structure and the pool rows
//! that it uses. About 2 MB of structure for each commit of Kairos.
//!
//! # The clone
//!
//! The nearest indexed commit below a commit, and the tree of a new commit,
//! come from a bare clone of the repository in `KAIROS_CODE_INDEX_DIR`
//! ([`git`]). The clone fetches from the `repo_url` of the repository, with
//! no credential.
//!
//! # The builder
//!
//! [`sweep`] fetches each repository that has an index. When the head of its
//! default branch has no index, it updates the index of the nearest indexed
//! commit below it to that head ([`kairos_index::update`] with a SCIP run),
//! and stores the result. [`run_builder`] runs a sweep on an interval, as the
//! embedding refresher does. The first index of a repository comes from an
//! upload: a full build of Kairos takes about 12 hours on a CPU.
//!
//! # The tools
//!
//! The image does not hold the tools of the builder (COLLIERY-T-2525). At
//! each start, [`prepare_tools`] checks them in `tools/` of
//! `KAIROS_CODE_INDEX_DIR`, and downloads the ones that are not there
//! ([`kairos_index::tools`]). Without `KAIROS_CODE_INDEX_DIR`, nothing is
//! downloaded.

pub mod git;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use kairos_db::models::repositories::Repository;
use kairos_index::tools::{ToolSet, Tools};
use kairos_index::{IndexError, Summarizer, UpdateOptions, UpdateReport};

use crate::blocking::BlockingTenantPool;
use crate::error::ApiError;

/// The most commits that a search for the nearest indexed commit reads
/// below the given commit.
pub const MAX_DISTANCE: usize = 1000;

/// The URL to fetch a repository from.
pub type RemoteOf = Arc<dyn Fn(&Repository) -> String + Send + Sync>;

/// The clones of the indexed repositories, and the work folders of the
/// builder.
pub struct CodeIndexService {
    dir: PathBuf,
    remote_of: RemoteOf,
    /// One lock for each clone: 2 fetches into one clone at the same time
    /// can fail.
    locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    /// The commits that a build could not index, so that each sweep does
    /// not try them again. Held here, not in the database: a restart tries
    /// them one more time.
    failed: Mutex<HashSet<(uuid::Uuid, String)>>,
    /// The SCIP options of a build: the tools of [`prepare_tools`], when
    /// they are ready.
    build: OnceLock<kairos_index::BuildOptions>,
}

impl std::fmt::Debug for CodeIndexService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeIndexService")
            .field("dir", &self.dir)
            .finish()
    }
}

impl CodeIndexService {
    /// A service with its clones in `dir`, which fetches each repository
    /// from the URL that `remote_of` gives.
    pub fn new(dir: PathBuf, remote_of: RemoteOf) -> Self {
        CodeIndexService {
            dir,
            remote_of,
            locks: Mutex::default(),
            failed: Mutex::default(),
            build: OnceLock::new(),
        }
    }

    /// The folder of the clones and of the tools.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Use `tools` for the SCIP run of each build. Only the first call
    /// counts.
    pub fn use_tools(&self, tools: &Tools) {
        let _ = self.build.set(tools.build_options());
    }

    /// The SCIP options of a build: the tools, else the pinned files at
    /// their default paths. Never a download.
    pub fn build_options(&self) -> kairos_index::BuildOptions {
        self.build
            .get()
            .cloned()
            .unwrap_or_else(|| kairos_index::BuildOptions {
                download: false,
                ..Default::default()
            })
    }

    /// A service that fetches each repository from its `repo_url`.
    pub fn from_repo_url(dir: PathBuf) -> Self {
        Self::new(dir, Arc::new(|repo: &Repository| repo.repo_url.clone()))
    }

    /// The folder of the bare clone of `repo`.
    fn clone_dir(&self, tenant: &str, repo: &Repository) -> PathBuf {
        self.dir
            .join("clones")
            .join(tenant)
            .join(format!("{}.git", repo.id))
    }

    fn lock(&self, path: &Path) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(locks.entry(path.to_path_buf()).or_default())
    }

    /// Fetch `repo` into its clone, and give the folder of the clone.
    pub(crate) fn fetch(&self, tenant: &str, repo: &Repository) -> Result<PathBuf, git::GitError> {
        let path = self.clone_dir(tenant, repo);
        let lock = self.lock(&path);
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        git::fetch(&path, &(self.remote_of)(repo))?;
        Ok(path)
    }

    /// The commits at and below `commit`, nearest first, from the clone of
    /// `repo`. The clone fetches when it does not have the commit. `None`:
    /// the repository does not have the commit.
    pub(crate) fn ancestors(
        &self,
        tenant: &str,
        repo: &Repository,
        commit: &str,
    ) -> Result<Option<Vec<String>>, git::GitError> {
        let mut path = self.clone_dir(tenant, repo);
        if !git::has_commit(&path, commit) {
            path = self.fetch(tenant, repo)?;
            if !git::has_commit(&path, commit) {
                return Ok(None);
            }
        }
        git::ancestors(&path, commit, MAX_DISTANCE).map(Some)
    }
}

/// Gives the builder a summarizer.
pub trait SummarizerSource: Send + Sync {
    /// Run `job` with a summarizer.
    fn run(
        &self,
        job: &mut dyn FnMut(&mut dyn Summarizer) -> Result<UpdateReport, IndexError>,
    ) -> Result<UpdateReport, IndexError>;
}

/// The fake summarizer of `kairos-index`: a fixed text for each input, with
/// no model. For tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeSummarizers;

impl SummarizerSource for FakeSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(&mut dyn Summarizer) -> Result<UpdateReport, IndexError>,
    ) -> Result<UpdateReport, IndexError> {
        job(&mut kairos_index::FakeSummarizer::default())
    }
}

/// Qwen3-4B through llama.cpp, from the model file at `model`. The model is
/// loaded for each build and freed after it, so the server does not keep
/// about 3 GB of memory between pushes.
#[cfg(feature = "llama")]
#[derive(Debug, Clone)]
pub struct LlamaSummarizers {
    pub model: PathBuf,
}

#[cfg(feature = "llama")]
impl SummarizerSource for LlamaSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(&mut dyn Summarizer) -> Result<UpdateReport, IndexError>,
    ) -> Result<UpdateReport, IndexError> {
        let model = kairos_index::LlamaModelFile::load(&self.model)?;
        let mut summarizer = model.summarizer()?;
        job(&mut summarizer)
    }
}

/// Why a build of the server without the `llama` feature has no builder.
const NO_SUMMARIZER: &str = "this build of the server has no summarizer (the feature llama)";

/// Whether this build of the server has a summarizer: the `llama` feature.
/// The reason when it has none.
pub fn has_summarizer() -> Result<(), String> {
    if cfg!(feature = "llama") {
        Ok(())
    } else {
        Err(NO_SUMMARIZER.to_string())
    }
}

/// The summarizer of this build of the server, with the model file of
/// `tools`. The reason when the build has none.
pub fn summarizers(tools: &Tools) -> Result<Arc<dyn SummarizerSource>, String> {
    #[cfg(feature = "llama")]
    {
        Ok(Arc::new(LlamaSummarizers {
            model: tools.model.clone(),
        }))
    }
    #[cfg(not(feature = "llama"))]
    {
        let _ = tools;
        Err(NO_SUMMARIZER.to_string())
    }
}

/// The folder of the tools in `KAIROS_CODE_INDEX_DIR`.
pub const TOOLS_DIR: &str = "tools";

/// Check the tools of the builder in `tools/` of `code_index_dir`, and
/// download each one that is not there (COLLIERY-T-2525). `Ok(None)`: the
/// deployment has no `KAIROS_CODE_INDEX_DIR`, so nothing is checked or
/// downloaded. The error names the file or the URL that failed.
pub fn prepare_tools(
    code_index_dir: Option<&Path>,
    set: &ToolSet,
) -> Result<Option<Tools>, String> {
    let Some(dir) = code_index_dir else {
        return Ok(None);
    };
    let tools = kairos_index::tools::prepare(&dir.join(TOOLS_DIR), set, &mut |d| {
        tracing::info!(file = %d.file, url = %d.url, "the code index builder downloads a tool");
    })
    .map_err(|e| e.to_string())?;
    Ok(Some(tools))
}

/// What a sweep did for one repository.
#[derive(Debug)]
pub struct BuildOutcome {
    pub tenant: String,
    /// The slug of the repository.
    pub repository: String,
    /// The head of the default branch, if the clone has the branch.
    pub commit: String,
    /// The indexed commit that the build started from.
    pub base: Option<String>,
    /// The report of the update, when the sweep built an index.
    pub report: Option<UpdateReport>,
    /// Why the sweep built no index, when it did not.
    pub note: Option<String>,
    pub elapsed: Duration,
}

/// The input of a build: the base, read from the database.
struct Base {
    commit: String,
    structure: Vec<u8>,
    pool: Vec<kairos_db::code_indexes::PoolRow>,
}

/// Fetch each repository that has an index, and update the index of the
/// head of its default branch when it has none. One repository at a time.
pub async fn sweep(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    summarizers: Arc<dyn SummarizerSource>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
) -> Vec<BuildOutcome> {
    let tenants = match blocking
        .run_public(|conn| kairos_db::list_tenants(conn).map_err(ApiError::internal))
        .await
    {
        Ok(tenants) => tenants,
        Err(e) => {
            tracing::warn!(error = ?e, "code index builder could not list tenants");
            return Vec::new();
        }
    };
    let mut outcomes = Vec::new();
    for tenant in tenants.into_iter().filter(|t| t.schema_exists) {
        let repos = blocking
            .run(&tenant.slug, |conn| {
                let ids = kairos_db::code_indexes::indexed_repositories(conn)
                    .map_err(ApiError::internal)?;
                ids.into_iter()
                    .map(|id| kairos_db::repositories::load(conn, id).map_err(ApiError::internal))
                    .collect::<Result<Vec<_>, _>>()
            })
            .await;
        let repos = match repos {
            Ok(repos) => repos,
            Err(e) => {
                tracing::warn!(tenant = %tenant.slug, error = ?e, "code index builder could not list repositories");
                continue;
            }
        };
        for repo in repos {
            let outcome = build_repository(
                blocking,
                service,
                &tenant.slug,
                repo,
                Arc::clone(&summarizers),
                Arc::clone(&embedder),
            )
            .await;
            match (&outcome.report, &outcome.note) {
                (Some(report), _) => tracing::info!(
                    tenant = %outcome.tenant,
                    repository = %outcome.repository,
                    commit = %outcome.commit,
                    base = ?outcome.base,
                    symbols = report.summary.symbols.summarized,
                    files = report.summary.files.summarized,
                    modules = report.summary.modules.summarized,
                    elapsed_secs = outcome.elapsed.as_secs_f64(),
                    "code index built"
                ),
                (None, Some(note)) if note.starts_with("failed") => tracing::warn!(
                    tenant = %outcome.tenant,
                    repository = %outcome.repository,
                    commit = %outcome.commit,
                    note = %note,
                    "code index not built"
                ),
                _ => {}
            }
            outcomes.push(outcome);
        }
    }
    outcomes
}

async fn build_repository(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    tenant: &str,
    repo: Repository,
    summarizers: Arc<dyn SummarizerSource>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
) -> BuildOutcome {
    let started = Instant::now();
    let mut outcome = BuildOutcome {
        tenant: tenant.to_string(),
        repository: repo.slug.clone(),
        commit: String::new(),
        base: None,
        report: None,
        note: None,
        elapsed: Duration::ZERO,
    };
    let done = |mut outcome: BuildOutcome, note: String| {
        outcome.note = Some(note);
        outcome.elapsed = started.elapsed();
        outcome
    };

    // The clone and the head of the default branch.
    let fetched = {
        let service = Arc::clone(service);
        let tenant = tenant.to_string();
        let repo = repo.clone();
        tokio::task::spawn_blocking(move || {
            let path = service.fetch(&tenant, &repo)?;
            let head = git::branch_head(&path, &repo.default_branch)?;
            let ancestors = match &head {
                Some(head) => git::ancestors(&path, head, MAX_DISTANCE)?,
                None => Vec::new(),
            };
            Ok::<_, git::GitError>((path, head, ancestors))
        })
        .await
    };
    let (clone, head, ancestors) = match fetched {
        Ok(Ok(fetched)) => fetched,
        Ok(Err(e)) => return done(outcome, format!("failed: {e}")),
        Err(e) => return done(outcome, format!("failed: {e}")),
    };
    let Some(head) = head else {
        return done(
            outcome,
            format!("the repository has no branch {}", repo.default_branch),
        );
    };
    outcome.commit = head.clone();
    let key = (repo.id, head.clone());
    if service
        .failed
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&key)
    {
        return done(outcome, "an earlier build of this commit failed".into());
    }

    // The nearest indexed commit, and its structure and the pool.
    let repo_id = repo.id;
    let base = blocking
        .run(tenant, move |conn| {
            let indexed: HashSet<String> = kairos_db::code_indexes::list(conn, repo_id)
                .map_err(ApiError::internal)?
                .into_iter()
                .map(|i| i.commit_sha)
                .collect();
            let Some(nearest) = ancestors.iter().find(|c| indexed.contains(*c)) else {
                return Ok(None);
            };
            if *nearest == ancestors[0] {
                return Ok(Some(None));
            }
            let structure = kairos_db::code_indexes::structure(conn, repo_id, nearest)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("the index went away"))?;
            let pool =
                kairos_db::code_indexes::whole_pool(conn, repo_id).map_err(ApiError::internal)?;
            Ok(Some(Some(Base {
                commit: nearest.clone(),
                structure,
                pool,
            })))
        })
        .await;
    let base = match base {
        Ok(Some(Some(base))) => base,
        Ok(Some(None)) => return done(outcome, "the head has an index".into()),
        Ok(None) => {
            return done(
                outcome,
                format!("no indexed commit is within {MAX_DISTANCE} commits below the head"),
            );
        }
        Err(e) => return done(outcome, format!("failed: {e:?}")),
    };
    outcome.base = Some(base.commit.clone());

    // The update, off the async threads: it runs rust-analyzer and the
    // model for minutes.
    let built = {
        let head = head.clone();
        let build_options = service.build_options();
        tokio::task::spawn_blocking(move || -> Result<_, String> {
            let work = tempfile::Builder::new()
                .prefix("kairos-code-index-")
                .tempdir()
                .map_err(|e| format!("no work folder: {e}"))?;
            let db = work.path().join("index.db");
            let tree = work.path().join("tree");
            std::fs::create_dir(&tree).map_err(|e| format!("no tree folder: {e}"))?;
            kairos_index::store::assemble(
                &base.structure,
                base.pool.into_iter().map(pool_row_of),
                &db,
            )
            .map_err(|e| e.to_string())?;
            git::checkout_tree(&clone, &head, &tree).map_err(|e| e.to_string())?;
            let options = UpdateOptions {
                // The server runs SCIP for each push (COLLIERY-I-0264,
                // decision 5 of 2026-10-01).
                rust_edges: true,
                build: build_options,
                ..Default::default()
            };
            let report = summarizers
                .run(&mut |summarizer| {
                    kairos_index::update(&tree, &db, summarizer, embedder.as_ref(), &options)
                })
                .map_err(|e| e.to_string())?;
            let split = kairos_index::store::split(&db).map_err(|e| e.to_string())?;
            Ok((report, split))
        })
        .await
    };
    let (report, split) = match built {
        Ok(Ok(built)) => built,
        Ok(Err(e)) => {
            service
                .failed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key);
            return done(outcome, format!("failed: {e}"));
        }
        Err(e) => return done(outcome, format!("failed: {e}")),
    };

    let index = kairos_db::code_indexes::NewCodeIndex {
        repository_id: repo.id,
        commit_sha: head,
        ref_name: Some(repo.default_branch.clone()),
        source: "build",
        structure: split.structure_gz,
        structure_bytes: split.structure_bytes as i64,
        summary_keys: split.keys.len() as i32,
        vector_model: split.vector_model,
        created_by: None,
    };
    let pool = used_rows(split.pool, &split.keys);
    let stored = blocking
        .run(tenant, move |conn| {
            kairos_db::code_indexes::put(conn, &index, &pool).map_err(ApiError::internal)
        })
        .await;
    if let Err(e) = stored {
        return done(outcome, format!("failed: {e:?}"));
    }
    outcome.report = Some(report);
    outcome.elapsed = started.elapsed();
    outcome
}

/// Run a [`sweep`] on an interval, for ever. An error is logged and the
/// next sweep tries again; it never stops the server.
pub async fn run_builder(
    blocking: BlockingTenantPool,
    service: Arc<CodeIndexService>,
    summarizers: Arc<dyn SummarizerSource>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
    interval: Duration,
) {
    tracing::info!(
        interval_secs = interval.as_secs(),
        "code index builder started"
    );
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        sweep(
            &blocking,
            &service,
            Arc::clone(&summarizers),
            Arc::clone(&embedder),
        )
        .await;
    }
}

/// A pool row of the database as `kairos-index` takes it.
pub(crate) fn pool_row_of(row: kairos_db::code_indexes::PoolRow) -> kairos_index::store::PoolRow {
    kairos_index::store::PoolRow {
        key: row.key,
        level: row.level,
        summary: row.summary,
        vector: row.vector,
    }
}

/// The rows of `pool` whose keys the structure uses, as the database takes
/// them. A row that no structure uses is not stored.
pub(crate) fn used_rows(
    pool: Vec<kairos_index::store::PoolRow>,
    keys: &[String],
) -> Vec<kairos_db::code_indexes::PoolRow> {
    let keys: HashSet<&str> = keys.iter().map(String::as_str).collect();
    pool.into_iter()
        .filter(|row| keys.contains(row.key.as_str()))
        .map(|row| kairos_db::code_indexes::PoolRow {
            key: row.key,
            level: row.level,
            summary: row.summary,
            vector: row.vector,
        })
        .collect()
}
