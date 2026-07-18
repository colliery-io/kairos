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
}

/// Insert for [`Board`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = boards)]
pub struct NewBoard {
    pub name: String,
    pub slug: String,
    pub board_level: BoardLevel,
    pub team_id: Option<Uuid>,
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
}

/// Insert for [`BoardColumn`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = board_columns)]
pub struct NewBoardColumn {
    pub board_id: Uuid,
    pub name: String,
    pub position: i32,
}

/// Partial update for [`BoardColumn`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = board_columns)]
pub struct BoardColumnChangeset {
    pub name: Option<String>,
    pub position: Option<i32>,
    pub updated_at: Option<DateTime<Utc>>,
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

/// Partial update for [`BoardTransition`] (rewiring an edge).
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = board_transitions)]
pub struct BoardTransitionChangeset {
    pub from_column_id: Option<Uuid>,
    pub to_column_id: Option<Uuid>,
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

/// Partial update for [`BoardMemberCapability`] (re-attribution; grants are
/// otherwise insert/delete).
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = board_member_capabilities)]
pub struct BoardMemberCapabilityChangeset {
    pub granted_at: Option<DateTime<Utc>>,
    pub granted_by: Option<Uuid>,
}
