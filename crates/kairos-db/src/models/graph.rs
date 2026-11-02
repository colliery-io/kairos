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

// ---------------------------------------------------------------------------
// item_history
// ---------------------------------------------------------------------------

/// An append-only content version (`item_history`, KAIROS-A-0004). A full
/// row has the text in `content`. A diff row (KAIROS-T-0651) has an empty
/// `content`, and `content_diff` from the full row `base_version`: read
/// the text with [`crate::history::version_row`].
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
    /// The agent key of the request that wrote the row (`api_keys.id`), or
    /// `None` (KAIROS-T-0359). The actor is still the person. The column
    /// default fills it from the session setting `kairos.agent_key`, so
    /// the insert struct has no such field.
    pub agent_key_id: Option<Uuid>,
    /// The version of the full row that `content_diff` applies to, or
    /// `None` for a full row (KAIROS-T-0651).
    pub base_version: Option<i32>,
    /// The diff from the full row `base_version`
    /// ([`kairos_core::history_diff`]), or `None` for a full row.
    pub content_diff: Option<String>,
}

/// Insert of a full [`ItemHistory`] row. The table is append-only, so
/// there is no changeset struct on purpose. The writes of the item
/// services go through [`crate::history::insert_version`], which chooses
/// a full row or a diff row.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = item_history)]
pub struct NewItemHistory {
    pub item_id: Uuid,
    pub version: i32,
    pub title: String,
    pub content: String,
    pub edited_by: Uuid,
}

/// Insert of a diff [`ItemHistory`] row (KAIROS-T-0651): `content` is
/// empty, and `content_diff` applies to the full row `base_version`.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = item_history)]
pub struct NewItemHistoryDiff {
    pub item_id: Uuid,
    pub version: i32,
    pub title: String,
    pub content: String,
    pub edited_by: Uuid,
    pub base_version: i32,
    pub content_diff: String,
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
    /// The item acted on. For a relationship between two items it is the
    /// source of the edge (KAIROS-T-0638; NULL in rows from before it). For
    /// an `impacts` link it is the document or the ADR of the link
    /// (COLLIERY-T-0269).
    pub entity_id: Option<Uuid>,
    /// `'strategy' | 'initiative' | 'task' | 'document' | 'adr'` (free TEXT
    /// in the DDL).
    pub entity_type: Option<String>,
    /// Structured context, e.g. `"column:Draft->Active"`.
    pub details: String,
    pub occurred_at: DateTime<Utc>,
    /// The agent key of the request that wrote the row (`api_keys.id`), or
    /// `None` (KAIROS-T-0359). The actor is still the person. The column
    /// default fills it from the session setting `kairos.agent_key`, so
    /// the insert struct has no such field.
    pub agent_key_id: Option<Uuid>,
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
