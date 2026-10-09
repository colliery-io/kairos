//! The cancel of a task on the server (KAIROS-T-0362). The rules of a
//! cancel are in `kairos_db::task_cancellations`; this module has what
//! the two surfaces (REST and MCP) share:
//!
//! - the mark on the task DTO ([`attach_cancellations`]) and in the MCP
//!   text ([`cancel_text`]);
//! - the cancel itself, with who may cancel ([`cancel`]);
//! - the HTTP form of the errors ([`map_cancel_error`]).

use diesel::pg::PgConnection;
use kairos_client::types as dto;
use kairos_db::models::claims::TaskCancellation;
use kairos_db::task_cancellations::{self, CancelError, Cancelled};
use kairos_db::task_claims;
use serde_json::json;
use uuid::Uuid;

use crate::error::ApiError;

/// The capability of a cancel: the capability of a move. A cancel moves
/// the task to the done column.
pub const CAPABILITY: &str = "transition_items";

fn mark_dto(
    mark: &TaskCancellation,
    names: &std::collections::HashMap<Uuid, String>,
) -> dto::TaskCancellation {
    dto::TaskCancellation {
        reason: mark.reason.clone(),
        cancelled_by: mark.cancelled_by.to_string(),
        cancelled_by_name: names
            .get(&mark.cancelled_by)
            .cloned()
            .unwrap_or_else(|| mark.cancelled_by.to_string()),
        cancelled_at: mark.cancelled_at.to_rfc3339(),
    }
}

/// Put the cancel marks of the tasks on their DTOs. Two queries for the
/// list.
pub fn attach_cancellations(
    conn: &mut PgConnection,
    tasks: &mut [dto::Task],
) -> Result<(), diesel::result::Error> {
    let ids: Vec<Uuid> = tasks.iter().filter_map(|t| t.id.parse().ok()).collect();
    let marks = task_cancellations::marks_of(conn, &ids)?;
    if marks.is_empty() {
        return Ok(());
    }
    let users: Vec<Uuid> = marks.values().map(|m| m.cancelled_by).collect();
    let names = task_claims::display_names(conn, &users)?;
    for task in tasks {
        let Ok(id) = task.id.parse::<Uuid>() else {
            continue;
        };
        task.cancellation = marks.get(&id).map(|mark| mark_dto(mark, &names));
    }
    Ok(())
}

/// The line of `get_item` for a cancelled task:
/// `- cancelled: 2026-10-09T10:00:00Z by Alice. Reason: …`. Empty for a
/// task that is not cancelled.
pub fn cancel_text(conn: &mut PgConnection, task_id: Uuid) -> Result<String, ApiError> {
    let Some(mark) = task_cancellations::mark_of(conn, task_id).map_err(ApiError::internal)? else {
        return Ok(String::new());
    };
    let name = task_claims::display_names(conn, &[mark.cancelled_by])
        .map_err(ApiError::internal)?
        .remove(&mark.cancelled_by)
        .unwrap_or_else(|| mark.cancelled_by.to_string());
    Ok(format!(
        "- cancelled: {} by {name}. Reason: {}\n",
        mark.cancelled_at.format("%Y-%m-%dT%H:%M:%SZ"),
        mark.reason
    ))
}

/// Cancel the task: the capability check ([`CAPABILITY`] on the board of
/// the task), the claim warning, and the cancel. The ONE path of REST
/// `POST /api/tasks/{short_code}/cancel` and MCP `cancel_item`.
pub fn cancel(
    conn: &mut PgConnection,
    slug: &str,
    task_id: Uuid,
    board_id: Uuid,
    reason: &str,
    user: Uuid,
) -> Result<Cancelled, ApiError> {
    // The reason first: an empty reason is wrong for each caller.
    if reason.trim().is_empty() {
        return Err(map_cancel_error(CancelError::EmptyReason));
    }
    crate::api::require_capability(conn, slug, Some(board_id), user, CAPABILITY)?;
    crate::claims::note(conn, task_id, user)?;
    task_cancellations::cancel_task(conn, task_id, reason, user).map_err(map_cancel_error)
}

/// [`CancelError`] → HTTP.
pub fn map_cancel_error(e: CancelError) -> ApiError {
    match e {
        CancelError::TaskNotFound(_) => ApiError::not_found(e.to_string()),
        CancelError::EmptyReason => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "reason" }))
        }
        CancelError::AlreadyDone { .. } => ApiError::unprocessable("TASK_DONE", e.to_string()),
        CancelError::NoDoneColumn { .. } => {
            ApiError::unprocessable("NO_DONE_COLUMN", e.to_string())
        }
        CancelError::Database(db) => ApiError::internal(db),
    }
}
