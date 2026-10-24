//! The periodic passes as jobs (KAIROS-T-0376, COLLIERY-I-0613).
//!
//! A periodic pass (retention, the new-commits sweep of the code index, the
//! report of the stale claims, the report of the keys that expire soon) is
//! a job of the kind `periodic` ([`crate::jobs::kinds::PERIODIC`]) with no
//! tenant. Its dedupe key names the pass ([`key`]), so each pass has one
//! open run at most. When a run ends, done or failed, the worker adds the
//! next run of the pass in the transaction of the end
//! ([`crate::jobs::JobHandler::next`]): one interval after the time the run
//! was due. So each pass runs once for each interval, whatever the number
//! of servers, and a pass whose server stopped is taken again by another
//! server when its lease runs out.
//!
//! Each server adds the passes that it runs with [`Worker::add_periodic`]:
//! that registers the pass, and adds a run that is due now when the pass
//! has no open run (an insert that the dedupe key refuses when a run is
//! open). So the first start of a deployment runs each pass at once, and a
//! later start changes nothing.
//!
//! A server takes only the runs of the passes that it added (KAIROS-T-0466,
//! [`kairos_db::jobs::Keyed`]): an `api` server does not take the
//! new-commits sweep, and a `worker` takes only that pass (KAIROS-T-0394).
//! A run of a pass that the server cannot run now (the worker of the code
//! index stopped) is given back with no try counted
//! ([`crate::jobs::JobError::not_here`]).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use kairos_db::jobs::{Job, NewJob};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::jobs::{JobError, JobFuture, JobHandle, JobHandler, Worker, kinds, priority};

/// The pass of retention (KAIROS-T-0366).
pub const RETENTION: &str = "retention";

/// The new-commits sweep of the code index (KAIROS-T-0374).
pub const CODE_INDEX_SWEEP: &str = "code-index-sweep";

/// The report of the stale claims (KAIROS-T-0531).
pub const STALE_CLAIMS: &str = "stale-claims";

/// The report of the keys that expire soon (KAIROS-T-0540).
pub const KEY_EXPIRING: &str = "key-expiring";

/// The look at the repositories whose issue import is due (KAIROS-T-0670).
pub const ISSUE_SYNC: &str = "issue-sync";

/// The dedupe key of the runs of the pass `pass`.
pub fn key(pass: &str) -> String {
    format!("periodic:{pass}")
}

/// The payload of a periodic run: the pass, and its interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeriodicPayload {
    pub pass: String,
    pub interval_secs: f64,
}

/// The work of one run of a pass. It calls [`JobHandle::progress`] as it
/// goes on.
pub type PassFn = Arc<dyn Fn(JobHandle) -> JobFuture + Send + Sync>;

/// The passes that a server runs.
#[derive(Default)]
pub struct Passes {
    passes: RwLock<HashMap<String, PassFn>>,
}

impl std::fmt::Debug for Passes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self
            .passes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        f.debug_struct("Passes").field("passes", &names).finish()
    }
}

impl Passes {
    fn get(&self, pass: &str) -> Option<PassFn> {
        self.passes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(pass)
            .cloned()
    }

    /// The dedupe keys of the passes ([`key`]), sorted.
    pub(crate) fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .passes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .map(|pass| key(pass))
            .collect();
        keys.sort();
        keys
    }

    fn insert(&self, pass: &str, run: PassFn) {
        self.passes
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(pass.to_string(), run);
    }
}

/// The first run of `pass`, due now.
fn first_run(pass: &str, interval: Duration, max_tries: i32) -> NewJob {
    NewJob {
        kind: kinds::PERIODIC.to_string(),
        tenant: None,
        payload: serde_json::json!(PeriodicPayload {
            pass: pass.to_string(),
            interval_secs: interval.as_secs_f64(),
        }),
        priority: priority::PERIODIC,
        dedupe_key: Some(key(pass)),
        run_after: None,
        max_tries: max_tries.max(1),
    }
}

/// The run that follows `job`: one interval after the time `job` was due,
/// or now when that time is past.
pub fn next_run(job: &Job) -> Option<NewJob> {
    let payload: PeriodicPayload = serde_json::from_str(&job.payload).ok()?;
    let interval = Duration::try_from_secs_f64(payload.interval_secs).ok()?;
    let mut next = first_run(&payload.pass, interval, job.max_tries);
    let due = job.run_after + chrono::Duration::from_std(interval).ok()?;
    next.run_after = Some(due.max(chrono::Utc::now()));
    Some(next)
}

/// The handler of the kind `periodic`: it runs the pass that the payload
/// names.
struct PeriodicHandler {
    passes: Arc<Passes>,
    instance: uuid::Uuid,
}

impl JobHandler for PeriodicHandler {
    fn run(&self, handle: JobHandle) -> JobFuture {
        let payload: Result<PeriodicPayload, _> = serde_json::from_str(&handle.job().payload);
        let payload = match payload {
            Ok(payload) => payload,
            Err(e) => {
                let reason = format!("The payload of the periodic run is wrong: {e}");
                return Box::pin(async move { Err(JobError::fail(reason)) });
            }
        };
        match self.passes.get(&payload.pass) {
            Some(run) => run(handle),
            None => {
                let reason = format!(
                    "The server {} does not run the pass {}.",
                    self.instance, payload.pass
                );
                Box::pin(async move { Err(JobError::not_here(reason)) })
            }
        }
    }

    fn next(&self, job: &Job) -> Option<NewJob> {
        next_run(job)
    }
}

impl Worker {
    /// Run the pass `pass` on this server at each `interval`
    /// (KAIROS-T-0376): register it, and add a run that is due now when the
    /// pass has no open run. The run that was added; `None`: a run is open.
    pub async fn add_periodic(
        &self,
        pass: &str,
        interval: Duration,
        run: PassFn,
    ) -> Result<Option<Job>, ApiError> {
        // KAIROS-T-0394: a server runs only the passes of its role, and adds
        // no run of a pass that it does not run.
        if !self.role().runs_pass(pass) {
            tracing::info!(
                pass,
                role = self.role().as_str(),
                "periodic pass: this role does not run the pass"
            );
            return Ok(None);
        }
        self.periodic.insert(pass, run);
        self.register(
            kinds::PERIODIC,
            Arc::new(PeriodicHandler {
                passes: Arc::clone(&self.periodic),
                instance: self.instance(),
            }),
        );
        let added = self
            .enqueue(first_run(pass, interval, self.settings().max_tries))
            .await?;
        match &added {
            Some(job) => {
                tracing::info!(pass, job = %job.id, interval_secs = interval.as_secs_f64(), "periodic pass: the first run is added")
            }
            None => {
                tracing::info!(
                    pass,
                    interval_secs = interval.as_secs_f64(),
                    "periodic pass: a run is open"
                )
            }
        }
        Ok(added)
    }
}

/// How often the retention pass runs (KAIROS-T-0366).
pub const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// The pass of retention (KAIROS-T-0366): on a connection of its own,
/// [`kairos_db::retention::run_retention_pass`] sweeps `item_history`,
/// `team_page_history` and `activity_log` of each tenant by the retention
/// settings, and deletes the expired password sessions, the old
/// notifications, code indexes, summaries and webhook deliveries
/// (KAIROS-T-0638). A failed pass is tried again, and the next
/// run is due one interval later all the same.
///
/// KAIROS-T-0631: the run writes its record in the field `result` of the
/// payload of its job ([`kairos_db::retention::RetentionPass::result`]):
/// `skipped: no target` when `archive` mode has no target, else the
/// counts.
pub fn retention_pass(
    database_url: String,
    config: kairos_core::retention::RetentionConfig,
) -> PassFn {
    Arc::new(move |handle: JobHandle| {
        let (url, config) = (database_url.clone(), config.clone());
        Box::pin(async move {
            tokio::task::spawn_blocking(move || retention_run(&url, &config, &handle))
                .await
                .map_err(|e| JobError::retry(format!("The retention pass stopped: {e}")))?
        })
    })
}

/// One run of the retention pass: connect, run the pass, log the result.
fn retention_run(
    database_url: &str,
    config: &kairos_core::retention::RetentionConfig,
    handle: &JobHandle,
) -> Result<(), JobError> {
    let mut conn = kairos_db::establish_migration_connection(database_url).map_err(|e| {
        tracing::error!(error = %e, "retention: the pass cannot connect to the database");
        JobError::retry(format!(
            "The retention pass cannot connect to the database: {e}"
        ))
    })?;
    let pass = kairos_db::retention::run_retention_pass(
        &mut conn,
        config,
        chrono::Utc::now(),
        &mut || {
            handle.progress();
        },
    )
    .map_err(|e| {
        tracing::error!(error = %e, "retention: the pass failed");
        JobError::retry(format!("The retention pass failed: {e}"))
    })?;
    for report in &pass.tenants {
        tracing::info!(
            tenant = %report.slug,
            mode = %report.mode,
            item_history_archived = report.item_history.rows_archived,
            item_history_pruned = report.item_history.rows_pruned,
            activity_log_archived = report.activity_log.rows_archived,
            activity_log_pruned = report.activity_log.rows_pruned,
            team_page_history_archived = report.team_page_history.rows_archived,
            team_page_history_pruned = report.team_page_history.rows_pruned,
            warnings = report.item_history.warnings
                + report.activity_log.warnings
                + report.team_page_history.warnings,
            "retention: tenant swept"
        );
    }
    let result = pass.result();
    if let Some(warning) = config.warning() {
        tracing::warn!(%result, %warning, "retention: the pass removed no history or activity");
    }
    let job = handle.job();
    let patch = serde_json::json!({ "result": result });
    match kairos_db::jobs::merge_payload(
        &mut conn,
        job.id,
        job.leased_by.unwrap_or_default(),
        &patch,
    ) {
        Ok(true) => {}
        Ok(false) => tracing::warn!(job = %job.id, "retention: the run lost its lease; no record"),
        Err(e) => {
            tracing::warn!(job = %job.id, error = %e, "retention: the record of the run was not written")
        }
    }
    tracing::info!(
        %result,
        tenants = pass.tenants.len(),
        expired_sessions_deleted = pass.expired_sessions_deleted,
        expired_auth_failures_deleted = pass.expired_auth_failures_deleted,
        read_notifications_deleted = pass.read_notifications_deleted,
        unread_notifications_deleted = pass.unread_notifications_deleted,
        code_indexes_deleted = pass.code_indexes_deleted,
        unused_summaries_deleted = pass.unused_summaries_deleted,
        webhook_deliveries_deleted = pass.webhook_deliveries_deleted,
        request_keys_deleted = pass.request_keys_deleted,
        "retention: pass done"
    );
    Ok(())
}

/// How often the pass of the stale claims runs (KAIROS-T-0531). A claim
/// is stale after days, so one hour is soon enough.
pub const STALE_CLAIMS_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The pass of the stale claims (KAIROS-T-0531): on a connection of its
/// own, [`kairos_db::task_claims::stale_claims_pass`] tells the holder and
/// the owner of the initiative of each claim with no write for `days`
/// days (`KAIROS_STALE_CLAIM_DAYS`). The claim stays. A failed pass is
/// tried again.
pub fn stale_claims_pass(database_url: String, days: u32) -> PassFn {
    Arc::new(move |handle: JobHandle| {
        let url = database_url.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || stale_claims_run(&url, days, &handle))
                .await
                .map_err(|e| JobError::retry(format!("The stale-claims pass stopped: {e}")))?
        })
    })
}

/// One run of the pass of the stale claims: connect, run the pass, log the
/// result.
fn stale_claims_run(database_url: &str, days: u32, handle: &JobHandle) -> Result<(), JobError> {
    let mut conn = kairos_db::establish_migration_connection(database_url).map_err(|e| {
        tracing::error!(error = %e, "stale claims: the pass cannot connect to the database");
        JobError::retry(format!(
            "The stale-claims pass cannot connect to the database: {e}"
        ))
    })?;
    let reported =
        kairos_db::task_claims::stale_claims_pass(&mut conn, days, chrono::Utc::now(), &mut || {
            handle.progress();
        })
        .map_err(|e| {
            tracing::error!(error = %e, "stale claims: the pass failed");
            JobError::retry(format!("The stale-claims pass failed: {e}"))
        })?;
    tracing::info!(days, reported, "stale claims: pass done");
    Ok(())
}

/// How often the pass of the keys that expire soon runs (KAIROS-T-0540). A
/// key is told 7 days before its expiry, so one hour is soon enough.
pub const KEY_EXPIRING_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The pass of the keys that expire soon (KAIROS-T-0540): on a connection
/// of its own, [`kairos_db::api_keys::key_expiring_pass`] tells the person
/// of each agent key, and the organization admins of each service-account
/// key, that expires within 7 days. Once for each key. A failed pass is
/// tried again.
pub fn key_expiring_pass(database_url: String) -> PassFn {
    Arc::new(move |handle: JobHandle| {
        let url = database_url.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || key_expiring_run(&url, &handle))
                .await
                .map_err(|e| JobError::retry(format!("The key-expiring pass stopped: {e}")))?
        })
    })
}

/// One run of the pass of the keys that expire soon: connect, run the
/// pass, log the result.
fn key_expiring_run(database_url: &str, handle: &JobHandle) -> Result<(), JobError> {
    let mut conn = kairos_db::establish_migration_connection(database_url).map_err(|e| {
        tracing::error!(error = %e, "key expiring: the pass cannot connect to the database");
        JobError::retry(format!(
            "The key-expiring pass cannot connect to the database: {e}"
        ))
    })?;
    let reported =
        kairos_db::api_keys::key_expiring_pass(&mut conn, chrono::Utc::now(), &mut || {
            handle.progress();
        })
        .map_err(|e| {
            tracing::error!(error = %e, "key expiring: the pass failed");
            JobError::retry(format!("The key-expiring pass failed: {e}"))
        })?;
    tracing::info!(reported, "key expiring: pass done");
    Ok(())
}
