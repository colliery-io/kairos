//! Models for the tenant board tables (KAIROS-A-0002/S-0004): boards,
//! columns, allowed transitions, and board-scoped capability grants
//! (KAIROS-A-0006).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::BoardLevel;
use super::teams::Team;
use crate::schema::{board_columns, board_member_capabilities, board_transitions, boards};

// ---------------------------------------------------------------------------
// boards
// ---------------------------------------------------------------------------

/// A configurable board (`boards`). Workflow items have no hardcoded phase
/// enums — their state is the board column they occupy (KAIROS-A-0002).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = boards)]
#[diesel(belongs_to(Team))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Board {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub board_level: BoardLevel,
    /// Set for delivery boards (per-team); NULL otherwise.
    pub team_id: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The short-code prefix of the board (COLLIERY-T-3099). It is set when
    /// the board is created and never changes.
    pub code_prefix: String,
    /// A board policy (KAIROS-T-0641, [`crate::board_policy`]): on a
    /// delivery board, a task moves into a claims column only when its
    /// parent initiative is active. True by default.
    pub parent_gate: bool,
    /// A board policy (KAIROS-T-0643, [`crate::board_policy`]): on an
    /// initiative board, an initiative moves into its Active column only
    /// with an owner. True by default.
    pub owner_gate: bool,
    /// A board policy (KAIROS-T-0565, [`crate::board_policy`]): on a
    /// delivery board, a task with an open blocker does not move into a
    /// claims column or a done column. False by default.
    pub blockers_gate: bool,
}

/// Insert for [`Board`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = boards)]
pub struct NewBoard {
    pub name: String,
    pub slug: String,
    pub board_level: BoardLevel,
    pub team_id: Option<Uuid>,
    /// It must match `kairos_core::short_code::PREFIX_RULE`.
    pub code_prefix: String,
}

/// Partial update for [`Board`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = boards)]
pub struct BoardChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub board_level: Option<BoardLevel>,
    pub team_id: Option<Option<Uuid>>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// board_columns
// ---------------------------------------------------------------------------

/// A board column (`board_columns`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = board_columns)]
#[diesel(belongs_to(Board))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BoardColumn {
    pub id: Uuid,
    pub board_id: Uuid,
    pub name: String,
    /// Display ordering, 0-indexed.
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Occupants count as completed for children-progress rollups
    /// (KAIROS-T-0080). Admin-set; the dead-end heuristic only suggests.
    pub is_done: bool,
    /// Soft delete (KAIROS-T-0161, ADR-20 one level down). A removed
    /// column stops being part of the board — it does not render, it is
    /// not a transition endpoint, nothing new lands in it — but the row
    /// stays, because the archived cards still holding its FK would
    /// otherwise lose the name of the column they were put away in.
    pub deleted_at: Option<DateTime<Utc>>,
    /// A task that a person moves into this column gets a claim for the
    /// person, and loses it when it leaves (KAIROS-T-0359,
    /// [`crate::task_claims`]). The seed sets it on the Active column of a
    /// delivery board. Admin-set afterwards.
    pub claims: bool,
    /// The most live items that the column should hold (KAIROS-T-0371);
    /// `None`: no limit. A move over the limit is allowed, with a warning.
    pub wip_limit: Option<i32>,
    /// A task that has a claim keeps it in this column, and a task with
    /// no claim gets none here (KAIROS-T-0529). The seed sets it on the
    /// Blocked column of a delivery board: a block does not end the work.
    pub keeps_claims: bool,
}

/// Insert for [`BoardColumn`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = board_columns)]
pub struct NewBoardColumn {
    pub board_id: Uuid,
    pub name: String,
    pub position: i32,
    pub is_done: bool,
    pub claims: bool,
    pub keeps_claims: bool,
}

// ---------------------------------------------------------------------------
// board_transitions
// ---------------------------------------------------------------------------

/// An allowed column-to-column transition (`board_transitions`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = board_transitions)]
#[diesel(belongs_to(Board))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BoardTransition {
    pub id: Uuid,
    pub board_id: Uuid,
    pub from_column_id: Uuid,
    pub to_column_id: Uuid,
}

/// Insert for [`BoardTransition`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = board_transitions)]
pub struct NewBoardTransition {
    pub board_id: Uuid,
    pub from_column_id: Uuid,
    pub to_column_id: Uuid,
}

// ---------------------------------------------------------------------------
// board_member_capabilities
// ---------------------------------------------------------------------------

/// A board-scoped capability grant (`board_member_capabilities`,
/// KAIROS-A-0006 whitelist; composite PK `(board_id, user_id, capability)`
/// per the KAIROS-T-0009 constraint upgrade). `capability` is a specific
/// capability or a glob (`*`, `manage_*`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = board_member_capabilities)]
#[diesel(primary_key(board_id, user_id, capability))]
#[diesel(belongs_to(Board))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BoardMemberCapability {
    pub board_id: Uuid,
    pub user_id: Uuid,
    pub capability: String,
    pub granted_at: DateTime<Utc>,
    pub granted_by: Uuid,
}

/// Insert for [`BoardMemberCapability`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = board_member_capabilities)]
pub struct NewBoardMemberCapability {
    pub board_id: Uuid,
    pub user_id: Uuid,
    pub capability: String,
    pub granted_by: Uuid,
}
