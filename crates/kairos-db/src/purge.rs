//! The delete of a task for good (KAIROS-T-0362): a purge.
//!
//! An archive (a soft delete, [`crate::items::soft_delete_item_as`]) hides
//! a task and keeps each row, so a restore can bring it back. A purge
//! removes the task and the rows that are about the task only. Nothing
//! can bring it back.
//!
//! # What a purge removes
//!
//! In one transaction, [`purge_task`] deletes:
//!
//! - the row of the task (`tasks`), live or archived;
//! - its claim (`task_claims`), by `ON DELETE CASCADE`, and its cancel
//!   mark (`task_cancellations`), by a delete: the table holds the marks
//!   of initiatives too, and has no foreign key (KAIROS-T-0678);
//! - its content versions (`item_history`) and its notes (`item_notes`,
//!   KAIROS-T-0630);
//! - its metadata values (`item_metadata`);
//! - each edge to or from it (`item_relationships`), and each edge
//!   proposal to or from it (`edge_proposals`);
//! - its forge links (`item_links`), its `impacts` rows (`item_impacts`),
//!   its external references (`item_external_refs`, KAIROS-T-0570), its
//!   embeddings (`item_embeddings`, `item_chunks`) and its retired codes
//!   (`retired_codes`);
//! - its forge intake row (`forge_issue_intake`, KAIROS-T-0570). Intake
//!   makes nothing for an issue that has a row, so with the row kept, the
//!   issue of a purged task could never be imported again;
//! - the notifications about it (`notifications`, KAIROS-T-0381).
//!
//! A purge of a task that blocks (live, and not done) frees the items that
//! it blocks (KAIROS-T-0568): each one gets `blocker_cleared` and, with no
//! open blocker left, fires the `unblocked` dispatch rules
//! ([`crate::notifications::on_blocks_removed`]). The step reads the
//! dependants before the delete of the edges. The server purges an
//! archived task only (KAIROS-T-0570), and an archived task blocks
//! nothing: the archive freed its dependants.
//!
//! # What a purge keeps
//!
//! The `activity_log` rows about the task stay: the log is the audit of
//! the organization, and the retention sweeper removes its rows by age
//! ([`crate::retention`]). The purge adds one row, action `purge`, with
//! the details `short_code:<code> title:<title>`, so the log names what
//! is gone. The items at the other end of the edges stay.
//!
//! The retention sweeper does not remove items: it prunes `item_history`
//! and `activity_log` by age. This is the one path that removes an item,
//! and a later sweep of archived items can call it.
//!
//! Who may purge, and when, is the rule of the server: `purge` on the
//! board of the task, and an archived task (KAIROS-T-0570). This module
//! does the delete only.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use crate::events::{self, EventKind};
use crate::models::enums::ActivityAction;
use crate::models::graph::NewActivityLogEntry;

/// Errors of [`purge_task`].
#[derive(Debug, thiserror::Error)]
pub enum PurgeError {
    /// No task with this id exists, live or archived.
    #[error("The task {0} does not exist.")]
    TaskNotFound(Uuid),
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// What [`purge_task`] removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purged {
    pub short_code: String,
    pub title: String,
}

/// Delete a task for good (module docs). The task can be live or
/// archived.
pub fn purge_task(
    conn: &mut PgConnection,
    task_id: Uuid,
    actor_id: Uuid,
) -> Result<Purged, PurgeError> {
    conn.transaction::<_, PurgeError, _>(|conn| {
        use crate::schema::{
            edge_proposals, forge_issue_intake, item_chunks, item_embeddings, item_external_refs,
            item_history, item_impacts, item_links, item_metadata, item_relationships,
            retired_codes, tasks,
        };
        let found: Option<(String, String)> = tasks::table
            .filter(tasks::id.eq(task_id))
            .select((tasks::short_code, tasks::title))
            .first(conn)
            .optional()?;
        let (short_code, title) = found.ok_or(PurgeError::TaskNotFound(task_id))?;

        // The event first: it reads the placement of the row, and the
        // clients of the board refetch when the transaction commits.
        events::emit_item_event_by_id(conn, EventKind::ItemDeleted, "task", task_id, actor_id)?;
        // KAIROS-T-0568: the items that the task blocks now lose a blocker,
        // after the delete of the edges. A done or archived task blocked
        // nothing.
        let freed = if crate::notifications::blocks_now(conn, task_id)? {
            crate::dispatch::dependants_of(conn, &[task_id], &[task_id])?
        } else {
            Vec::new()
        };

        diesel::delete(
            item_relationships::table.filter(
                item_relationships::source_id
                    .eq(task_id)
                    .or(item_relationships::target_id.eq(task_id)),
            ),
        )
        .execute(conn)?;
        diesel::delete(
            edge_proposals::table.filter(
                edge_proposals::source_id
                    .eq(task_id)
                    .or(edge_proposals::target_id.eq(task_id)),
            ),
        )
        .execute(conn)?;
        diesel::delete(item_metadata::table.filter(item_metadata::item_id.eq(task_id)))
            .execute(conn)?;
        crate::notifications::delete_for_item(conn, task_id)?;
        crate::watches::delete_for_item(conn, task_id)?;
        diesel::delete(item_links::table.filter(item_links::item_id.eq(task_id))).execute(conn)?;
        diesel::delete(item_impacts::table.filter(item_impacts::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(item_external_refs::table.filter(item_external_refs::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(forge_issue_intake::table.filter(forge_issue_intake::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(item_history::table.filter(item_history::item_id.eq(task_id)))
            .execute(conn)?;
        crate::item_notes::delete_for_item(conn, task_id)?;
        diesel::delete(item_chunks::table.filter(item_chunks::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(item_embeddings::table.filter(item_embeddings::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(retired_codes::table.filter(retired_codes::item_id.eq(task_id)))
            .execute(conn)?;
        // KAIROS-T-0678: no cascade from tasks any more.
        diesel::delete(
            crate::schema::task_cancellations::table
                .filter(crate::schema::task_cancellations::task_id.eq(task_id)),
        )
        .execute(conn)?;
        diesel::delete(tasks::table.filter(tasks::id.eq(task_id))).execute(conn)?;
        crate::notifications::on_blocks_removed(
            conn,
            &freed,
            crate::notifications::Cleared::Purged,
            actor_id,
        )?;

        diesel::insert_into(crate::schema::activity_log::table)
            .values(NewActivityLogEntry {
                actor_id,
                action: ActivityAction::Purge,
                entity_id: Some(task_id),
                entity_type: Some("task".to_string()),
                details: format!("short_code:{short_code} title:{title}"),
            })
            .execute(conn)?;
        Ok(Purged { short_code, title })
    })
}
