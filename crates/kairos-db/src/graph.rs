//! Relationship-graph services (KAIROS-T-0013, semantics per KAIROS-A-0001,
//! layering per KAIROS-A-0009): link/unlink for the five edge types with
//! application-enforced type rules and cycle prevention, plus the
//! both-direction neighbor query.
//!
//! The pure decisions live in [`kairos_core::graph`] — the type-rule matrix
//! ([`kairos_core::graph::check_link`]) and cycle detection over loaded
//! edges ([`kairos_core::graph::would_create_cycle`]). This module is the
//! SQL side of the same contract: it resolves both UUIDs to entity types
//! through the `entity_directory` view (which filters `deleted_at IS
//! NULL`, so soft-deleted items are typed [`GraphError::ItemNotFound`]),
//! loads the relationship's existing edges for the cycle check (`parent`
//! and `blocks` only), inserts, and writes `activity_log`.
//!
//! Every public function operates in the CURRENT `search_path` tenant
//! schema and runs in its own transaction (same conventions as
//! [`crate::items`]).
//!
//! # Audit rows (KAIROS-A-0004 / S-0004)
//!
//! Link writes `action = 'relationship_add'`, unlink `action =
//! 'relationship_remove'`, with `details =
//! "relationship:{rel}:{source_short}->{target_short}"` (the S-0004 DDL
//! comment's format, e.g. `relationship:parent:ACME-I-0001->ACME-T-0005`).
//! `entity_id`/`entity_type` are NULL for relationship actions (per the
//! DDL comment and [`crate::models::graph::ActivityLogEntry`]) — the edge
//! itself is the subject, and both endpoints are named in `details`.
//!
//! # Duplicate edges
//!
//! `UNIQUE (source_id, target_id, relationship)` is the enforcement;
//! inserting an existing edge maps the unique violation to the typed
//! [`GraphError::AlreadyLinked`] (not an idempotent no-op), so callers can
//! distinguish "changed" from "already so" and the activity log records
//! only real mutations — the same convention as
//! [`crate::abac::grant_capability`].

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::graph as rules;
use kairos_core::short_code::ItemType;

use crate::models::enums::{ActivityAction, RelationshipType, UnknownEnumValue};
use crate::models::graph::{ItemRelationship, NewActivityLogEntry, NewItemRelationship};

/// Errors from the relationship-graph services.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// No live item with this id exists in any entity table (unknown id or
    /// soft-deleted — `entity_directory` filters `deleted_at IS NULL`).
    #[error("item {0} does not exist")]
    ItemNotFound(Uuid),
    /// An item cannot be related to itself (mirrors the DDL `CHECK
    /// (source_id != target_id)`).
    #[error("item {0} cannot be linked to itself")]
    SelfLink(Uuid),
    /// The `(relationship, source_type, target_type)` combination is
    /// outside the KAIROS-A-0001 type-rule matrix.
    #[error(transparent)]
    Rule(#[from] rules::GraphRuleError),
    /// Adding this edge would create a directed cycle in an acyclic
    /// relationship (`parent`/`blocks`, KAIROS-A-0001).
    #[error("{relationship} edge {source_id} -> {target_id} would create a cycle")]
    CycleDetected {
        relationship: RelationshipType,
        source_id: Uuid,
        target_id: Uuid,
    },
    /// The edge already exists (`UNIQUE (source_id, target_id,
    /// relationship)`) — nothing changed and no activity row was written.
    #[error("{relationship} edge {source_id} -> {target_id} already exists")]
    AlreadyLinked {
        relationship: RelationshipType,
        source_id: Uuid,
        target_id: Uuid,
    },
    /// No such edge exists to unlink.
    #[error("no {relationship} edge {source_id} -> {target_id} exists")]
    NotLinked {
        relationship: RelationshipType,
        source_id: Uuid,
        target_id: Uuid,
    },
    /// Any other database error.
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

/// The pure mirror of a stored [`RelationshipType`] (kairos-core carries no
/// diesel types, KAIROS-A-0009).
fn core_relationship(relationship: RelationshipType) -> rules::Relationship {
    match relationship {
        RelationshipType::Parent => rules::Relationship::Parent,
        RelationshipType::Supports => rules::Relationship::Supports,
        RelationshipType::Informs => rules::Relationship::Informs,
        RelationshipType::Supersedes => rules::Relationship::Supersedes,
        RelationshipType::Blocks => rules::Relationship::Blocks,
    }
}

/// Parse an `entity_directory.entity_type` value. The view only emits the
/// five literals; anything else is schema drift and fails loudly.
fn parse_entity_type(value: &str) -> Result<ItemType, GraphError> {
    ItemType::ALL
        .iter()
        .copied()
        .find(|t| t.entity_type() == value)
        .ok_or_else(|| {
            GraphError::Database(DieselError::DeserializationError(Box::new(
                UnknownEnumValue {
                    enum_name: "ItemType",
                    value: value.to_string(),
                },
            )))
        })
}

#[derive(QueryableByName)]
struct DirectoryRow {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    short_code: String,
}

/// Resolve a UUID to its live entity type and short code via
/// `entity_directory` (KAIROS-A-0001's answer to "what is entity X?").
/// `None` = unknown id or soft-deleted.
fn resolve_entity(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(ItemType, String)>, GraphError> {
    let row: Option<DirectoryRow> =
        sql_query("SELECT entity_type, short_code FROM entity_directory WHERE id = $1")
            .bind::<SqlUuid, _>(id)
            .get_result(conn)
            .optional()?;
    row.map(|row| Ok((parse_entity_type(&row.entity_type)?, row.short_code)))
        .transpose()
}

/// Insert one relationship `activity_log` row: `entity_id`/`entity_type`
/// NULL, both endpoints named in `details` (module docs).
fn log_relationship_activity(
    conn: &mut PgConnection,
    actor: Uuid,
    action: ActivityAction,
    relationship: RelationshipType,
    source_label: &str,
    target_label: &str,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id: actor,
            action,
            entity_id: None,
            entity_type: None,
            details: format!("relationship:{relationship}:{source_label}->{target_label}"),
        })
        .execute(conn)?;
    Ok(())
}

/// All existing edges of ONE relationship type in the current tenant
/// schema, as pure [`rules::Edge`]s for the cycle check.
fn load_edges(
    conn: &mut PgConnection,
    relationship: RelationshipType,
) -> Result<Vec<rules::Edge>, DieselError> {
    use crate::schema::item_relationships;

    Ok(item_relationships::table
        .filter(item_relationships::relationship.eq(relationship))
        .select((item_relationships::source_id, item_relationships::target_id))
        .load::<(Uuid, Uuid)>(conn)?
        .into_iter()
        .map(|(source_id, target_id)| rules::Edge {
            source_id,
            target_id,
        })
        .collect())
}

/// Create a `relationship` edge from `source_id` to `target_id`
/// (KAIROS-A-0001 orientation — see [`kairos_core::graph`]'s semantics
/// table), in ONE transaction:
///
/// 1. both UUIDs resolve to live entities via `entity_directory`
///    (unknown/soft-deleted → [`GraphError::ItemNotFound`]);
/// 2. the pure type-rule matrix admits the combination
///    ([`kairos_core::graph::check_link`] → [`GraphError::Rule`]);
/// 3. for the acyclic relationships (`parent`/`blocks`) the edge must not
///    close a directed cycle over the relationship's existing edges
///    ([`kairos_core::graph::would_create_cycle`] →
///    [`GraphError::CycleDetected`]);
/// 4. insert (duplicate → [`GraphError::AlreadyLinked`], module docs) and
///    write the `relationship_add` activity row.
pub fn link_items(
    conn: &mut PgConnection,
    source_id: Uuid,
    target_id: Uuid,
    relationship: RelationshipType,
    actor: Uuid,
) -> Result<ItemRelationship, GraphError> {
    conn.transaction::<_, GraphError, _>(|conn| {
        if source_id == target_id {
            return Err(GraphError::SelfLink(source_id));
        }
        let (source_type, source_code) =
            resolve_entity(conn, source_id)?.ok_or(GraphError::ItemNotFound(source_id))?;
        let (target_type, target_code) =
            resolve_entity(conn, target_id)?.ok_or(GraphError::ItemNotFound(target_id))?;

        let rel = core_relationship(relationship);
        rules::check_link(rel, source_type, target_type)?;

        if rel.requires_acyclicity() {
            let edges = load_edges(conn, relationship)?;
            if rules::would_create_cycle(&edges, source_id, target_id) {
                return Err(GraphError::CycleDetected {
                    relationship,
                    source_id,
                    target_id,
                });
            }
        }

        let inserted = diesel::insert_into(crate::schema::item_relationships::table)
            .values(NewItemRelationship {
                source_id,
                target_id,
                relationship,
            })
            .returning(ItemRelationship::as_returning())
            .get_result(conn);
        let created = match inserted {
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                return Err(GraphError::AlreadyLinked {
                    relationship,
                    source_id,
                    target_id,
                });
            }
            other => other?,
        };

        log_relationship_activity(
            conn,
            actor,
            ActivityAction::RelationshipAdd,
            relationship,
            &source_code,
            &target_code,
        )?;
        // KAIROS-T-0022: a relationship change touches both endpoints'
        // views — one thin event per endpoint, delivered on commit.
        for (item_type, item_id) in [(source_type, source_id), (target_type, target_id)] {
            crate::events::emit_item_event_by_id(
                conn,
                crate::events::EventKind::RelationshipChanged,
                item_type.entity_type(),
                item_id,
                actor,
            )?;
        }
        Ok(created)
    })
}

/// Remove the `relationship` edge from `source_id` to `target_id`, in ONE
/// transaction, writing the `relationship_remove` activity row. A missing
/// edge is the typed [`GraphError::NotLinked`] (nothing changed, no
/// activity row). Endpoints that have since been soft-deleted are labeled
/// by UUID in the activity details (their short code is no longer in
/// `entity_directory`); the edge is removed regardless.
pub fn unlink_items(
    conn: &mut PgConnection,
    source_id: Uuid,
    target_id: Uuid,
    relationship: RelationshipType,
    actor: Uuid,
) -> Result<(), GraphError> {
    conn.transaction::<_, GraphError, _>(|conn| {
        use crate::schema::item_relationships::dsl;

        let deleted = diesel::delete(
            dsl::item_relationships
                .filter(dsl::source_id.eq(source_id))
                .filter(dsl::target_id.eq(target_id))
                .filter(dsl::relationship.eq(relationship)),
        )
        .execute(conn)?;
        if deleted == 0 {
            return Err(GraphError::NotLinked {
                relationship,
                source_id,
                target_id,
            });
        }

        let source = resolve_entity(conn, source_id)?;
        let target = resolve_entity(conn, target_id)?;
        let source_label = source
            .as_ref()
            .map_or_else(|| source_id.to_string(), |(_, code)| code.clone());
        let target_label = target
            .as_ref()
            .map_or_else(|| target_id.to_string(), |(_, code)| code.clone());
        log_relationship_activity(
            conn,
            actor,
            ActivityAction::RelationshipRemove,
            relationship,
            &source_label,
            &target_label,
        )?;
        // KAIROS-T-0022: one thin event per still-live endpoint (a
        // soft-deleted endpoint has no view to refresh), delivered on
        // commit.
        for (entity, item_id) in [(source, source_id), (target, target_id)] {
            if let Some((item_type, _)) = entity {
                crate::events::emit_item_event_by_id(
                    conn,
                    crate::events::EventKind::RelationshipChanged,
                    item_type.entity_type(),
                    item_id,
                    actor,
                )?;
            }
        }
        Ok(())
    })
}

/// One neighbor of an item in the relationship graph, hydrated through
/// `entity_directory` (feeds the relationships endpoint and item detail —
/// KAIROS-T-0020).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    /// The edge type connecting the item to this neighbor.
    pub relationship: RelationshipType,
    /// The neighbor's id.
    pub id: Uuid,
    /// The neighbor's short code.
    pub short_code: String,
    /// The neighbor's entity type.
    pub entity_type: ItemType,
    /// The neighbor's title.
    pub title: String,
}

/// Both directions of an item's relationships, each grouped by
/// relationship type (ordered by relationship, then edge creation time).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemRelationships {
    /// Edges where the item is the SOURCE: the neighbor is the target
    /// (e.g. the item's children via `parent`, the documents it is
    /// supported by via `supports`, the items it blocks via `blocks`).
    pub outgoing: Vec<Neighbor>,
    /// Edges where the item is the TARGET: the neighbor is the source
    /// (e.g. the item's parent via `parent`, the documents informing it
    /// via `informs`, the items blocking it via `blocks`).
    pub incoming: Vec<Neighbor>,
}

#[derive(QueryableByName)]
struct NeighborRow {
    #[diesel(sql_type = Text)]
    relationship: RelationshipType,
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    title: String,
}

impl NeighborRow {
    fn into_neighbor(self) -> Result<Neighbor, GraphError> {
        Ok(Neighbor {
            relationship: self.relationship,
            id: self.id,
            short_code: self.short_code,
            entity_type: parse_entity_type(&self.entity_type)?,
            title: self.title,
        })
    }
}

/// One direction of [`relationships_for`]: edges where `item_id` sits in
/// `own_column`, hydrating the OTHER end (`other_column`) through
/// `entity_directory`. Each query is backed by the matching S-0004 index
/// (`idx_item_relationships_source` / `idx_item_relationships_target`).
/// Soft-deleted neighbors drop out (the view filters them), matching every
/// other read path.
fn neighbors_of(
    conn: &mut PgConnection,
    item_id: Uuid,
    own_column: &str,
    other_column: &str,
) -> Result<Vec<Neighbor>, GraphError> {
    let rows: Vec<NeighborRow> = sql_query(format!(
        "SELECT r.relationship, d.id, d.short_code, d.entity_type, d.title \
         FROM item_relationships r \
         JOIN entity_directory d ON d.id = r.{other_column} \
         WHERE r.{own_column} = $1 \
         ORDER BY r.relationship ASC, r.created_at ASC, d.short_code ASC"
    ))
    .bind::<SqlUuid, _>(item_id)
    .load(conn)?;
    rows.into_iter().map(NeighborRow::into_neighbor).collect()
}

/// Every relationship touching `item_id`, in BOTH directions, grouped by
/// relationship type (see [`ItemRelationships`] for orientation). Two
/// indexed lookups: `source_id = item` uses
/// `idx_item_relationships_source(source_id, relationship)` and
/// `target_id = item` uses `idx_item_relationships_target(target_id,
/// relationship)` (S-0004). The item itself is not required to exist —
/// an unknown or soft-deleted id simply has no live edges to report.
pub fn relationships_for(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<ItemRelationships, GraphError> {
    Ok(ItemRelationships {
        outgoing: neighbors_of(conn, item_id, "source_id", "target_id")?,
        incoming: neighbors_of(conn, item_id, "target_id", "source_id")?,
    })
}

// ---------------------------------------------------------------------------
// Children progress (KAIROS-T-0080)
// ---------------------------------------------------------------------------

/// One column bucket of a parent's direct children.
#[derive(Debug, Clone, PartialEq, QueryableByName)]
pub struct ChildColumnCount {
    #[diesel(sql_type = SqlUuid)]
    pub column_id: Uuid,
    #[diesel(sql_type = Text)]
    pub column_name: String,
    #[diesel(sql_type = SqlUuid)]
    pub board_id: Uuid,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub is_done: bool,
    /// Whether the column's BOARD has any done-flagged column at all —
    /// distinguishes "0 children done" from "done is not configured
    /// here", so clients can show composition only (KAIROS-T-0080).
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub board_has_done: bool,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub count: i64,
}

/// The live workflow-item id → column_id union the progress queries join
/// children through: soft-deleted rows drop out, documents never appear
/// (no board position), and off-board ADRs are excluded. Only `parent`
/// edges are followed, so supports/informs material never counts.
const CHILD_COLUMNS_SQL: &str = "SELECT id, column_id FROM strategies WHERE deleted_at IS NULL \
     UNION ALL SELECT id, column_id FROM initiatives WHERE deleted_at IS NULL \
     UNION ALL SELECT id, column_id FROM tasks WHERE deleted_at IS NULL \
     UNION ALL SELECT id, column_id FROM adrs \
         WHERE deleted_at IS NULL AND column_id IS NOT NULL";

/// Direct `parent`-edge children of `parent_id`, grouped by their board
/// column — ONE query, column order within each board (KAIROS-T-0080).
pub fn children_progress(
    conn: &mut PgConnection,
    parent_id: Uuid,
) -> Result<Vec<ChildColumnCount>, DieselError> {
    sql_query(format!(
        "SELECT bc.id AS column_id, bc.name AS column_name, bc.board_id, \
                bc.is_done, \
                EXISTS(SELECT 1 FROM board_columns d \
                       WHERE d.board_id = bc.board_id AND d.is_done) AS board_has_done, \
                COUNT(*) AS count \
         FROM item_relationships r \
         JOIN ({CHILD_COLUMNS_SQL}) c ON c.id = r.target_id \
         JOIN board_columns bc ON bc.id = c.column_id \
         WHERE r.source_id = $1 AND r.relationship = 'parent' \
         GROUP BY bc.id, bc.name, bc.board_id, bc.is_done, bc.position \
         ORDER BY bc.board_id ASC, bc.position ASC"
    ))
    .bind::<SqlUuid, _>(parent_id)
    .load(conn)
}

#[derive(QueryableByName)]
struct BoardProgressRow {
    #[diesel(sql_type = SqlUuid)]
    parent_id: Uuid,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    is_done: bool,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    board_has_done: bool,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// A parent's `(done, total, has_done_semantics)` children rollup.
/// `has_done` is false when NO board hosting the children has a
/// done-flagged column — clients then show composition only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgressCounts {
    pub done: i64,
    pub total: i64,
    pub has_done: bool,
}

/// Children rollups for EVERY item on `board_id` that has direct
/// children — one grouped query for the whole board, never per-item
/// (the N+1 the KAIROS-T-0080 design forbids).
pub fn board_children_progress(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, ProgressCounts>, DieselError> {
    let rows: Vec<BoardProgressRow> = sql_query(format!(
        "SELECT r.source_id AS parent_id, bc.is_done, \
                EXISTS(SELECT 1 FROM board_columns d \
                       WHERE d.board_id = bc.board_id AND d.is_done) AS board_has_done, \
                COUNT(*) AS count \
         FROM item_relationships r \
         JOIN ({CHILD_COLUMNS_SQL}) c ON c.id = r.target_id \
         JOIN board_columns bc ON bc.id = c.column_id \
         JOIN (SELECT id FROM strategies WHERE board_id = $1 AND deleted_at IS NULL \
               UNION ALL SELECT id FROM initiatives WHERE board_id = $1 AND deleted_at IS NULL \
               UNION ALL SELECT id FROM tasks WHERE board_id = $1 AND deleted_at IS NULL \
               UNION ALL SELECT id FROM adrs WHERE board_id = $1 AND deleted_at IS NULL) p \
           ON p.id = r.source_id \
         WHERE r.relationship = 'parent' \
         GROUP BY r.source_id, bc.is_done, bc.board_id"
    ))
    .bind::<SqlUuid, _>(board_id)
    .load(conn)?;
    let mut progress: std::collections::HashMap<Uuid, ProgressCounts> =
        std::collections::HashMap::new();
    for row in rows {
        let entry = progress.entry(row.parent_id).or_default();
        entry.total += row.count;
        entry.has_done |= row.board_has_done;
        if row.is_done {
            entry.done += row.count;
        }
    }
    Ok(progress)
}

// ---------------------------------------------------------------------------
// Derived team work-documents (KAIROS-T-0084)
// ---------------------------------------------------------------------------

/// One document attached to a team's work, with its supports-parent for
/// attribution.
#[derive(Debug, Clone, QueryableByName)]
pub struct TeamWorkDocument {
    #[diesel(sql_type = Text)]
    pub short_code: String,
    #[diesel(sql_type = Text)]
    pub title: String,
    #[diesel(sql_type = Text)]
    pub lifecycle: String,
    #[diesel(sql_type = Text)]
    pub parent_short_code: String,
    #[diesel(sql_type = Text)]
    pub parent_title: String,
    #[diesel(sql_type = Text)]
    pub parent_type: String,
}

/// The documents attached to a team's WORK (KAIROS-T-0084): live documents
/// whose `supports` PARENT (edge source) is (a) a live task with `team_id
/// = {team}` or (b) any live item on the team's delivery board. One row
/// per document (`DISTINCT ON`), parent chosen deterministically
/// (lexicographically first short code), ordered by document short code.
///
/// Documents supporting org-level items deliberately do NOT appear — team
/// attribution follows the parent item. `delivery_board = None` (a team
/// without a delivery board) leaves only path (a).
pub fn team_work_documents(
    conn: &mut PgConnection,
    team_id: Uuid,
    delivery_board: Option<Uuid>,
) -> Result<Vec<TeamWorkDocument>, DieselError> {
    sql_query(
        "SELECT DISTINCT ON (d.short_code) \
             d.short_code, d.title, d.lifecycle, \
             p.short_code AS parent_short_code, \
             p.title AS parent_title, \
             p.entity_type AS parent_type \
         FROM item_relationships r \
         JOIN documents d ON d.id = r.target_id AND d.deleted_at IS NULL \
         JOIN entity_directory p ON p.id = r.source_id \
         LEFT JOIN tasks t ON t.id = r.source_id AND t.deleted_at IS NULL \
         WHERE r.relationship = 'supports' \
           AND (t.team_id = $1 OR p.board_id = $2) \
         ORDER BY d.short_code, p.short_code",
    )
    .bind::<SqlUuid, _>(team_id)
    .bind::<diesel::sql_types::Nullable<SqlUuid>, _>(delivery_board)
    .load(conn)
}
