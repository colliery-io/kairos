//! The review state of a task (KAIROS-T-0718, COLLIERY-I-0662).
//!
//! A task has one review state, a reserved field like the claim, the cancel
//! mark and the live state: `not_needed`, `needed` or `done`, with who set
//! it when. No row: nobody decided yet. A coordinator sets it when the work
//! has a PR and no claim; a reviewer sets `done` with the record on the
//! task. The fleet reads it before anything moves to Completed; Kairos does
//! not gate the move here.
//!
//! One row for each task at most; a write replaces it ([`set`]). A write
//! that changes the value writes one `update` row to the activity log,
//! `review:<value>`, so the activity says who decided and when. The row
//! stays when the task reaches a done column. The state is not content: a
//! write makes no new version of the task; the server emits `item_updated`,
//! then `task_review_changed`, and queues the dispatch rules with the
//! trigger `review_changed` ([`crate::dispatch::on_review_changed`]).

use std::collections::HashMap;
use std::fmt;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use uuid::Uuid;

use crate::models::claims::{NewTaskReview, TaskReview};

/// A review state. The wire values are [`Review::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Review {
    /// The task needs no review before it is done.
    NotNeeded,
    /// The task needs a review; a reviewer takes it.
    Needed,
    /// The review is done; its record is on the task.
    Done,
}

impl Review {
    /// Each value, in the order of the life of a task.
    pub const ALL: [Review; 3] = [Review::NotNeeded, Review::Needed, Review::Done];

    /// The wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Review::NotNeeded => "not_needed",
            Review::Needed => "needed",
            Review::Done => "done",
        }
    }

    /// The value of a wire text, with no case and no space at the ends;
    /// `None` for a text that is no value.
    pub fn parse(text: &str) -> Option<Review> {
        let text = text.trim().to_ascii_lowercase();
        Review::ALL.into_iter().find(|v| v.as_str() == text)
    }

    /// The wire values, joined for a message: `not_needed, needed, done`.
    pub fn names() -> String {
        Review::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for Review {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Set the review state of a task. Returns the row, and whether the value
/// changed: a write of the value the task has writes no activity row.
pub fn set(
    conn: &mut PgConnection,
    task_id: Uuid,
    review: Review,
    actor: Uuid,
) -> Result<(TaskReview, bool), DieselError> {
    use crate::schema::task_review::dsl;
    conn.transaction::<_, DieselError, _>(|conn| {
        let current: Option<TaskReview> = dsl::task_review
            .filter(dsl::task_id.eq(task_id))
            .select(TaskReview::as_select())
            .for_update()
            .first(conn)
            .optional()?;
        let changed = current.as_ref().map(|c| c.review.as_str()) != Some(review.as_str());
        let row = NewTaskReview {
            task_id,
            review: review.as_str().to_string(),
            updated_by: actor,
        };
        let written = diesel::insert_into(dsl::task_review)
            .values(&row)
            .on_conflict(dsl::task_id)
            .do_update()
            .set((&row, dsl::updated_at.eq(diesel::dsl::now)))
            .returning(TaskReview::as_select())
            .get_result(conn)?;
        if changed {
            diesel::insert_into(crate::schema::activity_log::table)
                .values(crate::models::NewActivityLogEntry {
                    actor_id: actor,
                    action: crate::models::enums::ActivityAction::Update,
                    entity_id: Some(task_id),
                    entity_type: Some("task".to_string()),
                    details: format!("{}{}", crate::item_activity::REVIEW_PREFIX, review),
                })
                .execute(conn)?;
        }
        Ok((written, changed))
    })
}

/// The review state of a task, or `None`.
pub fn review_of(
    conn: &mut PgConnection,
    task_id: Uuid,
) -> Result<Option<TaskReview>, DieselError> {
    use crate::schema::task_review::dsl;
    dsl::task_review
        .filter(dsl::task_id.eq(task_id))
        .select(TaskReview::as_select())
        .first(conn)
        .optional()
}

/// The review states of the tasks in `task_ids`, by task. A task with no
/// review state is not in the map. One query.
pub fn reviews_of(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
) -> Result<HashMap<Uuid, TaskReview>, DieselError> {
    use crate::schema::task_review::dsl;
    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(dsl::task_review
        .filter(dsl::task_id.eq_any(task_ids))
        .select(TaskReview::as_select())
        .load(conn)?
        .into_iter()
        .map(|review| (review.task_id, review))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_parses_from_its_wire_text_with_no_case_and_no_space() {
        assert_eq!(Review::parse("needed"), Some(Review::Needed));
        assert_eq!(Review::parse(" Not_Needed "), Some(Review::NotNeeded));
        assert_eq!(Review::parse("DONE"), Some(Review::Done));
        assert_eq!(Review::parse("later"), None);
        assert_eq!(Review::parse(""), None);
        for value in Review::ALL {
            assert_eq!(Review::parse(value.as_str()), Some(value));
        }
    }

    #[test]
    fn the_names_are_in_the_order_of_the_life_of_a_task() {
        assert_eq!(Review::names(), "not_needed, needed, done");
        assert_eq!(Review::Needed.to_string(), "needed");
    }
}
