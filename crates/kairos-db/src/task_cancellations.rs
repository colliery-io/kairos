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
//! A move of the task to a column that is not done (a transition, or a
//! move to a different board) removes the mark ([`after_column_change`]).
//! The `cancel` row of `activity_log` stays, so the history keeps the
//! reason. An archive does not remove the mark: a restored task is
//! cancelled still.
//!
//! Who may cancel is the rule of the server (`transition_items`, the
//! capability of a move); this module checks only the state of the task.

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
    /// The task is in a done column already.
    #[error(
        "The task {short_code} is in the done column {column:?}. You can cancel only a task \
         that is not done."
    )]
    AlreadyDone { short_code: String, column: String },
    /// The board of the task has no live done column.
    #[error(
        "The board of the task {short_code} has no done column. An admin of the board must \
         mark a column as done first."
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
        let row: Option<(String, Uuid, String, bool)> = tasks::table
            .inner_join(board_columns::table)
            .filter(tasks::id.eq(task_id))
            .filter(tasks::deleted_at.is_null())
            .select((
                tasks::short_code,
                tasks::board_id,
                board_columns::name,
                board_columns::is_done,
            ))
            .first(conn)
            .optional()?;
        let (short_code, board_id, from_column, done) =
            row.ok_or(CancelError::TaskNotFound(task_id))?;
        if done {
            return Err(CancelError::AlreadyDone {
                short_code,
                column: from_column,
            });
        }
        let target: Option<(Uuid, String)> = board_columns::table
            .filter(board_columns::board_id.eq(board_id))
            .filter(board_columns::is_done.eq(true))
            .filter(board_columns::deleted_at.is_null())
            .order(board_columns::position.asc())
            .select((board_columns::id, board_columns::name))
            .first(conn)
            .optional()?;
        let (to_column_id, to_column) = target.ok_or_else(|| CancelError::NoDoneColumn {
            short_code: short_code.clone(),
        })?;

        diesel::update(tasks::table.filter(tasks::id.eq(task_id)))
            .set((
                tasks::column_id.eq(to_column_id),
                tasks::updated_by.eq(actor_id),
                tasks::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;

        let mark = {
            use crate::schema::task_cancellations::dsl;
            diesel::insert_into(dsl::task_cancellations)
                .values(NewTaskCancellation {
                    task_id,
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
                .get_result(conn)?
        };

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
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemTransitioned,
            "task",
            task_id,
            actor_id,
        )?;
        Ok(Cancelled {
            short_code,
            from_column,
            to_column,
            mark,
        })
    })
}

/// The mark rule for a task that moved to `to_column_id`: a column that
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
