//! The runs of the code index builder (KAIROS-T-0331, COLLIERY-I-0610):
//! one row for each build after a push, each first build, each build on
//! request and each upload of an index file.
//!
//! A run starts as `running` and ends as `ok` or `failed`.
//!
//! Each build runs as a job of the job queue (KAIROS-T-0374,
//! [`crate::jobs`]), and the payload of the job names its run (`run`). The
//! dedupe key of a build job is the repository, so a repository has one
//! open build job at most. A `running` run that no open job names is stale:
//! its server stopped before this version, or the job ended without it. It
//! is ended by [`end_stale_except`] when the next run of the repository
//! starts, and by [`end_without_job`] when the builder starts. A run that
//! an open job names is never ended as stopped: a server takes the job
//! again, and the run goes on ([`adopt`]).
//!
//! Each run records the server that runs it (KAIROS-T-0354): the random id
//! of the start of that server.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::code_index_builds;

/// The error of a run that the server did not end.
pub const SERVER_STOPPED: &str = "The server stopped during the run.";

/// The most runs that one list gives.
pub const MAX_LIST: i64 = 100;

/// The runs that a list gives when the caller names no limit.
pub const DEFAULT_LIST: i64 = 20;

/// One run.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = code_index_builds)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Build {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub commit_sha: Option<String>,
    pub ref_name: Option<String>,
    pub trigger: String,
    pub outcome: String,
    pub error: Option<String>,
    pub files: Option<i32>,
    pub symbols: Option<i32>,
    pub edges: Option<i32>,
    pub summaries_made: Option<i32>,
    pub requested_by: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// The model that wrote the summaries of the run, as `provider/model`
    /// (KAIROS-T-0341). None for a run that made none.
    pub model: Option<String>,
    /// The start of the server that runs the run (KAIROS-T-0354). None for
    /// a run from before that version, and for a run that ended when it
    /// started (an upload, a failure before the run).
    pub server_instance: Option<Uuid>,
    /// The summaries that the run reused, with no model call
    /// (KAIROS-T-0335): from the pool of the repository, or from the work
    /// file of a run that did not end.
    pub summaries_reused: Option<i32>,
    /// The SCIP warnings of the run (KAIROS-T-0478): a JSON array of
    /// `{"language", "reason"}`, one for each language that fell back to
    /// the edges by name. None for a run with no warning.
    pub scip_warnings: Option<String>,
    /// The summaries that the index of the run does not have
    /// (KAIROS-T-0495): a summary call failed, or a child of the item has no
    /// summary. The next build makes them. None for a run with no counts.
    pub summaries_missing: Option<i32>,
}

#[derive(Insertable)]
#[diesel(table_name = code_index_builds)]
struct NewBuild<'a> {
    repository_id: Uuid,
    commit_sha: Option<&'a str>,
    ref_name: Option<&'a str>,
    trigger: &'a str,
    outcome: &'a str,
    requested_by: Option<Uuid>,
    server_instance: Option<Uuid>,
}

/// The counts of the index that a run wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub files: i32,
    pub symbols: i32,
    pub edges: i32,
    pub summaries_made: i32,
    pub summaries_reused: i32,
    pub summaries_missing: i32,
}

/// Start a run: a `running` row. `trigger` is `push`, `first`, `request`
/// or `upload`: the CHECK of the column `code_index_builds.trigger`.
/// `server_instance` is the start of the server that runs it
/// (KAIROS-T-0354).
pub fn start(
    conn: &mut PgConnection,
    repository_id: Uuid,
    trigger: &str,
    ref_name: Option<&str>,
    requested_by: Option<Uuid>,
    server_instance: Option<Uuid>,
) -> QueryResult<Build> {
    diesel::insert_into(code_index_builds::table)
        .values(NewBuild {
            repository_id,
            commit_sha: None,
            ref_name,
            trigger,
            outcome: "running",
            requested_by,
            server_instance,
        })
        .returning(Build::as_select())
        .get_result(conn)
}

/// Give a running run its commit, once the fetch gives it.
pub fn set_commit(conn: &mut PgConnection, id: Uuid, commit_sha: &str) -> QueryResult<()> {
    diesel::update(code_index_builds::table.find(id))
        .set(code_index_builds::commit_sha.eq(commit_sha))
        .execute(conn)
        .map(|_| ())
}

/// End a run as `ok`, with the counts of the index that it wrote, when the
/// run has them (an upload has none), and the model that wrote its
/// summaries. The commit is set with [`set_commit`] before.
pub fn end_ok(
    conn: &mut PgConnection,
    id: Uuid,
    counts: Option<Counts>,
    model: Option<&str>,
) -> QueryResult<Build> {
    diesel::update(code_index_builds::table.find(id))
        .set((
            code_index_builds::outcome.eq("ok"),
            code_index_builds::model.eq(model),
            code_index_builds::files.eq(counts.map(|c| c.files)),
            code_index_builds::symbols.eq(counts.map(|c| c.symbols)),
            code_index_builds::edges.eq(counts.map(|c| c.edges)),
            code_index_builds::summaries_made.eq(counts.map(|c| c.summaries_made)),
            code_index_builds::summaries_reused.eq(counts.map(|c| c.summaries_reused)),
            code_index_builds::summaries_missing.eq(counts.map(|c| c.summaries_missing)),
            code_index_builds::finished_at.eq(diesel::dsl::now),
        ))
        .returning(Build::as_select())
        .get_result(conn)
}

/// Record the SCIP warnings of a run (KAIROS-T-0478), as a JSON array.
/// `None` clears them.
pub fn set_scip_warnings(
    conn: &mut PgConnection,
    id: Uuid,
    warnings: Option<&str>,
) -> QueryResult<()> {
    diesel::update(code_index_builds::table.find(id))
        .set(code_index_builds::scip_warnings.eq(warnings))
        .execute(conn)
        .map(|_| ())
}

/// The SCIP warnings of the newest run that ended `ok` for each of
/// `repositories` (KAIROS-T-0668): the JSON array that the run recorded. A
/// repository with no such run, or whose run had no warning, has no entry.
pub fn newest_scip_warnings(
    conn: &mut PgConnection,
    repositories: &[Uuid],
) -> QueryResult<std::collections::HashMap<Uuid, String>> {
    let rows: Vec<(Uuid, Option<String>)> = code_index_builds::table
        .filter(code_index_builds::repository_id.eq_any(repositories))
        .filter(code_index_builds::outcome.eq("ok"))
        .distinct_on(code_index_builds::repository_id)
        .order((
            code_index_builds::repository_id,
            code_index_builds::started_at.desc(),
        ))
        .select((
            code_index_builds::repository_id,
            code_index_builds::scip_warnings,
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, warnings)| Some((id, warnings?)))
        .collect())
}

/// End a run as `failed`, with the text of the failure.
pub fn end_failed(conn: &mut PgConnection, id: Uuid, error: &str) -> QueryResult<Build> {
    diesel::update(code_index_builds::table.find(id))
        .set((
            code_index_builds::outcome.eq("failed"),
            code_index_builds::error.eq(Some(error)),
            code_index_builds::finished_at.eq(diesel::dsl::now),
        ))
        .returning(Build::as_select())
        .get_result(conn)
}

/// Write one finished run in one step: an upload, which has no running
/// phase that a reader can see.
pub fn record_finished(
    conn: &mut PgConnection,
    repository_id: Uuid,
    trigger: &str,
    commit_sha: &str,
    ref_name: Option<&str>,
    requested_by: Option<Uuid>,
    counts: Option<Counts>,
) -> QueryResult<Build> {
    conn.transaction(|conn| {
        let run = start(conn, repository_id, trigger, ref_name, requested_by, None)?;
        set_commit(conn, run.id, commit_sha)?;
        end_ok(conn, run.id, counts, None)
    })
}

/// The newest run of a repository, if it has one.
pub fn newest(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<Option<Build>> {
    code_index_builds::table
        .filter(code_index_builds::repository_id.eq(repository_id))
        .order(code_index_builds::started_at.desc())
        .select(Build::as_select())
        .first(conn)
        .optional()
}

/// Write one `failed` run for a failure that happened before a run had a
/// row (the fetch, the choice of the base). A failure with the same text as
/// the newest run of the repository, when that run failed, adds no row:
/// the builder tries again on each pass, and one row says what is wrong.
/// The row, or None when the failure is a repeat.
pub fn record_failure_once(
    conn: &mut PgConnection,
    repository_id: Uuid,
    trigger: &str,
    commit_sha: Option<&str>,
    ref_name: Option<&str>,
    error: &str,
) -> QueryResult<Option<Build>> {
    conn.transaction(|conn| {
        if let Some(last) = newest(conn, repository_id)?
            && last.outcome == "failed"
            && last.error.as_deref() == Some(error)
        {
            return Ok(None);
        }
        let run = start(conn, repository_id, trigger, ref_name, None, None)?;
        if let Some(commit) = commit_sha {
            set_commit(conn, run.id, commit)?;
        }
        end_failed(conn, run.id, error).map(Some)
    })
}

/// The running run of a repository, if it has one.
pub fn running(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<Option<Build>> {
    code_index_builds::table
        .filter(code_index_builds::repository_id.eq(repository_id))
        .filter(code_index_builds::outcome.eq("running"))
        .order(code_index_builds::started_at.desc())
        .select(Build::as_select())
        .first(conn)
        .optional()
}

/// Lock the row of a repository until the end of the transaction
/// (KAIROS-T-0354): the check for an active run and the start of a run are
/// one step, so 2 runs of a repository do not start at the same time.
pub fn lock_repository(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<()> {
    use crate::schema::repositories;
    repositories::table
        .find(repository_id)
        .select(repositories::id)
        .for_update()
        .first::<Uuid>(conn)
        .optional()
        .map(|_| ())
}

/// The runs of a repository, newest first, `limit` at most.
pub fn list(
    conn: &mut PgConnection,
    repository_id: Uuid,
    limit: i64,
    offset: i64,
) -> QueryResult<Vec<Build>> {
    // KAIROS-T-0584: the id breaks a tie of the time, so the pages keep one
    // order.
    code_index_builds::table
        .filter(code_index_builds::repository_id.eq(repository_id))
        .order((
            code_index_builds::started_at.desc(),
            code_index_builds::id.desc(),
        ))
        .limit(limit)
        .offset(offset)
        .select(Build::as_select())
        .load(conn)
}

/// The count of the runs of a repository.
pub fn count(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<i64> {
    code_index_builds::table
        .filter(code_index_builds::repository_id.eq(repository_id))
        .count()
        .get_result(conn)
}

/// End each `running` run of a repository but `keep`, as `failed` with
/// [`SERVER_STOPPED`]: no open job names them (KAIROS-T-0374). The count of
/// the runs ended.
pub fn end_stale_except(
    conn: &mut PgConnection,
    repository_id: Uuid,
    keep: Option<Uuid>,
) -> QueryResult<usize> {
    diesel::update(
        code_index_builds::table
            .filter(code_index_builds::repository_id.eq(repository_id))
            .filter(code_index_builds::outcome.eq("running"))
            .filter(code_index_builds::id.ne(keep.unwrap_or_else(Uuid::nil))),
    )
    .set((
        code_index_builds::outcome.eq("failed"),
        code_index_builds::error.eq(Some(SERVER_STOPPED)),
        code_index_builds::finished_at.eq(diesel::dsl::now),
    ))
    .execute(conn)
}

/// End each `running` run of the tenant `tenant` that no open job of the
/// job queue names (KAIROS-T-0374), as `failed` with [`SERVER_STOPPED`].
/// The builder of each server does this when it starts. A run of a job
/// that another server runs now, or that waits for a server, stays. The
/// count of the runs ended.
pub fn end_without_job(conn: &mut PgConnection, tenant: &str) -> QueryResult<usize> {
    diesel::sql_query(
        "UPDATE code_index_builds SET outcome = 'failed', error = $2, finished_at = now() \
         WHERE outcome = 'running' AND NOT EXISTS ( \
             SELECT 1 FROM public.jobs j \
             WHERE j.tenant = $1 AND j.state IN ('queued', 'leased') \
               AND j.payload->>'run' = code_index_builds.id::text)",
    )
    .bind::<diesel::sql_types::Text, _>(tenant)
    .bind::<diesel::sql_types::Text, _>(SERVER_STOPPED)
    .execute(conn)
}

/// Give a `running` run to the server `server_instance`: a server took the
/// job of the run again after its server stopped (KAIROS-T-0374), or the
/// job of a request starts. False when the run is not `running`.
pub fn adopt(conn: &mut PgConnection, id: Uuid, server_instance: Uuid) -> QueryResult<bool> {
    let n = diesel::update(
        code_index_builds::table
            .find(id)
            .filter(code_index_builds::outcome.eq("running")),
    )
    .set(code_index_builds::server_instance.eq(Some(server_instance)))
    .execute(conn)?;
    Ok(n == 1)
}

/// The run `id`.
pub fn get(conn: &mut PgConnection, id: Uuid) -> QueryResult<Option<Build>> {
    code_index_builds::table
        .find(id)
        .select(Build::as_select())
        .first(conn)
        .optional()
}
