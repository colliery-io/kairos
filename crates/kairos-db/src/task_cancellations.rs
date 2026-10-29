//! The cancel of a task (KAIROS-T-0362): a "won't do", with a reason.
//!
//! # What a cancel does
//!
//! In one transaction, [`cancel_task`]:
//!
//! 1. moves the task to the done column of its board (the first live
//!    column with `is_done`, by position). The transition graph of the
//!    board does not apply: a cancel is allowed from each column that is
//!    not done;
//! 2. writes the mark (`task_cancellations`): who cancelled the task,
//!    the reason, and the time;
//! 3. writes one `activity_log` row, action `cancel`, with the details
//!    `column:<from>-><to> reason:<reason>`;
//! 4. ends the claim of the task (the task leaves Active), with the
//!    reason `cancel` on the `release` row ([`crate::task_claims`]);
//! 5. sends the event `item_transitioned`.
//!
//! # When the mark ends
//!
//! A move of the task to a column that is not done removes the mark
//! ([`after_column_change`]): a reopen ([`crate::boards::reopen_task`],
//! KAIROS-T-0564), or a transition on a board with an edge out of done. A
//! done task does not move to a different board.
//! The `cancel` row of `activity_log` stays, so the history keeps the
//! reason. An archive does not remove the mark: a restored task is
//! cancelled still.
//!
//! Who may cancel is the rule of the server (`transition_items`, the
//! capability of a move); this module checks only the state of the task.
//!
//! # An initiative (KAIROS-T-0678)
//!
//! [`cancel_initiative`] does steps 1, 2, 3 and 5 for an initiative. An
//! initiative has no claim; its owner stays. The mark is in the same table,
//! keyed by the id of the item, so each reader that is keyed by the id
//! (the `[cancelled]` end of an edge, an open blocker, a card) reads it with
//! no change. A move of the initiative to a column that is not done removes
//! the mark. Kairos has no reopen of an initiative (KAIROS-A-0048).

use std::collections::HashMap;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use crate::events::{self, EventKind};
use crate::models::claims::{NewTaskCancellation, TaskCancellation};
use crate::models::enums::ActivityAction;
use crate::models::graph::NewActivityLogEntry;

/// Errors of [`cancel_task`].
#[derive(Debug, thiserror::Error)]
pub enum CancelError {
    /// No live task with this id exists.
    #[error("The task {0} does not exist.")]
    TaskNotFound(Uuid),
    /// The reason is empty or only white space.
    #[error("The reason is empty. Write the reason for the cancel.")]
    EmptyReason,
    /// No live initiative with this id exists (KAIROS-T-0678).
    #[error("The initiative {0} does not exist.")]
    InitiativeNotFound(Uuid),
    /// The item is in a done column already.
    #[error(
        "{short_code} is in the done column {column:?}. You can cancel only an item that is \
         not done."
    )]
    AlreadyDone { short_code: String, column: String },
    /// The board of the item has no live done column.
    #[error(
        "The board of {short_code} has no done column. An admin of the board must mark a \
         column as done first."
    )]
    NoDoneColumn { short_code: String },
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// What [`cancel_task`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancelled {
    pub short_code: String,
    pub from_column: String,
    pub to_column: String,
    /// The done column the task is in now (KAIROS-T-0566: the server
    /// checks its WIP limit).
    pub to_column_id: Uuid,
    pub mark: TaskCancellation,
}

/// The mark of a task, or `None`.
pub fn mark_of(
    conn: &mut PgConnection,
    task_id: Uuid,
) -> Result<Option<TaskCancellation>, DieselError> {
    use crate::schema::task_cancellations::dsl;
    dsl::task_cancellations
        .filter(dsl::task_id.eq(task_id))
        .select(TaskCancellation::as_select())
        .first(conn)
        .optional()
}

/// The marks of the tasks in `task_ids`, by task. One query.
pub fn marks_of(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
) -> Result<HashMap<Uuid, TaskCancellation>, DieselError> {
    use crate::schema::task_cancellations::dsl;
    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(dsl::task_cancellations
        .filter(dsl::task_id.eq_any(task_ids))
        .select(TaskCancellation::as_select())
        .load(conn)?
        .into_iter()
        .map(|mark| (mark.task_id, mark))
        .collect())
}

/// Cancel a live task (module docs). `reason` is trimmed, and it must not
/// be empty.
pub fn cancel_task(
    conn: &mut PgConnection,
    task_id: Uuid,
    reason: &str,
    actor_id: Uuid,
) -> Result<Cancelled, CancelError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(CancelError::EmptyReason);
    }
    conn.transaction::<_, CancelError, _>(|conn| {
        use crate::schema::{board_columns, tasks};
        // KAIROS-T-0534: the lock of the task row (not of its column) holds
        // a second cancel or move of the task until this one commits.
        tasks::table
            .filter(tasks::id.eq(task_id))
            .select(tasks::id)
            .for_update()
            .first::<Uuid>(conn)
            .optional()?;
        let row: Option<(String, Uuid, Uuid, String, bool)> = tasks::table
            .inner_join(board_columns::table)
            .filter(tasks::id.eq(task_id))
            .filter(tasks::deleted_at.is_null())
            .select((
                tasks::short_code,
                tasks::board_id,
                tasks::column_id,
                board_columns::name,
                board_columns::is_done,
            ))
            .first(conn)
            .optional()?;
        let (short_code, board_id, from_column_id, from_column, done) =
            row.ok_or(CancelError::TaskNotFound(task_id))?;
        if done {
            return Err(CancelError::AlreadyDone {
                short_code,
                column: from_column,
            });
        }
        let target = first_done_column(conn, board_id)?;
        let (to_column_id, to_column) = target.ok_or_else(|| CancelError::NoDoneColumn {
            short_code: short_code.clone(),
        })?;

        let updated = diesel::update(
            tasks::table
                .filter(tasks::id.eq(task_id))
                .filter(tasks::column_id.eq(from_column_id)),
        )
        .set((
            tasks::column_id.eq(to_column_id),
            tasks::updated_by.eq(actor_id),
            tasks::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
        if updated == 0 {
            // The task left the column of the read. Not reached while the
            // row lock holds; a done column is refused as above.
            let now: (String, bool) = tasks::table
                .inner_join(board_columns::table)
                .filter(tasks::id.eq(task_id))
                .select((board_columns::name, board_columns::is_done))
                .first(conn)?;
            return Err(match now {
                (column, true) => CancelError::AlreadyDone { short_code, column },
                _ => CancelError::Database(DieselError::NotFound),
            });
        }

        let mark = write_mark(conn, task_id, reason, actor_id)?;

        diesel::insert_into(crate::schema::activity_log::table)
            .values(NewActivityLogEntry {
                actor_id,
                action: ActivityAction::Cancel,
                entity_id: Some(task_id),
                entity_type: Some("task".to_string()),
                details: format!("column:{from_column}->{to_column} reason:{reason}"),
            })
            .execute(conn)?;
        // The done column holds no claims: the claim ends.
        crate::task_claims::after_column_change(conn, task_id, to_column_id, actor_id, "cancel")?;
        // KAIROS-T-0381: the person who created the task is told.
        crate::notifications::on_cancel(conn, task_id, reason, actor_id)?;
        // KAIROS-T-0567: a cancelled blocker still blocks. Its dependants
        // are told; Kairos unblocks nothing.
        crate::notifications::on_blocker_cancelled(conn, task_id, reason, actor_id)?;
        // KAIROS-T-0385: the dispatch rules of the done column.
        crate::dispatch::on_item_entered_column(conn, task_id, to_column_id, actor_id)?;
        // KAIROS-T-0386: the GitHub write-back of its pull requests.
        crate::writeback::on_item_changed(conn, task_id)?;
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemTransitioned,
            "task",
            task_id,
            actor_id,
        )?;
        // KAIROS-T-0574: the precise kind, after the coarse one.
        events::emit_item_event_by_id(conn, EventKind::TaskCancelled, "task", task_id, actor_id)?;
        Ok(Cancelled {
            short_code,
            from_column,
            to_column,
            to_column_id,
            mark,
        })
    })
}

/// Cancel a live initiative (module docs, KAIROS-T-0678). `reason` is
/// trimmed, and it must not be empty.
pub fn cancel_initiative(
    conn: &mut PgConnection,
    initiative_id: Uuid,
    reason: &str,
    actor_id: Uuid,
) -> Result<Cancelled, CancelError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(CancelError::EmptyReason);
    }
    conn.transaction::<_, CancelError, _>(|conn| {
        use crate::schema::{board_columns, initiatives};
        // The lock of the row, as for a task (KAIROS-T-0534).
        initiatives::table
            .filter(initiatives::id.eq(initiative_id))
            .select(initiatives::id)
            .for_update()
            .first::<Uuid>(conn)
            .optional()?;
        let row: Option<(String, Uuid, Uuid, String, bool)> = initiatives::table
            .inner_join(board_columns::table)
            .filter(initiatives::id.eq(initiative_id))
            .filter(initiatives::deleted_at.is_null())
            .select((
                initiatives::short_code,
                initiatives::board_id,
                initiatives::column_id,
                board_columns::name,
                board_columns::is_done,
            ))
            .first(conn)
            .optional()?;
        let (short_code, board_id, from_column_id, from_column, done) =
            row.ok_or(CancelError::InitiativeNotFound(initiative_id))?;
        if done {
            return Err(CancelError::AlreadyDone {
                short_code,
                column: from_column,
            });
        }
        let (to_column_id, to_column) =
            first_done_column(conn, board_id)?.ok_or_else(|| CancelError::NoDoneColumn {
                short_code: short_code.clone(),
            })?;
        diesel::update(
            initiatives::table
                .filter(initiatives::id.eq(initiative_id))
                .filter(initiatives::column_id.eq(from_column_id)),
        )
        .set((
            initiatives::column_id.eq(to_column_id),
            initiatives::updated_by.eq(actor_id),
            initiatives::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
        let mark = write_mark(conn, initiative_id, reason, actor_id)?;
        diesel::insert_into(crate::schema::activity_log::table)
            .values(NewActivityLogEntry {
                actor_id,
                action: ActivityAction::Cancel,
                entity_id: Some(initiative_id),
                entity_type: Some("initiative".to_string()),
                details: format!("column:{from_column}->{to_column} reason:{reason}"),
            })
            .execute(conn)?;
        // KAIROS-A-0038: a cancelled blocker still blocks. Its dependants
        // are told; Kairos unblocks nothing.
        crate::notifications::on_blocker_cancelled(conn, initiative_id, reason, actor_id)?;
        // KAIROS-T-0719: the dispatch rules of the done column, as for a task.
        crate::dispatch::on_item_entered_column(conn, initiative_id, to_column_id, actor_id)?;
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemTransitioned,
            "initiative",
            initiative_id,
            actor_id,
        )?;
        Ok(Cancelled {
            short_code,
            from_column,
            to_column,
            to_column_id,
            mark,
        })
    })
}

/// The first live done column of the board, by position.
fn first_done_column(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<Option<(Uuid, String)>, DieselError> {
    use crate::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .filter(board_columns::is_done.eq(true))
        .filter(board_columns::deleted_at.is_null())
        .order(board_columns::position.asc())
        .select((board_columns::id, board_columns::name))
        .first(conn)
        .optional()
}

/// Write the mark of the item `item_id` (a task or an initiative).
fn write_mark(
    conn: &mut PgConnection,
    item_id: Uuid,
    reason: &str,
    actor_id: Uuid,
) -> Result<TaskCancellation, DieselError> {
    use crate::schema::task_cancellations::dsl;
    diesel::insert_into(dsl::task_cancellations)
        .values(NewTaskCancellation {
            task_id: item_id,
            cancelled_by: actor_id,
            reason: reason.to_string(),
        })
        .on_conflict(dsl::task_id)
        .do_update()
        .set((
            dsl::cancelled_by.eq(actor_id),
            dsl::reason.eq(reason),
            dsl::cancelled_at.eq(diesel::dsl::now),
        ))
        .returning(TaskCancellation::as_returning())
        .get_result(conn)
}

/// The mark rule for a task or an initiative that moved to `to_column_id`: a column that
/// is not done removes the mark. Call it in the transaction of the move,
/// after the write of the column.
pub(crate) fn after_column_change(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_column_id: Uuid,
) -> Result<(), DieselError> {
    use crate::schema::{board_columns, task_cancellations};
    let done: bool = board_columns::table
        .filter(board_columns::id.eq(to_column_id))
        .select(board_columns::is_done)
        .first::<bool>(conn)
        .optional()?
        .unwrap_or(false);
    if !done {
        diesel::delete(task_cancellations::table.filter(task_cancellations::task_id.eq(task_id)))
            .execute(conn)?;
    }
    Ok(())
}
