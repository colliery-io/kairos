//! The runs of the code index builder (KAIROS-T-0331, COLLIERY-I-0610):
//! one row for each build after a push, each first build, each build on
//! request and each upload of an index file.
//!
//! A run starts as `running` and ends as `ok` or `failed`. A run that the
//! server never ended (the server stopped) is ended by [`end_stale`] at the
//! next pass of the builder for the repository.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::code_index_builds;

/// The values of `code_index_builds.trigger`. The column has a CHECK with
/// the same values.
pub const TRIGGERS: [&str; 4] = ["push", "first", "request", "upload"];

/// The values of `code_index_builds.outcome`.
pub const OUTCOMES: [&str; 3] = ["running", "ok", "failed"];

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
}

/// The counts of the index that a run wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub files: i32,
    pub symbols: i32,
    pub edges: i32,
    pub summaries_made: i32,
}

/// Start a run: a `running` row. `trigger` is one of [`TRIGGERS`].
pub fn start(
    conn: &mut PgConnection,
    repository_id: Uuid,
    trigger: &str,
    ref_name: Option<&str>,
    requested_by: Option<Uuid>,
) -> QueryResult<Build> {
    diesel::insert_into(code_index_builds::table)
        .values(NewBuild {
            repository_id,
            commit_sha: None,
            ref_name,
            trigger,
            outcome: "running",
            requested_by,
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
/// run has them (an upload has none). The commit is set with [`set_commit`]
/// before.
pub fn end_ok(conn: &mut PgConnection, id: Uuid, counts: Option<Counts>) -> QueryResult<Build> {
    diesel::update(code_index_builds::table.find(id))
        .set((
            code_index_builds::outcome.eq("ok"),
            code_index_builds::files.eq(counts.map(|c| c.files)),
            code_index_builds::symbols.eq(counts.map(|c| c.symbols)),
            code_index_builds::edges.eq(counts.map(|c| c.edges)),
            code_index_builds::summaries_made.eq(counts.map(|c| c.summaries_made)),
            code_index_builds::finished_at.eq(diesel::dsl::now),
        ))
        .returning(Build::as_select())
        .get_result(conn)
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
        let run = start(conn, repository_id, trigger, ref_name, requested_by)?;
        set_commit(conn, run.id, commit_sha)?;
        end_ok(conn, run.id, counts)
    })
}

/// End each `running` run of a repository as `failed` with
/// [`SERVER_STOPPED`]: the builder starts a pass for the repository, so no
/// run of it is active. The count of the runs ended.
pub fn end_stale(conn: &mut PgConnection, repository_id: Uuid) -> QueryResult<usize> {
    diesel::update(
        code_index_builds::table
            .filter(code_index_builds::repository_id.eq(repository_id))
            .filter(code_index_builds::outcome.eq("running")),
    )
    .set((
        code_index_builds::outcome.eq("failed"),
        code_index_builds::error.eq(Some(SERVER_STOPPED)),
        code_index_builds::finished_at.eq(diesel::dsl::now),
    ))
    .execute(conn)
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
        let run = start(conn, repository_id, trigger, ref_name, None)?;
        if let Some(commit) = commit_sha {
            set_commit(conn, run.id, commit)?;
        }
        end_failed(conn, run.id, error).map(Some)
    })
}

/// End each `running` run of the tenant as `failed` with [`SERVER_STOPPED`]:
/// the builder starts, so no run is active. The count of the runs ended.
pub fn end_all_stale(conn: &mut PgConnection) -> QueryResult<usize> {
    diesel::update(code_index_builds::table.filter(code_index_builds::outcome.eq("running")))
        .set((
            code_index_builds::outcome.eq("failed"),
            code_index_builds::error.eq(Some(SERVER_STOPPED)),
            code_index_builds::finished_at.eq(diesel::dsl::now),
        ))
        .execute(conn)
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

/// The runs of a repository, newest first, `limit` at most.
pub fn list(conn: &mut PgConnection, repository_id: Uuid, limit: i64) -> QueryResult<Vec<Build>> {
    code_index_builds::table
        .filter(code_index_builds::repository_id.eq(repository_id))
        .order(code_index_builds::started_at.desc())
        .limit(limit)
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
