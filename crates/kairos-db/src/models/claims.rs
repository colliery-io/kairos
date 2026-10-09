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
