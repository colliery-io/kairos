//! Models for the tenant graph & audit tables (KAIROS-A-0001/A-0004/S-0004):
//! the item relationship graph, append-only content history, and the
//! activity log.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::{ActivityAction, RelationshipType};
use crate::schema::{activity_log, item_history, item_relationships};

// ---------------------------------------------------------------------------
// item_relationships
// ---------------------------------------------------------------------------

/// A graph edge between two entities (`item_relationships`). Only UUIDs and
/// the relationship type — entities know what they are in their own tables
/// (KAIROS-A-0001).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = item_relationships)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ItemRelationship {
    pub id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub relationship: RelationshipType,
    pub created_at: DateTime<Utc>,
}

/// Insert for [`ItemRelationship`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = item_relationships)]
pub struct NewItemRelationship {
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub relationship: RelationshipType,
}

/// Partial update for [`ItemRelationship`] (edges are usually
/// insert/delete; retyping an edge is the supported update).
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = item_relationships)]
pub struct ItemRelationshipChangeset {
    pub relationship: Option<RelationshipType>,
}

// ---------------------------------------------------------------------------
// item_history
// ---------------------------------------------------------------------------

/// An append-only content version snapshot (`item_history`, KAIROS-A-0004).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = item_history)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ItemHistory {
    pub id: Uuid,
    pub item_id: Uuid,
    pub version: i32,
    pub title: String,
    pub content: String,
    pub edited_by: Uuid,
    pub edited_at: DateTime<Utc>,
}

/// Insert for [`ItemHistory`]. The table is append-only, so there is no
/// changeset struct on purpose.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = item_history)]
pub struct NewItemHistory {
    pub item_id: Uuid,
    pub version: i32,
    pub title: String,
    pub content: String,
    pub edited_by: Uuid,
}

// ---------------------------------------------------------------------------
// activity_log
// ---------------------------------------------------------------------------

/// An audit-trail entry (`activity_log`, KAIROS-A-0004): transitions,
/// relationship changes, capability grants/revocations, deletions.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = activity_log)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ActivityLogEntry {
    pub id: Uuid,
    pub actor_id: Uuid,
    pub action: ActivityAction,
    /// The item acted on (NULL for relationship actions).
    pub entity_id: Option<Uuid>,
    /// `'strategy' | 'initiative' | 'task' | 'document' | 'adr'` (free TEXT
    /// in the DDL).
    pub entity_type: Option<String>,
    /// Structured context, e.g. `"column:Draft->Active"`.
    pub details: String,
    pub occurred_at: DateTime<Utc>,
}

/// Insert for [`ActivityLogEntry`]. The log is append-only, so there is no
/// changeset struct on purpose.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = activity_log)]
pub struct NewActivityLogEntry {
    pub actor_id: Uuid,
    pub action: ActivityAction,
    pub entity_id: Option<Uuid>,
    pub entity_type: Option<String>,
    pub details: String,
}
