//! Models for the tenant entity tables (KAIROS-A-0001/S-0004): strategies,
//! initiatives, tasks, documents, and ADRs. Each entity type has its own
//! fully typed table; all share one UUID space. `version` supports
//! optimistic concurrency (KAIROS-A-0004); `deleted_at` is soft delete.
//!
//! `created_by`/`updated_by` reference `public.users` (cross-schema,
//! application-enforced).

use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::boards::{Board, BoardColumn};
use super::enums::{BucketType, Complexity, TaskType};
use super::teams::Team;
use super::templates::Template;
use crate::schema::{adrs, documents, initiatives, strategies, tasks};

// ---------------------------------------------------------------------------
// strategies
// ---------------------------------------------------------------------------

/// A strategy (Flight Level 3, `strategies`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = strategies)]
#[diesel(belongs_to(Board))]
#[diesel(belongs_to(BoardColumn, foreign_key = column_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Strategy {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub hypothesis: Option<String>,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`Strategy`]; `id`/`version`/timestamps come from defaults.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = strategies)]
pub struct NewStrategy {
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub hypothesis: Option<String>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
}

/// Partial update for [`Strategy`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = strategies)]
pub struct StrategyChangeset {
    pub title: Option<String>,
    pub content: Option<String>,
    pub board_id: Option<Uuid>,
    pub column_id: Option<Uuid>,
    pub hypothesis: Option<Option<String>>,
    pub version: Option<i32>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// initiatives
// ---------------------------------------------------------------------------

/// An initiative (Flight Level 2, `initiatives`). The DDL enforces
/// `is_bucket = true <=> bucket_type IS NOT NULL`.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = initiatives)]
#[diesel(belongs_to(Board))]
#[diesel(belongs_to(BoardColumn, foreign_key = column_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Initiative {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub complexity: Option<Complexity>,
    pub is_bucket: bool,
    pub bucket_type: Option<BucketType>,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`Initiative`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = initiatives)]
pub struct NewInitiative {
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub complexity: Option<Complexity>,
    pub is_bucket: bool,
    pub bucket_type: Option<BucketType>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
}

/// Partial update for [`Initiative`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = initiatives)]
pub struct InitiativeChangeset {
    pub title: Option<String>,
    pub content: Option<String>,
    pub board_id: Option<Uuid>,
    pub column_id: Option<Uuid>,
    pub complexity: Option<Option<Complexity>>,
    pub is_bucket: Option<bool>,
    pub bucket_type: Option<Option<BucketType>>,
    pub version: Option<i32>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// tasks
// ---------------------------------------------------------------------------

/// A task/bug/tech-debt item (Flight Level 1, `tasks`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = tasks)]
#[diesel(belongs_to(Board))]
#[diesel(belongs_to(BoardColumn, foreign_key = column_id))]
#[diesel(belongs_to(Team))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Task {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub task_type: TaskType,
    pub team_id: Option<Uuid>,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`Task`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = tasks)]
pub struct NewTask {
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub task_type: TaskType,
    pub team_id: Option<Uuid>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
}

/// Partial update for [`Task`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = tasks)]
pub struct TaskChangeset {
    pub title: Option<String>,
    pub content: Option<String>,
    pub board_id: Option<Uuid>,
    pub column_id: Option<Uuid>,
    pub task_type: Option<TaskType>,
    pub team_id: Option<Option<Uuid>>,
    pub version: Option<i32>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// documents
// ---------------------------------------------------------------------------

/// A supporting document (`documents`); child of any entity via the
/// relationship graph. Documents do not live on boards.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = documents)]
#[diesel(belongs_to(Template, foreign_key = template_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Document {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub template_id: Option<Uuid>,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`Document`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = documents)]
pub struct NewDocument {
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub template_id: Option<Uuid>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
}

/// Partial update for [`Document`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = documents)]
pub struct DocumentChangeset {
    pub title: Option<String>,
    pub content: Option<String>,
    pub template_id: Option<Option<Uuid>>,
    pub version: Option<i32>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// adrs
// ---------------------------------------------------------------------------

/// An Architecture Decision Record (`adrs`). ADRs may live on the ADR board;
/// the DDL enforces board_id/column_id both NULL or both set.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = adrs)]
#[diesel(belongs_to(Board))]
#[diesel(belongs_to(BoardColumn, foreign_key = column_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Adr {
    pub id: Uuid,
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Option<Uuid>,
    pub column_id: Option<Uuid>,
    pub decision_maker: Option<String>,
    pub decision_date: Option<NaiveDate>,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`Adr`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = adrs)]
pub struct NewAdr {
    pub short_code: String,
    pub title: String,
    pub content: String,
    pub board_id: Option<Uuid>,
    pub column_id: Option<Uuid>,
    pub decision_maker: Option<String>,
    pub decision_date: Option<NaiveDate>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
}

/// Partial update for [`Adr`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = adrs)]
pub struct AdrChangeset {
    pub title: Option<String>,
    pub content: Option<String>,
    pub board_id: Option<Option<Uuid>>,
    pub column_id: Option<Option<Uuid>>,
    pub decision_maker: Option<Option<String>>,
    pub decision_date: Option<Option<NaiveDate>>,
    pub version: Option<i32>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}
