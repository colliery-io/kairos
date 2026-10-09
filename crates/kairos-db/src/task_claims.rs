//! The claim of a task (KAIROS-T-0359, KAIROS-A-0024 decisions 4 and 5).
//!
//! A task in Active has a claim: the person who works on it. A person is
//! always responsible, also when an agent does the work with the agent key
//! of the person.
//!
//! # When a claim starts and ends
//!
//! A column with `board_columns.claims` (the Active column of a delivery
//! board) holds claimed tasks. The ONE place where a task changes column
//! ([`crate::boards::transition_task`] and [`crate::boards::move_task`])
//! calls [`after_column_change`] in its transaction:
//!
//! - A PERSON moves the task into a claims column: the task gets a claim
//!   for the person (the actor of the move), with the agent key of the
//!   request ([`crate::agent_mark::current`]). A claim of a different
//!   person ends. A move by the person who has the claim keeps it.
//! - A SERVICE ACCOUNT moves the task into a claims column: the task has
//!   no claim after the move (decision 5: machine work has no person, and
//!   a claim names a person). A claim that the task had ends too: the
//!   person who had the task in the column before did not move it here,
//!   and the claim must not name a person who is not responsible for
//!   this move.
//! - The task leaves a claims column, to any column: the claim ends.
//!
//! Also: an archive of the task ends its claim ([`drop_claims`]), and so
//! does an admin who removes the flag from a column
//! ([`drop_claims_in_column`]). A hand-off ([`hand_off`]) gives the claim
//! to a named person, and a release ([`release`]) leaves the task in the
//! column with no claim, free for anyone.
//!
//! Each change writes one `activity_log` row: `claim`, `hand_off` or
//! `release`, with the task as the entity.
//!
//! - `claim`: `claim:<user id>`, and ` from:<user id>` when the claim of a
//!   different person ended.
//! - `hand_off`: `hand_off:<user id or none>-><user id>`.
//! - `release`: `release:<user id> reason:<reason>`, where the reason is
//!   `release`, `transition`, `board_move`, `service_account`, `archive`
//!   or `column`.
//!
//! Who may hand off or release is the rule of the server; this module
//! checks only the state of the task.

use std::collections::HashMap;

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
    /// `hand_off`: the person has the claim already.
    #[error("{name} has the claim on {short_code} already. Kairos changed nothing.")]
    AlreadyHolder { short_code: String, name: String },
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
    use crate::schema::board_columns::dsl;
    Ok(dsl::board_columns
        .filter(dsl::id.eq(column_id))
        .select(dsl::claims)
        .first::<bool>(conn)
        .optional()?
        .unwrap_or(false))
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
    if !column_claims(conn, to_column_id)? {
        end_claim(conn, task_id, actor_id, reason)?;
        return Ok(());
    }
    if !is_person(conn, actor_id)? {
        // KAIROS-A-0024 decision 5: a service account takes no claim, and
        // the task is free after the move (module docs).
        end_claim(conn, task_id, actor_id, "service_account")?;
        return Ok(());
    }
    match existing {
        Some(claim) if claim.user_id == actor_id => Ok(()),
        existing => {
            let agent_key = crate::agent_mark::current(conn)?;
            put_claim(conn, task_id, actor_id, agent_key)?;
            let details = match existing {
                Some(previous) => format!("claim:{actor_id} from:{}", previous.user_id),
                None => format!("claim:{actor_id}"),
            };
            log(conn, actor_id, ActivityAction::Claim, task_id, details)
        }
    }
}

/// The short code and the column of a live task, with the check that the
/// column holds claims.
fn claimable_task(conn: &mut PgConnection, task_id: Uuid) -> Result<String, ClaimError> {
    use crate::schema::{board_columns, tasks};
    let row: Option<(String, String, bool)> = tasks::table
        .inner_join(board_columns::table)
        .filter(tasks::id.eq(task_id))
        .filter(tasks::deleted_at.is_null())
        .select((
            tasks::short_code,
            board_columns::name,
            board_columns::claims,
        ))
        .first(conn)
        .optional()?;
    let (short_code, column, claims) = row.ok_or(ClaimError::TaskNotFound(task_id))?;
    if !claims {
        return Err(ClaimError::NotInClaimsColumn { short_code, column });
    }
    Ok(short_code)
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
        let from = existing.map_or_else(|| "none".to_string(), |c| c.user_id.to_string());
        log(
            conn,
            actor_id,
            ActivityAction::HandOff,
            task_id,
            format!("hand_off:{from}->{to_user}"),
        )?;
        events::emit_item_event_by_id(conn, EventKind::ItemUpdated, "task", task_id, actor_id)?;
        Ok(claim)
    })
}

/// End the claim of a task in a claims column (a release). The task stays
/// in the column with no claim, free for anyone. Returns the claim that
/// ended.
pub fn release(
    conn: &mut PgConnection,
    task_id: Uuid,
    actor_id: Uuid,
) -> Result<TaskClaim, ClaimError> {
    conn.transaction::<_, ClaimError, _>(|conn| {
        let short_code = claimable_task(conn, task_id)?;
        let ended = end_claim(conn, task_id, actor_id, "release")?
            .ok_or(ClaimError::NoClaim { short_code })?;
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
