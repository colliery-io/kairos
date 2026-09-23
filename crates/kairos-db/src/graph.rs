//! Relationship-graph services (KAIROS-T-0013, semantics per KAIROS-A-0001,
//! layering per KAIROS-A-0009): link/unlink for the five edge types with
//! application-enforced type rules and cycle prevention, plus the
//! both-direction neighbor query.
//!
//! The pure decisions live in [`kairos_core::graph`] — the type-rule matrix
//! ([`kairos_core::graph::check_link`]) and cycle detection over loaded
//! edges ([`kairos_core::graph::would_create_cycle`]). This module is the
//! SQL side of the same contract: it resolves both UUIDs to entity types
//! through the `entity_directory` view under an explicit `deleted_at IS
//! NULL` (so soft-deleted items are typed [`GraphError::ItemNotFound`]),
//! loads the relationship's existing edges for the cycle check (`parent`
//! and `blocks` only), inserts, and writes `activity_log`.
//!
//! Every public function operates in the CURRENT `search_path` tenant
//! schema and runs in its own transaction (same conventions as
//! [`crate::items`]).
//!
//! # Liveness (KAIROS-T-0156, widened by KAIROS-T-0158)
//!
//! `entity_directory` used to filter `deleted_at IS NULL` in its own body,
//! so every query here was live-only whether it said so or not. Under
//! KAIROS-A-0020 archiving is a visibility DEFAULT, and a default has to be
//! something a caller can widen — so the view now reports `deleted_at` and
//! each query states its own mode. T-0156 gave every join in this module
//! the predicate the view used to apply, which kept behaviour identical;
//! T-0158 then widened exactly two of them, on purpose:
//!
//! - [`neighbors_of`] (and so [`relationships_for`]) reports archived
//!   neighbours **marked** via [`Neighbor::archived_at`];
//! - [`item_subgraph`]'s hydration and degree passes report archived nodes
//!   **marked** via [`SubgraphNode::archived_at`].
//!
//! Both serve the same question — *what is this live item connected to?* —
//! and dropping an endpoint answers it with silence rather than with a
//! smaller, honest number. `item_relationships` rows are hard-deleted, so
//! the edge to an archived item is intact; only the join ever hid it. The
//! two surfaces are widened together because they draw the same edges (a
//! panel and an explorer), and ADR-20 warns that a half-applied visibility
//! rule is worse than none: an auditor who finds the row on one surface
//! reasonably assumes the others agree.
//!
//! **Every other `entity_directory` join here stays live-only and says
//! so**, because each answers a different question:
//!
//! - [`resolve_entity`] types the endpoints of a WRITE. Linking or
//!   unlinking archived work is a mutation of frozen material, so it stays
//!   [`GraphError::ItemNotFound`].
//! - [`children_progress`] / [`board_children_progress`] (through
//!   [`CHILD_COLUMNS_SQL`]) and [`blocks_summary`] are *rollups*: ADR-20
//!   rule 5 says archived work is not live work, so counting it would
//!   report progress that nobody is making.
//! - [`team_work_documents`], [`team_link_rollup`] and
//!   [`repository_link_rollup`] are default listings, which ADR-20 rule 3
//!   keeps unchanged.
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

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_query;
use diesel::sql_types::{Array, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::graph as rules;
use kairos_core::short_code::ItemType;

use crate::models::enums::{ActivityAction, RelationshipType, UnknownEnumValue};
use crate::models::graph::{ItemRelationship, NewActivityLogEntry, NewItemRelationship};

/// Errors from the relationship-graph services.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// No live item with this id exists in any entity table (unknown id or
    /// soft-deleted — every lookup here spells `deleted_at IS NULL`).
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
/// Live-only: `None` = unknown id or soft-deleted. Linking archived work
/// stays refused — an edge is a write, and writes see live rows only
/// (KAIROS-I-0015 D5).
fn resolve_entity(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(ItemType, String)>, GraphError> {
    let row: Option<DirectoryRow> = sql_query(
        "SELECT entity_type, short_code FROM entity_directory \
         WHERE id = $1 AND deleted_at IS NULL",
    )
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
    /// When this neighbour was archived, or `None` while it is live
    /// (KAIROS-T-0158). Archived neighbours are REPORTED, not hidden —
    /// see the module's `# Liveness` section — so every caller that
    /// renders a neighbour must render this too. ADR-20: anything serving
    /// an archived row says so, or an auditor mistakes it for live work.
    pub archived_at: Option<DateTime<Utc>>,
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
    #[diesel(sql_type = Nullable<Timestamptz>)]
    deleted_at: Option<DateTime<Utc>>,
}

impl NeighborRow {
    fn into_neighbor(self) -> Result<Neighbor, GraphError> {
        Ok(Neighbor {
            relationship: self.relationship,
            id: self.id,
            short_code: self.short_code,
            entity_type: parse_entity_type(&self.entity_type)?,
            title: self.title,
            archived_at: self.deleted_at,
        })
    }
}

/// One direction of [`relationships_for`]: edges where `item_id` sits in
/// `own_column`, hydrating the OTHER end (`other_column`) through
/// `entity_directory`. Each query is backed by the matching S-0004 index
/// (`idx_item_relationships_source` / `idx_item_relationships_target`).
///
/// **Archived-inclusive, marked** (KAIROS-T-0158). This join deliberately
/// carries no `deleted_at IS NULL`: an item's relationships are a property
/// of the item being viewed, which is usually LIVE, and dropping an
/// archived endpoint does not narrow that answer — it falsifies it. "What
/// did this initiative contain?" returned fewer children than the truth,
/// with nothing marked, nothing counted and no flag anywhere that could
/// recover them. So the row comes back and `archived_at` says what it is.
/// The default is inclusion rather than an opt-in flag precisely because
/// the silent answer was the wrong one to serve by default.
fn neighbors_of(
    conn: &mut PgConnection,
    item_id: Uuid,
    own_column: &str,
    other_column: &str,
) -> Result<Vec<Neighbor>, GraphError> {
    let rows: Vec<NeighborRow> = sql_query(format!(
        "SELECT r.relationship, d.id, d.short_code, d.entity_type, d.title, d.deleted_at \
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
/// relationship)` (S-0004). The item itself is not required to exist — an
/// unknown id simply has no edges to report — and an ARCHIVED item reports
/// its edges like any other, since `item_relationships` rows are
/// hard-deleted and so survive the archive intact (KAIROS-T-0158).
/// Neighbours carry [`Neighbor::archived_at`]; every caller that renders a
/// neighbour must render that too.
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
/// children through: documents never appear (no board position), and
/// off-board ADRs are excluded. Only `parent` edges are followed, so
/// supports/informs material never counts.
///
/// **Live-only, and deliberately so** (KAIROS-A-0020 rule 5, confirmed by
/// KAIROS-T-0158). Every branch spells `deleted_at IS NULL` in its own
/// body, so this does not depend on the filter `entity_directory` used to
/// apply. Archived work is not live work: a rollup that counted it would
/// report progress nobody is making, and would make an initiative look
/// less finished the more of its work had been put away. This is the
/// opposite call from [`neighbors_of`], for the opposite reason —
/// *containment* is a fact about the record, *progress* is a fact about
/// live work.
const CHILD_COLUMNS_SQL: &str = "SELECT id, column_id FROM strategies WHERE deleted_at IS NULL \
     UNION ALL SELECT id, column_id FROM initiatives WHERE deleted_at IS NULL \
     UNION ALL SELECT id, column_id FROM tasks WHERE deleted_at IS NULL \
     UNION ALL SELECT id, column_id FROM adrs \
         WHERE deleted_at IS NULL AND column_id IS NOT NULL";

/// Direct `parent`-edge children of `parent_id`, grouped by their board
/// column — ONE query, column order within each board (KAIROS-T-0080).
/// Live children only, through [`CHILD_COLUMNS_SQL`] (ADR-20 rule 5): the
/// item's relationship LIST names its archived children
/// ([`relationships_for`]), its progress rollup does not count them.
pub fn children_progress(
    conn: &mut PgConnection,
    parent_id: Uuid,
) -> Result<Vec<ChildColumnCount>, DieselError> {
    sql_query(format!(
        "SELECT bc.id AS column_id, bc.name AS column_name, bc.board_id, \
                bc.is_done, \
                EXISTS(SELECT 1 FROM board_columns d \
                       WHERE d.board_id = bc.board_id AND d.is_done \
                         AND d.deleted_at IS NULL) AS board_has_done, \
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
///
/// Live-only on BOTH ends and says so in the SQL: the parents come from a
/// per-family union filtered `deleted_at IS NULL`, the children from
/// [`CHILD_COLUMNS_SQL`]. Neither predicate is inherited from a view
/// (KAIROS-T-0156 moved them here), and neither is widened: ADR-20 rule 5,
/// archived work is not live work.
pub fn board_children_progress(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, ProgressCounts>, DieselError> {
    let rows: Vec<BoardProgressRow> = sql_query(format!(
        "SELECT r.source_id AS parent_id, bc.is_done, \
                EXISTS(SELECT 1 FROM board_columns d \
                       WHERE d.board_id = bc.board_id AND d.is_done \
                         AND d.deleted_at IS NULL) AS board_has_done, \
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
         JOIN entity_directory p ON p.id = r.source_id AND p.deleted_at IS NULL \
         LEFT JOIN tasks t ON t.id = r.source_id AND t.deleted_at IS NULL \
         WHERE r.relationship = 'supports' \
           AND (t.team_id = $1 OR p.board_id = $2) \
         ORDER BY d.short_code, p.short_code",
    )
    .bind::<SqlUuid, _>(team_id)
    .bind::<diesel::sql_types::Nullable<SqlUuid>, _>(delivery_board)
    .load(conn)
}

// ---------------------------------------------------------------------------
// Focal subgraph (KAIROS-T-0088, design in KAIROS-I-0008)
// ---------------------------------------------------------------------------

/// One hydrated node of a focal subgraph. `status` is the board column
/// name for workflow items and the editorial lifecycle for documents (the
/// A-0018 two-vocabulary split); `degree` is the node's TOTAL edge count
/// so clients can render `+N` for undisplayed neighbors.
#[derive(Debug, Clone, PartialEq)]
pub struct SubgraphNode {
    pub id: Uuid,
    pub short_code: String,
    pub entity_type: ItemType,
    pub title: String,
    pub status: String,
    pub depth: i32,
    pub degree: i64,
    /// When this node was archived, or `None` while it is live
    /// (KAIROS-T-0158). Archived nodes are DRAWN, distinctly — see
    /// [`item_subgraph`] — never omitted; a renderer that ignores this
    /// field is claiming archived work is live.
    pub archived_at: Option<DateTime<Utc>>,
}

/// One typed directed edge between two visible subgraph nodes. `depth` is
/// the minimum view depth at which BOTH endpoints are visible
/// (`max(depth(source), depth(target))`).
#[derive(Debug, Clone, PartialEq)]
pub struct SubgraphEdge {
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub relationship: RelationshipType,
    pub depth: i32,
}

#[derive(QueryableByName)]
struct DepthRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    depth: i32,
}

#[derive(QueryableByName)]
struct NodeHydrationRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    degree: i64,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(QueryableByName)]
struct EdgeRow {
    #[diesel(sql_type = SqlUuid)]
    source_id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    target_id: Uuid,
    #[diesel(sql_type = Text)]
    relationship: RelationshipType,
}

/// The focal subgraph around `root` (KAIROS-T-0088): every node reachable
/// within `depth` hops over ANY relationship type in EITHER direction,
/// plus ALL edges among the visible set — including cross-links the walk
/// did not discover first (the links the old panels view could never
/// show). Cycle-safe like [`crate::search`]'s traverse (the UNION
/// deduplicates `(id, depth)` rows and the depth bound terminates the
/// recursion). The root is included at depth 0; callers resolve unknown
/// roots before calling.
///
/// **Archived-inclusive, marked** (KAIROS-T-0158), for the same reason as
/// [`neighbors_of`] and so that the explorer and the relationships panel
/// cannot disagree about the same edges. Dropping archived nodes here was
/// worse than a missing row: the walk in step 1 reads `item_relationships`
/// directly, so it already hops THROUGH an archived item — hydration then
/// deleted the middle of the path and left the far side floating with no
/// route back to the focus. An archived root fared worse still, since
/// resolution admits one (ADR-20 rule 1) and hydration then dropped the
/// focus out of its own subgraph. `archived_at` marks every such node;
/// clients draw it distinctly rather than silently.
///
/// `degree` therefore counts every neighbour that would hydrate, archived
/// included — it exists so a client can render `+N` for what it is not
/// showing, and a count that disagreed with the node set would make `+N`
/// wrong.
pub fn item_subgraph(
    conn: &mut PgConnection,
    root: Uuid,
    depth: u32,
) -> Result<(Vec<SubgraphNode>, Vec<SubgraphEdge>), GraphError> {
    // 1. The walk: visited ids with their MINIMUM discovery depth.
    let visited: Vec<DepthRow> = sql_query(
        "WITH RECURSIVE walk(id, depth) AS (
             SELECT $1::uuid, 0
             UNION
             SELECT CASE WHEN r.source_id = w.id THEN r.target_id ELSE r.source_id END,
                    w.depth + 1
             FROM walk w
             JOIN item_relationships r ON (r.source_id = w.id OR r.target_id = w.id)
             WHERE w.depth < $2
         )
         SELECT id, MIN(depth)::int4 AS depth FROM walk GROUP BY id",
    )
    .bind::<SqlUuid, _>(root)
    .bind::<diesel::sql_types::Integer, _>(depth as i32)
    .load(conn)?;
    let depth_of: std::collections::HashMap<Uuid, i32> =
        visited.iter().map(|row| (row.id, row.depth)).collect();
    let ids: Vec<Uuid> = visited.iter().map(|row| row.id).collect();

    // 2. Hydrate the visited ids: entity_directory for identity (NO
    //    liveness predicate — archived nodes are reported, marked, see the
    //    doc comment), a per-family union for status, and a neighbour count
    //    for degree. `status_of` keeps an archived item's real column name,
    //    which is exactly the audit answer ADR-20 rule 1 asks for: the
    //    `column_id` FK is intact, so the row still knows where it stood
    //    when it was put away.
    let rows: Vec<NodeHydrationRow> = sql_query(
        "WITH status_of AS (
             SELECT s.id, bc.name AS status FROM strategies s
                 JOIN board_columns bc ON bc.id = s.column_id
             UNION ALL
             SELECT i.id, bc.name FROM initiatives i
                 JOIN board_columns bc ON bc.id = i.column_id
             UNION ALL
             SELECT t.id, bc.name FROM tasks t
                 JOIN board_columns bc ON bc.id = t.column_id
             UNION ALL
             SELECT a.id, COALESCE(bc.name, 'off-board') FROM adrs a
                 LEFT JOIN board_columns bc ON bc.id = a.column_id
             UNION ALL
             SELECT d.id, d.lifecycle FROM documents d
         )
         SELECT d.id, d.short_code, d.entity_type, d.title, s.status, d.deleted_at,
                (SELECT COUNT(*) FROM item_relationships r
                    JOIN entity_directory other
                      ON other.id = CASE WHEN r.source_id = d.id
                                         THEN r.target_id ELSE r.source_id END
                    WHERE r.source_id = d.id OR r.target_id = d.id) AS degree
         FROM entity_directory d
         JOIN status_of s ON s.id = d.id
         WHERE d.id = ANY($1)",
    )
    .bind::<Array<SqlUuid>, _>(&ids)
    .load(conn)?;
    let mut nodes = rows
        .into_iter()
        .map(|row| {
            Ok(SubgraphNode {
                depth: depth_of.get(&row.id).copied().unwrap_or_default(),
                entity_type: parse_entity_type(&row.entity_type)?,
                id: row.id,
                short_code: row.short_code,
                title: row.title,
                status: row.status,
                degree: row.degree,
                archived_at: row.deleted_at,
            })
        })
        .collect::<Result<Vec<_>, GraphError>>()?;
    nodes.sort_by(|a, b| a.short_code.cmp(&b.short_code));
    let visible: std::collections::HashSet<Uuid> = nodes.iter().map(|n| n.id).collect();

    // 3. ALL edges among the visible set (cross-links included).
    let visible_ids: Vec<Uuid> = visible.iter().copied().collect();
    let edge_rows: Vec<EdgeRow> = sql_query(
        "SELECT source_id, target_id, relationship FROM item_relationships
         WHERE source_id = ANY($1) AND target_id = ANY($1)
         ORDER BY relationship ASC, source_id ASC, target_id ASC",
    )
    .bind::<Array<SqlUuid>, _>(&visible_ids)
    .load(conn)?;
    let edges = edge_rows
        .into_iter()
        .map(|row| SubgraphEdge {
            depth: depth_of
                .get(&row.source_id)
                .copied()
                .unwrap_or_default()
                .max(depth_of.get(&row.target_id).copied().unwrap_or_default()),
            source_id: row.source_id,
            target_id: row.target_id,
            relationship: row.relationship,
        })
        .collect();
    Ok((nodes, edges))
}

// ---------------------------------------------------------------------------
// Blocks rollup for board cards (KAIROS-T-0091)
// ---------------------------------------------------------------------------

/// The dependency counts one board card shows (KAIROS-T-0091).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlocksCounts {
    /// Live incoming `blocks` edges (things blocking this item).
    pub blocked_by: i64,
    /// Live outgoing `blocks` edges (things this item blocks).
    pub blocks: i64,
}

#[derive(QueryableByName)]
struct BlocksRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    blocked_by: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    blocks: i64,
}

/// Blocked-by/blocks counts for a set of items in ONE grouped query
/// (never per item — the T-0080 rollup discipline). Items with no live
/// blocks edges simply have no entry.
///
/// **Live-only, and deliberately so** (ADR-20 rule 5, confirmed by
/// KAIROS-T-0158, which widened [`neighbors_of`] and [`item_subgraph`] but
/// not this): the join spells its own `deleted_at IS NULL` rather than
/// inheriting one from the view. A board card's "blocked by 2" is a claim
/// about work that can still move, and archived work cannot block
/// anything. The archived dependency is still visible on the item's
/// relationship list, where it reads as history rather than as a count of
/// things standing in the way.
pub fn blocks_summary(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, BlocksCounts>, DieselError> {
    let rows: Vec<BlocksRow> = sql_query(
        "SELECT n.id,
                COUNT(*) FILTER (WHERE r.target_id = n.id) AS blocked_by,
                COUNT(*) FILTER (WHERE r.source_id = n.id) AS blocks
         FROM unnest($1::uuid[]) AS n(id)
         JOIN item_relationships r
           ON (r.source_id = n.id OR r.target_id = n.id)
          AND r.relationship = 'blocks'
         JOIN entity_directory other
           ON other.id = CASE WHEN r.source_id = n.id
                              THEN r.target_id ELSE r.source_id END
          AND other.deleted_at IS NULL
         GROUP BY n.id",
    )
    .bind::<Array<SqlUuid>, _>(ids)
    .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.id,
                BlocksCounts {
                    blocked_by: row.blocked_by,
                    blocks: row.blocks,
                },
            )
        })
        .collect())
}

/// Forge links across a team's WORK (KAIROS-T-0101). A link qualifies
/// three ways, matching how the team's work is defined elsewhere:
///
/// 1. its item is a task with `team_id = {team}`;
/// 2. its item sits on the team's delivery board;
/// 3. its repository is OWNED by the team (`repositories.team_id`, A-0019).
///
/// **Kept adjacent to [`team_work_documents`] on purpose**: paths 1 and 2
/// are that function's exact predicate. If the definition of "this team's
/// work" ever changes, both must change together, and adjacency is what
/// makes that obvious.
///
/// `states` filters (the panel asks "what is in flight", not for merged
/// history) and `limit` caps a busy team's result set. `DISTINCT` on the
/// link: one row however many ways it qualified.
pub fn team_link_rollup(
    conn: &mut PgConnection,
    team_id: Uuid,
    delivery_board: Option<Uuid>,
    states: &[&str],
    limit: i64,
) -> Result<Vec<TeamLinkRow>, DieselError> {
    sql_query(
        "SELECT DISTINCT ON (l.id) \
             l.id, l.kind, l.external_id, l.title, l.url, l.state, l.author, \
             l.forge_updated_at, \
             r.forge, r.repo_full_name, \
             d.short_code AS item_short_code, d.title AS item_title \
         FROM item_links l \
         JOIN forge_connections c ON c.id = l.connection_id AND c.deleted_at IS NULL \
         JOIN repositories r ON r.id = c.repository_id \
         JOIN entity_directory d ON d.id = l.item_id AND d.deleted_at IS NULL \
         LEFT JOIN tasks t ON t.id = l.item_id AND t.deleted_at IS NULL \
         WHERE l.state = ANY($3) \
           AND (t.team_id = $1 OR d.board_id = $2 OR r.team_id = $1) \
         ORDER BY l.id, l.forge_updated_at DESC \
         LIMIT $4",
    )
    .bind::<SqlUuid, _>(team_id)
    .bind::<diesel::sql_types::Nullable<SqlUuid>, _>(delivery_board)
    .bind::<Array<Text>, _>(states.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .load(conn)
}

/// One row of [`team_link_rollup`] — the link plus the repo and the work
/// item it belongs to, so the panel can link both ways.
#[derive(Debug, Clone, QueryableByName)]
pub struct TeamLinkRow {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub kind: String,
    #[diesel(sql_type = Text)]
    pub external_id: String,
    #[diesel(sql_type = Text)]
    pub title: String,
    #[diesel(sql_type = Text)]
    pub url: String,
    #[diesel(sql_type = Text)]
    pub state: String,
    #[diesel(sql_type = Text)]
    pub author: String,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    pub forge_updated_at: chrono::DateTime<chrono::Utc>,
    #[diesel(sql_type = Text)]
    pub forge: String,
    #[diesel(sql_type = Text)]
    pub repo_full_name: String,
    #[diesel(sql_type = Text)]
    pub item_short_code: String,
    #[diesel(sql_type = Text)]
    pub item_title: String,
}

/// Forge links on ONE repository (KAIROS-T-0106): the repository detail's
/// in-flight panel. Same row shape as [`team_link_rollup`] so the API
/// renders both with one DTO; `states` and `limit` as there.
pub fn repository_link_rollup(
    conn: &mut PgConnection,
    repository_id: Uuid,
    states: &[&str],
    limit: i64,
) -> Result<Vec<TeamLinkRow>, DieselError> {
    sql_query(
        "SELECT l.id, l.kind, l.external_id, l.title, l.url, l.state, l.author, \
             l.forge_updated_at, \
             r.forge, r.repo_full_name, \
             d.short_code AS item_short_code, d.title AS item_title \
         FROM item_links l \
         JOIN forge_connections c ON c.id = l.connection_id AND c.deleted_at IS NULL \
         JOIN repositories r ON r.id = c.repository_id \
         JOIN entity_directory d ON d.id = l.item_id AND d.deleted_at IS NULL \
         WHERE r.id = $1 AND l.state = ANY($2) \
         ORDER BY l.forge_updated_at DESC \
         LIMIT $3",
    )
    .bind::<SqlUuid, _>(repository_id)
    .bind::<Array<Text>, _>(states.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .load(conn)
}
