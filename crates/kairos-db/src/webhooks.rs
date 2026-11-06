//! Signed webhook deliveries (KAIROS-T-0384, COLLIERY-I-0614).
//!
//! A **target** is a URL on one board that gets deliveries. Kairos makes
//! its secret; the server seals it with `KAIROS_SECRETS_KEY` before this
//! module sees it, so the rows hold only the ciphertext. A **delivery** is
//! one event to one target: the body that the server signs and sends, its
//! status, and a log of each try (`webhook_delivery_tries`).
//!
//! A delivery is a job of the job queue (`public.jobs`, kind
//! [`JOB_KIND`]): [`queue_delivery`] writes the delivery and its job in the
//! transaction of the caller, so an event that is rolled back sends
//! nothing, and the queue gives each job to one server.
//!
//! A delivery to a target that is off is `skipped`. It is kept for
//! [`KEEP_DAYS`]; [`enable_target`] and [`redeliver_skipped`] queue it
//! again (KAIROS-T-0577).
//!
//! [`record_try`] writes the result of a try. A failed try adds one to the
//! failures in a row of the target; a good try sets them to 0. At the limit
//! of the caller, the target is turned off, and its owners
//! ([`owners`]: the person who made it and the managers of its board) get a
//! notification of the kind `delivery_failing`.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{
    Array, Bool, Bytea, Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid,
};
use kairos_core::notifications::NotificationKind;
use uuid::Uuid;

use crate::jobs::{self, NewJob};

/// The kind of the job of a delivery.
pub const JOB_KIND: &str = "webhook_delivery";

/// The priority of the job of a delivery: after an embedding batch (20),
/// before a periodic pass (10).
pub const JOB_PRIORITY: i32 = 15;

/// The tries of a delivery when the server has no setting
/// (`KAIROS_WEBHOOK_MAX_TRIES`).
pub const DEFAULT_MAX_TRIES: i32 = 12;

/// A finished delivery (delivered, failed or skipped) is kept for this many
/// days. Each new delivery of a target removes the older finished ones of
/// that target, and the retention pass removes them for each target
/// (KAIROS-T-0638), so the log of a quiet target goes too.
pub const KEEP_DAYS: i64 = 30;

/// The columns of a [`Target`].
const TARGET_COLUMNS: &str = "t.id, t.board_id, t.name, t.url, t.enabled, \
     t.consecutive_failures, t.disabled_at, t.disabled_reason, t.created_by, t.created_at";

/// A webhook target, with no secret.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Target {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    pub board_id: Uuid,
    #[diesel(sql_type = Text)]
    pub name: String,
    #[diesel(sql_type = Text)]
    pub url: String,
    #[diesel(sql_type = Bool)]
    pub enabled: bool,
    #[diesel(sql_type = Integer)]
    pub consecutive_failures: i32,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub disabled_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Text>)]
    pub disabled_reason: Option<String>,
    #[diesel(sql_type = SqlUuid)]
    pub created_by: Uuid,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// The sealed secret of a target (AES-256-GCM, sealed by the server).
#[derive(Clone, PartialEq, Eq, QueryableByName)]
pub struct SealedSecret {
    #[diesel(sql_type = Bytea)]
    pub ciphertext: Vec<u8>,
    #[diesel(sql_type = Bytea)]
    pub nonce: Vec<u8>,
    #[diesel(sql_type = Text)]
    pub key_id: String,
}

impl std::fmt::Debug for SealedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedSecret")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

/// A target to add. The server makes `id` first, because the associated
/// data of the sealed secret names it.
#[derive(Debug, Clone)]
pub struct NewTarget {
    pub id: Uuid,
    pub board_id: Uuid,
    pub name: String,
    pub url: String,
    pub secret: SealedSecret,
    pub created_by: Uuid,
}

/// One delivery: an event to a target.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Delivery {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    pub target_id: Uuid,
    #[diesel(sql_type = Text)]
    pub event: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub item_id: Option<Uuid>,
    /// The dispatch rule that queued the delivery (KAIROS-T-0385); `None`
    /// for a test delivery.
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub rule_id: Option<Uuid>,
    /// The JSON text that is signed and sent.
    #[diesel(sql_type = Text)]
    pub body: String,
    /// `pending|delivered|failed|skipped`.
    #[diesel(sql_type = Text)]
    pub status: String,
    #[diesel(sql_type = Integer)]
    pub tries: i32,
    #[diesel(sql_type = Nullable<Integer>)]
    pub response_code: Option<i32>,
    #[diesel(sql_type = Nullable<Text>)]
    pub last_error: Option<String>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub next_try_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub finished_at: Option<DateTime<Utc>>,
}

/// The columns of a [`Delivery`].
const DELIVERY_COLUMNS: &str = "d.id, d.target_id, d.event, d.item_id, d.rule_id, d.body, d.status, \
     d.tries, d.response_code, d.last_error, d.next_try_at, d.created_at, d.finished_at";

/// One try of a delivery: a row of the log.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct DeliveryTry {
    #[diesel(sql_type = SqlUuid)]
    pub delivery_id: Uuid,
    #[diesel(sql_type = Integer)]
    pub try_number: i32,
    #[diesel(sql_type = Timestamptz)]
    pub at: DateTime<Utc>,
    #[diesel(sql_type = Bool)]
    pub ok: bool,
    #[diesel(sql_type = Nullable<Integer>)]
    pub response_code: Option<i32>,
    #[diesel(sql_type = Nullable<Text>)]
    pub error: Option<String>,
    #[diesel(sql_type = Integer)]
    pub duration_ms: i32,
}

#[derive(QueryableByName)]
struct SchemaName {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct UserRow {
    #[diesel(sql_type = SqlUuid)]
    user_id: Uuid,
}

/// The slug of the tenant of the connection (`org_<slug>` is its schema).
pub fn current_tenant(conn: &mut PgConnection) -> Result<String, DieselError> {
    let schema: SchemaName =
        sql_query("SELECT COALESCE(current_schema()::text, 'public') AS name").get_result(conn)?;
    Ok(schema
        .name
        .strip_prefix("org_")
        .unwrap_or(&schema.name)
        .to_string())
}

/// Add a target.
pub fn insert_target(conn: &mut PgConnection, target: &NewTarget) -> Result<Target, DieselError> {
    sql_query(format!(
        "INSERT INTO webhook_targets AS t (id, board_id, name, url, secret_ciphertext, \
             secret_nonce, secret_key_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING {TARGET_COLUMNS}"
    ))
    .bind::<SqlUuid, _>(target.id)
    .bind::<SqlUuid, _>(target.board_id)
    .bind::<Text, _>(target.name.trim())
    .bind::<Text, _>(&target.url)
    .bind::<Bytea, _>(&target.secret.ciphertext)
    .bind::<Bytea, _>(&target.secret.nonce)
    .bind::<Text, _>(&target.secret.key_id)
    .bind::<SqlUuid, _>(target.created_by)
    .get_result(conn)
}

/// The target `id`.
pub fn get_target(conn: &mut PgConnection, id: Uuid) -> Result<Option<Target>, DieselError> {
    sql_query(format!(
        "SELECT {TARGET_COLUMNS} FROM webhook_targets t WHERE t.id = $1"
    ))
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .optional()
}

/// The targets of a board, oldest first.
pub fn list_targets(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<Target>, DieselError> {
    sql_query(format!(
        "SELECT {TARGET_COLUMNS} FROM webhook_targets t WHERE t.board_id = $1 \
         ORDER BY t.created_at, t.id"
    ))
    .bind::<SqlUuid, _>(board_id)
    .load(conn)
}

/// The sealed secret of the target `id`.
pub fn target_secret(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<SealedSecret>, DieselError> {
    sql_query(
        "SELECT secret_ciphertext AS ciphertext, secret_nonce AS nonce, secret_key_id AS key_id \
         FROM webhook_targets WHERE id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .optional()
}

/// Each target with its sealed secret, for the rotation of
/// `KAIROS_SECRETS_KEY`. The rows are locked.
pub fn secrets_for_update(
    conn: &mut PgConnection,
) -> Result<Vec<(Uuid, SealedSecret)>, DieselError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
        #[diesel(embed)]
        secret: SealedSecret,
    }
    let rows: Vec<Row> = sql_query(
        "SELECT id, secret_ciphertext AS ciphertext, secret_nonce AS nonce, \
             secret_key_id AS key_id \
         FROM webhook_targets ORDER BY id FOR UPDATE",
    )
    .load(conn)?;
    Ok(rows.into_iter().map(|r| (r.id, r.secret)).collect())
}

/// Replace the sealed secret of the target `id`.
pub fn set_secret(
    conn: &mut PgConnection,
    id: Uuid,
    secret: &SealedSecret,
) -> Result<bool, DieselError> {
    let n = sql_query(
        "UPDATE webhook_targets SET secret_ciphertext = $2, secret_nonce = $3, \
             secret_key_id = $4, updated_at = now() \
         WHERE id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Bytea, _>(&secret.ciphertext)
    .bind::<Bytea, _>(&secret.nonce)
    .bind::<Text, _>(&secret.key_id)
    .execute(conn)?;
    Ok(n == 1)
}

/// Remove the target `id`, with its deliveries and their log. False: no
/// such target.
pub fn delete_target(conn: &mut PgConnection, id: Uuid) -> Result<bool, DieselError> {
    let n = sql_query("DELETE FROM webhook_targets WHERE id = $1")
        .bind::<SqlUuid, _>(id)
        .execute(conn)?;
    Ok(n == 1)
}

/// Turn the target `id` on, set its failures in a row to 0, and queue its
/// `skipped` deliveries again ([`redeliver_skipped`], KAIROS-T-0577), in
/// one transaction. The second value: the deliveries queued again, oldest
/// first. `None`: no such target.
pub fn enable_target(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(Target, Vec<Delivery>)>, DieselError> {
    conn.transaction(|conn| {
        let target: Option<Target> = sql_query(format!(
            "UPDATE webhook_targets t SET enabled = true, consecutive_failures = 0, \
                 disabled_at = NULL, disabled_reason = NULL, updated_at = now() \
             WHERE t.id = $1 RETURNING {TARGET_COLUMNS}"
        ))
        .bind::<SqlUuid, _>(id)
        .get_result(conn)
        .optional()?;
        let Some(target) = target else {
            return Ok(None);
        };
        let redelivered = redeliver_skipped(conn, id, None)?;
        Ok(Some((target, redelivered)))
    })
}

/// Queue the `skipped` deliveries of the target `target_id` again
/// (KAIROS-T-0577): each one that is at most [`KEEP_DAYS`] old, and not
/// older than `since`. Each delivery keeps its id and its body, and becomes
/// `pending` with a new job. The job of a delivery waits from the time of
/// the event, so the queue takes the deliveries oldest first, and before
/// the newer ones of the same priority. The payload of the job names the
/// tries that the delivery had, so the log of the next try does not replace
/// a row. The deliveries queued again, oldest first. The caller checks that
/// the target is on.
pub fn redeliver_skipped(
    conn: &mut PgConnection,
    target_id: Uuid,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<Delivery>, DieselError> {
    conn.transaction(|conn| {
        let tenant = current_tenant(conn)?;
        let mut deliveries: Vec<Delivery> = sql_query(format!(
            "UPDATE webhook_deliveries d SET status = 'pending', last_error = NULL, \
                 next_try_at = NULL, finished_at = NULL \
             WHERE d.target_id = $1 AND d.status = 'skipped' \
               AND d.created_at >= now() - make_interval(days => $2) \
               AND ($3::timestamptz IS NULL OR d.created_at >= $3) \
             RETURNING {DELIVERY_COLUMNS}"
        ))
        .bind::<SqlUuid, _>(target_id)
        .bind::<Integer, _>(KEEP_DAYS as i32)
        .bind::<Nullable<Timestamptz>, _>(since)
        .load(conn)?;
        deliveries.sort_by_key(|d| (d.created_at, d.id));
        for delivery in &deliveries {
            jobs::enqueue(
                conn,
                &NewJob {
                    kind: JOB_KIND.to_string(),
                    tenant: Some(tenant.clone()),
                    payload: serde_json::json!({
                        "delivery_id": delivery.id,
                        "tries_before": delivery.tries,
                    }),
                    priority: JOB_PRIORITY,
                    dedupe_key: Some(format!("webhook:{tenant}:{}", delivery.id)),
                    run_after: Some(delivery.created_at),
                    max_tries: DEFAULT_MAX_TRIES,
                },
            )?;
        }
        Ok(deliveries)
    })
}

/// The owners of a target: the person who made it, and the persons who
/// manage its board (an explicit grant that covers `configure_boards`, or
/// `manage_structure` on the board or at the organization). An
/// organization admin with no grant is not an owner.
pub fn owners(conn: &mut PgConnection, target: &Target) -> Result<Vec<Uuid>, DieselError> {
    let rows: Vec<UserRow> = sql_query(
        r"SELECT DISTINCT user_id FROM (
              SELECT $2::uuid AS user_id
              UNION
              SELECT user_id FROM board_member_capabilities
               WHERE board_id = $1
                 AND ('configure_boards' LIKE replace(replace(replace(replace(
                          capability, '\', '\\'), '%', '\%'), '_', '\_'), '*', '%')
                      OR capability = '*')
              UNION
              SELECT user_id FROM structure_grants
               WHERE board_id IS NULL OR board_id = $1
          ) o ORDER BY user_id",
    )
    .bind::<SqlUuid, _>(target.board_id)
    .bind::<SqlUuid, _>(target.created_by)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| r.user_id).collect())
}

/// The body of a delivery: the id, the event, the time, the organization,
/// and the data of the event.
pub fn delivery_body(
    id: Uuid,
    event: &str,
    tenant: &str,
    created_at: DateTime<Utc>,
    data: &serde_json::Value,
) -> String {
    serde_json::json!({
        "id": id,
        "event": event,
        "created_at": created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "organization": tenant,
        "data": data,
    })
    .to_string()
}

/// Add a delivery of `event` with `data` to the target `target_id` (for the
/// dispatch rule `rule_id`, if any), and its job, in the transaction of the
/// caller. A target that is off gets a
/// delivery with the status `skipped` and no job. The finished deliveries
/// of the target older than [`KEEP_DAYS`] are removed. `None`: no such
/// target.
pub fn queue_delivery(
    conn: &mut PgConnection,
    target_id: Uuid,
    event: &str,
    item_id: Option<Uuid>,
    rule_id: Option<Uuid>,
    data: &serde_json::Value,
) -> Result<Option<Delivery>, DieselError> {
    conn.transaction(|conn| {
        let Some(target) = get_target(conn, target_id)? else {
            return Ok(None);
        };
        let tenant = current_tenant(conn)?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        let body = delivery_body(id, event, &tenant, now, data);
        let (status, error) =
            if target.enabled {
                ("pending", None)
            } else {
                (
                "skipped",
                Some("The target is off. Kairos sends the delivery when a board manager turns the \
                 target on again.".to_string()),
            )
            };
        let delivery: Delivery = sql_query(format!(
            "INSERT INTO webhook_deliveries AS d (id, target_id, event, item_id, body, status, \
                 last_error, created_at, finished_at, rule_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, \
                 CASE WHEN $6 = 'pending' THEN NULL ELSE $8 END, $9) \
             RETURNING {DELIVERY_COLUMNS}"
        ))
        .bind::<SqlUuid, _>(id)
        .bind::<SqlUuid, _>(target_id)
        .bind::<Text, _>(event)
        .bind::<Nullable<SqlUuid>, _>(item_id)
        .bind::<Text, _>(&body)
        .bind::<Text, _>(status)
        .bind::<Nullable<Text>, _>(error)
        .bind::<Timestamptz, _>(now)
        .bind::<Nullable<SqlUuid>, _>(rule_id)
        .get_result(conn)?;
        if delivery.status == "pending" {
            jobs::enqueue(
                conn,
                &NewJob {
                    kind: JOB_KIND.to_string(),
                    tenant: Some(tenant.clone()),
                    payload: serde_json::json!({ "delivery_id": id }),
                    priority: JOB_PRIORITY,
                    dedupe_key: Some(format!("webhook:{tenant}:{id}")),
                    run_after: None,
                    // The server sets the tries of its setting at each
                    // lease (KAIROS_WEBHOOK_MAX_TRIES).
                    max_tries: DEFAULT_MAX_TRIES,
                },
            )?;
        }
        sql_query(
            "DELETE FROM webhook_deliveries WHERE target_id = $1 AND finished_at IS NOT NULL \
               AND finished_at < now() - make_interval(days => $2)",
        )
        .bind::<SqlUuid, _>(target_id)
        .bind::<Integer, _>(KEEP_DAYS as i32)
        .execute(conn)?;
        Ok(Some(delivery))
    })
}

/// Delete at most `limit` deliveries that finished before `cutoff`, of
/// each target (the retention pass, KAIROS-T-0638, which calls it until a
/// page deletes fewer than `limit`). A pending delivery stays. The tries of
/// a delivery go with it. Returns the count.
pub fn delete_finished_before(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<usize, DieselError> {
    sql_query(
        "DELETE FROM webhook_deliveries WHERE id IN ( \
             SELECT id FROM webhook_deliveries WHERE finished_at < $1 LIMIT $2)",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .execute(conn)
}

/// The delivery `id`.
pub fn get_delivery(conn: &mut PgConnection, id: Uuid) -> Result<Option<Delivery>, DieselError> {
    sql_query(format!(
        "SELECT {DELIVERY_COLUMNS} FROM webhook_deliveries d WHERE d.id = $1"
    ))
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .optional()
}

/// The newest `limit` deliveries of the targets `target_ids`, newest
/// first; with `rule_id`, only the deliveries of that rule.
pub fn list_deliveries(
    conn: &mut PgConnection,
    target_ids: &[Uuid],
    rule_id: Option<Uuid>,
    before: Option<&Delivery>,
    limit: i64,
) -> Result<Vec<Delivery>, DieselError> {
    if target_ids.is_empty() {
        return Ok(Vec::new());
    }
    // KAIROS-T-0584: `before` is the cursor of the next page. The order is
    // the time, then the id, so two deliveries of one time keep their order.
    sql_query(format!(
        "SELECT {DELIVERY_COLUMNS} FROM webhook_deliveries d WHERE d.target_id = ANY($1) \
           AND ($2::uuid IS NULL OR d.rule_id = $2) \
           AND ($3::timestamptz IS NULL OR (d.created_at, d.id) < ($3, $4)) \
         ORDER BY d.created_at DESC, d.id DESC LIMIT $5"
    ))
    .bind::<Array<SqlUuid>, _>(target_ids)
    .bind::<Nullable<SqlUuid>, _>(rule_id)
    .bind::<Nullable<diesel::sql_types::Timestamptz>, _>(before.map(|d| d.created_at))
    .bind::<Nullable<SqlUuid>, _>(before.map(|d| d.id))
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .load(conn)
}

/// The tries of the deliveries `ids`, by delivery and try.
pub fn tries_of(conn: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<DeliveryTry>, DieselError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sql_query(
        "SELECT delivery_id, try AS try_number, at, ok, response_code, error, duration_ms \
         FROM webhook_delivery_tries WHERE delivery_id = ANY($1) ORDER BY delivery_id, try",
    )
    .bind::<Array<SqlUuid>, _>(ids)
    .load(conn)
}

/// The result of one try, to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TryResult {
    /// The number of the try (the tries of the job).
    pub try_number: i32,
    /// True: the target answered with a 2xx code.
    pub ok: bool,
    pub response_code: Option<i32>,
    /// Why the try failed. Never a secret.
    pub error: Option<String>,
    pub duration_ms: i32,
    /// True: no try follows (the last try of the job).
    pub last: bool,
    /// The time of the next try, when one follows.
    pub next_try_at: Option<DateTime<Utc>>,
}

/// What [`record_try`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The delivery after the write.
    pub delivery: Delivery,
    /// True: this try turned the target off. The delivery is failed, and
    /// no try follows.
    pub target_turned_off: bool,
}

/// Write the result of a try of the delivery `delivery_id`, in one
/// transaction: the row of the log, the status of the delivery, and the
/// failures in a row of the target. A failure that reaches `disable_after`
/// failures in a row turns the target off and tells its owners. `None`:
/// the delivery is gone.
pub fn record_try(
    conn: &mut PgConnection,
    delivery_id: Uuid,
    result: &TryResult,
    disable_after: i32,
) -> Result<Option<Recorded>, DieselError> {
    conn.transaction(|conn| {
        let Some(delivery) = get_delivery(conn, delivery_id)? else {
            return Ok(None);
        };
        // KAIROS-T-0749: the lock on the target comes first, as in
        // `delete_target`. The row of the try below takes a lock on the
        // delivery; taken before the lock on the target, it made a deadlock
        // with a removal of the target (target, then its deliveries). KEY
        // SHARE: tries that end at the same time do not wait for each other
        // (KAIROS-T-0637). A target removed in the meantime: the delivery is
        // gone.
        let live: Option<Target> = sql_query(format!(
            "SELECT {TARGET_COLUMNS} FROM webhook_targets t WHERE t.id = $1 FOR KEY SHARE"
        ))
        .bind::<SqlUuid, _>(delivery.target_id)
        .get_result(conn)
        .optional()?;
        if live.is_none() {
            return Ok(None);
        }
        sql_query(
            "INSERT INTO webhook_delivery_tries (delivery_id, try, ok, response_code, error, \
                 duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (delivery_id, try) DO UPDATE SET at = now(), ok = EXCLUDED.ok, \
                 response_code = EXCLUDED.response_code, error = EXCLUDED.error, \
                 duration_ms = EXCLUDED.duration_ms",
        )
        .bind::<SqlUuid, _>(delivery_id)
        .bind::<Integer, _>(result.try_number)
        .bind::<Bool, _>(result.ok)
        .bind::<Nullable<Integer>, _>(result.response_code)
        .bind::<Nullable<Text>, _>(result.error.as_deref())
        .bind::<Integer, _>(result.duration_ms)
        .execute(conn)?;
        let mut turned_off = false;
        if result.ok {
            // KAIROS-T-0637: no write when the count is 0 already. Up to 4
            // deliveries of one target end at the same time, and each write
            // of the row waits for the one before it.
            sql_query(
                "UPDATE webhook_targets SET consecutive_failures = 0 \
                 WHERE id = $1 AND consecutive_failures <> 0",
            )
            .bind::<SqlUuid, _>(delivery.target_id)
            .execute(conn)?;
        } else {
            let target: Option<Target> = sql_query(format!(
                "UPDATE webhook_targets t SET consecutive_failures = consecutive_failures + 1 \
                 WHERE t.id = $1 RETURNING {TARGET_COLUMNS}"
            ))
            .bind::<SqlUuid, _>(delivery.target_id)
            .get_result(conn)
            .optional()?;
            if let Some(target) = target
                && target.enabled
                && disable_after > 0
                && target.consecutive_failures >= disable_after
            {
                turn_off(conn, &target, result.error.as_deref())?;
                turned_off = true;
            }
        }
        let status = if result.ok {
            "delivered"
        } else if result.last || turned_off {
            "failed"
        } else {
            "pending"
        };
        let error = if turned_off && !result.last {
            Some(format!(
                "{} Kairos turned the target off, and does not try again.",
                result.error.as_deref().unwrap_or("The try failed.")
            ))
        } else {
            result.error.clone()
        };
        let delivery: Delivery = sql_query(format!(
            "UPDATE webhook_deliveries d SET status = $2, tries = GREATEST(d.tries, $3), \
                 response_code = $4, last_error = $5, \
                 next_try_at = CASE WHEN $2 = 'pending' THEN $6 ELSE NULL END, \
                 finished_at = CASE WHEN $2 = 'pending' THEN NULL ELSE now() END \
             WHERE d.id = $1 RETURNING {DELIVERY_COLUMNS}"
        ))
        .bind::<SqlUuid, _>(delivery_id)
        .bind::<Text, _>(status)
        .bind::<Integer, _>(result.try_number)
        .bind::<Nullable<Integer>, _>(result.response_code)
        .bind::<Nullable<Text>, _>(error)
        .bind::<Nullable<Timestamptz>, _>(result.next_try_at)
        .get_result(conn)?;
        Ok(Some(Recorded {
            delivery,
            target_turned_off: turned_off,
        }))
    })
}

/// Turn `target` off, and tell its owners.
fn turn_off(
    conn: &mut PgConnection,
    target: &Target,
    last_error: Option<&str>,
) -> Result<(), DieselError> {
    let reason = format!(
        "Kairos turned the target off after {} failed tries in a row. The last error: {}",
        target.consecutive_failures,
        last_error.unwrap_or("none")
    );
    sql_query(
        "UPDATE webhook_targets SET enabled = false, disabled_at = now(), disabled_reason = $2, \
             updated_at = now() \
         WHERE id = $1",
    )
    .bind::<SqlUuid, _>(target.id)
    .bind::<Text, _>(&reason)
    .execute(conn)?;
    let board: Option<String> = {
        use crate::schema::boards;
        boards::table
            .filter(boards::id.eq(target.board_id))
            .select(boards::slug)
            .first(conn)
            .optional()?
    };
    let text = format!(
        "Kairos turned off the webhook target {} of the board {} after {} failed tries in a \
         row. The last error: {} A board manager turns it on again in the board settings. Then \
         Kairos sends the deliveries that it skipped.",
        target.name,
        board.as_deref().unwrap_or("(removed)"),
        target.consecutive_failures,
        last_error.unwrap_or("none.")
    );
    let recipients = owners(conn, target)?;
    crate::notifications::notify(
        conn,
        &recipients,
        NotificationKind::DeliveryFailing,
        None,
        &text,
        Uuid::nil(),
    )?;
    Ok(())
}

/// Replace the body of the `pending` delivery `id` (KAIROS-T-0385: the
/// link of the data becomes a URL before the first try). False: the
/// delivery is not pending.
pub fn set_body(conn: &mut PgConnection, id: Uuid, body: &str) -> Result<bool, DieselError> {
    let n =
        sql_query("UPDATE webhook_deliveries SET body = $2 WHERE id = $1 AND status = 'pending'")
            .bind::<SqlUuid, _>(id)
            .bind::<Text, _>(body)
            .execute(conn)?;
    Ok(n == 1)
}

/// End a `pending` delivery with `status` (`failed` or `skipped`) and
/// `reason`, with no try. False: the delivery is not pending.
pub fn finish_delivery(
    conn: &mut PgConnection,
    id: Uuid,
    status: &str,
    reason: &str,
) -> Result<bool, DieselError> {
    let n = sql_query(
        "UPDATE webhook_deliveries SET status = $2, last_error = $3, next_try_at = NULL, \
             finished_at = now() \
         WHERE id = $1 AND status = 'pending'",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(status)
    .bind::<Text, _>(reason)
    .execute(conn)?;
    Ok(n == 1)
}
