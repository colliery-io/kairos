//! The code-index builds as jobs of the job queue (KAIROS-T-0374,
//! COLLIERY-I-0613).
//!
//! A build job has the kind of its model: `embedded_build` or
//! `hosted_build` ([`crate::jobs::kinds`]), so the limit of each kind on a
//! server replaces the gate of KAIROS-T-0318. Its payload names the
//! repository, the trigger (`request`, `push` for an update, `first`) and,
//! once the build has a run row, the run. Its priority comes from the
//! trigger ([`crate::jobs::priority`]). Its dedupe key is the repository
//! ([`job_key`]): one open build job for each repository, so a repository
//! never has 2 runs at the same time (KAIROS-T-0354).
//!
//! [`run_builder`] adds the periodic pass of the new-commits sweep
//! (KAIROS-T-0376, [`crate::periodic::CODE_INDEX_SWEEP`]): one run at each
//! interval for all the servers, which adds the update jobs
//! ([`enqueue_updates`]) and the first-build jobs ([`enqueue_first_builds`]).
//! The API adds a requested build
//! ([`crate::api::org::code_indexes::request_rebuild`]).
//!
//! A build job is not run again when its lease ran out or the watchdog
//! killed it ([`JobHandler::retry_stopped`]): the breaker of its repository
//! trips (KAIROS-T-0399), and the sweep skips the repository until a person
//! asks for a build.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use kairos_db::code_index_breakers as breakers;
use kairos_db::code_index_builds as runs;
use kairos_db::jobs::{self as queue, Job, NewJob};
use kairos_db::models::repositories::Repository;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    ACTIVE_RUN, BREAKER_TRIPPED, BuildCtx, BuildOutcome, CodeIndexService, Fetched, HAS_INDEX,
    MIN_REBUILD, OPTED_OUT, Pass, STOPPED, SummarizerSource, Target, YIELDED, build_repository,
    fetch_head, full_build_repository, notify_run, outage_note, record_failure_once, record_refs,
    repositories_of, update_order,
};
use crate::blocking::BlockingTenantPool;
use crate::error::ApiError;
use crate::jobs::{
    JobError, JobFuture, JobHandle, JobHandler, JobSettings, UnitFuture, Worker, kinds, priority,
};

/// The trigger of a requested build (KAIROS-T-0332).
pub const REQUEST: &str = "request";
/// The trigger of an update after a push.
pub const PUSH: &str = "push";
/// The trigger of a first build (KAIROS-T-0318).
pub const FIRST: &str = "first";

/// The dedupe key of the build jobs of a repository.
pub fn job_key(tenant: &str, repository: Uuid) -> String {
    format!("code-index:{tenant}:{repository}")
}

/// The payload of a build job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildPayload {
    pub repository_id: Uuid,
    /// `request`, `push` or `first`.
    pub trigger: String,
    /// The run of the job, once it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Uuid>,
}

fn priority_of(trigger: &str) -> i32 {
    match trigger {
        REQUEST => priority::REQUESTED_BUILD,
        PUSH => priority::UPDATE,
        _ => priority::FIRST_BUILD,
    }
}

/// The kind of the build jobs of `repo`: a build with a hosted summary
/// provider, or with the embedded model. Read from the settings of the
/// tenant of `conn`.
pub(crate) fn build_kind(
    conn: &mut PgConnection,
    repo: &Repository,
) -> Result<&'static str, ApiError> {
    let settings =
        kairos_db::code_index_settings::load_or_default(conn).map_err(ApiError::internal)?;
    Ok(if super::hosted_summaries(repo, &settings) {
        kinds::HOSTED_BUILD
    } else {
        kinds::EMBEDDED_BUILD
    })
}

/// Add a build job of `repo`, on a connection of the tenant `tenant`.
/// `Err(job)`: the repository has an open build job, and nothing was added.
pub(crate) fn enqueue_build(
    conn: &mut PgConnection,
    tenant: &str,
    repo: &Repository,
    trigger: &str,
    run: Option<Uuid>,
    run_after: Option<DateTime<Utc>>,
    max_tries: i32,
) -> Result<Result<Job, Job>, ApiError> {
    let kind = build_kind(conn, repo)?;
    let payload = BuildPayload {
        repository_id: repo.id,
        trigger: trigger.to_string(),
        run,
    };
    let job = NewJob {
        kind: kind.to_string(),
        tenant: Some(tenant.to_string()),
        payload: serde_json::to_value(&payload).map_err(ApiError::internal)?,
        priority: priority_of(trigger),
        dedupe_key: Some(job_key(tenant, repo.id)),
        run_after,
        max_tries,
    };
    // A job that ends between the insert and the read: try once more.
    for _ in 0..2 {
        if let Some(added) = queue::enqueue(conn, &job).map_err(ApiError::internal)? {
            return Ok(Ok(added));
        }
        if let Some(open) =
            queue::open_by_key(conn, &job_key(tenant, repo.id)).map_err(ApiError::internal)?
        {
            return Ok(Err(open));
        }
    }
    Err(ApiError::internal(
        "the build job is neither added nor open",
    ))
}

/// Cancel the build jobs of `repo` that wait for a server, on a connection
/// of the tenant `tenant` (KAIROS-T-0398): `code_index_build` was set to
/// off. A job that a server runs now goes on; its next try, if any, stops
/// at the switch ([`super::OPTED_OUT`]). The run of each cancelled job ends
/// as failed. The number of cancelled jobs.
pub(crate) fn cancel_builds(
    conn: &mut PgConnection,
    tenant: &str,
    repo: &Repository,
) -> Result<usize, ApiError> {
    let cancelled = queue::cancel_waiting(conn, &job_key(tenant, repo.id), super::TURNED_OFF)
        .map_err(ApiError::internal)?;
    for job in &cancelled {
        let Some(run) = serde_json::from_str::<BuildPayload>(&job.payload)
            .ok()
            .and_then(|p| p.run)
        else {
            continue;
        };
        if runs::get(conn, run)
            .map_err(ApiError::internal)?
            .is_some_and(|found| found.outcome == "running")
        {
            let ended =
                runs::end_failed(conn, run, super::TURNED_OFF).map_err(ApiError::internal)?;
            notify_run(conn, &repo.slug, ended.requested_by)?;
        }
    }
    Ok(cancelled.len())
}

/// The note of a repository whose breaker is tripped (KAIROS-T-0399).
fn breaker_note(breaker: &breakers::Breaker) -> String {
    format!(
        "{BREAKER_TRIPPED} since {}: {}",
        breaker.tripped_at.to_rfc3339(),
        breaker.reason
    )
}

/// The tripped breaker of the repository `repo` of `tenant`. An error is
/// logged, and reads as not tripped: the build job checks again.
async fn tripped(
    blocking: &BlockingTenantPool,
    tenant: &str,
    repo: Uuid,
) -> Option<breakers::Breaker> {
    match blocking
        .run(tenant, move |conn| {
            breakers::get(conn, repo).map_err(ApiError::internal)
        })
        .await
    {
        Ok(found) => found,
        Err(e) => {
            tracing::warn!(tenant, repository = %repo, error = ?e, "code index breaker not read");
            None
        }
    }
}

/// The note of a repository whose build job `job` is open.
fn open_note(job: &Job) -> String {
    let payload: Option<BuildPayload> = serde_json::from_str(&job.payload).ok();
    let trigger = payload
        .as_ref()
        .map(|p| p.trigger.clone())
        .unwrap_or_default();
    let run = payload
        .and_then(|p| p.run)
        .map(|run| format!(", run {run}"))
        .unwrap_or_default();
    format!(
        "{ACTIVE_RUN}: the build job {} is {} (trigger {trigger}{run})",
        job.id, job.state
    )
}

fn note_of(tenant: &str, repo: &Repository, commit: &str, note: String) -> BuildOutcome {
    BuildOutcome {
        tenant: tenant.to_string(),
        repository: repo.slug.clone(),
        commit: commit.to_string(),
        base: None,
        report: None,
        note: Some(note),
        elapsed: Duration::ZERO,
    }
}

/// The index of the head of a repository, for the sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeadIndex {
    /// An index of the schema version of this server: an update has
    /// nothing to do.
    Current,
    /// An index of another schema version, as a server upgrade leaves
    /// behind (KAIROS-T-0746): the server reads none of it, so the build
    /// does not wait for the least time between 2 updates.
    OtherSchema,
    Missing,
}

/// The index of each commit of `commits` of `repo` (KAIROS-T-0750: the
/// targets of one sweep, in one read). A commit that is not in the result
/// has no index; an error reads as no index for each.
async fn indexes_of(
    blocking: &BlockingTenantPool,
    tenant: &str,
    repo: Uuid,
    commits: &[String],
) -> std::collections::HashMap<String, HeadIndex> {
    let commits = commits.to_vec();
    blocking
        .run(tenant, move |conn| {
            let listed = kairos_db::code_indexes::list(conn, repo).map_err(ApiError::internal)?;
            let mut out = std::collections::HashMap::new();
            for commit in commits {
                if !listed.iter().any(|i| i.commit_sha == commit) {
                    out.insert(commit, HeadIndex::Missing);
                    continue;
                }
                let found =
                    super::stored_schema(conn, repo, &commit).map_err(ApiError::internal)?;
                let at = match found {
                    Some(found) if found != kairos_index::SCHEMA_VERSION => HeadIndex::OtherSchema,
                    _ => HeadIndex::Current,
                };
                out.insert(commit, at);
            }
            Ok(out)
        })
        .await
        .unwrap_or_default()
}

/// The new-commits sweep: fetch each repository that has an index, and add
/// an update job when the head of its default branch has no index. The
/// `run_after` of the job is the time of the newest index of the
/// repository, so the repository that has been stale longest goes first
/// (KAIROS-T-0334). A repository whose fetch fails gets no job
/// (KAIROS-T-0753): the sweep records the failure once, and waits before
/// it tries the repository again, longer after each failure. A note for
/// each repository that got no job.
pub async fn enqueue_updates(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    worker: &Worker,
) -> Vec<BuildOutcome> {
    enqueue_updates_with(blocking, service, worker, None).await
}

/// [`enqueue_updates`], with the progress of the periodic job of the sweep
/// for each repository (KAIROS-T-0376).
async fn enqueue_updates_with(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    worker: &Worker,
    progress: Option<&JobHandle>,
) -> Vec<BuildOutcome> {
    let max_tries = worker.settings().max_tries;
    let mut notes = Vec::new();
    // KAIROS-T-0753: the fetches of this pass, by tenant.
    let mut fetches: HashMap<String, PassFetches> = HashMap::new();
    for (tenant, mut repo, newest) in update_order(blocking).await {
        if let Some(handle) = progress {
            handle.progress();
        }
        if !repo.code_index_build_on() {
            notes.push(note_of(&tenant, &repo, "", OPTED_OUT.into()));
            continue;
        }
        // KAIROS-T-0753: a repository whose fetch failed waits before the
        // sweep tries it again; no job in the meantime.
        if let Some(left) = service.fetch_wait(repo.id) {
            let note = format!("{FETCH_WAITS}: the next try is in {} s", left.as_secs());
            if service.note_changed(repo.id, FETCH_WAITS) {
                tracing::info!(tenant = %tenant, repository = %repo.slug, %note, "code index update waits");
            }
            notes.push(note_of(&tenant, &repo, "", note));
            continue;
        }
        if let Some(breaker) = tripped(blocking, &tenant, repo.id).await {
            let note = breaker_note(&breaker);
            if service.note_changed(repo.id, &note) {
                tracing::info!(tenant = %tenant, repository = %repo.slug, %note, "code index update not added");
            }
            notes.push(note_of(&tenant, &repo, "", note));
            continue;
        }
        let mut commit = String::new();
        fetches.entry(tenant.clone()).or_default().attempted += 1;
        let fetched = match fetch_head(blocking, service, &tenant, &mut repo, false).await {
            Ok(fetched) => Some(fetched),
            // KAIROS-T-0753: no job for a repository that the server cannot
            // fetch; one run row for each distinct failure; the wait is set
            // by `fetch_head`. The breaker stays as it is (KAIROS-T-0399).
            Err(why) => {
                record_failure_once(blocking, &tenant, &repo, PUSH, None, &why).await;
                let wait = service.fetch_wait(repo.id).unwrap_or_default();
                fetches
                    .entry(tenant.clone())
                    .or_default()
                    .failed
                    .push(FetchFailure {
                        repo: repo.id,
                        slug: repo.slug.clone(),
                        note: format!("{why} The next try is in {} s.", wait.as_secs()),
                    });
                notes.push(note_of(&tenant, &repo, "", why));
                continue;
            }
        };
        if let Some(Fetched {
            head,
            targets,
            present,
            ..
        }) = fetched
        {
            // KAIROS-T-0750: the head of the default branch, then the tags
            // and the pull request heads in the window of the builder. A
            // target whose commit has a current index gets its ref
            // recorded here; the others need a build.
            let commits: Vec<String> = targets.iter().map(|t| t.commit.clone()).collect();
            let indexed = indexes_of(blocking, &tenant, repo.id, &commits).await;
            let mut needs: Vec<(&Target, HeadIndex)> = Vec::new();
            let mut recorded: Vec<&Target> = Vec::new();
            let mut head_failed = false;
            for target in &targets {
                if service.failed_before(&(repo.id, target.commit.clone())) {
                    head_failed |= target.commit == head;
                    continue;
                }
                let at = indexed
                    .get(&target.commit)
                    .copied()
                    .unwrap_or(HeadIndex::Missing);
                if at == HeadIndex::Current {
                    recorded.push(target);
                } else {
                    needs.push((target, at));
                }
            }
            record_refs(blocking, &tenant, &repo, &recorded, Some(&present)).await;
            if needs.is_empty() {
                let note = if head_failed {
                    "an earlier build of this commit failed"
                } else {
                    "the head has an index"
                };
                notes.push(note_of(&tenant, &repo, &head, note.into()));
                continue;
            }
            // The least time between 2 updates of one repository
            // (KAIROS-T-0334): a repository updated a short time ago waits
            // for a later pass. A commit with an index of another schema
            // version does not wait (KAIROS-T-0746): after an upgrade, no
            // read of the server can use that index.
            let all_missing = needs.iter().all(|(_, at)| *at == HeadIndex::Missing);
            if let (Some(min), Some(newest), true) = (service.min_rebuild(), newest, all_missing) {
                let age = (Utc::now() - newest).to_std().unwrap_or_default();
                if age < min {
                    let note = format!(
                        "{MIN_REBUILD}: the newest index is {} s old, and \
                         KAIROS_CODE_INDEX_MIN_REBUILD_SECS is {}",
                        age.as_secs(),
                        min.as_secs()
                    );
                    tracing::debug!(tenant = %tenant, repository = %repo.slug, %note, "code index update waits");
                    notes.push(note_of(&tenant, &repo, &head, note));
                    continue;
                }
            }
            commit = needs[0].0.commit.clone();
        }
        let (slug, added) = {
            let tenant = tenant.clone();
            let repo = repo.clone();
            let slug = repo.slug.clone();
            let added = blocking
                .run(&tenant.clone(), move |conn| {
                    enqueue_build(conn, &tenant, &repo, PUSH, None, newest, max_tries)
                })
                .await;
            (slug, added)
        };
        match added {
            Ok(Ok(job)) => {
                tracing::info!(tenant = %tenant, repository = %slug, job = %job.id, kind = %job.kind, "code index update job added")
            }
            Ok(Err(open)) => {
                let note = open_note(&open);
                tracing::info!(tenant = %tenant, repository = %slug, %note, "code index update job not added");
                notes.push(note_of(&tenant, &repo, &commit, note));
            }
            Err(e) => {
                tracing::warn!(tenant = %tenant, repository = %slug, error = ?e, "code index update job not added")
            }
        }
    }
    // KAIROS-T-0753: one warning for an outage of the forge; else one for
    // each repository whose fetch failed, once per change of the text.
    for (tenant, PassFetches { attempted, failed }) in &fetches {
        if let Some(note) = outage_note(failed.len(), *attempted) {
            let repositories: Vec<&str> = failed.iter().map(|f| f.slug.as_str()).collect();
            tracing::warn!(tenant = %tenant, repositories = ?repositories, %note, "code index fetch failed");
            continue;
        }
        for failure in failed {
            if service.note_changed(failure.repo, &failure.note) {
                tracing::warn!(tenant = %tenant, repository = %failure.slug, note = %failure.note, "code index fetch failed");
            }
        }
    }
    worker.wake();
    notes
}

/// The fetches of one pass of the sweep in one tenant (KAIROS-T-0753).
#[derive(Default)]
struct PassFetches {
    /// The repositories that the pass fetched (a waiting one is not one).
    attempted: usize,
    failed: Vec<FetchFailure>,
}

/// A repository whose fetch failed in a pass (KAIROS-T-0753).
struct FetchFailure {
    repo: Uuid,
    slug: String,
    /// The text of the failure and the next try.
    note: String,
}

/// The note of a repository that waits after a failed fetch (KAIROS-T-0753).
pub const FETCH_WAITS: &str = "the fetch of the repository failed and the sweep waits";

/// Add a first-build job for each repository that has no index
/// (KAIROS-T-0318), with [`enqueue_first_build`]. A note for each
/// repository that got no job.
pub async fn enqueue_first_builds(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    worker: &Worker,
) -> Vec<BuildOutcome> {
    let mut notes = Vec::new();
    for (tenant, repos) in repositories_of(blocking, Pass::FirstBuilds).await {
        for repo in repos {
            if let Some(note) = enqueue_first_build(blocking, service, worker, &tenant, &repo).await
            {
                notes.push(note);
            }
        }
    }
    worker.wake();
    notes
}

/// Add the first-build job of `repo` of `tenant`, a repository that had no
/// index when the sweep read the list. The `run_after` of the job is the
/// time at which the repository was registered. The note of the
/// repository when it got no job.
pub async fn enqueue_first_build(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    worker: &Worker,
    tenant: &str,
    repo: &Repository,
) -> Option<BuildOutcome> {
    let max_tries = worker.settings().max_tries;
    if !repo.code_index_build_on() {
        if service.note_changed(repo.id, OPTED_OUT) {
            tracing::info!(tenant = %tenant, repository = %repo.slug, note = OPTED_OUT, "code index first build not started");
        }
        return Some(note_of(tenant, repo, "", OPTED_OUT.into()));
    }
    // KAIROS-T-0753: the fetch of the first build failed; the sweep waits.
    if let Some(left) = service.fetch_wait(repo.id) {
        let note = format!("{FETCH_WAITS}: the next try is in {} s", left.as_secs());
        if service.note_changed(repo.id, FETCH_WAITS) {
            tracing::info!(tenant = %tenant, repository = %repo.slug, %note, "code index first build waits");
        }
        return Some(note_of(tenant, repo, "", note));
    }
    if let Some(breaker) = tripped(blocking, tenant, repo.id).await {
        let note = breaker_note(&breaker);
        if service.note_changed(repo.id, &note) {
            tracing::info!(tenant = %tenant, repository = %repo.slug, %note, "code index first build not started");
        }
        return Some(note_of(tenant, repo, "", note));
    }
    let added = {
        let tenant_c = tenant.to_string();
        let repo = repo.clone();
        let at = repo.created_at;
        blocking
            .run(tenant, move |conn| {
                // KAIROS-T-0450: the list that names the repository was read
                // before this call, and the first build of another server
                // can end in between: its job is done, so the dedupe key no
                // longer blocks. Read the open job FIRST, then the index. A
                // build stores its index before its job ends, so when no job
                // is open here, the index of a build that ended is visible.
                if let Some(open) = queue::open_by_key(conn, &job_key(&tenant_c, repo.id))
                    .map_err(ApiError::internal)?
                {
                    return Ok(Err(Some(open)));
                }
                if !kairos_db::code_indexes::list(conn, repo.id)
                    .map_err(ApiError::internal)?
                    .is_empty()
                {
                    return Ok(Err(None));
                }
                Ok(
                    enqueue_build(conn, &tenant_c, &repo, FIRST, None, Some(at), max_tries)?
                        .map_err(Some),
                )
            })
            .await
    };
    match added {
        Ok(Ok(job)) => {
            tracing::debug!(tenant = %tenant, repository = %repo.slug, job = %job.id, "code index first-build job added");
            None
        }
        Ok(Err(Some(open))) => Some(note_of(tenant, repo, "", open_note(&open))),
        Ok(Err(None)) => Some(note_of(tenant, repo, "", HAS_INDEX.into())),
        Err(e) => {
            tracing::warn!(tenant = %tenant, repository = %repo.slug, error = ?e, "code index first-build job not added");
            None
        }
    }
}

/// End the `running` runs of each tenant that no open job names, when the
/// builder of a server starts (KAIROS-T-0331, KAIROS-T-0354,
/// KAIROS-T-0374): their server stopped before this version, or their job
/// ended without them. A run of a job that a server runs, or that waits for
/// a server, stays.
pub async fn end_stale_runs(blocking: &BlockingTenantPool) {
    let tenants = match blocking
        .run_public(|conn| kairos_db::list_tenants(conn).map_err(ApiError::internal))
        .await
    {
        Ok(tenants) => tenants,
        Err(e) => {
            tracing::warn!(error = ?e, "code index builder could not list tenants");
            return;
        }
    };
    for tenant in tenants.into_iter().filter(|t| t.schema_exists) {
        let slug = tenant.slug.clone();
        match blocking
            .run(&tenant.slug, move |conn| {
                runs::end_without_job(conn, &slug).map_err(ApiError::internal)
            })
            .await
        {
            Ok(0) => {}
            Ok(n) => {
                tracing::info!(tenant = %tenant.slug, runs = n, "code index runs ended: no job runs them")
            }
            Err(e) => {
                tracing::warn!(tenant = %tenant.slug, error = ?e, "code index stale runs not ended")
            }
        }
    }
}

/// The time after the end of a sweep run, on top of 2 intervals of the
/// sweep, in which a server with the role `api` takes the builder as
/// running (KAIROS-T-0394).
pub const BUILDER_SEEN_SLACK: Duration = Duration::from_secs(5 * 60);

/// The note of a deployment in which no code index builder runs
/// (KAIROS-T-0394), or `None`. A server whose role builds runs the builder
/// itself: `None`. A server with the role `api` looks for a worker: a run
/// of the new-commits sweep that ended `done` in the last 2 intervals of
/// the sweep and [`BUILDER_SEEN_SLACK`].
pub(crate) fn builder_note(
    conn: &mut PgConnection,
    role: crate::config::ServerRole,
) -> Result<Option<String>, ApiError> {
    if role.builds() {
        return Ok(None);
    }
    let key = crate::periodic::key(crate::periodic::CODE_INDEX_SWEEP);
    let last = queue::last_done(conn, &key).map_err(ApiError::internal)?;
    let now = Utc::now();
    let ended = last.as_ref().and_then(|job| {
        let finished = job.finished_at?;
        let payload: crate::periodic::PeriodicPayload = serde_json::from_str(&job.payload).ok()?;
        let interval = Duration::try_from_secs_f64(payload.interval_secs).ok()?;
        let window = chrono::Duration::from_std(interval * 2 + BUILDER_SEEN_SLACK).ok()?;
        Some((finished, finished + window > now))
    });
    Ok(match ended {
        Some((_, true)) => None,
        Some((finished, false)) => Some(format!(
            "No code index builder runs. This server has the role api (KAIROS_ROLE=api), and \
             the last sweep of a worker ended at {}. Kairos builds no index until an operator \
             starts a server with KAIROS_ROLE=worker.",
            finished.to_rfc3339()
        )),
        None => Some(
            "No code index builder runs. This server has the role api (KAIROS_ROLE=api), and no \
             server with KAIROS_ROLE=worker ran the sweep. Kairos builds no index until an \
             operator starts a worker."
                .to_string(),
        ),
    })
}

/// The handler of the build jobs (KAIROS-T-0374).
#[derive(Clone)]
pub struct BuildHandler {
    blocking: BlockingTenantPool,
    service: Arc<CodeIndexService>,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
    /// The outcome of each build, when the caller reads them (a one-pass
    /// run, and the tests).
    outcomes: Option<Arc<Mutex<Vec<BuildOutcome>>>>,
}

impl BuildHandler {
    /// The handler of the builds of `service`, with the embedded summarizer
    /// `summarizers` (none: the image has no embedded model) and the
    /// embedder of the vectors.
    pub fn new(
        blocking: BlockingTenantPool,
        service: Arc<CodeIndexService>,
        summarizers: Option<Arc<dyn SummarizerSource>>,
        embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
    ) -> Self {
        BuildHandler {
            blocking,
            service,
            summarizers,
            embedder,
            outcomes: None,
        }
    }

    /// Keep the outcome of each build in `outcomes`.
    pub fn with_outcomes(mut self, outcomes: Arc<Mutex<Vec<BuildOutcome>>>) -> Self {
        self.outcomes = Some(outcomes);
        self
    }

    /// Run the build jobs of both kinds on `worker`.
    pub fn register(self, worker: &Worker) {
        let handler: Arc<dyn JobHandler> = Arc::new(self);
        worker.register(kinds::EMBEDDED_BUILD, Arc::clone(&handler));
        worker.register(kinds::HOSTED_BUILD, handler);
    }

    async fn build(self, handle: JobHandle) -> Result<(), JobError> {
        let job = handle.job().clone();
        let tenant = job
            .tenant
            .clone()
            .ok_or_else(|| JobError::fail("The build job has no tenant."))?;
        let payload: BuildPayload = serde_json::from_str(&job.payload)
            .map_err(|e| JobError::fail(format!("The payload of the build job is wrong: {e}")))?;
        let instance = job
            .leased_by
            .ok_or_else(|| JobError::fail("The build job has no lease."))?;
        let repo_id = payload.repository_id;
        let repo = match self
            .blocking
            .run(&tenant, move |conn| {
                kairos_db::repositories::load(conn, repo_id).map_err(ApiError::internal)
            })
            .await
        {
            Ok(repo) => repo,
            Err(e) => {
                let note = format!("failed: the repository is not readable: {e:?}");
                super::record_end(&self.blocking, &tenant, payload.run, Err(&note)).await;
                return Err(JobError::fail(note));
            }
        };
        // KAIROS-T-0399: a job that waited while the breaker of its
        // repository tripped does not build. A request resets the breaker
        // before its job is added.
        if payload.trigger != REQUEST
            && let Some(breaker) = tripped(&self.blocking, &tenant, repo_id).await
        {
            let note = breaker_note(&breaker);
            super::record_end(&self.blocking, &tenant, payload.run, Err(&note)).await;
            log_outcome(
                &self.service,
                &payload.trigger,
                repo_id,
                &note_of(&tenant, &repo, "", note.clone()),
            );
            if let Some(outcomes) = &self.outcomes {
                outcomes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(note_of(&tenant, &repo, "", note));
            }
            return Ok(());
        }
        let ctx = BuildCtx {
            handle: handle.clone(),
            instance,
            run: payload.run,
        };
        // KAIROS-T-0750: a push job builds each target that needs an index
        // (the head of the default branch, then the tags and the pull
        // request heads), one outcome for each.
        let outcomes = match payload.trigger.as_str() {
            PUSH => {
                build_repository(
                    &self.blocking,
                    &self.service,
                    &tenant,
                    repo,
                    self.summarizers.clone(),
                    Arc::clone(&self.embedder),
                    &ctx,
                )
                .await
            }
            REQUEST | FIRST => {
                let trigger = if payload.trigger == REQUEST {
                    REQUEST
                } else {
                    // A first build of hours gives its slot to an update.
                    handle.allow_yield();
                    FIRST
                };
                vec![
                    full_build_repository(
                        &self.blocking,
                        &self.service,
                        &tenant,
                        repo,
                        self.summarizers.clone(),
                        Arc::clone(&self.embedder),
                        trigger,
                        &ctx,
                    )
                    .await,
                ]
            }
            other => {
                return Err(JobError::fail(format!(
                    "The build job has an unknown trigger {other:?}."
                )));
            }
        };
        // A stop of a target wins over a yield, and a yield over a failure:
        // a stopped or yielded run goes on when a server takes the job
        // again, and a failure before it stays recorded on its run.
        let (mut stopped, mut yielded, mut failed) = (false, false, None);
        for outcome in outcomes {
            log_outcome(&self.service, &payload.trigger, repo_id, &outcome);
            match (&outcome.report, outcome.note.as_deref()) {
                (Some(_), _) => {}
                (None, Some(YIELDED)) => yielded = true,
                (None, Some(STOPPED)) => stopped = true,
                (None, Some(note)) if note.starts_with("failed") && failed.is_none() => {
                    failed = Some(note.to_string());
                }
                _ => {}
            }
            if let Some(outcomes) = &self.outcomes {
                outcomes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(outcome);
            }
        }
        if stopped {
            Err(JobError::retry(STOPPED))
        } else if yielded {
            Err(JobError::yielded())
        } else if let Some(note) = failed {
            Err(JobError::fail(note))
        } else {
            Ok(())
        }
    }

    /// The lease of the job ran out, or the watchdog killed it
    /// (KAIROS-T-0399): trip the breaker of its repository, with the reason.
    async fn trip(self, job: Job) {
        let (Some(tenant), Ok(payload)) = (
            job.tenant.clone(),
            serde_json::from_str::<BuildPayload>(&job.payload),
        ) else {
            return;
        };
        let repo = payload.repository_id;
        let reason = job
            .last_error
            .clone()
            .unwrap_or_else(|| "the build job stopped".to_string());
        let job_id = job.id;
        let tripped = {
            let reason = reason.clone();
            self.blocking
                .run(&tenant, move |conn| {
                    crate::api::org::run_in_transaction(conn, |conn| {
                        breakers::trip(conn, repo, &reason, Some(job_id))
                            .map_err(ApiError::internal)?;
                        if let Ok(found) = kairos_db::repositories::load(conn, repo) {
                            notify_run(conn, &found.slug, None)?;
                        }
                        Ok(())
                    })
                })
                .await
        };
        match tripped {
            Ok(()) => tracing::warn!(
                tenant = %tenant,
                repository = %repo,
                job = %job_id,
                %reason,
                "code index breaker tripped: the build is not run again until a person asks for a build"
            ),
            Err(e) => {
                tracing::warn!(tenant = %tenant, repository = %repo, error = ?e, "code index breaker not tripped")
            }
        }
    }

    /// The job failed after its last try: end its run as failed, with the
    /// reason.
    async fn end_run(self, job: Job) {
        let (Some(tenant), Ok(payload)) = (
            job.tenant.clone(),
            serde_json::from_str::<BuildPayload>(&job.payload),
        ) else {
            return;
        };
        let Some(run) = payload.run else { return };
        let reason = format!(
            "failed: {}",
            job.last_error
                .unwrap_or_else(|| "the build job failed".to_string())
        );
        if let Err(e) = self
            .blocking
            .run(&tenant, move |conn| {
                crate::api::org::run_in_transaction(conn, |conn| {
                    let Some(found) = runs::get(conn, run).map_err(ApiError::internal)? else {
                        return Ok(());
                    };
                    if found.outcome != "running" {
                        return Ok(());
                    }
                    let ended = runs::end_failed(conn, run, &reason).map_err(ApiError::internal)?;
                    if let Ok(repo) = kairos_db::repositories::load(conn, ended.repository_id) {
                        notify_run(conn, &repo.slug, ended.requested_by)?;
                    }
                    Ok(())
                })
            })
            .await
        {
            tracing::warn!(tenant = %tenant, run = %run, error = ?e, "code index run end not recorded");
        }
    }
}

impl JobHandler for BuildHandler {
    fn run(&self, handle: JobHandle) -> JobFuture {
        Box::pin(self.clone().build(handle))
    }

    fn failed(&self, job: Job) -> UnitFuture {
        Box::pin(self.clone().end_run(job))
    }

    /// A build that took its server down would take it down again (GitHub
    /// #34): it is not run again (KAIROS-T-0399).
    fn retry_stopped(&self) -> bool {
        false
    }

    fn stopped(&self, job: Job) -> UnitFuture {
        Box::pin(self.clone().trip(job))
    }
}

/// Log the outcome of a build job.
fn log_outcome(service: &CodeIndexService, trigger: &str, repo: Uuid, outcome: &BuildOutcome) {
    match (&outcome.report, &outcome.note) {
        (Some(report), _) => {
            service.forget_note(repo);
            tracing::info!(
                tenant = %outcome.tenant,
                repository = %outcome.repository,
                commit = %outcome.commit,
                base = ?outcome.base,
                trigger,
                files = report.build.files,
                symbols = report.build.symbols,
                summaries = report.summary.symbols.summarized
                    + report.summary.files.summarized
                    + report.summary.modules.summarized,
                reused = report.summary.symbols.reused
                    + report.summary.files.reused
                    + report.summary.modules.reused,
                elapsed_secs = outcome.elapsed.as_secs_f64(),
                "code index built"
            );
        }
        (None, Some(note)) if note.starts_with("failed") => {
            service.note_changed(repo, note);
            tracing::warn!(
                tenant = %outcome.tenant,
                repository = %outcome.repository,
                commit = %outcome.commit,
                trigger,
                note = %note,
                elapsed_secs = outcome.elapsed.as_secs_f64(),
                "code index not built"
            );
        }
        (None, Some(note)) => {
            if service.note_changed(repo, note) {
                tracing::info!(
                    tenant = %outcome.tenant,
                    repository = %outcome.repository,
                    trigger,
                    note = %note,
                    "code index build not done"
                );
            }
        }
        (None, None) => {}
    }
}

/// Start the builder (KAIROS-T-0374, KAIROS-T-0376): end the stale runs,
/// run the build jobs on `worker`, and add the periodic pass of the
/// new-commits sweep, at each `interval`. A run of the pass adds the update
/// jobs of the new commits and the first-build jobs; the queue gives each
/// run to one server, so one server fetches the repositories for each
/// interval. The worker of the server runs the jobs. An error is logged;
/// it never stops the server.
pub async fn run_builder(
    worker: Arc<Worker>,
    blocking: BlockingTenantPool,
    service: Arc<CodeIndexService>,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
    interval: Duration,
) {
    tracing::info!(
        interval_secs = interval.as_secs(),
        "code index builder started"
    );
    end_stale_runs(&blocking).await;
    BuildHandler::new(
        blocking.clone(),
        Arc::clone(&service),
        summarizers,
        embedder,
    )
    .register(&worker);
    let pass = sweep_pass(blocking, service, Arc::downgrade(&worker));
    if let Err(e) = worker
        .add_periodic(crate::periodic::CODE_INDEX_SWEEP, interval, pass)
        .await
    {
        tracing::warn!(error = ?e, "the first run of the code index sweep was not added");
    }
}

/// The periodic pass of the new-commits sweep (KAIROS-T-0376):
/// [`enqueue_updates`], then [`enqueue_first_builds`], on the worker of the
/// server. It holds the worker weakly: the worker holds the pass.
fn sweep_pass(
    blocking: BlockingTenantPool,
    service: Arc<CodeIndexService>,
    worker: std::sync::Weak<Worker>,
) -> crate::periodic::PassFn {
    Arc::new(move |handle: JobHandle| {
        let (blocking, service, worker) = (blocking.clone(), Arc::clone(&service), worker.clone());
        Box::pin(async move {
            let Some(worker) = worker.upgrade() else {
                return Err(JobError::not_here("The worker of this server stopped."));
            };
            enqueue_updates_with(&blocking, &service, &worker, Some(&handle)).await;
            handle.progress();
            enqueue_first_builds(&blocking, &service, &worker).await;
            Ok(())
        })
    })
}

/// A worker of the server of `service` that runs the build jobs once, and
/// keeps their outcomes.
fn pass_worker(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
) -> (Arc<Worker>, Arc<Mutex<Vec<BuildOutcome>>>) {
    let worker = Arc::new(Worker::new(
        blocking.clone(),
        service.instance(),
        JobSettings::default(),
    ));
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    BuildHandler::new(blocking.clone(), Arc::clone(service), summarizers, embedder)
        .with_outcomes(Arc::clone(&outcomes))
        .register(&worker);
    (worker, outcomes)
}

async fn drain(
    worker: Arc<Worker>,
    outcomes: Arc<Mutex<Vec<BuildOutcome>>>,
    mut notes: Vec<BuildOutcome>,
) -> Vec<BuildOutcome> {
    worker.drain().await;
    notes.extend(outcomes.lock().unwrap_or_else(|e| e.into_inner()).drain(..));
    notes
}

/// One pass of the new-commits sweep on the job path: [`enqueue_updates`], then
/// run the build jobs that are ready on a worker of this server until none
/// is left. The notes of the repositories that got no job, then the
/// outcome of each job, in the order of their ends. For a one-shot run and
/// the tests; a server runs [`run_builder`].
pub async fn sweep(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
) -> Vec<BuildOutcome> {
    let (worker, outcomes) = pass_worker(blocking, service, summarizers, embedder);
    let notes = enqueue_updates(blocking, service, &worker).await;
    drain(worker, outcomes, notes).await
}

/// One pass of the first builds on the job path: [`enqueue_first_builds`],
/// then run the build jobs that are ready, as [`sweep`] does.
pub async fn first_builds(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
) -> Vec<BuildOutcome> {
    let (worker, outcomes) = pass_worker(blocking, service, summarizers, embedder);
    let notes = enqueue_first_builds(blocking, service, &worker).await;
    drain(worker, outcomes, notes).await
}

/// Run the build jobs that are ready (the requested builds, KAIROS-T-0332,
/// first), as [`sweep`] does, with no new job.
pub async fn requested_builds(
    blocking: &BlockingTenantPool,
    service: &Arc<CodeIndexService>,
    summarizers: Option<Arc<dyn SummarizerSource>>,
    embedder: Arc<dyn kairos_embed::EmbeddingProvider>,
) -> Vec<BuildOutcome> {
    let (worker, outcomes) = pass_worker(blocking, service, summarizers, embedder);
    drain(worker, outcomes, Vec::new()).await
}
