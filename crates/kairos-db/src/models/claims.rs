//! Model of the claim of a task (`task_claims`, KAIROS-T-0359,
//! KAIROS-A-0024). The rules are in [`crate::task_claims`].

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::task_claims;

/// The claim of a task: the person who has the task in Active.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = task_claims)]
#[diesel(primary_key(task_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TaskClaim {
    pub task_id: Uuid,
    /// The person (`public.users.id`). Always a person, never a service
    /// account (KAIROS-A-0024 decision 5).
    pub user_id: Uuid,
    /// The agent key of the request that made the claim, or `None` when
    /// the person made it without an agent key, or at a hand-off.
    pub agent_key_id: Option<Uuid>,
    pub claimed_at: DateTime<Utc>,
}

/// Insert for [`TaskClaim`]. `claimed_at` is the default, `now()`.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = task_claims)]
pub struct NewTaskClaim {
    pub task_id: Uuid,
    pub user_id: Uuid,
    pub agent_key_id: Option<Uuid>,
}

/// The mark of a cancelled task (`task_cancellations`, KAIROS-T-0362).
/// The rules are in [`crate::task_cancellations`].
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::schema::task_cancellations)]
#[diesel(primary_key(task_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TaskCancellation {
    pub task_id: Uuid,
    /// The person or the service account that cancelled the task
    /// (`public.users.id`).
    pub cancelled_by: Uuid,
    /// Why the task is cancelled. Never empty.
    pub reason: String,
    pub cancelled_at: DateTime<Utc>,
}

/// Insert for [`TaskCancellation`]. `cancelled_at` is the default, `now()`.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::task_cancellations)]
pub struct NewTaskCancellation {
    pub task_id: Uuid,
    pub cancelled_by: Uuid,
    pub reason: String,
}

/// The live state of a task (`task_state`, KAIROS-T-0506): the branch,
/// the last commit, pushed or not, the PR, the next step, and who wrote it
/// when. One row for each task at most. The rules are in
/// [`crate::task_state`].
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::schema::task_state)]
#[diesel(primary_key(task_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TaskState {
    pub task_id: Uuid,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub pushed: bool,
    pub pr_url: Option<String>,
    pub next_step: Option<String>,
    pub updated_at: DateTime<Utc>,
    /// The person or the service account that wrote the state
    /// (`public.users.id`).
    pub updated_by: Uuid,
}

/// The review state of a task (KAIROS-T-0718): one row for each task at
/// most. No row: nobody decided yet.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::schema::task_review)]
#[diesel(primary_key(task_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TaskReview {
    pub task_id: Uuid,
    /// `not_needed`, `needed` or `done` ([`crate::task_review::Review`]).
    pub review: String,
    pub updated_at: DateTime<Utc>,
    /// The person or the service account that set it (`public.users.id`).
    pub updated_by: Uuid,
}

/// Insert or replace for [`TaskReview`]. `updated_at` is `now()`.
#[derive(Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::task_review)]
pub struct NewTaskReview {
    pub task_id: Uuid,
    pub review: String,
    pub updated_by: Uuid,
}

/// Insert or replace for [`TaskState`]. `updated_at` is `now()`.
#[derive(Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::task_state)]
#[diesel(treat_none_as_null = true)]
pub struct NewTaskState {
    pub task_id: Uuid,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub pushed: bool,
    pub pr_url: Option<String>,
    pub next_step: Option<String>,
    pub updated_by: Uuid,
}
