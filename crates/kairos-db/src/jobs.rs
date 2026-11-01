//! The job queue of the background work (KAIROS-T-0373, COLLIERY-I-0613).
//!
//! One table in the public schema (`public.jobs`), with the slug of the
//! organization in `tenant`: each server takes the next job of all the
//! organizations with one query. A job of the deployment has no tenant.
//!
//! The life of a job:
//!
//! - [`enqueue`] adds it as `queued`. A job whose `dedupe_key` is the key of
//!   a `queued` or `leased` job is not added.
//! - [`take`] leases the next job of some kinds: by priority (higher first),
//!   then by `run_after` and `created_at` (older first), only when
//!   `run_after` is past. `FOR UPDATE SKIP LOCKED`: 2 servers never lease
//!   one job. The lease counts a try. A `leased` job whose lease ran out is
//!   taken as a `queued` one: its server stopped or stalled.
//!   [`take_with`] takes some kinds only by their dedupe key ([`Keyed`]).
//! - [`heartbeat`] moves the lease of a job forward, when the job made
//!   progress.
//! - [`complete`], [`release`] and [`fail`] end a lease. [`release`] queues
//!   the job again while it has tries, and fails it after `max_tries`.
//! - [`fail_expired`] fails each job whose lease ran out on its last try.
//!   [`fail_expired_with`] also fails, on any try, a job of a kind that is
//!   not taken again after its lease ran out ([`take_with`]).
//! - [`cancel_waiting`] fails the jobs of a dedupe key that wait for a
//!   server.
//! - [`rearm`] ends a lease well and queues the same row again: a job that
//!   runs for ever as one row (KAIROS-T-0636).
//!
//! A kind can have a unique index of its own on the key of a `queued` job
//! (the write-back of a pull request or an issue, KAIROS-T-0637): one job
//! waits for each key, and a job that runs does not count. Such a kind adds
//! its jobs with [`enqueue_with_conflict`]. When [`release`], [`requeue`]
//! or [`defer`] would put a job back in the queue next to a waiting job of
//! its key, the job ends `done`: the waiting job does its work.
//!
//! Each write of a lease names the server (`leased_by`): a server that lost
//! its lease to another server changes nothing.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{
    Array, BigInt, Double, Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid,
};
use uuid::Uuid;

/// The states of a job.
pub const STATES: [&str; 4] = ["queued", "leased", "done", "failed"];

/// The columns of a [`Job`], in the order of the struct.
const COLUMNS: &str = "id, kind, tenant, payload::text AS payload, priority, dedupe_key, \
                       created_at, run_after, state, leased_by, lease_until, heartbeat_at, \
                       tries, max_tries, last_error, finished_at";

/// A row of `public.jobs`.
#[derive(Debug, Clone, PartialEq, QueryableByName)]
pub struct Job {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub kind: String,
    /// The slug of the organization; `None` for a job of the deployment.
    #[diesel(sql_type = Nullable<Text>)]
    pub tenant: Option<String>,
    /// The JSON of the payload, as text.
    #[diesel(sql_type = Text)]
    pub payload: String,
    #[diesel(sql_type = Integer)]
    pub priority: i32,
    #[diesel(sql_type = Nullable<Text>)]
    pub dedupe_key: Option<String>,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
    #[diesel(sql_type = Timestamptz)]
    pub run_after: DateTime<Utc>,
    #[diesel(sql_type = Text)]
    pub state: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub leased_by: Option<Uuid>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub lease_until: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub heartbeat_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Integer)]
    pub tries: i32,
    #[diesel(sql_type = Integer)]
    pub max_tries: i32,
    #[diesel(sql_type = Nullable<Text>)]
    pub last_error: Option<String>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub finished_at: Option<DateTime<Utc>>,
}

impl Job {
    /// The payload as JSON. `Null` when the text is not JSON, which the
    /// column does not allow.
    pub fn payload_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.payload).unwrap_or(serde_json::Value::Null)
    }
}

/// A job to add.
#[derive(Debug, Clone)]
pub struct NewJob {
    pub kind: String,
    pub tenant: Option<String>,
    pub payload: serde_json::Value,
    /// A higher priority runs first.
    pub priority: i32,
    /// When set, the job is not added while a `queued` or `leased` job has
    /// the same key.
    pub dedupe_key: Option<String>,
    /// Not taken before this time; `None`: now. Inside one priority, the
    /// job with the oldest time goes first, so a time in the past puts a
    /// job ahead of the others of its priority.
    pub run_after: Option<DateTime<Utc>>,
    /// The leases that the job gets before it fails.
    pub max_tries: i32,
}

impl NewJob {
    /// A job of `kind` with an empty payload, priority 0, no key, and 3
    /// tries.
    pub fn new(kind: &str) -> Self {
        NewJob {
            kind: kind.to_string(),
            tenant: None,
            payload: serde_json::json!({}),
            priority: 0,
            dedupe_key: None,
            run_after: None,
            max_tries: 3,
        }
    }
}

/// The conflict target of [`enqueue`]: the index `jobs_dedupe_key`.
const DEDUPE_CONFLICT: &str =
    "(dedupe_key) WHERE state IN ('queued', 'leased') AND dedupe_key IS NOT NULL";

/// Add `job`. `None`: a `queued` or `leased` job has its `dedupe_key`, and
/// nothing was added.
pub fn enqueue(conn: &mut PgConnection, job: &NewJob) -> QueryResult<Option<Job>> {
    enqueue_with_conflict(conn, job, DEDUPE_CONFLICT)
}

/// Add `job`, unless the unique index that `conflict` names (the conflict
/// target of `ON CONFLICT`, as SQL: the columns or expressions, and the
/// `WHERE` of a partial index) has its key: then `None`, and nothing was
/// added. A transaction that adds the same key at the same time waits for
/// this one, and adds nothing when this one commits (KAIROS-T-0637).
/// `conflict` is SQL: a constant of the code, never an input.
pub fn enqueue_with_conflict(
    conn: &mut PgConnection,
    job: &NewJob,
    conflict: &'static str,
) -> QueryResult<Option<Job>> {
    sql_query(format!(
        "INSERT INTO public.jobs (kind, tenant, payload, priority, dedupe_key, run_after, max_tries) \
         VALUES ($1, $2, $3::jsonb, $4, $5, COALESCE($6, now()), $7) \
         ON CONFLICT {conflict} DO NOTHING \
         RETURNING {COLUMNS}"
    ))
    .bind::<Text, _>(&job.kind)
    .bind::<Nullable<Text>, _>(job.tenant.as_deref())
    .bind::<Text, _>(job.payload.to_string())
    .bind::<Integer, _>(job.priority)
    .bind::<Nullable<Text>, _>(job.dedupe_key.as_deref())
    .bind::<Nullable<Timestamptz>, _>(job.run_after)
    .bind::<Integer, _>(job.max_tries)
    .get_result(conn)
    .optional()
}

/// Lease the next job of one of `kinds` to the server `instance`, for
/// `lease_secs`: by priority, then by age, of the jobs whose `run_after` is
/// past. A `queued` job, or a `leased` job whose lease ran out and that has
/// tries left. The lease counts a try. `None`: no job is ready.
pub fn take(
    conn: &mut PgConnection,
    instance: Uuid,
    kinds: &[String],
    lease_secs: f64,
) -> QueryResult<Option<Job>> {
    take_with(conn, instance, kinds, &[], &Keyed::default(), lease_secs)
}

/// The kinds whose jobs a server takes only by their dedupe key
/// (KAIROS-T-0466): a job of one of `kinds` is taken only when its
/// `dedupe_key` is one of `keys`. The periodic passes: a server takes only
/// the runs of the passes that it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keyed {
    /// The kinds that are taken by key.
    pub kinds: Vec<String>,
    /// The dedupe keys of those kinds that the server takes.
    pub keys: Vec<String>,
}

/// [`take`], but a `leased` job of one of `no_retake` whose lease ran out
/// is not taken again: [`fail_expired_with`] fails it (KAIROS-T-0399, a
/// code index build that took its server down). A job of a kind of `keyed`
/// is taken only when its key is one of the keys of `keyed`.
pub fn take_with(
    conn: &mut PgConnection,
    instance: Uuid,
    kinds: &[String],
    no_retake: &[String],
    keyed: &Keyed,
    lease_secs: f64,
) -> QueryResult<Option<Job>> {
    if kinds.is_empty() {
        return Ok(None);
    }
    sql_query(format!(
        "UPDATE public.jobs SET \
             last_error = CASE WHEN state = 'leased' \
                 THEN 'The lease ran out: the server ' || leased_by::text || \
                      ' stopped or stalled.' \
                 ELSE last_error END, \
             state = 'leased', leased_by = $1, \
             lease_until = now() + make_interval(secs => $2), \
             heartbeat_at = now(), tries = tries + 1 \
         WHERE id = ( \
             SELECT id FROM public.jobs \
             WHERE kind = ANY($3) AND run_after <= now() \
               AND (state = 'queued' \
                    OR (state = 'leased' AND lease_until < now() AND tries < max_tries \
                        AND NOT (kind = ANY($4)))) \
               AND (NOT (kind = ANY($5)) OR dedupe_key = ANY($6)) \
             ORDER BY priority DESC, run_after, created_at \
             LIMIT 1 \
             FOR UPDATE SKIP LOCKED) \
         RETURNING {COLUMNS}"
    ))
    .bind::<SqlUuid, _>(instance)
    .bind::<Double, _>(lease_secs)
    .bind::<Array<Text>, _>(kinds)
    .bind::<Array<Text>, _>(no_retake)
    .bind::<Array<Text>, _>(&keyed.kinds)
    .bind::<Array<Text>, _>(&keyed.keys)
    .get_result(conn)
    .optional()
}

/// Move the lease of job `id` of the server `instance` to `lease_secs` from
/// now, and record the heartbeat. False: the server does not hold the lease
/// now (another server took the job, or it ended).
pub fn heartbeat(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    lease_secs: f64,
) -> QueryResult<bool> {
    let n = sql_query(
        "UPDATE public.jobs SET heartbeat_at = now(), lease_until = now() + make_interval(secs => $3) \
         WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(instance)
    .bind::<Double, _>(lease_secs)
    .execute(conn)?;
    Ok(n == 1)
}

/// End the lease of job `id` of the server `instance` as `done`. False: the
/// server did not hold the lease.
pub fn complete(conn: &mut PgConnection, id: Uuid, instance: Uuid) -> QueryResult<bool> {
    let n = sql_query(
        "UPDATE public.jobs SET state = 'done', finished_at = now(), lease_until = NULL \
         WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(instance)
    .execute(conn)?;
    Ok(n == 1)
}

/// End the lease of job `id` of the server `instance` with `error`: the job
/// is `queued` again when it has tries left (`run_after` now plus
/// `retry_after_secs`), else `failed`. The job after the write; `None`: the
/// server did not hold the lease. A job of a kind with a queued-key index
/// whose key has a waiting job ends `done` instead (see the module).
pub fn release(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    error: &str,
    retry_after_secs: f64,
) -> QueryResult<Option<Job>> {
    let back = back_to_queue(conn, id, instance, Some(error), |conn| {
        release_row(conn, id, instance, error, retry_after_secs)
    })?;
    Ok(back.unwrap_or_else(|done| done))
}

fn release_row(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    error: &str,
    retry_after_secs: f64,
) -> QueryResult<Option<Job>> {
    sql_query(format!(
        "UPDATE public.jobs SET \
             state = CASE WHEN tries < max_tries THEN 'queued' ELSE 'failed' END, \
             finished_at = CASE WHEN tries < max_tries THEN NULL ELSE now() END, \
             run_after = CASE WHEN tries < max_tries \
                 THEN now() + make_interval(secs => $4) ELSE run_after END, \
             leased_by = CASE WHEN tries < max_tries THEN NULL ELSE leased_by END, \
             lease_until = NULL, last_error = $3 \
         WHERE id = $1 AND leased_by = $2 AND state = 'leased' \
         RETURNING {COLUMNS}"
    ))
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(instance)
    .bind::<Text, _>(error)
    .bind::<Double, _>(retry_after_secs)
    .get_result(conn)
    .optional()
}

/// End the lease of job `id` of the server `instance` as `failed` with
/// `error`, with no more tries. False: the server did not hold the lease.
pub fn fail(conn: &mut PgConnection, id: Uuid, instance: Uuid, error: &str) -> QueryResult<bool> {
    let n = sql_query(
        "UPDATE public.jobs SET state = 'failed', finished_at = now(), lease_until = NULL, \
             last_error = $3 \
         WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(instance)
    .bind::<Text, _>(error)
    .execute(conn)?;
    Ok(n == 1)
}

/// Fail each job of `kinds` whose lease ran out on its last try: no server
/// will take it again. The failed jobs.
pub fn fail_expired(conn: &mut PgConnection, kinds: &[String]) -> QueryResult<Vec<Job>> {
    fail_expired_with(conn, kinds, &[])
}

/// [`fail_expired`], and also each job of `no_retake` whose lease ran out,
/// on any try: [`take_with`] does not take it again (KAIROS-T-0399). The
/// failed jobs.
pub fn fail_expired_with(
    conn: &mut PgConnection,
    kinds: &[String],
    no_retake: &[String],
) -> QueryResult<Vec<Job>> {
    sql_query(format!(
        "UPDATE public.jobs SET state = 'failed', finished_at = now(), lease_until = NULL, \
             last_error = CASE WHEN tries >= max_tries \
                 THEN 'The lease ran out on the last try (' || tries || ' of ' || \
                     max_tries || '): the server ' || leased_by::text || ' stopped or stalled.' \
                 ELSE 'The lease ran out on try ' || tries || ' of ' || max_tries || \
                     ': the server ' || leased_by::text || ' stopped or stalled, and the job \
                     is not run again.' END \
         WHERE id IN ( \
             SELECT id FROM public.jobs \
             WHERE kind = ANY($1) AND state = 'leased' AND lease_until < now() \
               AND (tries >= max_tries OR kind = ANY($2)) \
             FOR UPDATE SKIP LOCKED) \
         RETURNING {COLUMNS}"
    ))
    .bind::<Array<Text>, _>(kinds)
    .bind::<Array<Text>, _>(no_retake)
    .load(conn)
}

/// Remove the `done` and `failed` jobs that ended before `before`. The
/// number removed.
pub fn purge_finished(conn: &mut PgConnection, before: DateTime<Utc>) -> QueryResult<usize> {
    sql_query("DELETE FROM public.jobs WHERE state IN ('done', 'failed') AND finished_at < $1")
        .bind::<Timestamptz, _>(before)
        .execute(conn)
}

/// The job `id`.
pub fn get(conn: &mut PgConnection, id: Uuid) -> QueryResult<Option<Job>> {
    sql_query(format!("SELECT {COLUMNS} FROM public.jobs WHERE id = $1"))
        .bind::<SqlUuid, _>(id)
        .get_result(conn)
        .optional()
}

/// The jobs of `kind`, oldest first.
pub fn list(conn: &mut PgConnection, kind: &str) -> QueryResult<Vec<Job>> {
    sql_query(format!(
        "SELECT {COLUMNS} FROM public.jobs WHERE kind = $1 ORDER BY created_at, id"
    ))
    .bind::<Text, _>(kind)
    .load(conn)
}

/// The `queued` or `leased` job with `dedupe_key`.
pub fn open_by_key(conn: &mut PgConnection, dedupe_key: &str) -> QueryResult<Option<Job>> {
    sql_query(format!(
        "SELECT {COLUMNS} FROM public.jobs \
         WHERE dedupe_key = $1 AND state IN ('queued', 'leased')"
    ))
    .bind::<Text, _>(dedupe_key)
    .get_result(conn)
    .optional()
}

/// The job with `dedupe_key` that was added last, in any state
/// (KAIROS-T-0487: the last import of the issues of a repository).
pub fn last_by_key(conn: &mut PgConnection, dedupe_key: &str) -> QueryResult<Option<Job>> {
    sql_query(format!(
        "SELECT {COLUMNS} FROM public.jobs WHERE dedupe_key = $1 \
         ORDER BY created_at DESC, id DESC LIMIT 1"
    ))
    .bind::<Text, _>(dedupe_key)
    .get_result(conn)
    .optional()
}

/// Fail each job with `dedupe_key` that waits for a server, with `reason`
/// in `last_error` and no more tries (KAIROS-T-0398): a `queued` job, and a
/// `leased` job whose lease ran out (its server stopped, and a server takes
/// it again). A `leased` job whose lease runs is not changed: its server
/// runs it. The failed jobs.
pub fn cancel_waiting(
    conn: &mut PgConnection,
    dedupe_key: &str,
    reason: &str,
) -> QueryResult<Vec<Job>> {
    sql_query(format!(
        "UPDATE public.jobs SET state = 'failed', finished_at = now(), lease_until = NULL, \
             last_error = $2 \
         WHERE dedupe_key = $1 \
           AND (state = 'queued' OR (state = 'leased' AND lease_until < now())) \
         RETURNING {COLUMNS}"
    ))
    .bind::<Text, _>(dedupe_key)
    .bind::<Text, _>(reason)
    .load(conn)
}

/// Put the job `id` of the server `instance` back in the queue with no
/// try counted: it gave its slot to a job of a higher priority
/// (KAIROS-T-0374). It keeps its `run_after`, so it keeps its place in its
/// priority. False: the server did not hold the lease.
pub fn requeue(conn: &mut PgConnection, id: Uuid, instance: Uuid) -> QueryResult<bool> {
    let back = back_to_queue(conn, id, instance, None, |conn| {
        sql_query(
            "UPDATE public.jobs SET state = 'queued', leased_by = NULL, lease_until = NULL, \
                 tries = GREATEST(tries - 1, 0) \
             WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
        )
        .bind::<SqlUuid, _>(id)
        .bind::<SqlUuid, _>(instance)
        .execute(conn)
    })?;
    Ok(back.map_or_else(|done| done.is_some(), |n| n == 1))
}

/// Add the fields of `patch` (a JSON object) to the payload of the job
/// `id` of the server `instance`, so that a later lease of the job reads
/// them. False: the server did not hold the lease.
pub fn merge_payload(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    patch: &serde_json::Value,
) -> QueryResult<bool> {
    let n = sql_query(
        "UPDATE public.jobs SET payload = payload || $3::jsonb \
         WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(instance)
    .bind::<Text, _>(patch.to_string())
    .execute(conn)?;
    Ok(n == 1)
}

/// Give the `queued` job `id` a higher `priority`, and add the fields of
/// `patch` to its payload: a request for the work of a job that waits. The
/// job after the write; `None`: the job is not queued now.
pub fn raise(
    conn: &mut PgConnection,
    id: Uuid,
    priority: i32,
    patch: &serde_json::Value,
) -> QueryResult<Option<Job>> {
    sql_query(format!(
        "UPDATE public.jobs SET priority = GREATEST(priority, $2), \
             payload = payload || $3::jsonb, run_after = LEAST(run_after, now()) \
         WHERE id = $1 AND state = 'queued' \
         RETURNING {COLUMNS}"
    ))
    .bind::<SqlUuid, _>(id)
    .bind::<Integer, _>(priority)
    .bind::<Text, _>(patch.to_string())
    .get_result(conn)
    .optional()
}

/// True when a `queued` job of `kind` with a priority higher than
/// `priority` is ready to run.
pub fn higher_waits(conn: &mut PgConnection, kind: &str, priority: i32) -> QueryResult<bool> {
    #[derive(QueryableByName)]
    struct Found {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        found: bool,
    }
    sql_query(
        "SELECT EXISTS ( \
             SELECT 1 FROM public.jobs \
             WHERE kind = $1 AND state = 'queued' AND priority > $2 AND run_after <= now()) \
         AS found",
    )
    .bind::<Text, _>(kind)
    .bind::<Integer, _>(priority)
    .get_result::<Found>(conn)
    .map(|f| f.found)
}

/// The `done` job with `dedupe_key` that ended last. A chain of jobs that
/// carries a cursor (the embedding refresh, KAIROS-T-0375) reads where the
/// last job stopped from it.
pub fn last_done(conn: &mut PgConnection, dedupe_key: &str) -> QueryResult<Option<Job>> {
    sql_query(format!(
        "SELECT {COLUMNS} FROM public.jobs \
         WHERE dedupe_key = $1 AND state = 'done' \
         ORDER BY finished_at DESC, created_at DESC LIMIT 1"
    ))
    .bind::<Text, _>(dedupe_key)
    .get_result(conn)
    .optional()
}

/// End the lease of job `id` of the server `instance` well, and queue the
/// same row again to run after `after_secs` (KAIROS-T-0636): a job that runs
/// for ever as one row, as the embedding sweep of an organization. Its tries
/// start again at 0, and `last_error` is cleared. False: the server did not
/// hold the lease.
pub fn rearm(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    after_secs: f64,
) -> QueryResult<bool> {
    let n = sql_query(
        "UPDATE public.jobs SET state = 'queued', leased_by = NULL, lease_until = NULL, \
             tries = 0, last_error = NULL, finished_at = NULL, \
             run_after = now() + make_interval(secs => $3) \
         WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(instance)
    .bind::<Double, _>(after_secs)
    .execute(conn)?;
    Ok(n == 1)
}

/// Put the job `id` of the server `instance` back in the queue with no try
/// counted, to run after `after_secs`, with `reason` in `last_error`: this
/// server cannot run it, and another server can (KAIROS-T-0376). False:
/// the server did not hold the lease.
pub fn defer(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    reason: &str,
    after_secs: f64,
) -> QueryResult<bool> {
    let back = back_to_queue(conn, id, instance, Some(reason), |conn| {
        sql_query(
            "UPDATE public.jobs SET state = 'queued', leased_by = NULL, lease_until = NULL, \
                 tries = GREATEST(tries - 1, 0), last_error = $3, \
                 run_after = now() + make_interval(secs => $4) \
             WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
        )
        .bind::<SqlUuid, _>(id)
        .bind::<SqlUuid, _>(instance)
        .bind::<Text, _>(reason)
        .bind::<Double, _>(after_secs)
        .execute(conn)
    })?;
    Ok(back.map_or_else(|done| done.is_some(), |n| n == 1))
}

/// Why a job ended `done` when it went back to the queue: a waiting job of
/// its key does its work (KAIROS-T-0637).
pub const SUPERSEDED: &str = "A queued job of the same key waits, and does this work.";

/// Run `back`, the write that puts the leased job `id` back in the queue,
/// in a savepoint. A unique violation there means that a `queued` job of
/// the same key waits (a queued-key index of the kind, KAIROS-T-0637): the
/// job then ends `done`, with `reason` and [`SUPERSEDED`] in `last_error`.
/// `Ok(Ok(_))`: the result of `back`; `Ok(Err(_))`: the job after the end,
/// `None` when the server did not hold the lease.
fn back_to_queue<T>(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    reason: Option<&str>,
    back: impl FnOnce(&mut PgConnection) -> QueryResult<T>,
) -> QueryResult<Result<T, Option<Job>>> {
    match conn.transaction::<T, DieselError, _>(back) {
        Ok(value) => Ok(Ok(value)),
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            let error = match reason {
                Some(reason) => format!("{reason} {SUPERSEDED}"),
                None => SUPERSEDED.to_string(),
            };
            let row = sql_query(format!(
                "UPDATE public.jobs SET state = 'done', finished_at = now(), lease_until = NULL, \
                     last_error = $3 \
                 WHERE id = $1 AND leased_by = $2 AND state = 'leased' \
                 RETURNING {COLUMNS}"
            ))
            .bind::<SqlUuid, _>(id)
            .bind::<SqlUuid, _>(instance)
            .bind::<Text, _>(&error)
            .get_result(conn)
            .optional()?;
            Ok(Err(row))
        }
        Err(e) => Err(e),
    }
}

/// Set the `max_tries` of the job `id` of the server `instance`: a kind
/// whose tries are a setting of the server (KAIROS-T-0384, the webhook
/// deliveries) writes it at each lease. False: the server did not hold the
/// lease.
pub fn set_max_tries(
    conn: &mut PgConnection,
    id: Uuid,
    instance: Uuid,
    max_tries: i32,
) -> QueryResult<bool> {
    let n = sql_query(
        "UPDATE public.jobs SET max_tries = GREATEST($3, 1) \
         WHERE id = $1 AND leased_by = $2 AND state = 'leased'",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(instance)
    .bind::<Integer, _>(max_tries)
    .execute(conn)?;
    Ok(n == 1)
}

/// The number of jobs of one kind in one state ([`counts`]).
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct KindStateCount {
    #[diesel(sql_type = Text)]
    pub kind: String,
    #[diesel(sql_type = Text)]
    pub state: String,
    #[diesel(sql_type = BigInt)]
    pub count: i64,
}

/// The number of jobs of each kind in each state, by kind and then state
/// (KAIROS-T-0576). One grouped query. `tenant`: only the jobs of that
/// organization and the jobs of the deployment (no tenant); `None`: all
/// the jobs.
pub fn counts(conn: &mut PgConnection, tenant: Option<&str>) -> QueryResult<Vec<KindStateCount>> {
    sql_query(
        "SELECT kind, state, count(*) AS count FROM public.jobs \
         WHERE $1::text IS NULL OR tenant = $1 OR tenant IS NULL \
         GROUP BY kind, state ORDER BY kind, state",
    )
    .bind::<Nullable<Text>, _>(tenant)
    .load(conn)
}

/// The `failed` jobs that ended last, newest first, at most `limit`
/// (KAIROS-T-0576). `tenant` as in [`counts`].
pub fn last_failures(
    conn: &mut PgConnection,
    tenant: Option<&str>,
    limit: i64,
) -> QueryResult<Vec<Job>> {
    sql_query(format!(
        "SELECT {COLUMNS} FROM public.jobs \
         WHERE state = 'failed' AND ($1::text IS NULL OR tenant = $1 OR tenant IS NULL) \
         ORDER BY finished_at DESC NULLS LAST, created_at DESC, id \
         LIMIT $2"
    ))
    .bind::<Nullable<Text>, _>(tenant)
    .bind::<BigInt, _>(limit)
    .load(conn)
}
