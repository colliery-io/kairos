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
