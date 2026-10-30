//! The review state of a task on the server (KAIROS-T-0718): the write
//! shared by REST and MCP, the DTO, and the line of `get_item`. The rules
//! are in [`kairos_db::task_review`].
//!
//! Who may write it: the edit rule of the task
//! ([`crate::api::require_item_edit`]): the creator of the task, the holder
//! of its claim, or `manage_tasks` on its board. The review state is not content: a write
//! makes no new version of the task. A write that changes the value emits
//! `item_updated`, then `task_review_changed`, and queues the dispatch
//! rules with the trigger `review_changed`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::TaskReview;
use kairos_db::task_claims::display_names;
use kairos_db::task_review::{self, Review};
use uuid::Uuid;

use crate::error::ApiError;

/// The write of REST and MCP: the value, the edit rule, then the row. A
/// text that is no value is a 422 that names the values. A write of the
/// value the task has changes nothing and emits nothing.
pub fn write_review(
    conn: &mut PgConnection,
    role: kairos_db::models::OrgRole,
    user: Uuid,
    task_id: Uuid,
    body: &dto::SetTaskReviewRequest,
) -> Result<dto::TaskReview, ApiError> {
    let Some(review) = Review::parse(&body.review) else {
        return Err(ApiError::validation(format!(
            "{:?} is not a review state. The values are: {}.",
            body.review,
            Review::names()
        ))
        .with_details(
            serde_json::json!({ "field": "review", "values": Review::ALL.map(Review::as_str) }),
        ));
    };
    crate::api::require_item_edit(conn, role, user, task_id, ItemType::Task)?;
    let (row, changed) =
        task_review::set(conn, task_id, review, user).map_err(ApiError::internal)?;
    if changed {
        kairos_db::events::emit_item_event_by_id(
            conn,
            kairos_db::events::EventKind::ItemUpdated,
            "task",
            task_id,
            user,
        )
        .map_err(ApiError::internal)?;
        kairos_db::events::emit_item_event_by_id(
            conn,
            kairos_db::events::EventKind::TaskReviewChanged,
            "task",
            task_id,
            user,
        )
        .map_err(ApiError::internal)?;
        kairos_db::dispatch::on_review_changed(conn, task_id, review.as_str(), user)
            .map_err(ApiError::internal)?;
    }
    let names = display_names(conn, &[row.updated_by]).map_err(ApiError::internal)?;
    Ok(review_dto(&row, &names))
}

/// The DTO of a review row.
pub(crate) fn review_dto(row: &TaskReview, names: &HashMap<Uuid, String>) -> dto::TaskReview {
    dto::TaskReview {
        review: row.review.clone(),
        updated_at: row.updated_at.to_rfc3339(),
        updated_by: row.updated_by.to_string(),
        updated_by_name: names
            .get(&row.updated_by)
            .cloned()
            .unwrap_or_else(|| row.updated_by.to_string()),
    }
}

/// Put the review states of the tasks on their DTOs. Two queries for the
/// list.
pub fn attach_reviews<T: crate::api::convert::TaskDto>(
    conn: &mut PgConnection,
    tasks: &mut [T],
) -> Result<(), diesel::result::Error> {
    let ids: Vec<Uuid> = tasks
        .iter()
        .filter_map(|t| t.task_id().parse().ok())
        .collect();
    let reviews = task_review::reviews_of(conn, &ids)?;
    if reviews.is_empty() {
        return Ok(());
    }
    let users: Vec<Uuid> = reviews.values().map(|r| r.updated_by).collect();
    let names = display_names(conn, &users)?;
    for task in tasks {
        let Ok(id) = task.task_id().parse::<Uuid>() else {
            continue;
        };
        task.set_review(reviews.get(&id).map(|row| review_dto(row, &names)));
    }
    Ok(())
}

/// The line of the review state in `get_item`:
/// `- review: needed (set 5 min ago by Robin)`. `get_item` reads the review
/// in its facts query (KAIROS-T-0726).
pub fn review_line(review: &dto::TaskReview, now: DateTime<Utc>) -> String {
    format!("- review: {}\n", review_text(review, now))
}

/// The text of a review state: the value, and who set it when.
pub fn review_text(review: &dto::TaskReview, now: DateTime<Utc>) -> String {
    let when = DateTime::parse_from_rfc3339(&review.updated_at)
        .map(|at| crate::task_state::ago(at.with_timezone(&Utc), now))
        .unwrap_or_else(|_| review.updated_at.clone());
    format!(
        "{} (set {when} by {})",
        review.review, review.updated_by_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_names_the_value_and_who_set_it_when() {
        let now = DateTime::parse_from_rfc3339("2026-10-28T10:05:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let review = dto::TaskReview {
            review: "needed".into(),
            updated_at: "2026-10-28T10:00:00+00:00".into(),
            updated_by: "u1".into(),
            updated_by_name: "Robin".into(),
        };
        assert_eq!(review_text(&review, now), "needed (set 5 min ago by Robin)");
    }
}
