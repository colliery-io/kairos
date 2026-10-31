//! The claim of a task (KAIROS-T-0359, KAIROS-A-0024 decisions 4 and 5).
//!
//! A task in Active has a claim: the person who works on it. A person is
//! always responsible, also when an agent does the work with the agent key
//! of the person.
//!
//! # When a claim starts and ends
//!
//! A column with `board_columns.claims` (the Active column of a delivery
//! board) holds claimed tasks, and a column with `keeps_claims` (the
//! Blocked column, KAIROS-T-0529) keeps them. The ONE place where a task
//! changes column ([`crate::boards::transition_task`] and
//! [`crate::boards::move_task`]) calls [`after_column_change`] in its
//! transaction:
//!
//! - The task moves into a column that keeps claims: nothing changes. A
//!   claim stays, whoever moves the task, and a task with no claim gets
//!   none. A block does not end the work (KAIROS-T-0529).
//! - The task has a claim and moves into a claims column (back from
//!   Blocked, or a board move): the claim stays.
//! - A PERSON moves a task with no claim into a claims column: the task
//!   gets a claim for the person (the actor of the move), with the agent
//!   key of the request ([`crate::agent_mark::current`]).
//! - A SERVICE ACCOUNT moves a task with no claim into a claims column:
//!   the task has no claim after the move (decision 5: machine work has
//!   no person, and a claim names a person).
//! - The task moves into any other column: the claim ends.
//!
//! Also: an archive of the task ends its claim ([`drop_claims`]), and so
//! does an admin who removes the flag from a column
//! ([`drop_claims_in_column`]). A person who leaves the organization loses
//! each claim ([`drop_claims_of_user`], KAIROS-T-0531): the task stays in
//! its column, free. A hand-off ([`hand_off`]) gives the claim to a named
//! person, and a release ([`release`]) leaves the task in the column with
//! no claim, free for anyone.
//!
//! # A stale claim
//!
//! A claim never ends by age. [`notify_stale_claims`] finds each claim
//! with no write for some days: no change of the task (`tasks.updated_at`)
//! and no change of its state (`task_state.updated_at`) since the claim
//! started. It tells the holder and the owner of the initiative of the
//! task (`claim_stale`), and records the report in
//! `task_claims.stale_notified_at`. A claim gets one report for each quiet
//! period: a write after the report starts a new period.
//!
//! Each change writes one `activity_log` row: `claim`, `hand_off` or
//! `release`, with the task as the entity.
//!
//! - `claim`: `claim:<user id>`, and ` from:<user id>` when the claim of a
//!   different person ended.
//! - `hand_off`: `hand_off:<user id or none>-><user id>`.
//! - `release`: `release:<user id> reason:<reason>`, where the reason is
//!   `release`, `transition`, `board_move`, `service_account`, `archive`,
//!   `column` or `left`.
//!
//! Each change also emits `claim_changed` (KAIROS-T-0574) with the
//! `change` `taken`, `handed_off`, `released` (a [`release`]) or `ended`
//! (each other end of a claim).
//!
//! Who may hand off or release is the rule of the server; this module
//! checks only the state of the task.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use crate::events::{self, EventKind};
use crate::models::claims::{NewTaskClaim, TaskClaim};
use crate::models::enums::ActivityAction;
use crate::models::graph::NewActivityLogEntry;
use crate::models::public::USER_KIND_HUMAN;

/// Errors of [`hand_off`] and [`release`].
#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    /// No live task with this id exists.
    #[error("The task {0} does not exist.")]
    TaskNotFound(Uuid),
    /// The task is not in a column that holds claims.
    #[error(
        "The task {short_code} is in the column {column:?}. Only a task in a column that holds \
         claims (Active) has a claim. Move the task to Active first."
    )]
    NotInClaimsColumn { short_code: String, column: String },
    /// `release`: the task has no claim.
    #[error("The task {short_code} has no claim. Kairos changed nothing.")]
    NoClaim { short_code: String },
    /// `hand_off` or `claim`: the person has the claim already.
    #[error("{name} has the claim on {short_code} already. Kairos changed nothing.")]
    AlreadyHolder { short_code: String, name: String },
    /// `claim`: a different person has the claim (KAIROS-T-0528).
    #[error(
        "{holder} has the claim on {short_code} since {since}. Ask for a hand-off (hand_off_item), \
         or wait for a release."
    )]
    Claimed {
        short_code: String,
        holder: String,
        holder_id: Uuid,
        since: DateTime<Utc>,
    },
    /// `hand_off`: the target is not a person (a service account, or no
    /// user).
    #[error(
        "Only a person can have a claim. {0} is not a person. Send a person of the organization."
    )]
    NotAPerson(String),
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// The claim of a task, or `None`.
pub fn claim_of(conn: &mut PgConnection, task_id: Uuid) -> Result<Option<TaskClaim>, DieselError> {
    use crate::schema::task_claims::dsl;
    dsl::task_claims
        .filter(dsl::task_id.eq(task_id))
        .select(TaskClaim::as_select())
        .first(conn)
        .optional()
}

/// The claims of the tasks in `task_ids`, by task. A task with no claim is
/// not in the map. One query.
pub fn claims_of(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
) -> Result<HashMap<Uuid, TaskClaim>, DieselError> {
    use crate::schema::task_claims::dsl;
    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(dsl::task_claims
        .filter(dsl::task_id.eq_any(task_ids))
        .select(TaskClaim::as_select())
        .load(conn)?
        .into_iter()
        .map(|claim| (claim.task_id, claim))
        .collect())
}

/// The display names of the users in `user_ids` (`public.users`). An
/// unknown id is not in the map. One query.
pub fn display_names(
    conn: &mut PgConnection,
    user_ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, DieselError> {
    use crate::schema::users;
    if user_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(users::table
        .filter(users::id.eq_any(user_ids))
        .select((users::id, users::display_name))
        .load::<(Uuid, String)>(conn)?
        .into_iter()
        .collect())
}

/// Whether `user_id` is a person (`users.kind = 'human'`). An unknown id
/// is not a person.
pub fn is_person(conn: &mut PgConnection, user_id: Uuid) -> Result<bool, DieselError> {
    use crate::schema::users;
    let kind: Option<String> = users::table
        .filter(users::id.eq(user_id))
        .select(users::kind)
        .first(conn)
        .optional()?;
    Ok(kind.as_deref() == Some(USER_KIND_HUMAN))
}

/// Whether the column holds claims. An unknown column does not.
pub fn column_claims(conn: &mut PgConnection, column_id: Uuid) -> Result<bool, DieselError> {
    Ok(column_flags(conn, column_id)?.0)
}

/// The two claim flags of a column, `(claims, keeps_claims)`
/// (KAIROS-T-0529). An unknown column has neither.
fn column_flags(conn: &mut PgConnection, column_id: Uuid) -> Result<(bool, bool), DieselError> {
    use crate::schema::board_columns::dsl;
    Ok(dsl::board_columns
        .filter(dsl::id.eq(column_id))
        .select((dsl::claims, dsl::keeps_claims))
        .first::<(bool, bool)>(conn)
        .optional()?
        .unwrap_or((false, false)))
}

/// One `activity_log` row about the claim of a task.
fn log(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    task_id: Uuid,
    details: String,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(task_id),
            entity_type: Some("task".to_string()),
            details,
        })
        .execute(conn)?;
    Ok(())
}

/// Give the claim of the task to `user_id`: a new row, or the row of the
/// task changes to the person. `claimed_at` is now.
fn put_claim(
    conn: &mut PgConnection,
    task_id: Uuid,
    user_id: Uuid,
    agent_key_id: Option<Uuid>,
) -> Result<TaskClaim, DieselError> {
    use crate::schema::task_claims::dsl;
    diesel::insert_into(dsl::task_claims)
        .values(NewTaskClaim {
            task_id,
            user_id,
            agent_key_id,
        })
        .on_conflict(dsl::task_id)
        .do_update()
        .set((
            dsl::user_id.eq(user_id),
            dsl::agent_key_id.eq(agent_key_id),
            dsl::claimed_at.eq(diesel::dsl::now),
            dsl::stale_notified_at.eq(None::<DateTime<Utc>>),
        ))
        .returning(TaskClaim::as_returning())
        .get_result(conn)
}

/// Remove the claim of the task and write the `release` row with
/// `reason`. Nothing when the task has no claim.
fn end_claim(
    conn: &mut PgConnection,
    task_id: Uuid,
    actor_id: Uuid,
    reason: &str,
) -> Result<Option<TaskClaim>, DieselError> {
    use crate::schema::task_claims::dsl;
    let ended: Option<TaskClaim> =
        diesel::delete(dsl::task_claims.filter(dsl::task_id.eq(task_id)))
            .returning(TaskClaim::as_returning())
            .get_result(conn)
            .optional()?;
    if let Some(claim) = &ended {
        log(
            conn,
            actor_id,
            ActivityAction::Release,
            task_id,
            format!("release:{} reason:{reason}", claim.user_id),
        )?;
        // KAIROS-T-0533: the next step was the plan of the person who held
        // the claim; the next holder starts from the facts.
        crate::task_state::clear_next_step(conn, task_id, actor_id)?;
        // KAIROS-T-0574.
        let change = if reason == "release" {
            events::ClaimChange::Released
        } else {
            events::ClaimChange::Ended
        };
        events::emit_claim_changed(conn, task_id, change, actor_id)?;
    }
    Ok(ended)
}

/// The claim rule for a task that moved from `from_column_id` to
/// `to_column_id` (module docs). `reason` is the reason of a `release`
/// row for a task that leaves a claims column: `transition` or
/// `board_move`. Call it in the transaction of the move, after the write
/// of the column.
pub(crate) fn after_column_change(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_column_id: Uuid,
    actor_id: Uuid,
    reason: &str,
) -> Result<(), DieselError> {
    let existing = claim_of(conn, task_id)?;
    let (claims, keeps_claims) = column_flags(conn, to_column_id)?;
    if keeps_claims {
        // KAIROS-T-0529: the column keeps a claim (Blocked): the claim
        // stays, whoever moves the task, and a task with no claim gets
        // none. A block does not end the work.
        return Ok(());
    }
    if !claims {
        end_claim(conn, task_id, actor_id, reason)?;
        return Ok(());
    }
    if existing.is_some() {
        // A move into a claims column of a task that has a claim (back
        // from Blocked, or a board move): the claim stays (KAIROS-T-0529).
        return Ok(());
    }
    if !is_person(conn, actor_id)? {
        // KAIROS-A-0024 decision 5: a service account takes no claim, and
        // the task is free after the move (module docs).
        return Ok(());
    }
    let agent_key = crate::agent_mark::current(conn)?;
    put_claim(conn, task_id, actor_id, agent_key)?;
    log(
        conn,
        actor_id,
        ActivityAction::Claim,
        task_id,
        format!("claim:{actor_id}"),
    )?;
    // KAIROS-T-0574.
    events::emit_claim_changed(conn, task_id, events::ClaimChange::Taken, actor_id)
}

/// The short code and the column of a live task, with the check that the
/// column holds or keeps claims (KAIROS-T-0529). Locks the task row
/// (KAIROS-T-0640).
fn claimable_task(conn: &mut PgConnection, task_id: Uuid) -> Result<String, ClaimError> {
    use crate::schema::{board_columns, tasks};
    // KAIROS-T-0640: lock the task row first, in the transaction of the
    // claim, the hand-off or the release. A second change of the claim of
    // the task waits, then reads the claim that the first one wrote. Only
    // the task row: changes of other tasks in the column do not wait.
    tasks::table
        .filter(tasks::id.eq(task_id))
        .select(tasks::id)
        .for_update()
        .first::<Uuid>(conn)
        .optional()?;
    let row: Option<(String, String, bool, bool)> = tasks::table
        .inner_join(board_columns::table)
        .filter(tasks::id.eq(task_id))
        .filter(tasks::deleted_at.is_null())
        .select((
            tasks::short_code,
            board_columns::name,
            board_columns::claims,
            board_columns::keeps_claims,
        ))
        .first(conn)
        .optional()?;
    let (short_code, column, claims, keeps) = row.ok_or(ClaimError::TaskNotFound(task_id))?;
    if !claims && !keeps {
        return Err(ClaimError::NotInClaimsColumn { short_code, column });
    }
    Ok(short_code)
}

/// Take the claim of a task in a claims column that has no claim
/// (KAIROS-T-0528). `user` must be a person. The claim carries the agent
/// key of the request, as a claim by a move does. A task with a claim is
/// refused with the holder.
pub fn claim(conn: &mut PgConnection, task_id: Uuid, user: Uuid) -> Result<TaskClaim, ClaimError> {
    conn.transaction::<_, ClaimError, _>(|conn| {
        let short_code = claimable_task(conn, task_id)?;
        if !is_person(conn, user)? {
            return Err(ClaimError::NotAPerson(user.to_string()));
        }
        if let Some(existing) = claim_of(conn, task_id)? {
            let name = display_names(conn, &[existing.user_id])?
                .remove(&existing.user_id)
                .unwrap_or_else(|| existing.user_id.to_string());
            if existing.user_id == user {
                return Err(ClaimError::AlreadyHolder { short_code, name });
            }
            return Err(ClaimError::Claimed {
                short_code,
                holder: name,
                holder_id: existing.user_id,
                since: existing.claimed_at,
            });
        }
        let agent_key = crate::agent_mark::current(conn)?;
        let claim = put_claim(conn, task_id, user, agent_key)?;
        log(
            conn,
            user,
            ActivityAction::Claim,
            task_id,
            format!("claim:{user}"),
        )?;
        // KAIROS-T-0386: the comment shows the claim.
        crate::writeback::on_item_changed(conn, task_id)?;
        events::emit_item_event_by_id(conn, EventKind::ItemUpdated, "task", task_id, user)?;
        events::emit_claim_changed(conn, task_id, events::ClaimChange::Taken, user)?;
        Ok(claim)
    })
}

/// Give the claim of a task in a claims column to the person `to_user`
/// (a hand-off). The task can have a claim or not. The claim has no agent
/// key: the agent of the new person did not make it. `actor_id` is the
/// person or the service account that sends the hand-off.
pub fn hand_off(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_user: Uuid,
    actor_id: Uuid,
) -> Result<TaskClaim, ClaimError> {
    conn.transaction::<_, ClaimError, _>(|conn| {
        let short_code = claimable_task(conn, task_id)?;
        if !is_person(conn, to_user)? {
            return Err(ClaimError::NotAPerson(to_user.to_string()));
        }
        let existing = claim_of(conn, task_id)?;
        if let Some(claim) = &existing
            && claim.user_id == to_user
        {
            let name = display_names(conn, &[to_user])?
                .remove(&to_user)
                .unwrap_or_else(|| to_user.to_string());
            return Err(ClaimError::AlreadyHolder { short_code, name });
        }
        let claim = put_claim(conn, task_id, to_user, None)?;
        // KAIROS-T-0533: the next step was the previous holder's plan.
        crate::task_state::clear_next_step(conn, task_id, actor_id)?;
        let from = existing.map_or_else(|| "none".to_string(), |c| c.user_id.to_string());
        log(
            conn,
            actor_id,
            ActivityAction::HandOff,
            task_id,
            format!("hand_off:{from}->{to_user}"),
        )?;
        // KAIROS-T-0381: the person who gets the claim is told.
        crate::notifications::on_hand_off(conn, task_id, to_user, actor_id)?;
        // KAIROS-T-0386: the comment shows the claim.
        crate::writeback::on_item_changed(conn, task_id)?;
        events::emit_item_event_by_id(conn, EventKind::ItemUpdated, "task", task_id, actor_id)?;
        events::emit_claim_changed(conn, task_id, events::ClaimChange::HandedOff, actor_id)?;
        Ok(claim)
    })
}

/// End the claim of a task in a claims column (a release). The task stays
/// in the column with no claim, free for anyone: [`claim`] takes it.
/// Returns the claim that ended.
pub fn release(
    conn: &mut PgConnection,
    task_id: Uuid,
    actor_id: Uuid,
) -> Result<TaskClaim, ClaimError> {
    conn.transaction::<_, ClaimError, _>(|conn| {
        let short_code = claimable_task(conn, task_id)?;
        let ended = end_claim(conn, task_id, actor_id, "release")?
            .ok_or(ClaimError::NoClaim { short_code })?;
        // KAIROS-T-0386: the comment shows the claim.
        crate::writeback::on_item_changed(conn, task_id)?;
        events::emit_item_event_by_id(conn, EventKind::ItemUpdated, "task", task_id, actor_id)?;
        Ok(ended)
    })
}

/// End the claims of the tasks in `task_ids` (an archive of the tasks).
pub(crate) fn drop_claims(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
    actor_id: Uuid,
    reason: &str,
) -> Result<(), DieselError> {
    for task_id in claims_of(conn, task_ids)?.into_keys() {
        end_claim(conn, task_id, actor_id, reason)?;
    }
    Ok(())
}

/// End the claims of the tasks in a column (an admin removed the flag
/// `claims` from it).
pub(crate) fn drop_claims_in_column(
    conn: &mut PgConnection,
    column_id: Uuid,
    actor_id: Uuid,
) -> Result<(), DieselError> {
    use crate::schema::tasks;
    let task_ids: Vec<Uuid> = tasks::table
        .filter(tasks::column_id.eq(column_id))
        .select(tasks::id)
        .load(conn)?;
    drop_claims(conn, &task_ids, actor_id, "column")
}

/// End each claim of the person `user_id`, who left the organization
/// (KAIROS-T-0531). Each task stays in its column with no claim, free for
/// anyone. The `release` row has the reason `left`. The members of the
/// team of the board of each task are told (`claim_dropped`). Call it in
/// the transaction that removes the membership. Returns the tasks.
pub fn drop_claims_of_user(
    conn: &mut PgConnection,
    user_id: Uuid,
    actor_id: Uuid,
) -> Result<Vec<Uuid>, DieselError> {
    use crate::schema::task_claims::dsl;
    let mut task_ids: Vec<Uuid> = dsl::task_claims
        .filter(dsl::user_id.eq(user_id))
        .select(dsl::task_id)
        .load(conn)?;
    task_ids.sort();
    let mut ended = Vec::with_capacity(task_ids.len());
    for task_id in task_ids {
        // KAIROS-T-0640: lock the task row, as a claim, a hand-off and a
        // release do. Then end the claim only when the person still has it.
        use crate::schema::tasks;
        tasks::table
            .filter(tasks::id.eq(task_id))
            .select(tasks::id)
            .for_update()
            .first::<Uuid>(conn)
            .optional()?;
        if claim_of(conn, task_id)?.is_none_or(|claim| claim.user_id != user_id) {
            continue;
        }
        ended.push(task_id);
    }
    for task_id in &ended {
        end_claim(conn, *task_id, actor_id, "left")?;
        crate::notifications::on_claim_dropped(conn, *task_id, user_id, actor_id)?;
        // KAIROS-T-0386: the comment shows the claim.
        crate::writeback::on_item_changed(conn, *task_id)?;
        events::emit_item_event_by_id(conn, EventKind::ItemUpdated, "task", *task_id, actor_id)?;
    }
    Ok(ended)
}

/// One claim that [`notify_stale_claims`] reports.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct StaleClaim {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    pub task_id: Uuid,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub short_code: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub title: String,
    /// The person who has the claim.
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    pub user_id: Uuid,
    /// The owner of the initiative of the task, when it has one.
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Uuid>)]
    pub owner_id: Option<Uuid>,
    /// The last write: the latest of the start of the claim, the change of
    /// the task and the change of its state.
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    pub last_write: DateTime<Utc>,
}

/// The last write of a claim `c` on the task `t`, with its state `s`
/// (module docs).
const LAST_WRITE_SQL: &str = "GREATEST(c.claimed_at, t.updated_at, \
     COALESCE(s.updated_at, c.claimed_at))";

/// Report each stale claim of the tenant of the current `search_path`
/// (module docs): a claim with no write for `days` days before `now`, and
/// no report since its last write. The holder and the owner of the
/// initiative of the task are told (`claim_stale`), as Kairos (no actor).
/// The claim stays. `days` 0 reports nothing. Returns the claims reported.
pub fn notify_stale_claims(
    conn: &mut PgConnection,
    days: u32,
    now: DateTime<Utc>,
) -> Result<Vec<StaleClaim>, DieselError> {
    use diesel::sql_types::Timestamptz;
    if days == 0 {
        return Ok(Vec::new());
    }
    let cutoff = now - chrono::Duration::days(i64::from(days));
    conn.transaction(|conn| {
        let stale: Vec<StaleClaim> = diesel::sql_query(format!(
            "SELECT c.task_id, t.short_code, t.title, c.user_id, \
                    (SELECT o.user_id FROM item_relationships r \
                       JOIN initiatives i ON i.id = r.source_id AND i.deleted_at IS NULL \
                       JOIN item_owners o ON o.item_id = i.id \
                      WHERE r.target_id = t.id AND r.relationship = 'parent' \
                      ORDER BY i.short_code LIMIT 1) AS owner_id, \
                    {last} AS last_write \
               FROM task_claims c \
               JOIN tasks t ON t.id = c.task_id AND t.deleted_at IS NULL \
               LEFT JOIN task_state s ON s.task_id = c.task_id \
              WHERE {last} < $1 \
                AND (c.stale_notified_at IS NULL OR c.stale_notified_at < {last}) \
              ORDER BY t.short_code \
              FOR UPDATE OF c SKIP LOCKED",
            last = LAST_WRITE_SQL,
        ))
        .bind::<Timestamptz, _>(cutoff)
        .load(conn)?;
        for claim in &stale {
            crate::notifications::on_claim_stale(conn, claim, now)?;
            use crate::schema::task_claims::dsl;
            diesel::update(dsl::task_claims.filter(dsl::task_id.eq(claim.task_id)))
                .set(dsl::stale_notified_at.eq(Some(now)))
                .execute(conn)?;
        }
        Ok(stale)
    })
}

/// [`notify_stale_claims`] in each tenant (KAIROS-T-0531): the work of
/// the periodic pass `stale-claims`. `progress` is called before each
/// tenant. Returns the count of the claims reported.
pub fn stale_claims_pass(
    conn: &mut PgConnection,
    days: u32,
    now: DateTime<Utc>,
    progress: &mut dyn FnMut(),
) -> Result<u64, crate::tenant::TenantError> {
    if days == 0 {
        return Ok(0);
    }
    let mut reported = 0u64;
    for tenant_info in crate::tenant::list_tenants(conn)? {
        progress();
        if !tenant_info.schema_exists {
            continue;
        }
        let schema = crate::tenant::tenant_schema_name(&tenant_info.slug);
        diesel::sql_query(format!("SET search_path TO \"{schema}\"")).execute(conn)?;
        let result = notify_stale_claims(conn, days, now);
        let reset = diesel::sql_query("SET search_path TO DEFAULT").execute(conn);
        reported += result?.len() as u64;
        reset?;
    }
    Ok(reported)
}
