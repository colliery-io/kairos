//! The import of the open issues of a repository (KAIROS-T-0483,
//! COLLIERY-I-0623): the issues that were open before the webhook got the
//! Issues event. Each one goes through the same path as an opened-issue
//! event ([`crate::forge::intake::take_issue`]): the filter, then one task
//! per issue id. An import that runs twice makes nothing the second time.
//!
//! The import is a job of the job queue (KAIROS-T-0487, kind [`KIND`]): the
//! request adds the job and answers at once. One import of a repository
//! waits or runs at a time (the dedupe key [`job_key`]). The job reads the
//! list page by page, takes each issue of a page, and writes the next page
//! and the counts to its payload in the same transaction: a job that a
//! server takes again goes on from the next page, with its counts. The
//! payload keeps the result after the job ends.
//!
//! The cadence (KAIROS-T-0670) queues the same job on the interval of the
//! repository ([`crate::forge::sync`]). The job is a sync: it lists every
//! open issue and compares it with the record of taken issues
//! (`forge_issue_intake`). An issue with no record becomes a task. The job
//! writes the sync state of the repository ([`kairos_db::issue_sync`]).
//! The cadence needs only the read token: no webhook connection.
//!
//! The token is the read token of the repository, the one that the code
//! index builder uses ([`crate::credentials::fetch_token`]). It needs Issues:
//! read on GitHub, and `read_api` on GitLab.
//!
//! - GitHub: `GET {KAIROS_GITHUB_API_URL}/repos/{owner}/{repo}/issues?state=open`.
//!   The list has the pull requests too; they are skipped.
//! - GitLab: `GET {scheme://host}/api/v4/projects/{path}/issues?state=opened`,
//!   the host of the URL of the repository. Confidential issues are skipped.
//!
//! Pages of 100, with no limit of pages. When the forge answers with its
//! rate limit, the job waits until the reset time that the forge gives
//! ([`rate_limit_wait`]), and reads the same page again. It reports
//! progress while it waits, so the watchdog does not stop it.

use std::sync::Arc;
use std::time::Duration;

use diesel::pg::PgConnection;
use kairos_client::types_repositories as dto;
use kairos_core::forge::ForgeIssue;
use kairos_db::jobs::{Job, NewJob};
use kairos_db::models::enums::Forge;
use kairos_db::models::repositories::Repository;
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::blocking::BlockingTenantPool;
use crate::error::ApiError;
use crate::forge::intake::{Outcome, Source, take_issue};
use crate::jobs::{JobError, JobFuture, JobHandle, JobHandler, Worker};
use crate::secrets::SecretsKey;
use kairos_db::issue_sync;

/// The kind of the import job.
pub const KIND: &str = "issue_import";

/// The priority of the import job: after the code-index builds and the
/// embedding batches, before the periodic passes.
pub const PRIORITY: i32 = 15;

/// Issues per page.
pub const PER_PAGE: usize = 100;

/// The time of one request.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The code of the error inside the job when another server holds the
/// lease.
const LEASE_LOST: &str = "LEASE_LOST";

/// The wait for a rate limit with no reset time.
const DEFAULT_WAIT: Duration = Duration::from_secs(60);

/// The longest wait for one rate limit.
const MAX_WAIT: Duration = Duration::from_secs(60 * 60);

/// The dedupe key of the import of a repository: one waits or runs at a
/// time.
pub fn job_key(tenant: &str, repository: Uuid) -> String {
    format!("issue-import:{tenant}:{repository}")
}

/// One page of the list.
#[derive(Debug, Clone)]
pub enum Page {
    /// The issues of the page; `last`: no page follows.
    Rows { issues: Vec<ForgeIssue>, last: bool },
    /// The forge answered with its rate limit: wait, then ask again.
    RateLimited { wait: Duration },
}

/// Why the list could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ListError {
    /// The forge refused the token (401, 403 that is not the rate limit,
    /// 404).
    #[error("{0}")]
    Refused(String),
    /// The request failed, or the forge answered with an error.
    #[error("{0}")]
    Failed(String),
}

/// The API base of GitLab for a repository URL: `scheme://host[:port]/api/v4`.
/// Pure.
pub fn gitlab_api_base(repo_url: &str) -> Option<String> {
    let url = url::Url::parse(repo_url).ok()?;
    let host = url.host_str()?;
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some(format!("{}://{host}{port}/api/v4", url.scheme()))
}

/// The URL of one page. Pure.
pub fn page_url(forge: Forge, api_base: &str, repo_full_name: &str, page: usize) -> String {
    let base = api_base.trim_end_matches('/');
    match forge {
        Forge::Gitlab => format!(
            "{base}/projects/{}/issues?state=opened&per_page={PER_PAGE}&page={page}",
            url::form_urlencoded::byte_serialize(repo_full_name.as_bytes()).collect::<String>()
        ),
        _ => format!(
            "{base}/repos/{repo_full_name}/issues?state=open&per_page={PER_PAGE}&page={page}"
        ),
    }
}

/// Whether a refusal is the rate limit of the forge: 429, or 403 with no
/// requests left or a `Retry-After`. Pure.
pub fn is_rate_limit(status: StatusCode, headers: &reqwest::header::HeaderMap) -> bool {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    status == StatusCode::FORBIDDEN
        && (headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            == Some("0")
            || headers.contains_key(reqwest::header::RETRY_AFTER))
}

/// The wait for a rate limit, at `now` (seconds since 1970): the
/// `Retry-After` seconds, else the reset time of GitHub
/// (`x-ratelimit-reset`) or of GitLab (`RateLimit-Reset`), else 60
/// seconds. At least 1 second, at most 1 hour. Pure.
pub fn rate_limit_wait(headers: &reqwest::header::HeaderMap, now: u64) -> Duration {
    let number = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    let wait = if let Some(secs) = number("retry-after") {
        Duration::from_secs(secs)
    } else if let Some(reset) = number("x-ratelimit-reset").or_else(|| number("ratelimit-reset")) {
        Duration::from_secs(reset.saturating_sub(now))
    } else {
        DEFAULT_WAIT
    };
    wait.clamp(Duration::from_secs(1), MAX_WAIT)
}

/// The refusal of the token, for a person. Pure.
fn refusal(forge: Forge, status: StatusCode) -> String {
    let (name, permission) = match forge {
        Forge::Gitlab => ("GitLab", "read_api"),
        _ => ("GitHub", "Issues: read"),
    };
    format!(
        "{name} refused the read token of the repository ({status}). The token needs \
         {permission} on the repository."
    )
}

/// The HTTP client of the import.
pub fn client() -> Result<reqwest::Client, ListError> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("Kairos/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| ListError::Failed(format!("The HTTP client did not start: {e}.")))
}

/// Read one page of the open issues of a repository.
pub async fn fetch_page(
    client: &reqwest::Client,
    forge: Forge,
    api_base: &str,
    repo_full_name: &str,
    token: &str,
    page: usize,
) -> Result<Page, ListError> {
    let mut request = client.get(page_url(forge, api_base, repo_full_name, page));
    request = match forge {
        Forge::Gitlab => request.header("PRIVATE-TOKEN", token),
        _ => request
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28"),
    };
    let response = request.send().await.map_err(|e| {
        ListError::Failed(format!(
            "The request to the forge failed: {}.",
            e.without_url()
        ))
    })?;
    let status = response.status();
    if !status.is_success() {
        if is_rate_limit(status, response.headers()) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            return Ok(Page::RateLimited {
                wait: rate_limit_wait(response.headers(), now),
            });
        }
        if matches!(status.as_u16(), 401 | 403 | 404) {
            return Err(ListError::Refused(refusal(forge, status)));
        }
        return Err(ListError::Failed(format!(
            "The forge answered {status} to the list of issues."
        )));
    }
    let rows: Vec<Value> = response.json().await.map_err(|e| {
        ListError::Failed(format!(
            "The list of issues is not JSON: {}.",
            e.without_url()
        ))
    })?;
    let last = rows.len() < PER_PAGE;
    let issues = rows
        .iter()
        .filter_map(|row| match forge {
            Forge::Gitlab => kairos_core::forge::gitlab_api_issue(row, repo_full_name),
            _ => kairos_core::forge::github_issue(row, repo_full_name),
        })
        .collect();
    Ok(Page::Rows { issues, last })
}

/// What an import needs: the read token and the API base. An import needs
/// no webhook connection, the manual import included (KAIROS-T-0670). The
/// refusals name what is missing. The request checks this before it adds
/// the job, and the job again when it starts.
pub struct Target {
    pub repository: Repository,
    /// The live webhook connection, when the repository has one.
    pub connection_id: Option<Uuid>,
    pub token: String,
    pub api_base: String,
}

/// Read the [`Target`] of the repository `reference`.
pub fn target(
    conn: &mut PgConnection,
    tenant: &str,
    reference: &str,
    key: Option<&SecretsKey>,
    github_api: &str,
) -> Result<Target, ApiError> {
    let repository = kairos_db::repositories::resolve(conn, reference)
        .map_err(crate::api::org::repositories::map_error)?;
    if matches!(repository.forge, Forge::Other) {
        return Err(ApiError::validation(format!(
            "The repository {} has the forge `other`. Only a github repository or a gitlab \
             repository has issues to import.",
            repository.slug
        ))
        .with_details(json!({ "field": "repository" })));
    }
    let connection = kairos_db::forge::find_connection_for_repository(conn, repository.id)
        .map_err(crate::api::org::forge::map_error)?;
    let (token, _) = crate::credentials::fetch_token(conn, tenant, &repository, key)
        .map_err(crate::credentials::token_refusal)?;
    let token = token.ok_or_else(|| {
        let permission = match repository.forge {
            Forge::Gitlab => "read_api",
            _ => "Issues: read",
        };
        ApiError::validation(format!(
            "The repository {} has no read token. Set the read token of the repository \
             (Admin, Repositories, or `kairos repos credential set`) with {permission}, then \
             import.",
            repository.slug
        ))
        .with_details(json!({ "field": "token" }))
    })?;
    let api_base = match repository.forge {
        Forge::Gitlab => gitlab_api_base(&repository.repo_url).ok_or_else(|| {
            ApiError::validation(format!(
                "The URL of the repository {} has no host.",
                repository.slug
            ))
            .with_details(json!({ "field": "repo_url" }))
        })?,
        _ => github_api.to_string(),
    };
    Ok(Target {
        connection_id: connection.map(|c| c.id),
        token: token.expose().to_string(),
        api_base,
        repository,
    })
}

/// Who asked for an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// An org admin: `POST /api/repositories/{slug}/import-issues`.
    Manual,
    /// The cadence of the repository (KAIROS-T-0670).
    Cadence,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Manual => "manual",
            Trigger::Cadence => "cadence",
        }
    }
}

/// The job that imports the open issues of `repository`.
pub fn new_job(tenant: &str, repository: &Repository, max_tries: i32, trigger: Trigger) -> NewJob {
    NewJob {
        tenant: Some(tenant.to_string()),
        payload: json!({
            "repository_id": repository.id,
            "repository": repository.slug,
            "page": 1,
            "trigger": trigger.as_str(),
        }),
        priority: PRIORITY,
        dedupe_key: Some(job_key(tenant, repository.id)),
        max_tries,
        ..NewJob::new(KIND)
    }
}

/// The counts of an import, as the payload of the job keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Counts {
    /// The next page to read.
    page: usize,
    result: dto::IssueImportResult,
    /// The reasons of the issues that got no task.
    reasons: Vec<String>,
}

impl Counts {
    fn of(payload: &Value) -> Self {
        let n = |name: &str| payload[name].as_u64().unwrap_or(0) as usize;
        let strings = |name: &str| -> Vec<String> {
            payload[name]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        Counts {
            page: n("page").max(1),
            result: dto::IssueImportResult {
                imported: n("imported"),
                skipped: n("skipped"),
                filtered: n("filtered"),
                not_taken: n("not_taken"),
                items: strings("items"),
                message: payload["message"].as_str().unwrap_or_default().to_string(),
            },
            reasons: strings("reasons"),
        }
    }

    fn patch(&self) -> Value {
        json!({
            "page": self.page,
            "imported": self.result.imported,
            "skipped": self.result.skipped,
            "filtered": self.result.filtered,
            "not_taken": self.result.not_taken,
            "items": self.result.items,
            "reasons": self.reasons,
        })
    }

    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Created(code) => {
                self.result.imported += 1;
                self.result.items.push(code);
            }
            Outcome::Duplicate => self.result.skipped += 1,
            Outcome::Filtered => self.result.filtered += 1,
            Outcome::NotRouted(reason) => {
                self.result.not_taken += 1;
                if !self.reasons.contains(&reason) {
                    self.reasons.push(reason);
                }
            }
        }
    }

    /// The sentence for a person.
    fn message(&self) -> String {
        let r = &self.result;
        let mut message = format!(
            "Kairos imported {} issue(s), skipped {} taken before, and did not take {} that \
             the forge filter excludes.",
            r.imported, r.skipped, r.filtered
        );
        if r.not_taken > 0 {
            message.push_str(&format!(
                " {} issue(s) got no task: {}",
                r.not_taken,
                self.reasons.join(" ")
            ));
        }
        message
    }
}

/// The sync state as the API shows it (KAIROS-T-0670). `row`: the row of
/// `forge_issue_sync`, or `None` when the repository never had one.
/// `record_count`: the issues in the record.
pub fn sync_dto(row: Option<&issue_sync::IssueSync>, record_count: i64) -> dto::IssueSync {
    let at = |t: Option<chrono::DateTime<chrono::Utc>>| t.map(|t| t.to_rfc3339());
    match row {
        None => dto::IssueSync {
            record_count,
            ..Default::default()
        },
        Some(row) => dto::IssueSync {
            next_run_at: at(row.next_run_at),
            last_run_at: at(row.last_run_at),
            last_run_complete: row.last_run_complete,
            last_complete_at: at(row.last_complete_at),
            record_count,
            last_run_listed: i64::from(row.last_run_listed),
            last_run_imported: i64::from(row.last_run_imported),
            last_error: row.last_error.clone(),
            last_error_at: at(row.last_error_at),
        },
    }
}

/// The import of a repository that has had no job yet (KAIROS-T-0670): the
/// cadence is on, and the first run is due.
pub fn idle_dto(repository: &Repository) -> dto::IssueImport {
    dto::IssueImport {
        job: String::new(),
        repository: repository.slug.clone(),
        state: "idle".to_string(),
        created_at: repository.updated_at.to_rfc3339(),
        finished_at: None,
        waiting_until: None,
        pages: 0,
        result: dto::IssueImportResult::default(),
        error: None,
        interval: dto::interval_off(),
        sync: dto::IssueSync::default(),
    }
}

/// The import as the API shows it: the job and the counts of its payload.
pub fn import_dto(job: &Job) -> dto::IssueImport {
    let payload = job.payload_json();
    let counts = Counts::of(&payload);
    let waiting_until = payload["waiting_until"].as_str().map(str::to_string);
    let state = match job.state.as_str() {
        "leased" if waiting_until.is_some() => "waiting",
        "leased" => "running",
        other => other,
    };
    let mut result = counts.result.clone();
    if result.message.is_empty() {
        result.message = counts.message();
    }
    dto::IssueImport {
        job: job.id.to_string(),
        repository: payload["repository"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        state: state.to_string(),
        created_at: job.created_at.to_rfc3339(),
        finished_at: job.finished_at.map(|t| t.to_rfc3339()),
        waiting_until: if job.state == "leased" {
            waiting_until
        } else {
            None
        },
        pages: counts.page.saturating_sub(1),
        result,
        error: job.last_error.clone(),
        interval: dto::interval_off(),
        sync: dto::IssueSync::default(),
    }
}

/// The handler of the import jobs.
#[derive(Clone)]
pub struct IssueImportService {
    blocking: BlockingTenantPool,
    secrets_key: Option<SecretsKey>,
    github_api: String,
}

impl IssueImportService {
    pub fn new(
        blocking: BlockingTenantPool,
        secrets_key: Option<SecretsKey>,
        github_api: String,
    ) -> Self {
        IssueImportService {
            blocking,
            secrets_key,
            github_api,
        }
    }

    /// Run the import jobs on `worker`.
    pub fn register(self, worker: &Worker) {
        worker.register(KIND, Arc::new(self));
    }

    /// Write `patch` to the payload of the job. False: this server does not
    /// hold the lease now.
    async fn patch(&self, job: &Job, patch: Value) -> Result<bool, JobError> {
        let (id, instance) = (job.id, job.leased_by.unwrap_or_default());
        self.blocking
            .run_public(move |conn| {
                kairos_db::jobs::merge_payload(conn, id, instance, &patch)
                    .map_err(ApiError::internal)
            })
            .await
            .map_err(|e| JobError::retry(format!("The database did not answer: {}", e.message)))
    }

    /// Run one import job: write the start and the end to the sync state of
    /// the repository (KAIROS-T-0670), and run the pages between.
    async fn run_job(self, handle: JobHandle) -> Result<(), JobError> {
        let job = handle.job().clone();
        let tenant = job
            .tenant
            .clone()
            .ok_or_else(|| JobError::fail("The import has no organization."))?;
        let payload = job.payload_json();
        let repository_id = payload["repository_id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| JobError::fail("The payload of the import has no repository_id."))?;
        let mut counts = Counts::of(&payload);
        self.sync_write(&tenant, repository_id, None).await;
        let ran = self
            .run_pages(&handle, &tenant, repository_id, &mut counts)
            .await;
        let seen = (counts.result.imported
            + counts.result.skipped
            + counts.result.filtered
            + counts.result.not_taken) as i32;
        let end = match &ran {
            Ok(()) => issue_sync::RunEnd {
                complete: true,
                listed: seen,
                imported: counts.result.imported as i32,
                error: None,
            },
            Err(e) => issue_sync::RunEnd {
                complete: false,
                listed: seen,
                imported: counts.result.imported as i32,
                error: Some(e.reason.as_str()),
            },
        };
        self.sync_write(&tenant, repository_id, Some(&end)).await;
        ran
    }

    /// Write the sync state: the start of a run (`end` is `None`), or its
    /// end. A failed write is logged: the state is for a person, and the
    /// import does not fail for it.
    async fn sync_write(
        &self,
        tenant: &str,
        repository_id: Uuid,
        end: Option<&issue_sync::RunEnd<'_>>,
    ) {
        let end = end.map(|e| {
            (
                e.complete,
                e.listed,
                e.imported,
                e.error.map(str::to_string),
            )
        });
        let written = self
            .blocking
            .run(tenant, move |conn| {
                let now = chrono::Utc::now();
                match &end {
                    None => issue_sync::run_started(conn, repository_id, now),
                    Some((complete, listed, imported, error)) => issue_sync::run_finished(
                        conn,
                        repository_id,
                        now,
                        &issue_sync::RunEnd {
                            complete: *complete,
                            listed: *listed,
                            imported: *imported,
                            error: error.as_deref(),
                        },
                    ),
                }
                .map_err(ApiError::internal)
            })
            .await;
        if let Err(e) = written {
            tracing::warn!(repository = %repository_id, error = %e.message, "issue import: the sync state was not written");
        }
    }

    async fn run_pages(
        &self,
        handle: &JobHandle,
        tenant: &str,
        repository_id: Uuid,
        counts: &mut Counts,
    ) -> Result<(), JobError> {
        let job = handle.job().clone();
        let tenant = tenant.to_string();
        // The repository, the connection and the token, again: they can
        // change after the request.
        let (key, github_api, slug) = (
            self.secrets_key.clone(),
            self.github_api.clone(),
            tenant.clone(),
        );
        let target = self
            .blocking
            .run(&tenant, move |conn| {
                target(
                    conn,
                    &slug,
                    &repository_id.to_string(),
                    key.as_ref(),
                    &github_api,
                )
            })
            .await
            .map_err(|e| JobError::fail(e.message))?;
        let client = client().map_err(|e| JobError::retry(e.to_string()))?;
        let lost = || JobError::retry("Another server holds the lease of the import now.");
        loop {
            if !handle.progress() {
                return Err(JobError::retry("The import was stopped."));
            }
            let page = fetch_page(
                &client,
                target.repository.forge,
                &target.api_base,
                &target.repository.repo_full_name,
                &target.token,
                counts.page,
            )
            .await
            .map_err(|e| match e {
                ListError::Refused(reason) => JobError::fail(reason),
                ListError::Failed(reason) => JobError::retry(reason),
            })?;
            let (issues, last) = match page {
                Page::RateLimited { wait } => {
                    let until =
                        chrono::Utc::now() + chrono::Duration::from_std(wait).unwrap_or_default();
                    tracing::info!(
                        job = %job.id,
                        page = counts.page,
                        wait_secs = wait.as_secs(),
                        "issue import: the forge rate limit; waiting"
                    );
                    if !self
                        .patch(&job, json!({ "waiting_until": until.to_rfc3339() }))
                        .await?
                    {
                        return Err(lost());
                    }
                    let end = tokio::time::Instant::now() + wait;
                    while tokio::time::Instant::now() < end {
                        if !handle.progress() {
                            return Err(JobError::retry("The import was stopped."));
                        }
                        let step = end
                            .saturating_duration_since(tokio::time::Instant::now())
                            .min(Duration::from_secs(1));
                        tokio::time::sleep(step).await;
                    }
                    if !self.patch(&job, json!({ "waiting_until": null })).await? {
                        return Err(lost());
                    }
                    continue;
                }
                Page::Rows { issues, last } => (issues, last),
            };
            // Take the issues of the page, and write the next page and the
            // counts in the same transaction.
            let mut next = (*counts).clone();
            let (slug, connection_id) = (tenant.clone(), target.connection_id);
            let (id, instance) = (job.id, job.leased_by.unwrap_or_default());
            let taken = self
                .blocking
                .run(&tenant, move |conn| {
                    crate::api::org::run_in_transaction(conn, |conn| {
                        let actor = kairos_db::forge_actor::ensure_for_slug(conn, &slug)
                            .map_err(ApiError::internal)?;
                        for issue in &issues {
                            next.add(take_issue(
                                conn,
                                &slug,
                                actor,
                                repository_id,
                                connection_id,
                                issue,
                                Source::Import,
                            )?);
                        }
                        next.page += 1;
                        let mut patch = next.patch();
                        // A wait of an earlier try is over.
                        patch["waiting_until"] = Value::Null;
                        if last {
                            patch["message"] = json!(next.message());
                        }
                        if !kairos_db::jobs::merge_payload(conn, id, instance, &patch)
                            .map_err(ApiError::internal)?
                        {
                            return Err(ApiError::new(
                                StatusCode::CONFLICT,
                                LEASE_LOST,
                                "Another server holds the lease of the import now.",
                            ));
                        }
                        Ok(next)
                    })
                })
                .await;
            *counts = match taken {
                Ok(next) => next,
                Err(e) if e.code == LEASE_LOST => return Err(lost()),
                Err(e) => {
                    return Err(JobError::retry(format!(
                        "The issues of page {} were not taken: {}",
                        counts.page, e.message
                    )));
                }
            };
            if last {
                return Ok(());
            }
        }
    }
}

impl JobHandler for IssueImportService {
    fn run(&self, handle: JobHandle) -> JobFuture {
        Box::pin(self.clone().run_job(handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_urls() {
        assert_eq!(
            page_url(Forge::Github, "https://api.github.com/", "acme/app", 2),
            "https://api.github.com/repos/acme/app/issues?state=open&per_page=100&page=2"
        );
        assert_eq!(
            page_url(
                Forge::Gitlab,
                "https://gitlab.com/api/v4",
                "acme/sub/app",
                1
            ),
            "https://gitlab.com/api/v4/projects/acme%2Fsub%2Fapp/issues?state=opened&per_page=100&page=1"
        );
    }

    #[test]
    fn the_gitlab_api_is_on_the_host_of_the_repository() {
        assert_eq!(
            gitlab_api_base("https://gitlab.com/acme/app").as_deref(),
            Some("https://gitlab.com/api/v4")
        );
        assert_eq!(
            gitlab_api_base("http://192.0.2.10:8929/acme/app").as_deref(),
            Some("http://192.0.2.10:8929/api/v4")
        );
        assert_eq!(gitlab_api_base("not a url"), None);
    }

    #[test]
    fn the_rate_limit_is_told_from_a_refusal() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert!(is_rate_limit(StatusCode::TOO_MANY_REQUESTS, &headers));
        assert!(!is_rate_limit(StatusCode::FORBIDDEN, &headers));
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        assert!(is_rate_limit(StatusCode::FORBIDDEN, &headers));
        assert!(!is_rate_limit(StatusCode::UNAUTHORIZED, &headers));
    }

    #[test]
    fn the_wait_of_a_rate_limit_comes_from_the_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(rate_limit_wait(&headers, 1000), DEFAULT_WAIT);
        headers.insert("x-ratelimit-reset", "1030".parse().unwrap());
        assert_eq!(rate_limit_wait(&headers, 1000), Duration::from_secs(30));
        // A reset in the past waits 1 second.
        assert_eq!(rate_limit_wait(&headers, 2000), Duration::from_secs(1));
        headers.insert("retry-after", "7".parse().unwrap());
        assert_eq!(rate_limit_wait(&headers, 1000), Duration::from_secs(7));
        let mut gitlab = reqwest::header::HeaderMap::new();
        gitlab.insert("ratelimit-reset", "1100".parse().unwrap());
        assert_eq!(rate_limit_wait(&gitlab, 1000), Duration::from_secs(100));
        // At most 1 hour.
        gitlab.insert("ratelimit-reset", "999999".parse().unwrap());
        assert_eq!(rate_limit_wait(&gitlab, 1000), MAX_WAIT);
    }

    #[test]
    fn the_counts_go_through_the_payload() {
        let mut counts = Counts::of(&json!({"page": 1}));
        counts.add(Outcome::Created("PAY-T-0001".into()));
        counts.add(Outcome::Duplicate);
        counts.add(Outcome::Filtered);
        counts.add(Outcome::NotRouted("No delivery board.".into()));
        counts.add(Outcome::NotRouted("No delivery board.".into()));
        counts.page = 2;
        let again = Counts::of(&counts.patch());
        assert_eq!(again, counts);
        assert_eq!(again.result.not_taken, 2);
        assert_eq!(again.reasons, vec!["No delivery board.".to_string()]);
        assert!(again.message().contains("imported 1 issue(s)"));
        assert!(again.message().contains("No delivery board."));
    }

    #[test]
    fn a_refusal_names_the_permission() {
        assert!(refusal(Forge::Github, StatusCode::FORBIDDEN).contains("Issues: read"));
        assert!(refusal(Forge::Gitlab, StatusCode::UNAUTHORIZED).contains("read_api"));
    }
}
