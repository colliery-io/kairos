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
//! ([`git`]). The clone fetches from the `repo_url` of the repository. When
//! the organization has a read token for the host of the repository
//! (KAIROS-T-0431), the fetch gives that token to git; else, when the
//! repository has a read token (COLLIERY-T-3105), it gives that one. Git
//! gets it through an askpass helper ([`git`]); [`crate::credentials`]
//! chooses, reads and decrypts it with `KAIROS_SECRETS_KEY`. The log of each
//! fetch and the note of a failed fetch name the kind of token, never the
//! value. With no token, the fetch has no credential, and a remote that asks
//! for one fails at once.
//!
//! # The builder
//!
//! Each build is a job of the job queue of the servers ([`crate::jobs`],
//! KAIROS-T-0374, COLLIERY-I-0613), so 2 or more servers share the builds
//! and each build runs once. The jobs are in [`jobs`]:
//!
//! - a build that a person asks for (KAIROS-T-0332): the API adds the job;
//! - an update after a push: [`jobs::enqueue_updates`] fetches each
//!   repository that has an index, and adds a job when the head of its
//!   default branch has no index ([`kairos_index::update`] with a SCIP run
//!   from the nearest indexed commit below it);
//! - a first build of a repository that has no index (KAIROS-T-0318):
//!   [`jobs::enqueue_first_builds`], a full build of the head.
//!
//! [`jobs::run_builder`] adds the periodic pass of the sweep
//! (KAIROS-T-0376): at each `KAIROS_CODE_INDEX_POLL_SECS`, one server adds
//! the update jobs and the first-build jobs. The dedupe key of a
//! build job is the repository: a repository has one open build job at
//! most, whatever server adds it.
//!
//! The order (KAIROS-T-0334): by priority, a requested build first, then
//! an update, then a first build. Inside a priority, the job with the
//! oldest `run_after` first. The `run_after` of an update is the time of
//! the newest index of the repository, which is the earliest time at which
//! it became stale; the `run_after` of a first build is the time at which
//! the repository was registered. So the repository that has been stale
//! longest goes first, whatever the order of the list or of the pushes. A
//! repository that a build just updated goes last in the next pass. With
//! `KAIROS_CODE_INDEX_MIN_REBUILD_SECS` (off by default), a repository whose
//! newest index is younger than that time gets no update job in the pass.
//!
//! The limits per kind of job replace the gate of KAIROS-T-0318: a build
//! with the embedded model is of the kind `embedded_build` (1 on each
//! server, `KAIROS_JOB_LIMIT_EMBEDDED_BUILD`), and a build with a hosted
//! provider is of the kind `hosted_build` (4). A first build gives its slot
//! to an update or a requested build of its kind that waits
//! ([`crate::jobs::JobHandle::should_yield`]): it stops after its next
//! summary, and goes back to the queue with no try counted. So a first
//! build of hours does not stop the updates.
//!
//! A hosted summarizer sends at most `concurrency` requests at a time to
//! its endpoint, for all the builds of the server together, and waits after
//! a 429 or a 503 ([`kairos_index::rate`], KAIROS-T-0495). A summary that
//! it still cannot make does not fail the build: the run counts it as
//! missing (`summaries_missing`), and the next build makes it. A build that
//! makes no summary at all, for a reason that is not the rate, fails.
//!
//! A build reports progress for each summary call, while a call waits for
//! a permit or a back-off of the endpoint, for each embedding batch, and
//! while its structure step runs ([`STRUCTURE_STEP_LIMIT`]). With no
//! progress for the lease time, its server kills it, and a server takes the
//! job again. A server that stops loses its leases, and another server
//! takes those jobs when the leases run out.
//!
//! Each build keeps its summaries across a stop, also on another server
//! (KAIROS-T-0335, KAIROS-T-0378). Each summary is written to the pool of
//! the repository in Postgres when the model makes it ([`PoolSink`]), with
//! no vector and no index that uses it yet. The next build of the
//! repository, on any server, links the summaries of the pool first: an
//! update assembles its work database from the base structure and the
//! whole pool, and a full build adds the pool to its work database before
//! it summarizes. So a build that stops on one server and goes on on
//! another server, with an empty `KAIROS_CODE_INDEX_DIR`, makes only the
//! summaries that were not made. The structure is not stored: the next
//! build makes it again (minutes).
//!
//! The work databases in `first-builds/` and `updates/` of
//! `KAIROS_CODE_INDEX_DIR` are a local cache: they hold the summaries too,
//! and an update copies the summaries of the old work database into the
//! new one. The keys of a summary name its input, so a summary of a build
//! that did not end is right for each later build that needs its key. An
//! index uses only the summaries that its structure uses: a download never
//! has a summary that its structure does not use. A stored index marks
//! the rows that it uses (`last_used_at`); the retention pass deletes the
//! rows that no index used, `KAIROS_CODE_INDEX_POOL_DAYS` (30) after a
//! build made them. The run row counts the summaries that a run reused.
//! The builder removes a work database after it stores the index. A
//! repository with `code_index_build = off` gets no build.
//!
//! # The runs
//!
//! Each run has a row ([`kairos_db::code_index_builds`]) with the server
//! that runs it (KAIROS-T-0354). The job of the run names the run in its
//! payload: when a server takes the job again, the run goes on, on that
//! server ([`kairos_db::code_index_builds::adopt`]). A `running` run that no
//! open job names is stale: the next run of the repository ends it as
//! failed, and so does the builder of each server when it starts
//! ([`jobs::end_stale_runs`]). A job that fails after its last try ends its
//! run as failed, with the reason.
//!
//! # The breaker
//!
//! A build job whose lease ran out (its server stopped, ran out of memory,
//! or stalled), or that the watchdog killed, is not run again: the breaker
//! of its repository trips (KAIROS-T-0399, GitHub #34,
//! [`kairos_db::code_index_breakers`]). While it is tripped, the sweep adds
//! no update and no first build for the repository, and a build job of the
//! repository that waits ends with [`BREAKER_TRIPPED`]. A build that a
//! person asks for resets it.
//!
//! # The tools
//!
//! The image does not hold the tools of the builder (COLLIERY-T-2525). At
//! each start, [`prepare_tools`] checks them in `tools/` of
//! `KAIROS_CODE_INDEX_DIR`, and downloads the ones that are not there
//! ([`kairos_index::tools`]). Without `KAIROS_CODE_INDEX_DIR`, nothing is
//! downloaded.

pub mod git;
pub mod jobs;

pub use jobs::{
    BuildHandler, end_stale_runs, enqueue_first_build, enqueue_first_builds, enqueue_updates,
    first_builds, job_key, requested_builds, run_builder, sweep,
};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use kairos_db::code_index_builds as runs;
use kairos_db::models::repositories::Repository;
use kairos_index::tools::{ToolSet, Tools};
use kairos_index::{IndexError, Summarizer, SummaryRequest, UpdateOptions, UpdateReport};

use crate::blocking::BlockingTenantPool;
use crate::error::ApiError;
use crate::jobs::JobHandle;
use crate::secrets::SecretsKey;

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
    /// `KAIROS_SECRETS_KEY`, to decrypt the read token of a repository
    /// (COLLIERY-T-3105).
    secrets: Option<SecretsKey>,
    /// The last reason why the builder did not build each repository, so
    /// that the log gets a reason once, not at each pass.
    notes: Mutex<HashMap<uuid::Uuid, String>>,
    /// The start of this server (KAIROS-T-0354): a random id, recorded on
    /// each run. A `running` run of another id is of a server that stopped.
    instance: uuid::Uuid,
    /// `KAIROS_CODE_INDEX_MIN_REBUILD_SECS` (KAIROS-T-0334): the least time
    /// between 2 updates of one repository. 0: off.
    min_rebuild_secs: std::sync::atomic::AtomicU64,
    /// The dependency cache of the SCIP runners (KAIROS-T-0474).
    deps: DependencySettings,
    /// The last disk use of the tools and the caches, and when it was read
    /// (KAIROS-T-0478).
    disk: Mutex<Option<(Instant, kairos_client::types_code_index::CodeIndexDiskUse)>>,
    /// The waits of a hosted summarizer after a 429 (KAIROS-T-0495): the
    /// default, or the short waits of a test.
    summary_backoff: Mutex<kairos_index::rate::Backoff>,
}

/// How long a reading of the disk use is kept: a walk of the tools folder
/// reads each file of the toolchains.
const DISK_USE_TTL: Duration = Duration::from_secs(60);

/// The language that uses a tool folder of `tools/`.
fn tool_language(folder: &str) -> Option<&'static str> {
    let name = folder.rsplit_once('-').map_or(folder, |(n, _)| n);
    match name {
        "go" | "scip-go" => Some("go"),
        "scip-typescript" => Some("typescript"),
        "node" => Some("typescript, python"),
        "scip-python" | "python" | "uv" => Some("python"),
        "rust" | "rust-src" => Some("rust"),
        _ if folder.starts_with("rust-analyzer") => Some("rust"),
        _ => None,
    }
}

/// The settings of the dependency cache of the SCIP runners of Go,
/// TypeScript and Python (KAIROS-T-0474).
#[derive(Debug, Clone)]
pub struct DependencySettings {
    /// `KAIROS_CODE_INDEX_DEPS_CAP_MB`: the cap of the total size of the
    /// dependency caches.
    pub cap_bytes: u64,
    /// `KAIROS_CODE_INDEX_MIN_FREE_MB`: under this free space, the caches
    /// are removed before an install.
    pub min_free_bytes: u64,
    /// `KAIROS_CODE_INDEX_DEPS_ENV`: more variables of the environment that
    /// a dependency step gets.
    pub pass_variables: Vec<String>,
    /// `KAIROS_CODE_INDEX_TOOL_DOWNLOADS`: uv may fetch the CPython that a
    /// project asks for (KAIROS-T-0668).
    pub tool_downloads: bool,
}

impl Default for DependencySettings {
    fn default() -> Self {
        DependencySettings {
            cap_bytes: 10 * 1024 * 1024 * 1024,
            min_free_bytes: 5 * 1024 * 1024 * 1024,
            pass_variables: Vec::new(),
            tool_downloads: false,
        }
    }
}

/// The folder of the dependency caches in `KAIROS_CODE_INDEX_DIR`
/// (KAIROS-T-0474).
pub const DEPS_DIR: &str = "deps";

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
            secrets: None,
            notes: Mutex::default(),
            instance: uuid::Uuid::new_v4(),
            min_rebuild_secs: std::sync::atomic::AtomicU64::new(0),
            deps: DependencySettings::default(),
            disk: Mutex::new(None),
            summary_backoff: Mutex::default(),
        }
    }

    /// The disk use of the tools in `tools/` and of the dependency caches
    /// in `deps/` (KAIROS-T-0478), read again after [`DISK_USE_TTL`].
    pub fn disk_use(&self) -> kairos_client::types_code_index::CodeIndexDiskUse {
        use kairos_client::types_code_index::{
            CodeIndexCacheUse, CodeIndexDiskUse, CodeIndexToolUse,
        };
        let mut kept = self.disk.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, disk)) = kept.as_ref()
            && at.elapsed() < DISK_USE_TTL
        {
            return disk.clone();
        }
        let tools_dir = self.dir.join(TOOLS_DIR);
        let mut tools: Vec<CodeIndexToolUse> = kairos_index::languages::tool_usage(&tools_dir)
            .into_iter()
            .map(|(name, bytes)| CodeIndexToolUse {
                language: tool_language(&name).map(str::to_string),
                name,
                bytes,
            })
            .collect();
        // The rust-analyzer binary is a file, not a folder.
        for tool in &mut tools {
            if tool.name == "other" {
                let ra = tools_dir.join("rust-analyzer");
                let ra_bytes = std::fs::metadata(&ra).map(|m| m.len()).unwrap_or(0);
                if ra_bytes > 0 && tool.bytes >= ra_bytes {
                    tool.bytes -= ra_bytes;
                }
            }
        }
        if let Ok(meta) = std::fs::metadata(tools_dir.join("rust-analyzer")) {
            tools.push(CodeIndexToolUse {
                name: "rust-analyzer".into(),
                language: Some("rust".into()),
                bytes: meta.len(),
            });
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        let mut caches: std::collections::BTreeMap<String, (u32, u64)> = Default::default();
        let entries = kairos_index::languages::caches(&self.dir.join(DEPS_DIR));
        let caches_bytes = entries.iter().map(|e| e.bytes).sum();
        for e in entries {
            let language = e.path.rsplit('/').next().unwrap_or_default().to_string();
            let c = caches.entry(language).or_default();
            c.0 += 1;
            c.1 += e.bytes;
        }
        let disk = CodeIndexDiskUse {
            tools,
            caches: caches
                .into_iter()
                .map(|(language, (repositories, bytes))| CodeIndexCacheUse {
                    language,
                    repositories,
                    bytes,
                })
                .collect(),
            caches_bytes,
            caches_cap_bytes: self.deps.cap_bytes,
            free_bytes: kairos_index::languages::free_bytes(&self.dir),
        };
        *kept = Some((Instant::now(), disk.clone()));
        disk
    }

    /// The settings of the dependency cache (KAIROS-T-0474).
    pub fn with_dependency_settings(mut self, deps: DependencySettings) -> Self {
        self.deps = deps;
        self
    }

    /// The least time between 2 updates of one repository
    /// (`KAIROS_CODE_INDEX_MIN_REBUILD_SECS`, KAIROS-T-0334). 0: off.
    pub fn with_min_rebuild_secs(self, secs: u64) -> Self {
        self.set_min_rebuild_secs(secs);
        self
    }

    /// Set the waits of a hosted summarizer after a 429 (KAIROS-T-0495). A
    /// test gives short waits; the default is 2 s to 2 min, 6 tries.
    pub fn set_summary_backoff(&self, backoff: kairos_index::rate::Backoff) {
        *self
            .summary_backoff
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = backoff;
    }

    fn summary_backoff(&self) -> kairos_index::rate::Backoff {
        self.summary_backoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Change the time of [`Self::with_min_rebuild_secs`].
    pub fn set_min_rebuild_secs(&self, secs: u64) {
        self.min_rebuild_secs
            .store(secs, std::sync::atomic::Ordering::Relaxed);
    }

    fn min_rebuild(&self) -> Option<Duration> {
        match self
            .min_rebuild_secs
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => None,
            secs => Some(Duration::from_secs(secs)),
        }
    }

    /// Record `instance` as the start of this server on each run, in place
    /// of a random id (KAIROS-T-0373: the id of the job worker).
    pub fn with_instance(mut self, instance: uuid::Uuid) -> Self {
        self.instance = instance;
        self
    }

    /// The start of this server, as the runs record it (KAIROS-T-0354).
    pub fn instance(&self) -> uuid::Uuid {
        self.instance
    }

    /// The work database of the first build of `repo`. It stays after a
    /// failure or a restart, so that the next build reuses its summaries.
    fn first_build_db(&self, tenant: &str, repo: &Repository) -> PathBuf {
        self.dir
            .join("first-builds")
            .join(tenant)
            .join(format!("{}.db", repo.id))
    }

    /// The work database of the update of `repo` (KAIROS-T-0335). It stays
    /// after a failure or a restart, so that the next update reuses its
    /// summaries.
    fn update_db(&self, tenant: &str, repo: &Repository) -> PathBuf {
        self.dir
            .join("updates")
            .join(tenant)
            .join(format!("{}.db", repo.id))
    }

    /// Record `note` as the last reason for `repo`. True when it is not the
    /// reason that was recorded before.
    fn note_changed(&self, repo: uuid::Uuid, note: &str) -> bool {
        let mut notes = self.notes.lock().unwrap_or_else(|e| e.into_inner());
        if notes.get(&repo).map(String::as_str) == Some(note) {
            return false;
        }
        notes.insert(repo, note.to_string());
        true
    }

    fn forget_note(&self, repo: uuid::Uuid) {
        self.notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&repo);
    }

    /// Decrypt the read token of each repository with `key`
    /// (`KAIROS_SECRETS_KEY`, COLLIERY-T-3105). With no key, a repository
    /// that has a token fails to fetch with a text that names the setting.
    pub fn with_secrets_key(mut self, key: Option<SecretsKey>) -> Self {
        self.secrets = key;
        self
    }

    /// The key of [`Self::with_secrets_key`].
    pub fn secrets_key(&self) -> Option<&SecretsKey> {
        self.secrets.as_ref()
    }

    /// The URL to fetch `repo` from.
    pub fn remote_of(&self, repo: &Repository) -> String {
        (self.remote_of)(repo)
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

    /// The options of a build of `repo` of `tenant`: [`Self::build_options`],
    /// and, once the tools are ready, the tools of Go, TypeScript and Python
    /// in `tools/`, fetched on first need, with the dependency cache of the
    /// repository in `deps/` (KAIROS-T-0474).
    pub fn build_options_for(&self, tenant: &str, repo: &Repository) -> kairos_index::BuildOptions {
        let mut options = self.build_options();
        if self.build.get().is_some() {
            options.scip.tools = kairos_index::languages::LanguageTools {
                dir: Some(self.dir.join(TOOLS_DIR)),
                fetch: true,
                cache: Some(kairos_index::languages::DependencyCache {
                    root: self.dir.join(DEPS_DIR),
                    key: kairos_index::languages::cache_key(&[tenant, &repo.id.to_string()]),
                    cap_bytes: self.deps.cap_bytes,
                    min_free_bytes: self.deps.min_free_bytes,
                }),
                pass_variables: self.deps.pass_variables.clone(),
                tool_downloads: self.deps.tool_downloads,
                ..Default::default()
            };
        }
        options
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

    /// Fetch `repo` into its clone, with its read token when it has one, and
    /// give the folder of the clone.
    pub fn fetch(
        &self,
        tenant: &str,
        repo: &Repository,
        token: Option<&git::GitToken>,
    ) -> Result<PathBuf, git::GitError> {
        let path = self.clone_dir(tenant, repo);
        let lock = self.lock(&path);
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        git::fetch(&path, &(self.remote_of)(repo), token)?;
        Ok(path)
    }

    /// The default branch of the remote of `repo` (KAIROS-T-0430), read
    /// with `token`: [`git::remote_head`].
    pub fn remote_head(
        &self,
        repo: &Repository,
        token: Option<&git::GitToken>,
    ) -> Result<Option<String>, git::GitError> {
        git::remote_head(&(self.remote_of)(repo), token)
    }

    /// The commits at and below `commit`, nearest first, from the clone of
    /// `repo`. The clone fetches when it does not have the commit. `None`:
    /// the repository does not have the commit.
    pub(crate) fn ancestors(
        &self,
        tenant: &str,
        repo: &Repository,
        commit: &str,
        token: Option<&git::GitToken>,
    ) -> Result<Option<Vec<String>>, git::GitError> {
        let mut path = self.clone_dir(tenant, repo);
        if !git::has_commit(&path, commit) {
            path = self.fetch(tenant, repo, token)?;
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

    /// The model of the summaries, as `provider/model` (KAIROS-T-0341),
    /// for the record of a run.
    fn model_name(&self) -> String;
}

/// A hosted summarizer source (KAIROS-T-0341): an OpenAI-compatible chat
/// endpoint, from the settings of a tenant. A new client for each build.
pub struct HostedSummarizers {
    pub config: kairos_index::hosted::HostedConfig,
}

impl SummarizerSource for HostedSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(&mut dyn Summarizer) -> Result<UpdateReport, IndexError>,
    ) -> Result<UpdateReport, IndexError> {
        let mut summarizer = kairos_index::hosted::HostedChat::new(self.config.clone())
            .map_err(IndexError::Model)?;
        job(&mut summarizer)
    }

    fn model_name(&self) -> String {
        format!("{}/{}", self.config.provider, self.config.model)
    }
}

/// The Bedrock summarizer source (KAIROS-T-0342), from the settings of a
/// tenant. A new client for each build.
pub struct BedrockSummarizers {
    pub config: kairos_index::bedrock::BedrockConfig,
}

impl SummarizerSource for BedrockSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(&mut dyn Summarizer) -> Result<UpdateReport, IndexError>,
    ) -> Result<UpdateReport, IndexError> {
        let mut summarizer =
            kairos_index::bedrock::Bedrock::new(self.config.clone()).map_err(IndexError::Model)?;
        job(&mut summarizer)
    }

    fn model_name(&self) -> String {
        format!("bedrock/{}", self.config.model_id)
    }
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

    fn model_name(&self) -> String {
        kairos_index::FakeSummarizer::default().model()
    }
}

/// The text of a summary call or an embedding batch of a build that its
/// server killed (KAIROS-T-0374).
pub const STOPPED: &str = "the job of the build was stopped";

/// The text of a summary call of a first build that gives its slot to a
/// job of a higher priority (KAIROS-T-0374).
pub const YIELDED: &str = "the build gave its slot to a build of a higher priority";

/// The longest time for which the structure step of a build counts as
/// progress (KAIROS-T-0374). The step (a SCIP run of rust-analyzer) gives no
/// progress of its own, so the build reports progress while the step runs,
/// for this time at most; a step that takes longer counts as a stall, and
/// the watchdog kills the build one lease later.
pub const STRUCTURE_STEP_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

/// Writes each summary that a build makes to the pool of its repository in
/// Postgres when the model makes it (KAIROS-T-0378). So a build that stops
/// keeps its summaries also when the next lease of its job runs on another
/// server, with another `KAIROS_CODE_INDEX_DIR`. A write that fails is
/// logged: the summary stays in the work database of this server.
#[derive(Clone)]
pub(crate) struct PoolSink {
    blocking: BlockingTenantPool,
    tenant: String,
    repository: uuid::Uuid,
    runtime: tokio::runtime::Handle,
}

impl PoolSink {
    /// The sink of the builds of `repository`, on the runtime of the
    /// caller. Call it on a thread of the runtime; [`Self::write`] runs on
    /// a blocking thread.
    pub(crate) fn new(blocking: &BlockingTenantPool, tenant: &str, repository: uuid::Uuid) -> Self {
        PoolSink {
            blocking: blocking.clone(),
            tenant: tenant.to_string(),
            repository,
            runtime: tokio::runtime::Handle::current(),
        }
    }

    /// Add the summaries of `requests` that came back in `results` to the
    /// pool. A summary that is empty or an error is not written, as the
    /// work database does not take it.
    fn write(&self, model: &str, requests: &[SummaryRequest], results: &[Result<String, String>]) {
        let made: Vec<kairos_db::code_indexes::MadeSummary> = requests
            .iter()
            .zip(results)
            .filter_map(|(request, result)| {
                let text = result.as_ref().ok()?.trim();
                (!text.is_empty()).then(|| kairos_db::code_indexes::MadeSummary {
                    key: request.key.clone(),
                    level: request.level.as_str().to_string(),
                    summary: text.to_string(),
                    model: model.to_string(),
                })
            })
            .collect();
        if made.is_empty() {
            return;
        }
        let repository = self.repository;
        let written = self
            .runtime
            .block_on(self.blocking.run(&self.tenant, move |conn| {
                kairos_db::code_indexes::add_made(conn, repository, &made)
                    .map_err(ApiError::internal)
            }));
        if let Err(e) = written {
            tracing::warn!(
                tenant = %self.tenant,
                repository = %self.repository,
                error = ?e,
                "code index summaries not written to the pool; the work database keeps them"
            );
        }
    }
}

/// A summarizer that reports the progress of its build for each call
/// (KAIROS-T-0374), and writes each summary to the pool of the repository
/// ([`PoolSink`], KAIROS-T-0378). A call of a build that was killed, or
/// that must give its slot, fails with [`STOPPED`] or [`YIELDED`]: the
/// summaries made before stay in the pool and in the work database.
///
/// The build reports progress while a call waits for a permit of the
/// endpoint or for the back-off after a 429 (KAIROS-T-0495,
/// [`Summarizer::heartbeat`]), so the watchdog does not kill it as a stall.
/// A request in flight or a model that hangs reports none.
struct ProgressSummarizer<'a> {
    inner: &'a mut dyn Summarizer,
    handle: &'a JobHandle,
    pool: &'a PoolSink,
    /// Why the build stops: [`STOPPED`] or [`YIELDED`], once a check gave it.
    halted: Option<String>,
}

impl<'a> ProgressSummarizer<'a> {
    fn new(inner: &'a mut dyn Summarizer, handle: &'a JobHandle, pool: &'a PoolSink) -> Self {
        let beat = handle.clone();
        inner.heartbeat(Arc::new(move || {
            beat.progress();
        }));
        ProgressSummarizer {
            inner,
            handle,
            pool,
            halted: None,
        }
    }

    fn check(&mut self) -> Result<(), String> {
        let why = if !self.handle.progress() {
            STOPPED
        } else if self.handle.should_yield() {
            YIELDED
        } else {
            return Ok(());
        };
        self.halted = Some(why.to_string());
        Err(why.to_string())
    }
}

impl Summarizer for ProgressSummarizer<'_> {
    fn model(&self) -> String {
        self.inner.model()
    }

    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String> {
        self.check()?;
        let summary = self.inner.summarize(request);
        self.handle.progress();
        self.pool.write(
            &self.inner.model(),
            std::slice::from_ref(request),
            std::slice::from_ref(&summary),
        );
        summary
    }

    fn concurrency(&self) -> usize {
        self.inner.concurrency()
    }

    fn summarize_many(&mut self, requests: &[SummaryRequest]) -> Vec<Result<String, String>> {
        if let Err(why) = self.check() {
            return requests.iter().map(|_| Err(why.clone())).collect();
        }
        let summaries = self.inner.summarize_many(requests);
        self.handle.progress();
        self.pool.write(&self.inner.model(), requests, &summaries);
        summaries
    }

    fn halted(&self) -> Option<String> {
        self.halted.clone()
    }
}

/// An embedder that reports the progress of its build for each batch
/// (KAIROS-T-0374).
struct ProgressEmbedder<'a> {
    inner: &'a dyn kairos_embed::EmbeddingProvider,
    handle: &'a JobHandle,
}

impl kairos_embed::EmbeddingProvider for ProgressEmbedder<'_> {
    fn model_id(&self) -> &kairos_embed::ModelId {
        self.inner.model_id()
    }

    fn embed(
        &self,
        texts: &[String],
    ) -> Result<Vec<kairos_embed::Embedding>, kairos_embed::EmbedError> {
        if !self.handle.progress() {
            return Err(kairos_embed::EmbedError::Failed(STOPPED.to_string()));
        }
        let vectors = self.inner.embed(texts);
        self.handle.progress();
        vectors
    }
}

/// Run `step` (the structure step of a build) and report progress while it
/// runs, for [`STRUCTURE_STEP_LIMIT`] at most (KAIROS-T-0374).
fn with_progress<T>(handle: &JobHandle, step: impl FnOnce() -> T) -> T {
    let done = std::sync::atomic::AtomicBool::new(false);
    handle.progress();
    let out = std::thread::scope(|scope| {
        scope.spawn(|| {
            let started = Instant::now();
            while !done.load(std::sync::atomic::Ordering::Relaxed)
                && started.elapsed() < STRUCTURE_STEP_LIMIT
            {
                handle.progress();
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        let out = step();
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        out
    });
    handle.progress();
    out
}

/// Qwen3-4B through llama.cpp, from the model file at `model`, on `threads`
/// CPU threads (`KAIROS_CODE_INDEX_THREADS`). The model is loaded for a
/// build and freed when no build uses it, so the server does not keep about
/// 3 GB of memory between pushes. When 2 builds run at the same time
/// (`KAIROS_JOB_LIMIT_EMBEDDED_BUILD` above 1), they share one loaded model,
/// each with its own context (KAIROS-T-0318).
#[cfg(feature = "llama")]
#[derive(Debug, Clone)]
pub struct LlamaSummarizers {
    pub model: PathBuf,
    pub threads: u32,
    loaded: Arc<Mutex<std::sync::Weak<kairos_index::LlamaModelFile>>>,
}

#[cfg(feature = "llama")]
impl LlamaSummarizers {
    /// The summarizers of the model file at `model`, on `threads` threads.
    pub fn new(model: PathBuf, threads: u32) -> Self {
        LlamaSummarizers {
            model,
            threads,
            loaded: Arc::default(),
        }
    }

    /// The loaded model: the one that a build uses now, else a new load.
    fn model(&self) -> Result<Arc<kairos_index::LlamaModelFile>, IndexError> {
        let mut loaded = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(model) = loaded.upgrade() {
            return Ok(model);
        }
        let model = Arc::new(kairos_index::LlamaModelFile::load(&self.model)?);
        *loaded = Arc::downgrade(&model);
        Ok(model)
    }
}

#[cfg(feature = "llama")]
impl SummarizerSource for LlamaSummarizers {
    fn run(
        &self,
        job: &mut dyn FnMut(&mut dyn Summarizer) -> Result<UpdateReport, IndexError>,
    ) -> Result<UpdateReport, IndexError> {
        let model = self.model()?;
        let mut summarizer = model.summarizer_with_threads(self.threads)?;
        job(&mut summarizer)
    }

    fn model_name(&self) -> String {
        format!(
            "embedded/{}",
            self.model
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "model".to_string())
        )
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
/// `tools`, on `threads` CPU threads. The reason when the build has none.
pub fn summarizers(tools: &Tools, threads: u32) -> Result<Arc<dyn SummarizerSource>, String> {
    #[cfg(feature = "llama")]
    {
        if tools.model.is_none() {
            return Err(NO_SUMMARIZER.to_string());
        }
        Ok(Arc::new(llama_summarizers(tools, threads)))
    }
    #[cfg(not(feature = "llama"))]
    {
        let _ = (tools, threads);
        Err(NO_SUMMARIZER.to_string())
    }
}

#[cfg(feature = "llama")]
fn llama_summarizers(tools: &Tools, threads: u32) -> LlamaSummarizers {
    LlamaSummarizers::new(
        tools
            .model
            .clone()
            .expect("the caller checked that the set has the model"),
        threads,
    )
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
    /// The indexed commit that the build started from. `None` for a first
    /// build.
    pub base: Option<String>,
    /// The report of the build, when the sweep built an index.
    pub report: Option<UpdateReport>,
    /// Why the sweep built no index, when it did not.
    pub note: Option<String>,
    pub elapsed: Duration,
}

/// The note of a first-build job of a repository that has an index
/// (KAIROS-T-0378).
pub const HAS_INDEX: &str = "the repository has an index";

/// The note of a repository with `code_index_build = off`.
pub const OPTED_OUT: &str = "the repository has code_index_build off";

/// The error of a build job, and of its run, that waited for a server when
/// `code_index_build` was set to off (KAIROS-T-0398).
pub const TURNED_OFF: &str = "cancelled: code_index_build was set to off";

/// The note of a repository whose breaker is tripped (KAIROS-T-0399): a
/// build job of the repository stopped its server or stalled. A build that a
/// person asks for resets it.
pub const BREAKER_TRIPPED: &str = "the code index breaker of the repository tripped";

/// The note of an `embedded` repository on a server with no embedded model
/// (KAIROS-T-0341, KAIROS-T-0344).
pub const NO_EMBEDDED_MODEL: &str = "failed: This image has no embedded model. Set a hosted \
                                      provider for the organization and put the repository on \
                                      it (the default summarizer hosted, or the repository \
                                      set to hosted), or run the image with the model.";

/// What a build uses for its summaries and its vectors (KAIROS-T-0341),
/// chosen from the settings of the tenant and the opt-in of the repository.
pub(crate) struct Models {
    pub summarizers: Arc<dyn SummarizerSource>,
    pub embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
}

/// Choose the models of a build (KAIROS-T-0341). `embedded` is the
/// summarizer of the server, when the image has one. The error is the note
/// of a failed run.
async fn models_for(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    tenant: &str,
    repo: &Repository,
    embedded: &Option<Arc<dyn SummarizerSource>>,
    embedder: &Arc<dyn kairos_embed::EmbeddingProvider>,
) -> Result<Models, String> {
    let settings = blocking
        .run(tenant, |conn| {
            kairos_db::code_index_settings::load_or_default(conn).map_err(ApiError::internal)
        })
        .await
        .map_err(|e| format!("failed: the settings of the organization are not readable: {e:?}"))?;
    let open = |field: &str, sealed: Option<kairos_db::code_index_settings::SealedSecret>| {
        open_secret(service.secrets_key(), tenant, field, sealed)
            .map_err(|why| format!("failed: {why}"))
    };

    let hosted_summaries = hosted_summaries(repo, &settings);
    let summarizers: Arc<dyn SummarizerSource> = if hosted_summaries {
        let secret = open("summary.secret", settings.summary_secret())?;
        match settings.summary_provider.as_str() {
            "ollama-cloud" => {
                let (Some(base_url), Some(model)) = (
                    settings.summary_base_url.clone(),
                    settings.summary_model.clone(),
                ) else {
                    return Err(
                        "failed: the provider ollama-cloud of the organization has no \
                                base URL or no model"
                            .to_string(),
                    );
                };
                let mut config = kairos_index::hosted::HostedConfig::new(
                    "ollama-cloud",
                    &base_url,
                    &model,
                    settings.concurrency.max(1) as usize,
                );
                config.api_key = secret;
                config.backoff = service.summary_backoff();
                Arc::new(HostedSummarizers { config })
            }
            "bedrock" => {
                let (Some(region), Some(model_id)) = (
                    settings.summary_region.clone(),
                    settings.summary_model.clone(),
                ) else {
                    return Err(
                        "failed: the provider bedrock of the organization has no region \
                                or no model id"
                            .to_string(),
                    );
                };
                let secret = secret.ok_or_else(|| {
                    "failed: the provider bedrock of the organization has no credentials"
                        .to_string()
                })?;
                let mut parts = secret.splitn(3, ':');
                let (Some(access_key_id), Some(secret_access_key)) = (parts.next(), parts.next())
                else {
                    return Err("failed: the credentials of bedrock are not <access key \
                                id>:<secret access key>"
                        .to_string());
                };
                let session_token = parts.next().filter(|t| !t.is_empty()).map(str::to_string);
                // A region that is a URL is an endpoint override, for a test
                // server; the signature then names us-east-1.
                let (region, endpoint) =
                    if region.starts_with("http://") || region.starts_with("https://") {
                        ("us-east-1".to_string(), Some(region))
                    } else {
                        (region, None)
                    };
                let mut config = kairos_index::bedrock::BedrockConfig::new(
                    &region,
                    &model_id,
                    access_key_id,
                    secret_access_key,
                    settings.concurrency.max(1) as usize,
                );
                config.session_token = session_token;
                config.endpoint = endpoint;
                config.backoff = service.summary_backoff();
                Arc::new(BedrockSummarizers { config })
            }
            other => {
                return Err(format!(
                    "failed: the provider {other:?} of the organization is not available in \
                     this version of Kairos"
                ));
            }
        }
    } else {
        embedded
            .clone()
            .ok_or_else(|| NO_EMBEDDED_MODEL.to_string())?
    };

    let hosted_vectors = settings.vector_provider == "remote";
    let embedder: Arc<dyn kairos_embed::EmbeddingProvider> = if hosted_vectors {
        let (Some(base_url), Some(model)) = (
            settings.vector_base_url.clone(),
            settings.vector_model.clone(),
        ) else {
            return Err(
                "failed: the vector provider remote of the organization has no base URL \
                        or no model"
                    .to_string(),
            );
        };
        let api_key = open("vectors.secret", settings.vector_secret())?;
        let mut config = kairos_embed::remote::RemoteConfig::new(base_url, model);
        config.api_key = api_key;
        // The provider probes the endpoint when it is made: off the async
        // threads.
        let provider =
            tokio::task::spawn_blocking(move || kairos_embed::remote::RemoteProvider::new(config))
                .await
                .map_err(|e| format!("failed: {e}"))?
                .map_err(|e| format!("failed: the vector endpoint of the organization: {e}"))?;
        Arc::new(provider)
    } else {
        Arc::clone(embedder)
    };
    Ok(Models {
        summarizers,
        embedder,
    })
}

/// Open a sealed secret of the code index settings of `tenant`
/// (`summary.secret` or `vectors.secret`). `None` when none is stored. The
/// error says why the secret does not open.
pub(crate) fn open_secret(
    key: Option<&SecretsKey>,
    tenant: &str,
    field: &str,
    sealed: Option<kairos_db::code_index_settings::SealedSecret>,
) -> Result<Option<String>, String> {
    let Some(sealed) = sealed else {
        return Ok(None);
    };
    let key = key.ok_or_else(|| {
        "the organization has a hosted provider with a secret, and this deployment has no \
         KAIROS_SECRETS_KEY to open it"
            .to_string()
    })?;
    let aad = crate::api::org::code_index_settings::settings_aad(tenant, field);
    let plain = key
        .open(&aad, &sealed.key_id, &sealed.nonce, &sealed.ciphertext)
        .map_err(|e| format!("the secret {field} of the organization does not open: {e:?}"))?;
    String::from_utf8(plain)
        .map(Some)
        .map_err(|_| format!("the secret {field} is not text"))
}

/// The input of a build: the base, read from the database.
struct Base {
    commit: String,
    structure: Vec<u8>,
    pool: Vec<kairos_db::code_indexes::PoolRow>,
}

/// Where an update of a repository starts (KAIROS-T-0347).
enum Start {
    /// The head has an index: nothing to do.
    HeadIndexed,
    /// The nearest indexed commit below the head.
    Base(Base),
    /// The nearest index (the head's or another) is of another schema
    /// version: an update cannot read it, so the repository gets a full
    /// build.
    OldSchema { commit: String, found: i64 },
}

/// Which repositories a pass of the builder reads.
#[derive(Debug, Clone, Copy)]
enum Pass {
    /// The repositories that have an index.
    Updates,
    /// The repositories that have no index.
    FirstBuilds,
}

/// The repositories of each tenant for `pass`, by tenant. An error is
/// logged, and the tenant is not in the result.
async fn repositories_of(
    blocking: &BlockingTenantPool,
    pass: Pass,
) -> Vec<(String, Vec<Repository>)> {
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
    let mut out = Vec::new();
    for tenant in tenants.into_iter().filter(|t| t.schema_exists) {
        let repos = blocking
            .run(&tenant.slug, move |conn| {
                let ids = match pass {
                    Pass::Updates => kairos_db::code_indexes::indexed_repositories(conn),
                    Pass::FirstBuilds => kairos_db::code_indexes::unindexed_repositories(conn),
                }
                .map_err(ApiError::internal)?;
                ids.into_iter()
                    .map(|id| kairos_db::repositories::load(conn, id).map_err(ApiError::internal))
                    .collect::<Result<Vec<_>, _>>()
            })
            .await;
        match repos {
            Ok(repos) => out.push((tenant.slug, repos)),
            Err(e) => {
                tracing::warn!(tenant = %tenant.slug, error = ?e, "code index builder could not list repositories");
            }
        }
    }
    out
}

/// The repositories of a pass of the new-commits sweep, over all tenants, the
/// repository with the oldest newest index first (KAIROS-T-0334), with the
/// time of its newest index. A tie keeps the order of the tenants and of
/// the slugs.
async fn update_order(
    blocking: &BlockingTenantPool,
) -> Vec<(String, Repository, Option<chrono::DateTime<chrono::Utc>>)> {
    let mut order = Vec::new();
    for (tenant, repos) in repositories_of(blocking, Pass::Updates).await {
        let newest: HashMap<uuid::Uuid, chrono::DateTime<chrono::Utc>> = blocking
            .run(&tenant, |conn| {
                kairos_db::code_indexes::newest_writes(conn).map_err(ApiError::internal)
            })
            .await
            .map(|rows| rows.into_iter().collect())
            .unwrap_or_else(|e| {
                tracing::warn!(tenant = %tenant, error = ?e, "code index builder could not read the times of the indexes");
                HashMap::new()
            });
        for repo in repos {
            let at = newest.get(&repo.id).copied();
            order.push((at, tenant.clone(), repo));
        }
    }
    // No time sorts first: such a repository is not known to be fresh.
    order.sort_by_key(|(at, _, _)| *at);
    order
        .into_iter()
        .map(|(at, tenant, repo)| (tenant, repo, at))
        .collect()
}

/// The clone of a repository, fetched, and the head of its default branch.
struct Fetched {
    clone: PathBuf,
    head: String,
    /// The commits at and below the head, nearest first, when asked for.
    ancestors: Vec<String>,
}

/// Read the token of `repo`, read the default branch of its remote, fetch
/// its clone, and find the head of the branch to index. The error is the
/// note of the outcome.
///
/// KAIROS-T-0430: the branch to index is the branch set by hand, or else
/// the default branch of the remote (`HEAD`), read at each fetch, or else
/// the stored `default_branch` when the remote gives none. The branch of
/// the remote is written to the row ([`record_forge_branch`]), and `repo`
/// gets the branch that is indexed.
async fn fetch_head(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    tenant: &str,
    repo: &mut Repository,
    with_ancestors: bool,
) -> Result<Fetched, String> {
    // The token of the fetch (KAIROS-T-0431): the read token of the
    // organization for the host, else the token of the repository
    // (COLLIERY-T-3105), else none. The log names the kind, never the value.
    let token = {
        let key = service.secrets_key().cloned();
        let tenant_slug = tenant.to_string();
        let row = repo.clone();
        blocking
            .run(tenant, move |conn| {
                Ok(crate::credentials::fetch_token(
                    conn,
                    &tenant_slug,
                    &row,
                    key.as_ref(),
                ))
            })
            .await
    };
    let (token, source) = match token {
        Ok(Ok(token)) => token,
        Ok(Err(e)) => return Err(format!("failed: {e}")),
        Err(e) => return Err(format!("failed: {e:?}")),
    };
    tracing::info!(tenant, repository = %repo.slug, token = %source, "code index fetch");

    // The default branch of the remote, the clone, and the head of the
    // branch to index.
    let fetched = {
        let service = Arc::clone(service);
        let tenant = tenant.to_string();
        let repo = repo.clone();
        tokio::task::spawn_blocking(move || {
            // A remote that gives no default branch, or a failed read,
            // leaves the stored branch: the fetch then says what is wrong.
            let forge_branch = service
                .remote_head(&repo, token.as_ref())
                .unwrap_or_else(|e| {
                    tracing::warn!(repository = %repo.slug, error = %e, "the default branch of the forge was not read");
                    None
                });
            let branch = indexed_branch(&repo, forge_branch.as_deref()).to_string();
            let path = service.fetch(&tenant, &repo, token.as_ref())?;
            let head = git::branch_head(&path, &branch)?;
            let ancestors = match &head {
                Some(head) if with_ancestors => git::ancestors(&path, head, MAX_DISTANCE)?,
                _ => Vec::new(),
            };
            Ok::<_, git::GitError>((path, head, ancestors, forge_branch, branch))
        })
        .await
    };
    let (clone, head, ancestors, forge_branch, branch) = match fetched {
        Ok(Ok(fetched)) => fetched,
        Ok(Err(e)) => return Err(format!("failed: {e} The fetch used {source}.")),
        Err(e) => return Err(format!("failed: {e}")),
    };
    if let Some(forge_branch) = forge_branch {
        record_forge_branch(blocking, tenant, repo, &forge_branch).await;
    }
    repo.default_branch = branch;
    let Some(head) = head else {
        return Err(format!(
            "the repository has no branch {}",
            repo.default_branch
        ));
    };
    Ok(Fetched {
        clone,
        head,
        ancestors,
    })
}

/// The branch that the builder indexes (KAIROS-T-0430): the branch set by
/// hand; else the default branch of the remote, when it gave one and git
/// accepts its form; else the stored branch.
fn indexed_branch<'a>(repo: &'a Repository, forge_branch: Option<&'a str>) -> &'a str {
    match forge_branch {
        Some(branch)
            if repo.follows_forge()
                && kairos_core::repositories::check_default_branch(branch).is_ok() =>
        {
            branch
        }
        _ => &repo.default_branch,
    }
}

/// Write the default branch of the remote to the row of `repo`
/// (KAIROS-T-0430), when it is new. A failed write is logged: the build
/// goes on with the branch it read.
async fn record_forge_branch(
    blocking: &BlockingTenantPool,
    tenant: &str,
    repo: &mut Repository,
    branch: &str,
) {
    let unchanged = repo.forge_branch.as_deref() == Some(branch)
        && (!repo.follows_forge() || repo.default_branch == branch);
    if unchanged || kairos_core::repositories::check_default_branch(branch).is_err() {
        return;
    }
    let repo_id = repo.id;
    let value = branch.to_string();
    match blocking
        .run(tenant, move |conn| {
            kairos_db::repositories::record_forge_branch(conn, repo_id, &value)
                .map_err(ApiError::internal)
        })
        .await
    {
        Ok(row) => {
            if row.default_branch != repo.default_branch {
                tracing::info!(
                    tenant,
                    repository = %repo.slug,
                    from = %repo.default_branch,
                    to = %row.default_branch,
                    "code index follows the new default branch of the forge"
                );
            }
            repo.forge_branch = row.forge_branch;
            repo.default_branch_source = row.default_branch_source;
        }
        Err(e) => {
            tracing::warn!(tenant, repository = %repo.slug, error = ?e, "the default branch of the forge was not recorded");
        }
    }
}

impl CodeIndexService {
    fn failed_before(&self, key: &(uuid::Uuid, String)) -> bool {
        self.failed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(key)
    }

    fn record_failure(&self, key: (uuid::Uuid, String)) {
        self.failed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key);
    }
}

/// Store the index of `head` of `repo`, built by the builder.
/// The counts of a run, from its report (KAIROS-T-0331).
/// The SCIP warnings of a run as the run row keeps them (KAIROS-T-0478): a
/// JSON array of `{"language", "reason"}`, or `None` with no warning. Each
/// warning is in the log too.
fn warnings_of(tenant: &str, slug: &str, report: &UpdateReport) -> Option<String> {
    let warnings = &report.build.scip_warnings;
    for w in warnings {
        tracing::warn!(
            tenant,
            repository = slug,
            language = %w.language,
            reason = %w.reason,
            "code index build: no SCIP index for the language, its edges are by name"
        );
    }
    (!warnings.is_empty())
        .then(|| serde_json::to_string(warnings).ok())
        .flatten()
}

/// Log the summaries that a build could not make (KAIROS-T-0495). The
/// build ends ok; the next build makes them.
fn log_missing(tenant: &str, slug: &str, report: &UpdateReport) {
    if let Some(failure) = &report.summary.failure {
        tracing::warn!(
            tenant,
            repository = slug,
            missing = report.summary.missing(),
            rate_limited = report.summary.rate_limited,
            first = %failure.name,
            error = %failure.message,
            "code index build ended with missing summaries; the next build makes them"
        );
    }
}

fn counts_of(report: &UpdateReport) -> runs::Counts {
    let clamp = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    runs::Counts {
        files: clamp(report.build.files),
        symbols: clamp(report.build.symbols),
        edges: clamp(report.build.edges.total()),
        summaries_made: clamp(
            report.summary.symbols.summarized
                + report.summary.files.summarized
                + report.summary.modules.summarized,
        ),
        summaries_reused: clamp(
            report.summary.symbols.reused
                + report.summary.files.reused
                + report.summary.modules.reused,
        ),
        summaries_missing: clamp(report.summary.missing()),
    }
}

/// Tell the readers of a repository that a run started or ended
/// (KAIROS-T-0333): the event `code_index_build_changed`, with the slug of
/// the repository as its short code. Inside the transaction of the write.
pub(crate) fn notify_run(
    conn: &mut diesel::pg::PgConnection,
    slug: &str,
    actor: Option<uuid::Uuid>,
) -> Result<(), ApiError> {
    kairos_db::events::emit_event(
        conn,
        &kairos_db::events::ThinEvent {
            event: kairos_db::events::EventKind::CodeIndexBuildChanged,
            entity_type: "repository".to_string(),
            short_code: slug.to_string(),
            board_id: None,
            column_id: None,
            actor: actor.unwrap_or_else(uuid::Uuid::nil),
        },
    )
    .map_err(ApiError::internal)
}

/// The slug of the repository of a run, for its event.
fn slug_of_run(conn: &mut diesel::pg::PgConnection, run: &runs::Build) -> Option<String> {
    kairos_db::repositories::load(conn, run.repository_id)
        .ok()
        .map(|repo| repo.slug)
}

/// The start of the note of a repository that waits for the least time
/// between 2 updates (KAIROS-T-0334).
pub const MIN_REBUILD: &str = "the repository was updated a short time ago";

/// The start of the note of a repository that has an open build job
/// (KAIROS-T-0354, KAIROS-T-0374): it gets no second job.
pub const ACTIVE_RUN: &str = "a run of the repository is active";

/// The context of a build that runs as a job (KAIROS-T-0374): the handle
/// of the job, the server that runs it, and the run that the job names,
/// when it names one (the run of a request, or the run of an earlier lease
/// of the job).
#[derive(Clone)]
pub(crate) struct BuildCtx {
    pub handle: JobHandle,
    pub instance: uuid::Uuid,
    pub run: Option<uuid::Uuid>,
}

/// Start the record of a run (KAIROS-T-0331, KAIROS-T-0374): the run that
/// the job names goes on, on this server; else a new `running` row of this
/// server, which the payload of the job then names. The other `running`
/// rows of the repository are stale (no open job names them) and end as
/// failed. One transaction, with the row of the repository locked. `None`
/// when the write failed: the failure is logged, and the build goes on with
/// no record.
async fn record_start(
    blocking: &BlockingTenantPool,
    tenant: &str,
    repo: &Repository,
    trigger: &'static str,
    ctx: &BuildCtx,
) -> Option<uuid::Uuid> {
    let repo_id = repo.id;
    let branch = repo.default_branch.clone();
    let slug = repo.slug.clone();
    let instance = ctx.instance;
    let keep = ctx.run;
    let job = ctx.handle.job().id;
    let written = blocking
        .run(tenant, move |conn| {
            crate::api::org::run_in_transaction(conn, |conn| {
                runs::lock_repository(conn, repo_id).map_err(ApiError::internal)?;
                runs::end_stale_except(conn, repo_id, keep).map_err(ApiError::internal)?;
                if let Some(id) = keep
                    && runs::adopt(conn, id, instance).map_err(ApiError::internal)?
                {
                    return Ok(id);
                }
                let run = runs::start(conn, repo_id, trigger, Some(&branch), None, Some(instance))
                    .map_err(ApiError::internal)?;
                kairos_db::jobs::merge_payload(
                    conn,
                    job,
                    instance,
                    &serde_json::json!({ "run": run.id }),
                )
                .map_err(ApiError::internal)?;
                notify_run(conn, &slug, None)?;
                Ok(run.id)
            })
        })
        .await;
    match written {
        Ok(run) => Some(run),
        Err(e) => {
            tracing::warn!(tenant, repository = %repo.slug, error = ?e, "code index run not recorded");
            None
        }
    }
}

/// Give the record of a run its commit.
async fn record_commit(
    blocking: &BlockingTenantPool,
    tenant: &str,
    run: Option<uuid::Uuid>,
    commit: &str,
) {
    let Some(id) = run else { return };
    let commit = commit.to_string();
    if let Err(e) = blocking
        .run(tenant, move |conn| {
            runs::set_commit(conn, id, &commit).map_err(ApiError::internal)
        })
        .await
    {
        tracing::warn!(tenant, error = ?e, "code index run commit not recorded");
    }
}

/// End the record of a run: `ok` with the counts, or `failed` with the
/// text.
async fn record_end(
    blocking: &BlockingTenantPool,
    tenant: &str,
    run: Option<uuid::Uuid>,
    result: Result<(runs::Counts, String, Option<String>), &str>,
) {
    let Some(id) = run else { return };
    let result = result.map_err(str::to_owned);
    if let Err(e) = blocking
        .run(tenant, move |conn| {
            crate::api::org::run_in_transaction(conn, |conn| {
                let run = match &result {
                    Ok((counts, model, warnings)) => {
                        runs::set_scip_warnings(conn, id, warnings.as_deref())
                            .and_then(|()| runs::end_ok(conn, id, Some(*counts), Some(model)))
                    }
                    Err(error) => runs::end_failed(conn, id, error),
                }
                .map_err(ApiError::internal)?;
                if let Some(slug) = slug_of_run(conn, &run) {
                    notify_run(conn, &slug, run.requested_by)?;
                }
                Ok(())
            })
        })
        .await
    {
        tracing::warn!(tenant, error = ?e, "code index run end not recorded");
    }
}

/// Record a failure that came before the run had a row: the fetch, or the
/// choice of the base. A repeat of the newest failure adds no row
/// ([`runs::record_failure_once`]).
async fn record_failure_once(
    blocking: &BlockingTenantPool,
    tenant: &str,
    repo: &Repository,
    trigger: &'static str,
    commit: Option<&str>,
    error: &str,
) {
    let repo_id = repo.id;
    let branch = repo.default_branch.clone();
    let slug = repo.slug.clone();
    let commit = commit.map(str::to_owned);
    let error = error.to_string();
    if let Err(e) = blocking
        .run(tenant, move |conn| {
            crate::api::org::run_in_transaction(conn, |conn| {
                let written = runs::record_failure_once(
                    conn,
                    repo_id,
                    trigger,
                    commit.as_deref(),
                    Some(&branch),
                    &error,
                )
                .map_err(ApiError::internal)?;
                if written.is_some() {
                    notify_run(conn, &slug, None)?;
                }
                Ok(())
            })
        })
        .await
    {
        tracing::warn!(tenant, repository = %repo.slug, error = ?e, "code index failure not recorded");
    }
}

/// The ADRs that the code of an index cites (KAIROS-T-0703), as the store
/// keeps them.
pub(crate) fn cited_codes(
    cited: &[kairos_index::CitedAdr],
) -> Vec<kairos_db::code_indexes::CitedCode> {
    cited
        .iter()
        .map(|c| kairos_db::code_indexes::CitedCode {
            code: c.code.clone(),
            places: i32::try_from(c.places).unwrap_or(i32::MAX),
            files: i32::try_from(c.files).unwrap_or(i32::MAX),
        })
        .collect()
}

async fn store_built(
    blocking: &BlockingTenantPool,
    tenant: &str,
    repo: &Repository,
    head: String,
    split: kairos_index::store::Split,
) -> Result<(), String> {
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
        citations: cited_codes(&split.citations),
    };
    let pool = used_rows(split.pool, &split.keys);
    let keys = split.keys;
    blocking
        .run(tenant, move |conn| {
            kairos_db::code_indexes::put(conn, &index, &pool, &keys).map_err(ApiError::internal)
        })
        .await
        .map(|_| ())
        .map_err(|e| format!("failed: {e:?}"))
}

/// The update of the index of one repository after a push, as a job
/// (KAIROS-T-0374): from the nearest indexed commit below the head of its
/// default branch.
pub(crate) async fn build_repository(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    tenant: &str,
    mut repo: Repository,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
    ctx: &BuildCtx,
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
    if !repo.code_index_build_on() {
        record_end(blocking, tenant, ctx.run, Err(OPTED_OUT)).await;
        return done(outcome, OPTED_OUT.into());
    }

    let Fetched {
        clone,
        head,
        ancestors,
    } = match fetch_head(blocking, service, tenant, &mut repo, true).await {
        Ok(fetched) => fetched,
        Err(note) => {
            match ctx.run {
                Some(_) => record_end(blocking, tenant, ctx.run, Err(&note)).await,
                None => record_failure_once(blocking, tenant, &repo, "push", None, &note).await,
            }
            return done(outcome, note);
        }
    };
    outcome.commit = head.clone();
    let key = (repo.id, head.clone());
    if service.failed_before(&key) {
        record_end(
            blocking,
            tenant,
            ctx.run,
            Err("an earlier build of this commit failed"),
        )
        .await;
        return done(outcome, "an earlier build of this commit failed".into());
    }

    // The nearest indexed commit, and its structure and the pool. An
    // index of another schema version (a server upgrade bumped it) cannot
    // start an update: the repository gets a full build, in place of a
    // failed update on each tick (KAIROS-T-0347).
    let repo_id = repo.id;
    let start = blocking
        .run(tenant, move |conn| {
            let listed =
                kairos_db::code_indexes::list(conn, repo_id).map_err(ApiError::internal)?;
            let indexed: HashSet<String> = listed.into_iter().map(|i| i.commit_sha).collect();
            let Some(nearest) = ancestors.iter().find(|c| indexed.contains(*c)) else {
                return Ok(None);
            };
            let prefix = kairos_db::code_indexes::structure_prefix(
                conn,
                repo_id,
                nearest,
                kairos_index::store::VERSION_PREFIX_BYTES as i32,
            )
            .map_err(ApiError::internal)?
            .unwrap_or_default();
            if let Some(found) = kairos_index::store::structure_version(&prefix)
                .filter(|found| *found != kairos_index::SCHEMA_VERSION)
            {
                return Ok(Some(Start::OldSchema {
                    commit: nearest.clone(),
                    found,
                }));
            }
            if *nearest == ancestors[0] {
                return Ok(Some(Start::HeadIndexed));
            }
            let structure = kairos_db::code_indexes::structure(conn, repo_id, nearest)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("the index went away"))?;
            let pool =
                kairos_db::code_indexes::whole_pool(conn, repo_id).map_err(ApiError::internal)?;
            Ok(Some(Start::Base(Base {
                commit: nearest.clone(),
                structure,
                pool,
            })))
        })
        .await;
    let base = match start {
        Ok(Some(Start::Base(base))) => base,
        Ok(Some(Start::HeadIndexed)) => {
            record_end(blocking, tenant, ctx.run, Err("the head has an index")).await;
            return done(outcome, "the head has an index".into());
        }
        Ok(Some(Start::OldSchema { commit, found })) => {
            tracing::warn!(
                tenant = %tenant,
                repository = %repo.slug,
                commit = %commit,
                found,
                reads = kairos_index::SCHEMA_VERSION,
                "code index of another schema version: a full build"
            );
            return Box::pin(full_build_repository(
                blocking,
                service,
                tenant,
                repo,
                summarizers,
                embedder,
                "push",
                ctx,
            ))
            .await;
        }
        // No indexed commit is an ancestor of the head within the limit: the
        // history was rewritten, or the index is of a commit that is gone.
        // An update cannot start, so the head gets a full build, in place of
        // a failed update that stays (KAIROS-T-0356).
        Ok(None) => {
            tracing::warn!(
                tenant = %tenant,
                repository = %repo.slug,
                commit = %head,
                limit = MAX_DISTANCE,
                "no indexed commit is below the head: a full build"
            );
            return Box::pin(full_build_repository(
                blocking,
                service,
                tenant,
                repo,
                summarizers,
                embedder,
                "push",
                ctx,
            ))
            .await;
        }
        Err(e) => {
            let note = format!("failed: {e:?}");
            match ctx.run {
                Some(_) => record_end(blocking, tenant, ctx.run, Err(&note)).await,
                None => {
                    record_failure_once(blocking, tenant, &repo, "push", Some(&head), &note).await
                }
            }
            return done(outcome, note);
        }
    };
    outcome.base = Some(base.commit.clone());
    let run = record_start(blocking, tenant, &repo, "push", ctx).await;
    record_commit(blocking, tenant, run, &head).await;
    let models = match models_for(blocking, service, tenant, &repo, &summarizers, &embedder).await {
        Ok(models) => models,
        Err(note) => {
            record_end(blocking, tenant, run, Err(&note)).await;
            return done(outcome, note);
        }
    };
    let model_name = models.summarizers.model_name();

    // The update, off the async threads: it runs rust-analyzer and the
    // model for minutes. The limit of its kind of job keeps the other
    // builds of the model off the CPU (KAIROS-T-0374).
    let db = service.update_db(tenant, &repo);
    let built = {
        let head = head.clone();
        let service = Arc::clone(service);
        let db = db.clone();
        let tenant = tenant.to_string();
        let slug = repo.slug.clone();
        let handle = ctx.handle.clone();
        let sink = PoolSink::new(blocking, &tenant, repo.id);
        let build = service.build_options_for(&tenant, &repo);
        tokio::task::spawn_blocking(move || -> Result<_, String> {
            let work = tempfile::Builder::new()
                .prefix("kairos-code-index-")
                .tempdir()
                .map_err(|e| format!("no work folder: {e}"))?;
            let tree = work.path().join("tree");
            std::fs::create_dir(&tree).map_err(|e| format!("no tree folder: {e}"))?;
            let update = UpdateOptions {
                // The server runs SCIP for each push (COLLIERY-I-0264,
                // decision 5 of 2026-10-01).
                rust_edges: true,
                build,
                ..Default::default()
            };
            let build = with_progress(&handle, || -> Result<_, String> {
                prepare_update_db(&db, &base.structure, base.pool, &tenant, &slug)?;
                git::checkout_tree(&clone, &head, &tree).map_err(|e| e.to_string())?;
                kairos_index::update_structure(&tree, &db, &update).map_err(|e| e.to_string())
            })?;
            let Models {
                summarizers,
                embedder,
            } = models;
            let report = summarizers
                .run(&mut |summarizer| {
                    let mut summarizer = ProgressSummarizer::new(summarizer, &handle, &sink);
                    let embedder = ProgressEmbedder {
                        inner: embedder.as_ref(),
                        handle: &handle,
                    };
                    let (report, changed) = kairos_index::summarize_or_rebuild(
                        &tree,
                        &db,
                        &mut summarizer,
                        &embedder,
                        &update,
                        &build,
                    )?;
                    if let Some(file) = changed {
                        tracing::warn!(
                            tenant,
                            repository = slug,
                            file,
                            "code index build: a file changed after the build of the structure; the structure was built again"
                        );
                    }
                    Ok(report)
                })
                .map_err(|e| e.to_string())?;
            let split = kairos_index::store::split(&db).map_err(|e| e.to_string())?;
            Ok((report, split))
        })
        .await
    };
    let (report, split) = match built {
        Ok(Ok(built)) => built,
        // The job was killed: its run goes on when a server takes the job
        // again.
        Ok(Err(_)) if ctx.handle.cancelled() => return done(outcome, STOPPED.into()),
        Ok(Err(e)) => {
            service.record_failure(key);
            let note = format!("failed: {e}");
            record_end(blocking, tenant, run, Err(&note)).await;
            return done(outcome, note);
        }
        Err(e) => {
            let note = format!("failed: {e}");
            record_end(blocking, tenant, run, Err(&note)).await;
            return done(outcome, note);
        }
    };
    if let Err(note) = store_built(blocking, tenant, &repo, head, split).await {
        record_end(blocking, tenant, run, Err(&note)).await;
        return done(outcome, note);
    }
    log_missing(tenant, &repo.slug, &report);
    record_end(
        blocking,
        tenant,
        run,
        Ok((
            counts_of(&report),
            model_name,
            warnings_of(tenant, &repo.slug, &report),
        )),
    )
    .await;
    remove_database(&db);
    outcome.report = Some(report);
    outcome.elapsed = started.elapsed();
    outcome
}

/// Write the work database `db` of an update (KAIROS-T-0335): the
/// structure of the base and the pool of the repository, and the summaries
/// of the work database of an update that did not end, when `db` has one.
/// Those summaries are of keys that no index stores yet: the model made
/// them before a restart or a failure. The error is the text of a failed
/// run.
fn prepare_update_db(
    db: &Path,
    structure: &[u8],
    pool: Vec<kairos_db::code_indexes::PoolRow>,
    tenant: &str,
    slug: &str,
) -> Result<(), String> {
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("no folder {}: {e}", parent.display()))?;
    }
    let mut next = db.as_os_str().to_owned();
    next.push(".next");
    let next = PathBuf::from(next);
    remove_database(&next);
    kairos_index::store::assemble(structure, pool.into_iter().map(pool_row_of), &next)
        .map_err(|e| e.to_string())?;
    if db.is_file() {
        // A work database that cannot give its summaries (another schema
        // version, the vectors of another model) is not used.
        match kairos_index::store::copy_pool(db, &next) {
            Ok(()) => tracing::info!(
                tenant,
                repository = slug,
                "code index update reuses the summaries of an update that did not end"
            ),
            Err(e) => tracing::warn!(
                tenant,
                repository = slug,
                error = %e,
                "code index update cannot reuse the summaries of an update that did not end"
            ),
        }
    }
    remove_database(db);
    std::fs::rename(&next, db).map_err(|e| format!("no work database {}: {e}", db.display()))
}

/// A full build of one repository, as a job (KAIROS-T-0374): the first
/// build of a repository that has no index (KAIROS-T-0318, `trigger =
/// first`), a build that a person asked for (KAIROS-T-0332, `trigger =
/// request`, with the run row that the request made), or the full build of
/// an update that cannot start from an index (`trigger = push`). A
/// requested build skips the memory of the commits that failed before: the
/// person asks for one more try. A first build gives its slot to a job of
/// a higher priority when [`JobHandle::should_yield`] says so: it ends
/// with the note [`YIELDED`], and its run stays `running` for the next
/// lease of its job.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn full_build_repository(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    tenant: &str,
    mut repo: Repository,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
    trigger: &'static str,
    ctx: &BuildCtx,
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
    if !repo.code_index_build_on() {
        record_end(blocking, tenant, ctx.run, Err(OPTED_OUT)).await;
        return done(outcome, OPTED_OUT.into());
    }
    // A first-build job that a sweep added from a list it read before the
    // first build of the repository ended (KAIROS-T-0378): the repository
    // has an index now, and the updates follow it.
    if trigger == jobs::FIRST {
        let repo_id = repo.id;
        let indexed = blocking
            .run(tenant, move |conn| {
                kairos_db::code_indexes::list(conn, repo_id).map_err(ApiError::internal)
            })
            .await
            .map(|listed| !listed.is_empty())
            .unwrap_or(false);
        if indexed {
            record_end(blocking, tenant, ctx.run, Err(HAS_INDEX)).await;
            return done(outcome, HAS_INDEX.into());
        }
    }

    let Fetched { clone, head, .. } = match fetch_head(blocking, service, tenant, &mut repo, false)
        .await
    {
        Ok(fetched) => fetched,
        Err(note) => {
            match ctx.run {
                Some(_) => record_end(blocking, tenant, ctx.run, Err(&note)).await,
                None => record_failure_once(blocking, tenant, &repo, trigger, None, &note).await,
            }
            return done(outcome, note);
        }
    };
    outcome.commit = head.clone();
    let key = (repo.id, head.clone());
    if trigger != "request" && service.failed_before(&key) {
        record_end(
            blocking,
            tenant,
            ctx.run,
            Err("an earlier build of this commit failed"),
        )
        .await;
        return done(outcome, "an earlier build of this commit failed".into());
    }
    tracing::info!(
        tenant = %tenant,
        repository = %repo.slug,
        commit = %head,
        trigger,
        "code index full build started"
    );
    let run = record_start(blocking, tenant, &repo, trigger, ctx).await;
    record_commit(blocking, tenant, run, &head).await;
    let models = match models_for(blocking, service, tenant, &repo, &summarizers, &embedder).await {
        Ok(models) => models,
        Err(note) => {
            record_end(blocking, tenant, run, Err(&note)).await;
            return done(outcome, note);
        }
    };
    let model_name = models.summarizers.model_name();

    // The pool of the repository in Postgres (KAIROS-T-0378): the summaries
    // that an earlier lease of this job made, on any server, and those of
    // the indexes of the repository. The build links them first, so it
    // makes only the rest. A vector of another model than the one of this
    // build is left out: the build embeds that summary again.
    let pool = match linkable_pool(blocking, tenant, repo.id, models.embedder.as_ref()).await {
        Ok(pool) => pool,
        Err(note) => {
            record_end(blocking, tenant, run, Err(&note)).await;
            return done(outcome, note);
        }
    };

    // The build, off the async threads: it runs for hours on a large
    // repository. It reports progress for each summary and each embedding
    // batch, and stops at the next one when it must give its slot.
    let db = service.first_build_db(tenant, &repo);
    let built = {
        let head = head.clone();
        let service = Arc::clone(service);
        let db = db.clone();
        let handle = ctx.handle.clone();
        let sink = PoolSink::new(blocking, tenant, repo.id);
        let (tenant, slug) = (tenant.to_string(), repo.slug.clone());
        let build = service.build_options_for(&tenant, &repo);
        tokio::task::spawn_blocking(move || -> Result<_, String> {
            if let Some(parent) = db.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("no folder {}: {e}", parent.display()))?;
            }
            let work = tempfile::Builder::new()
                .prefix("kairos-code-index-first-")
                .tempdir()
                .map_err(|e| format!("no work folder: {e}"))?;
            let tree = work.path().join("tree");
            std::fs::create_dir(&tree).map_err(|e| format!("no tree folder: {e}"))?;
            let update = UpdateOptions {
                rust_edges: true,
                build,
                ..Default::default()
            };
            let build = with_progress(&handle, || -> Result<_, String> {
                git::checkout_tree(&clone, &head, &tree).map_err(|e| e.to_string())?;
                match kairos_index::update_structure(&tree, &db, &update) {
                    Ok(build) => Ok(build),
                    Err(e) => {
                        // A work database that the build cannot read (for
                        // example of another schema version) is removed, so
                        // that the next build starts again from nothing.
                        remove_database(&db);
                        Err(e.to_string())
                    }
                }
            })?;
            match kairos_index::store::add_pool(&db, pool) {
                Ok(0) => {}
                Ok(added) => tracing::info!(
                    tenant,
                    repository = slug,
                    added,
                    "code index full build links the summaries of the pool"
                ),
                Err(e) => return Err(format!("the pool of the repository: {e}")),
            }
            let Models {
                summarizers,
                embedder,
            } = models;
            let report = summarizers
                .run(&mut |summarizer| {
                    let mut summarizer = ProgressSummarizer::new(summarizer, &handle, &sink);
                    let embedder = ProgressEmbedder {
                        inner: embedder.as_ref(),
                        handle: &handle,
                    };
                    let (report, changed) = kairos_index::summarize_or_rebuild(
                        &tree,
                        &db,
                        &mut summarizer,
                        &embedder,
                        &update,
                        &build,
                    )?;
                    if let Some(file) = changed {
                        tracing::warn!(
                            tenant,
                            repository = slug,
                            file,
                            "code index build: a file changed after the build of the structure; the structure was built again"
                        );
                    }
                    Ok(report)
                })
                .map_err(|e| e.to_string())?;
            let split = kairos_index::store::split(&db).map_err(|e| e.to_string())?;
            Ok((report, split))
        })
        .await
    };
    let (report, split) = match built {
        Ok(Ok(built)) => built,
        // The job was killed: its run goes on when a server takes the job
        // again.
        Ok(Err(_)) if ctx.handle.cancelled() => return done(outcome, STOPPED.into()),
        // The build gives its slot: the run stays, and the summaries stay
        // in the work database for the next lease of the job.
        Ok(Err(e)) if ctx.handle.should_yield() && e.contains(YIELDED) => {
            return done(outcome, YIELDED.into());
        }
        Ok(Err(e)) => {
            service.record_failure(key);
            let note = format!("failed: {e}");
            record_end(blocking, tenant, run, Err(&note)).await;
            return done(outcome, note);
        }
        Err(e) => {
            let note = format!("failed: {e}");
            record_end(blocking, tenant, run, Err(&note)).await;
            return done(outcome, note);
        }
    };
    if let Err(note) = store_built(blocking, tenant, &repo, head, split).await {
        record_end(blocking, tenant, run, Err(&note)).await;
        return done(outcome, note);
    }
    log_missing(tenant, &repo.slug, &report);
    record_end(
        blocking,
        tenant,
        run,
        Ok((
            counts_of(&report),
            model_name,
            warnings_of(tenant, &repo.slug, &report),
        )),
    )
    .await;
    remove_database(&db);
    outcome.report = Some(report);
    outcome.elapsed = started.elapsed();
    outcome
}

/// The rows of the pool of `repository` in Postgres for a build whose
/// vectors come from `embedder` (KAIROS-T-0378): each row, with its vector
/// when the vectors of the pool are of the model of `embedder`. The error
/// is the note of a failed run.
async fn linkable_pool(
    blocking: &BlockingTenantPool,
    tenant: &str,
    repository: uuid::Uuid,
    embedder: &dyn kairos_embed::EmbeddingProvider,
) -> Result<Vec<kairos_index::store::PoolRow>, String> {
    let model = kairos_index::vector_model_name(embedder);
    let (rows, pool_model) = blocking
        .run(tenant, move |conn| {
            let rows = kairos_db::code_indexes::whole_pool(conn, repository)
                .map_err(ApiError::internal)?;
            let model = kairos_db::code_indexes::vector_model(conn, repository)
                .map_err(ApiError::internal)?;
            Ok((rows, model))
        })
        .await
        .map_err(|e| format!("failed: the pool of the repository is not readable: {e:?}"))?;
    let keep_vectors = pool_model.is_none_or(|pool| pool == model);
    Ok(rows
        .into_iter()
        .map(|row| {
            let mut row = pool_row_of(row);
            if !keep_vectors {
                row.vector = None;
            }
            row
        })
        .collect())
}

/// Remove the SQLite database at `path` and its journal files. A file that
/// is not there is not an error.
fn remove_database(path: &Path) {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(file));
    }
}

/// Whether the summaries of `repo` come from the hosted provider of the
/// tenant: its own value, or the default of the organization when it
/// follows the organization (KAIROS-T-0358), and the tenant has a hosted
/// provider. The ONE rule, for the builder and for the API.
pub(crate) fn hosted_summaries(
    repo: &Repository,
    settings: &kairos_db::code_index_settings::Settings,
) -> bool {
    repo.summaries_with(&settings.default_summaries) == "hosted"
        && settings.summary_provider != "embedded"
}

/// A pool row of the database as `kairos-index` takes it.
pub(crate) fn pool_row_of(row: kairos_db::code_indexes::PoolRow) -> kairos_index::store::PoolRow {
    kairos_index::store::PoolRow {
        key: row.key,
        level: row.level,
        summary: row.summary,
        vector: row.vector,
        model: row.model,
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
            model: row.model,
        })
        .collect()
}

#[cfg(all(test, feature = "llama"))]
mod tests {
    use super::*;

    #[test]
    fn the_builder_summarizes_on_the_threads_of_the_setting() {
        let tools = Tools {
            model: Some(PathBuf::from("/tools/model.gguf")),
            rust_analyzer: PathBuf::from("/tools/rust-analyzer"),
            rust_src: PathBuf::from("/tools/rust-src.tar.gz"),
            sysroot: PathBuf::from("/tools/rust"),
            downloaded: Vec::new(),
            hashed: Vec::new(),
        };
        let summarizers = llama_summarizers(&tools, 6);
        assert_eq!(summarizers.model, PathBuf::from("/tools/model.gguf"));
        assert_eq!(summarizers.threads, 6);
    }

    /// KAIROS-T-0486: the uv folder counts for Python in the disk use.
    #[test]
    fn each_tool_folder_has_its_language() {
        assert_eq!(tool_language("uv-0.13.1"), Some("python"));
        assert_eq!(tool_language("python-3.13.16"), Some("python"));
        assert_eq!(tool_language("scip-go-0.2.7"), Some("go"));
        assert_eq!(tool_language("other"), None);
    }
}
