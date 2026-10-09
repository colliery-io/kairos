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
//! - its claim (`task_claims`) and its cancel mark (`task_cancellations`),
//!   by `ON DELETE CASCADE`;
//! - its content versions (`item_history`);
//! - its metadata values (`item_metadata`);
//! - each edge to or from it (`item_relationships`), and each edge
//!   proposal to or from it (`edge_proposals`);
//! - its forge links (`item_links`), its `impacts` rows (`item_impacts`),
//!   its embeddings (`item_embeddings`, `item_chunks`) and its retired
//!   codes (`retired_codes`).
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
//! Who may purge is the rule of the server (`manage_tasks` on the board
//! of the task); this module does the delete only.

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
            edge_proposals, item_chunks, item_embeddings, item_history, item_impacts, item_links,
            item_metadata, item_relationships, retired_codes, tasks,
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
        diesel::delete(item_links::table.filter(item_links::item_id.eq(task_id))).execute(conn)?;
        diesel::delete(item_impacts::table.filter(item_impacts::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(item_history::table.filter(item_history::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(item_chunks::table.filter(item_chunks::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(item_embeddings::table.filter(item_embeddings::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(retired_codes::table.filter(retired_codes::item_id.eq(task_id)))
            .execute(conn)?;
        diesel::delete(tasks::table.filter(tasks::id.eq(task_id))).execute(conn)?;

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
