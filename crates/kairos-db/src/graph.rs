//! Relationship-graph services (KAIROS-T-0013, semantics per KAIROS-A-0001,
//! layering per KAIROS-A-0009): link/unlink for the six edge types with
//! application-enforced type rules and cycle prevention, plus the
//! both-direction neighbor query.
//!
//! The pure decisions live in [`kairos_core::graph`] — the type-rule matrix
//! ([`kairos_core::graph::check_link`]). This module is the
//! SQL side of the same contract: it resolves both UUIDs to entity types
//! through the `entity_directory` view under an explicit `deleted_at IS
//! NULL` (so soft-deleted items are typed [`GraphError::ItemNotFound`]),
//! runs the cycle check in the database (`parent`, `blocks` and `amends`,
//! [`check_acyclic`]), inserts, and writes `activity_log`.
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
//! - [`blocks_summary`] is a *rollup*: ADR-20 rule 5 says archived work
//!   is not live work, so an archived item blocks nothing.
//! - [`children_progress`] / [`page_children_progress`] (through
//!   [`CHILD_COLUMNS_SQL`]) read archived children too (KAIROS-T-0571):
//!   a child archived from a done column is done work, and a child
//!   archived from another column is `removed`, not open work.
//! - [`team_work_documents`], [`team_link_rollup`] and
//!   [`repository_link_rollup`] are default listings, which ADR-20 rule 3
//!   keeps unchanged.
//!
//! # Done work and `blocks` (COLLIERY-T-0214)
//!
//! A `blocks` edge is a blocker only while the work at BOTH ends can still
//! move. Work in a terminal column (`board_columns.is_done`) is finished:
//! it cannot stand in the way of anything, and nothing can stand in its
//! way. The edge stays, as history, exactly as the edge to an archived
//! item stays. The two read surfaces split the same way they do for
//! archived work:
//!
//! - [`blocks_summary`] (the rollup) does not count an edge when either
//!   end sits in a terminal column;
//! - [`neighbors_of`] (the list) reports the neighbour **marked** via
//!   [`Neighbor::done`];
//! - [`item_subgraph`] (the picture) reports the node **marked** via
//!   [`SubgraphNode::done`] (COLLIERY-T-0233), so the graph view can draw
//!   a `blocks` arrow with a done end as history.
//!
//! All read the column's flag through [`ITEM_COLUMNS_SQL`], never the
//! column's name, and none stores anything: the answer follows the
//! item's current column.
//!
//! # Audit rows (KAIROS-A-0004 / S-0004)
//!
//! Link writes `action = 'relationship_add'`, unlink `action =
//! 'relationship_remove'`, with `details =
//! "relationship:{rel}:{source_short}->{target_short}"` (the S-0004 DDL
//! comment's format, e.g. `relationship:parent:ACME-I-0001->ACME-T-0005`).
//! `entity_id`/`entity_type` name the SOURCE of the edge (KAIROS-T-0638),
//! so a read or a purge of an item finds the rows of its edges; both
//! endpoints are named in `details`. Rows from before KAIROS-T-0638 had
//! NULL there; the migration `retention_more` set the source where the
//! label of the source names a known item.
//!
//! # Duplicate edges
//!
//! `UNIQUE (source_id, target_id, relationship)` is the enforcement;
//! inserting an existing edge maps the unique violation to the typed
//! [`GraphError::AlreadyLinked`] (not an idempotent no-op), so callers can
//! distinguish "changed" from "already so" and the activity log records
//! only real mutations — the same convention as
//! [`crate::abac::grant_capability`].
//!
//! # One parent (KAIROS-T-0572)
//!
//! An item has one parent. [`link_items`] refuses a `parent` edge to an
//! item that has a parent with [`GraphError::ParentExists`]. The partial
//! unique index `item_relationships_one_parent` (on `target_id` where the
//! relationship is `parent`) refuses it in the database too, so two writes
//! at the same time cannot both win. [`set_parent`] replaces the edge, or
//! removes it, in one transaction.

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
    #[error("The item {0} does not exist.")]
    ItemNotFound(Uuid),
    /// An item cannot be related to itself (mirrors the DDL `CHECK
    /// (source_id != target_id)`).
    #[error("An edge cannot go from the item {0} to the same item. Send two different items.")]
    SelfLink(Uuid),
    /// The `(relationship, source_type, target_type)` combination is
    /// outside the KAIROS-A-0001 type-rule matrix.
    #[error(transparent)]
    Rule(#[from] rules::GraphRuleError),
    /// Adding this edge would create a directed cycle in an acyclic
    /// relationship (`parent`/`blocks`, KAIROS-A-0001).
    #[error(
        "{article} {relationship} edge from {source_id} to {target_id} makes a cycle. {article} {relationship} edge must not make a cycle.",
        article = core_relationship(*.relationship).article()
    )]
    CycleDetected {
        relationship: RelationshipType,
        source_id: Uuid,
        target_id: Uuid,
    },
    /// The edge already exists (`UNIQUE (source_id, target_id,
    /// relationship)`) — nothing changed and no activity row was written.
    #[error("The {relationship} edge from {source_id} to {target_id} exists already.")]
    AlreadyLinked {
        relationship: RelationshipType,
        source_id: Uuid,
        target_id: Uuid,
    },
    /// The item has a parent already (KAIROS-T-0572): an item has one
    /// parent. The unique index `item_relationships_one_parent` refuses a
    /// second one too. [`set_parent`] changes the parent.
    #[error(
        "{item} has the parent {parent} already. An item has one parent. To change the parent, set the new parent: it replaces the old one."
    )]
    ParentExists {
        /// The short code of the item (the target of the edge).
        item: String,
        /// The short code of the parent that the item has.
        parent: String,
    },
    /// No such edge exists to unlink.
    #[error("No {relationship} edge goes from {source_id} to {target_id}.")]
    NotLinked {
        relationship: RelationshipType,
        source_id: Uuid,
        target_id: Uuid,
    },
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

/// The pure mirror of a stored [`RelationshipType`] (kairos-core carries no
/// diesel types, KAIROS-A-0009).
///
/// Public since COLLIERY-T-0227: a create checks the type rules of the edge
/// before it writes the item, and must name the relationship as
/// [`link_items`] does.
pub fn core_relationship(relationship: RelationshipType) -> rules::Relationship {
    match relationship {
        RelationshipType::Parent => rules::Relationship::Parent,
        RelationshipType::Supports => rules::Relationship::Supports,
        RelationshipType::Informs => rules::Relationship::Informs,
        RelationshipType::Supersedes => rules::Relationship::Supersedes,
        RelationshipType::Amends => rules::Relationship::Amends,
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

/// The entity type of `id`, live or archived, via `entity_directory`.
/// `None`: no such item.
fn entity_type_of(conn: &mut PgConnection, id: Uuid) -> Result<Option<String>, DieselError> {
    Ok(
        sql_query("SELECT entity_type, short_code FROM entity_directory WHERE id = $1")
            .bind::<SqlUuid, _>(id)
            .get_result::<DirectoryRow>(conn)
            .optional()?
            .map(|row| row.entity_type),
    )
}

/// Insert one relationship `activity_log` row: `entity_id`/`entity_type`
/// name the source (KAIROS-T-0638), both endpoints named in `details`
/// (module docs).
#[allow(clippy::too_many_arguments)]
fn log_relationship_activity(
    conn: &mut PgConnection,
    actor: Uuid,
    action: ActivityAction,
    relationship: RelationshipType,
    source_id: Uuid,
    source_type: Option<String>,
    source_label: &str,
    target_label: &str,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id: actor,
            action,
            entity_id: Some(source_id),
            entity_type: source_type,
            details: format!("relationship:{relationship}:{source_label}->{target_label}"),
        })
        .execute(conn)?;
    Ok(())
}

#[derive(QueryableByName)]
struct Reached {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    reached: bool,
}

/// Refuse a `relationship` edge from `source_id` to `target_id` that makes
/// a directed cycle (KAIROS-T-0634). Call it in the transaction of the
/// insert, before the insert.
///
/// 1. The lock: `pg_advisory_xact_lock` on the tenant schema and the
///    relationship. Two links of one relationship in one tenant take the
///    check and the insert in turn. Without the lock, the links A→B and
///    B→A at the same time each do not see the edge of the other, and both
///    commit: a cycle. The second link waits until the first commits; its
///    check is a new statement, so it sees the edge of the first. The lock
///    stays until the transaction ends.
/// 2. The check: a recursive CTE walks the edges of the relationship from
///    `target_id`, and stops at the first row that is `source_id`
///    (`LIMIT 1`). The database does the walk, on the index
///    `(source_id, relationship)`: the server does not load each edge of
///    the tenant. `UNION` (not `UNION ALL`) keeps the walk finite on bad
///    data that has a loop already.
fn check_acyclic(
    conn: &mut PgConnection,
    relationship: RelationshipType,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<(), GraphError> {
    let edges = format!("edges:{relationship}");
    sql_query("SELECT pg_advisory_xact_lock(hashtext(current_schema()), hashtext($1))")
        .bind::<Text, _>(&edges)
        .execute(conn)?;
    let cycle: Reached = sql_query(
        "WITH RECURSIVE reach(id) AS ( \
             SELECT $1::uuid \
             UNION \
             SELECT r.target_id FROM item_relationships r \
               JOIN reach ON r.source_id = reach.id \
              WHERE r.relationship = $3) \
         SELECT EXISTS (SELECT 1 FROM reach WHERE id = $2 LIMIT 1) AS reached",
    )
    .bind::<SqlUuid, _>(target_id)
    .bind::<SqlUuid, _>(source_id)
    .bind::<Text, _>(relationship.to_string())
    .get_result(conn)?;
    if cycle.reached {
        return Err(GraphError::CycleDetected {
            relationship,
            source_id,
            target_id,
        });
    }
    Ok(())
}

/// The name of the unique index that holds one parent per item
/// (KAIROS-T-0572, migration `2026-10-17-002200_one_parent`).
pub const ONE_PARENT_INDEX: &str = "item_relationships_one_parent";

/// The short code of an item, archived or not, or its id when the item is
/// gone: the label of an end in an activity row or a refusal.
fn label_of(conn: &mut PgConnection, id: Uuid) -> Result<String, GraphError> {
    let row: Option<DirectoryRow> =
        sql_query("SELECT entity_type, short_code FROM entity_directory WHERE id = $1")
            .bind::<SqlUuid, _>(id)
            .get_result(conn)
            .optional()?;
    Ok(row.map_or_else(|| id.to_string(), |row| row.short_code))
}

/// The parent of an item: the source of its `parent` edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentRef {
    /// The id of the parent.
    pub id: Uuid,
    /// The type of the parent, when it is in `entity_directory`.
    pub item_type: Option<ItemType>,
    /// The short code of the parent (its id when the parent is gone).
    pub short_code: String,
    /// Whether the parent is archived. The edge to an archived parent
    /// stays (module docs, `# Liveness`).
    pub archived: bool,
}

#[derive(QueryableByName)]
struct ParentRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Nullable<Text>)]
    entity_type: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    short_code: Option<String>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    deleted_at: Option<DateTime<Utc>>,
}

/// The parent of `item_id`, archived or not, or `None` (KAIROS-T-0572).
/// An item has one parent; with data from before the migration the oldest
/// edge answers, as `get_item` did.
pub fn parent_of(conn: &mut PgConnection, item_id: Uuid) -> Result<Option<ParentRef>, GraphError> {
    let row: Option<ParentRow> = sql_query(
        "SELECT r.source_id AS id, d.entity_type, d.short_code, d.deleted_at \
           FROM item_relationships r \
           LEFT JOIN entity_directory d ON d.id = r.source_id \
          WHERE r.target_id = $1 AND r.relationship = 'parent' \
          ORDER BY r.created_at, r.id LIMIT 1",
    )
    .bind::<SqlUuid, _>(item_id)
    .get_result(conn)
    .optional()?;
    row.map(|row| {
        Ok(ParentRef {
            id: row.id,
            item_type: row
                .entity_type
                .as_deref()
                .map(parse_entity_type)
                .transpose()?,
            short_code: row.short_code.unwrap_or_else(|| row.id.to_string()),
            archived: row.deleted_at.is_some(),
        })
    })
    .transpose()
}

/// Insert one edge, in a savepoint, so that a refusal of the database
/// leaves the transaction usable. A duplicate is
/// [`GraphError::AlreadyLinked`]; a second parent that a write at the same
/// time made is [`GraphError::ParentExists`] (the index
/// [`ONE_PARENT_INDEX`]).
fn insert_edge(
    conn: &mut PgConnection,
    source_id: Uuid,
    target_id: Uuid,
    relationship: RelationshipType,
) -> Result<ItemRelationship, GraphError> {
    let inserted = conn.transaction::<_, DieselError, _>(|conn| {
        diesel::insert_into(crate::schema::item_relationships::table)
            .values(NewItemRelationship {
                source_id,
                target_id,
                relationship,
            })
            .returning(ItemRelationship::as_returning())
            .get_result(conn)
    });
    match inserted {
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info))
            if info.constraint_name() == Some(ONE_PARENT_INDEX) =>
        {
            let parent = parent_of(conn, target_id)?
                .map_or_else(|| "another item".to_string(), |parent| parent.short_code);
            Err(GraphError::ParentExists {
                item: label_of(conn, target_id)?,
                parent,
            })
        }
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            Err(GraphError::AlreadyLinked {
                relationship,
                source_id,
                target_id,
            })
        }
        other => Ok(other?),
    }
}

/// Create a `relationship` edge from `source_id` to `target_id`
/// (KAIROS-A-0001 orientation — see [`kairos_core::graph`]'s semantics
/// table), in ONE transaction:
///
/// 1. both UUIDs resolve to live entities via `entity_directory`
///    (unknown/soft-deleted → [`GraphError::ItemNotFound`]);
/// 2. the pure type-rule matrix admits the combination
///    ([`kairos_core::graph::check_link`] → [`GraphError::Rule`]);
/// 3. for the acyclic relationships (`parent`/`blocks`/`amends`) the edge must not
///    close a directed cycle over the relationship's existing edges
///    ([`check_acyclic`], under a lock → [`GraphError::CycleDetected`]);
/// 4. a `parent` edge to an item that has a different parent is
///    [`GraphError::ParentExists`] (KAIROS-T-0572, module docs);
/// 5. insert (duplicate → [`GraphError::AlreadyLinked`], module docs) and
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
            check_acyclic(conn, relationship, source_id, target_id)?;
        }

        // KAIROS-T-0572: an item has one parent. The same parent again is
        // the duplicate below.
        if relationship == RelationshipType::Parent
            && let Some(parent) = parent_of(conn, target_id)?
            && parent.id != source_id
        {
            return Err(GraphError::ParentExists {
                item: target_code,
                parent: parent.short_code,
            });
        }

        let created = insert_edge(conn, source_id, target_id, relationship)?;

        log_relationship_activity(
            conn,
            actor,
            ActivityAction::RelationshipAdd,
            relationship,
            source_id,
            Some(source_type.entity_type().to_string()),
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
        let source_type = match &source {
            Some((item_type, _)) => Some(item_type.entity_type().to_string()),
            None => entity_type_of(conn, source_id)?,
        };
        log_relationship_activity(
            conn,
            actor,
            ActivityAction::RelationshipRemove,
            relationship,
            source_id,
            source_type,
            &source_label,
            &target_label,
        )?;
        // KAIROS-T-0568: the target lost a blocker that blocked it. A
        // blocker that is done or archived blocked nothing.
        if relationship == RelationshipType::Blocks
            && crate::notifications::blocks_now(conn, source_id)?
        {
            crate::notifications::on_blocks_removed(
                conn,
                &[crate::dispatch::Freed {
                    dependant: target_id,
                    blocker_code: source_label.clone(),
                }],
                crate::notifications::Cleared::Unlinked,
                actor,
            )?;
        }
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

/// What [`set_parent`] did (KAIROS-T-0572).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentChange {
    /// The short code of the item.
    pub item: String,
    /// The parent before the write, by short code, or `None`.
    pub previous: Option<String>,
    /// The parent after the write, by short code, or `None`.
    pub parent: Option<String>,
    /// Whether the write changed an edge. `false`: the item had this
    /// parent already (or no parent, for `None`), and nothing was written.
    pub changed: bool,
}

/// Set the parent of `item_id` to `parent_id`, or remove it with `None`,
/// in ONE transaction (KAIROS-T-0572). The old `parent` edge goes and the
/// new one comes, so no reader sees the item with two parents or, for a
/// change, with none.
///
/// 1. The item and the new parent are live
///    ([`GraphError::ItemNotFound`]); they differ ([`GraphError::SelfLink`]).
/// 2. The type rules admit the edge ([`GraphError::Rule`]).
/// 3. The same parent again, or `None` for an item with no parent, writes
///    nothing: `changed` is false.
/// 4. The cycle check of the new edge ([`check_acyclic`]). It takes its
///    lock before the delete, in the order of [`link_items`]: the lock
///    first, then the rows. The old edge goes into the item, so it is not
///    on a path from the item, and the check gives the same answer.
/// 5. The old edge goes, with its `relationship_remove` row. The old
///    parent can be archived: the edge to it goes all the same.
/// 6. The insert of the new edge, with its
///    `relationship_add` row. A second parent that a write at the same
///    time made is [`GraphError::ParentExists`]; the transaction then
///    removes nothing.
/// 7. One `relationship_changed` event for each live item of the three.
///
/// WHO may do it is not decided here: the server asks the link rule for
/// each edge that changes.
pub fn set_parent(
    conn: &mut PgConnection,
    item_id: Uuid,
    parent_id: Option<Uuid>,
    actor: Uuid,
) -> Result<ParentChange, GraphError> {
    conn.transaction::<_, GraphError, _>(|conn| {
        let (item_type, item_code) =
            resolve_entity(conn, item_id)?.ok_or(GraphError::ItemNotFound(item_id))?;
        let new_parent = match parent_id {
            Some(parent_id) => {
                if parent_id == item_id {
                    return Err(GraphError::SelfLink(item_id));
                }
                let (parent_type, parent_code) =
                    resolve_entity(conn, parent_id)?.ok_or(GraphError::ItemNotFound(parent_id))?;
                rules::check_link(rules::Relationship::Parent, parent_type, item_type)?;
                Some((parent_id, parent_type, parent_code))
            }
            None => None,
        };
        let previous = parent_of(conn, item_id)?;

        let unchanged = match (&previous, &new_parent) {
            (None, None) => true,
            (Some(old), Some((new_id, _, _))) => old.id == *new_id,
            _ => false,
        };
        if unchanged {
            return Ok(ParentChange {
                item: item_code,
                previous: previous.as_ref().map(|old| old.short_code.clone()),
                parent: new_parent.map(|(_, _, code)| code),
                changed: false,
            });
        }

        if let Some((new_id, _, _)) = &new_parent {
            check_acyclic(conn, RelationshipType::Parent, *new_id, item_id)?;
        }

        if let Some(old) = &previous {
            use crate::schema::item_relationships::dsl;
            let deleted = diesel::delete(
                dsl::item_relationships
                    .filter(dsl::source_id.eq(old.id))
                    .filter(dsl::target_id.eq(item_id))
                    .filter(dsl::relationship.eq(RelationshipType::Parent)),
            )
            .execute(conn)?;
            // 0: a write at the same time removed the edge first. That
            // write has its own row.
            if deleted > 0 {
                let old_type = old.item_type.map(|t| t.entity_type().to_string());
                log_relationship_activity(
                    conn,
                    actor,
                    ActivityAction::RelationshipRemove,
                    RelationshipType::Parent,
                    old.id,
                    old_type,
                    &old.short_code,
                    &item_code,
                )?;
            }
        }

        if let Some((new_id, new_type, new_code)) = &new_parent {
            insert_edge(conn, *new_id, item_id, RelationshipType::Parent)?;
            log_relationship_activity(
                conn,
                actor,
                ActivityAction::RelationshipAdd,
                RelationshipType::Parent,
                *new_id,
                Some(new_type.entity_type().to_string()),
                new_code,
                &item_code,
            )?;
        }

        // KAIROS-T-0022: one thin event for each live end, on commit.
        let mut ends = vec![(item_type, item_id)];
        if let Some(old) = &previous
            && let (Some(old_type), false) = (old.item_type, old.archived)
        {
            ends.push((old_type, old.id));
        }
        if let Some((new_id, new_type, _)) = &new_parent {
            ends.push((*new_type, *new_id));
        }
        for (end_type, end_id) in ends {
            crate::events::emit_item_event_by_id(
                conn,
                crate::events::EventKind::RelationshipChanged,
                end_type.entity_type(),
                end_id,
                actor,
            )?;
        }

        Ok(ParentChange {
            item: item_code,
            previous: previous.map(|old| old.short_code),
            parent: new_parent.map(|(_, _, code)| code),
            changed: true,
        })
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
    /// Whether this neighbour sits in a terminal column
    /// (`board_columns.is_done`) right now (COLLIERY-T-0214). Finished
    /// work does not block and is not blocked, so a `blocks` edge to a
    /// done neighbour is history, not a blocker. It is reported, not
    /// hidden, for the same reason as `archived_at`: every caller that
    /// renders a `blocks` neighbour must render this too, or a reader
    /// takes a resolved blocker for an open one. `false` for a neighbour
    /// with no column (a document, an off-board ADR).
    pub done: bool,
    /// Whether this neighbour is a cancelled task (KAIROS-T-0567). A
    /// cancelled task sits in a done column, so `done` is true too, but a
    /// cancelled blocker is unresolved: it still blocks. A caller that
    /// renders a `blocks` neighbour renders `[cancelled]`, not `[done]`.
    pub cancelled: bool,
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
    #[diesel(sql_type = diesel::sql_types::Bool)]
    done: bool,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    cancelled: bool,
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
            done: self.done,
            cancelled: self.cancelled,
        })
    }
}

/// Every item that can hold a board position, as id → column_id: the
/// join that tells [`blocks_summary`] and [`neighbors_of`] whether an item
/// sits in a terminal column (COLLIERY-T-0214). Documents never appear (no
/// board position); an off-board ADR appears with a NULL column.
///
/// Unlike [`CHILD_COLUMNS_SQL`] this carries NO liveness predicate, on
/// purpose. It answers "which column?", not "is it live?": each caller
/// decides liveness on its own `entity_directory` join, and an archived
/// item keeps its `column_id`, so a done-and-archived neighbour is
/// reported as both. Callers LEFT JOIN it, and `board_columns` after it,
/// so an item with no row here or no column row is treated as not done
/// rather than dropped.
pub(crate) const ITEM_COLUMNS_SQL: &str = "SELECT id, column_id FROM strategies \
     UNION ALL SELECT id, column_id FROM initiatives \
     UNION ALL SELECT id, column_id FROM tasks \
     UNION ALL SELECT id, column_id FROM adrs";

/// Both directions of [`relationships_for`] in one statement
/// (KAIROS-T-0626): the edges where `item_id` is the source and the edges
/// where it is the target, hydrating the OTHER end through
/// `entity_directory`. Each branch is backed by the matching S-0004 index
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
///
/// **Done neighbours are marked too** (COLLIERY-T-0214). The two LEFT
/// JOINs read the terminal flag of the neighbour's current column into
/// [`Neighbor::done`], in this same query. The row is never dropped for
/// it: a completed blocker is still part of the record, it has only
/// stopped being in the way.
fn neighbors_of(conn: &mut PgConnection, item_id: Uuid) -> Result<ItemRelationships, GraphError> {
    // KAIROS-T-0626: both directions in one statement. Each branch of the
    // UNION ALL keeps its own index (`source_id = $1`, `target_id = $1`).
    let rows: Vec<DirectedNeighborRow> = sql_query(format!(
        "SELECT r.outgoing, r.relationship, d.id, d.short_code, d.entity_type, d.title, \
                d.deleted_at, \
                COALESCE(bc.is_done, false) AS done, \
                EXISTS (SELECT 1 FROM task_cancellations k WHERE k.task_id = d.id) AS cancelled \
         FROM (SELECT true AS outgoing, relationship, target_id AS other_id, created_at \
                 FROM item_relationships WHERE source_id = $1 \
               UNION ALL \
               SELECT false, relationship, source_id, created_at \
                 FROM item_relationships WHERE target_id = $1) r \
         JOIN entity_directory d ON d.id = r.other_id \
         LEFT JOIN ({ITEM_COLUMNS_SQL}) p ON p.id = d.id \
         LEFT JOIN board_columns bc ON bc.id = p.column_id \
         ORDER BY r.outgoing DESC, r.relationship ASC, r.created_at ASC, d.short_code ASC"
    ))
    .bind::<SqlUuid, _>(item_id)
    .load(conn)?;
    let mut relationships = ItemRelationships {
        outgoing: Vec::new(),
        incoming: Vec::new(),
    };
    for row in rows {
        let neighbor = row.row.into_neighbor()?;
        if row.outgoing {
            relationships.outgoing.push(neighbor);
        } else {
            relationships.incoming.push(neighbor);
        }
    }
    Ok(relationships)
}

/// A row of [`neighbors_of`]: the direction of the edge and the neighbour.
#[derive(QueryableByName)]
struct DirectedNeighborRow {
    /// True when the item is the source of the edge.
    #[diesel(sql_type = diesel::sql_types::Bool)]
    outgoing: bool,
    #[diesel(embed)]
    row: NeighborRow,
}

/// Every relationship touching `item_id`, in BOTH directions, grouped by
/// relationship type (see [`ItemRelationships`] for orientation). One
/// statement with two indexed branches: `source_id = item` uses
/// `idx_item_relationships_source(source_id, relationship)` and
/// `target_id = item` uses `idx_item_relationships_target(target_id,
/// relationship)` (S-0004). The item itself is not required to exist — an
/// unknown id simply has no edges to report — and an ARCHIVED item reports
/// its edges like any other, since `item_relationships` rows are
/// hard-deleted and so survive the archive intact (KAIROS-T-0158).
/// Neighbours carry [`Neighbor::archived_at`]; every caller that renders a
/// neighbour must render that too. They also carry [`Neighbor::done`]
/// (COLLIERY-T-0214), which every caller that renders a `blocks` neighbour
/// must render.
pub fn relationships_for(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<ItemRelationships, GraphError> {
    neighbors_of(conn, item_id)
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
    /// The cancelled tasks among `count` (KAIROS-T-0567).
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub cancelled: i64,
    /// The archived children of the column that do not count: the column
    /// is not done (KAIROS-T-0571). Not in `count`.
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub removed: i64,
}

/// The workflow-item id → column_id union the progress queries join
/// children through, with `archived` (`deleted_at IS NOT NULL`):
/// documents never appear (no board position), and off-board ADRs are
/// excluded. Only `parent` edges are followed, so supports/informs
/// material never counts.
///
/// **Archived children count by the column they were archived from**
/// (KAIROS-T-0571, an amendment of the KAIROS-T-0158 reading of ADR-20
/// rule 5). "Archive completed" is the way to clean a board. A rollup
/// that dropped the archived children made an initiative look less
/// finished the more of its finished work was put away. So:
///
/// - an archived child in a done column counts as done (or as cancelled,
///   with the cancel mark): it is finished work, not live work;
/// - an archived child in another column is `removed`: not open work, and
///   not in the total.
///
/// The relationship LIST names each archived child ([`neighbors_of`]).
const CHILD_COLUMNS_SQL: &str = "SELECT id, column_id, deleted_at IS NOT NULL AS archived \
         FROM strategies \
     UNION ALL SELECT id, column_id, deleted_at IS NOT NULL FROM initiatives \
     UNION ALL SELECT id, column_id, deleted_at IS NOT NULL FROM tasks \
     UNION ALL SELECT id, column_id, deleted_at IS NOT NULL FROM adrs \
         WHERE column_id IS NOT NULL";

/// The SQL of the three counts of a group of children (KAIROS-T-0571):
/// `count` (the children that count), `cancelled` (the cancelled tasks
/// among them) and `removed` (archived from a column that is not done).
const CHILD_COUNTS_SQL: &str = "COUNT(*) FILTER (WHERE NOT c.archived OR bc.is_done) AS count, \
     COUNT(*) FILTER (WHERE (NOT c.archived OR bc.is_done) \
                        AND EXISTS (SELECT 1 FROM task_cancellations k \
                                    WHERE k.task_id = c.id)) AS cancelled, \
     COUNT(*) FILTER (WHERE c.archived AND NOT bc.is_done) AS removed";

/// Direct `parent`-edge children of `parent_id`, grouped by their board
/// column — ONE query, column order within each board (KAIROS-T-0080).
/// Archived children count by their column ([`CHILD_COLUMNS_SQL`]): a
/// column that has only removed children has `count` 0.
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
                {CHILD_COUNTS_SQL} \
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
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    cancelled: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    removed: i64,
}

/// A parent's `(done, cancelled, total, has_done_semantics)` children
/// rollup. `has_done` is false when NO board hosting the children has a
/// done-flagged column — clients then show composition only.
/// KAIROS-T-0567: `done` does not count a cancelled task; `cancelled`
/// does, and `total - done - cancelled` is open. KAIROS-T-0571: an
/// archived child in a done column counts; one from another column is in
/// `removed`, not in `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgressCounts {
    pub done: i64,
    pub cancelled: i64,
    pub total: i64,
    pub has_done: bool,
    pub removed: i64,
}

impl ProgressCounts {
    /// The children that are not done and not cancelled.
    pub fn open(&self) -> i64 {
        self.total - self.done - self.cancelled
    }
}

/// Children rollups for the items `parent_ids` of the board `board_id`
/// (the items of one page) that have direct children: one grouped query
/// for the page, never per item (the N+1 the KAIROS-T-0080 design
/// forbids). KAIROS-T-0627: the query reads the edges of the page's items
/// only, not of the whole board.
///
/// The parents are live: they come from a per-family union filtered
/// `deleted_at IS NULL`, so an archived item of the page has no rollup.
/// The children come from [`CHILD_COLUMNS_SQL`], archived ones by their
/// column (KAIROS-T-0571). A parent has an entry when it has a child that
/// counts (`total > 0`).
pub fn page_children_progress(
    conn: &mut PgConnection,
    board_id: Uuid,
    parent_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, ProgressCounts>, DieselError> {
    if parent_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let parents = "WHERE board_id = $1 AND deleted_at IS NULL AND id = ANY($2)";
    let rows: Vec<BoardProgressRow> = sql_query(format!(
        "SELECT r.source_id AS parent_id, bc.is_done, \
                EXISTS(SELECT 1 FROM board_columns d \
                       WHERE d.board_id = bc.board_id AND d.is_done \
                         AND d.deleted_at IS NULL) AS board_has_done, \
                {CHILD_COUNTS_SQL} \
         FROM (SELECT id FROM strategies {parents} \
               UNION ALL SELECT id FROM initiatives {parents} \
               UNION ALL SELECT id FROM tasks {parents} \
               UNION ALL SELECT id FROM adrs {parents}) p \
         JOIN item_relationships r \
           ON r.source_id = p.id AND r.relationship = 'parent' \
         JOIN ({CHILD_COLUMNS_SQL}) c ON c.id = r.target_id \
         JOIN board_columns bc ON bc.id = c.column_id \
         GROUP BY r.source_id, bc.is_done, bc.board_id"
    ))
    .bind::<SqlUuid, _>(board_id)
    .bind::<diesel::sql_types::Array<SqlUuid>, _>(parent_ids)
    .load(conn)?;
    let mut progress: std::collections::HashMap<Uuid, ProgressCounts> =
        std::collections::HashMap::new();
    for row in rows {
        let entry = progress.entry(row.parent_id).or_default();
        entry.total += row.count;
        entry.has_done |= row.board_has_done;
        entry.cancelled += row.cancelled;
        entry.removed += row.removed;
        if row.is_done {
            entry.done += row.count - row.cancelled;
        }
    }
    progress.retain(|_, counts| counts.total > 0);
    Ok(progress)
}

// ---------------------------------------------------------------------------
// Derived team work-documents (KAIROS-T-0084)
// ---------------------------------------------------------------------------

/// One document of a team, with the reason it is listed: `owned` when a
/// board of the team owns it, and the supports-parent when it supports
/// the team's work. A row has at least one of the two.
#[derive(Debug, Clone, QueryableByName)]
pub struct TeamWorkDocument {
    #[diesel(sql_type = Text)]
    pub short_code: String,
    #[diesel(sql_type = Text)]
    pub title: String,
    #[diesel(sql_type = Text)]
    pub lifecycle: String,
    /// A live board of the team is the owner board (`documents.board_id`).
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub owned: bool,
    #[diesel(sql_type = Nullable<Text>)]
    pub parent_short_code: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub parent_title: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub parent_type: Option<String>,
}

/// The documents of a team (KAIROS-T-0084, KAIROS-T-0393). A live
/// document is listed when either holds:
///
/// - **owned**: its owner board (`documents.board_id`) is a live board of
///   the team (`boards.team_id`). Most documents support an initiative on
///   a shared board, or nothing, so ownership is what finds them;
/// - **supports**: its `supports` PARENT (edge source) is (a) a live task
///   with `team_id = {team}` or (b) any live item on the team's delivery
///   board. The parent is chosen deterministically (lexicographically
///   first short code).
///
/// One row per document, ordered by document short code. A document that
/// only supports an org-level item, and is owned by a board of a
/// different team, does not appear. `delivery_board = None` (a team
/// without a delivery board) leaves only path (a) of supports.
pub fn team_work_documents(
    conn: &mut PgConnection,
    team_id: Uuid,
    delivery_board: Option<Uuid>,
) -> Result<Vec<TeamWorkDocument>, DieselError> {
    sql_query(
        "WITH supported AS ( \
             SELECT DISTINCT ON (r.target_id) \
                 r.target_id AS document_id, \
                 p.short_code, p.title, p.entity_type \
             FROM item_relationships r \
             JOIN entity_directory p ON p.id = r.source_id AND p.deleted_at IS NULL \
             LEFT JOIN tasks t ON t.id = r.source_id AND t.deleted_at IS NULL \
             WHERE r.relationship = 'supports' \
               AND (t.team_id = $1 OR p.board_id = $2) \
             ORDER BY r.target_id, p.short_code \
         ) \
         SELECT d.short_code, d.title, d.lifecycle, \
             (b.id IS NOT NULL) AS owned, \
             s.short_code AS parent_short_code, \
             s.title AS parent_title, \
             s.entity_type AS parent_type \
         FROM documents d \
         LEFT JOIN boards b ON b.id = d.board_id AND b.deleted_at IS NULL \
             AND b.team_id = $1 \
         LEFT JOIN supported s ON s.document_id = d.id \
         WHERE d.deleted_at IS NULL \
           AND (b.id IS NOT NULL OR s.document_id IS NOT NULL) \
         ORDER BY d.short_code",
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
    /// Whether this node sits in a terminal column
    /// (`board_columns.is_done`) right now (COLLIERY-T-0233, the rule of
    /// COLLIERY-T-0214). A `blocks` edge with a done node at either end
    /// is history, not a blocker, and a renderer must draw it so. `false`
    /// for a node with no column (a document, an off-board ADR). Same
    /// definition as [`Neighbor::done`]: read through
    /// [`ITEM_COLUMNS_SQL`], never stored.
    pub done: bool,
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
    #[diesel(sql_type = diesel::sql_types::Bool)]
    done: bool,
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
///
/// **Done nodes are marked** (COLLIERY-T-0233) via [`SubgraphNode::done`],
/// with the same two LEFT JOINs as [`neighbors_of`], so the picture and
/// the list cannot disagree about which `blocks` edges are still open.
pub fn item_subgraph(
    conn: &mut PgConnection,
    root: Uuid,
    depth: u32,
) -> Result<(Vec<SubgraphNode>, Vec<SubgraphEdge>), GraphError> {
    // 1. The walk: visited ids with their MINIMUM discovery depth.
    // KAIROS-T-0628: one UNION ALL arm per direction, so each arm probes its
    // relationship index (source or target); the CHECK source_id != target_id
    // keeps the arms disjoint, so no edge is counted twice.
    let visited: Vec<DepthRow> = sql_query(
        "WITH RECURSIVE walk(id, depth) AS (
             SELECT $1::uuid, 0
             UNION
             SELECT n.other_id, w.depth + 1
             FROM walk w
             CROSS JOIN LATERAL (
                 SELECT r.target_id AS other_id FROM item_relationships r
                     WHERE r.source_id = w.id
                 UNION ALL
                 SELECT r.source_id FROM item_relationships r
                     WHERE r.target_id = w.id
             ) n
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
    //    when it was put away. `done` is the terminal flag of that same
    //    column (COLLIERY-T-0233), read the way `neighbors_of` reads it.
    let rows: Vec<NodeHydrationRow> = sql_query(format!(
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
                COALESCE(bc.is_done, false) AS done,
                (SELECT COUNT(*) FROM (
                     SELECT r.target_id AS other_id FROM item_relationships r
                         WHERE r.source_id = d.id
                     UNION ALL
                     SELECT r.source_id FROM item_relationships r
                         WHERE r.target_id = d.id
                 ) e
                 JOIN entity_directory other ON other.id = e.other_id) AS degree
         FROM entity_directory d
         JOIN status_of s ON s.id = d.id
         LEFT JOIN ({ITEM_COLUMNS_SQL}) p ON p.id = d.id
         LEFT JOIN board_columns bc ON bc.id = p.column_id
         WHERE d.id = ANY($1)"
    ))
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
                done: row.done,
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
    /// Open incoming `blocks` edges (things blocking this item): the
    /// blocker is live, and neither end is in a terminal column.
    pub blocked_by: i64,
    /// Open outgoing `blocks` edges (things this item blocks): the blocked
    /// item is live, and neither end is in a terminal column.
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
/// (never per item — the T-0080 rollup discipline). Items with no open
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
///
/// **Open-only, for the same reason** (COLLIERY-T-0214). The product
/// decision of 2026-09-26: "when tickets are complete they by definition
/// can't block or be blocking anymore." Work in a terminal column cannot
/// move, so it is no more in the way than archived work is, and it has
/// nothing left to wait for. An edge therefore counts only while NEITHER
/// end sits in a terminal column:
///
/// - a done blocker adds nothing to the blocked item's `blocked_by`;
/// - a done blocked item adds nothing to its blocker's `blocks`;
/// - a done item has no counts of its own, and so no entry.
///
/// Before this, an import that carried 177 `blocks` edges between
/// completed tasks filled a board with "blocked by 1" on cards in
/// Completed — blockers that blocked nothing.
///
/// "Terminal" is the column's `is_done` FLAG, which is the board's
/// decision; the column's name is never read. The flag is read for both
/// ends inside this one grouped query, through [`ITEM_COLUMNS_SQL`]:
/// nothing is stored, so the count returns when an item moves out of a
/// terminal column or the board unmarks the column. The joins are LEFT
/// joins, so an item with no column (an off-board ADR) or no column row
/// is treated as not done, and never drops out of the count for that.
///
/// The edge itself stays, and [`relationships_for`] still lists it, with
/// the done end marked ([`Neighbor::done`]).
pub fn blocks_summary(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, BlocksCounts>, DieselError> {
    // KAIROS-T-0628: one UNION ALL arm per direction, so each arm probes its
    // relationship index (source or target); the CHECK source_id != target_id
    // keeps the arms disjoint, so no edge is counted twice.
    let rows: Vec<BlocksRow> = sql_query(format!(
        "SELECT n.id,
                COUNT(*) FILTER (WHERE n.blocked) AS blocked_by,
                COUNT(*) FILTER (WHERE NOT n.blocked) AS blocks
         FROM (
             SELECT i.id, r.source_id AS other_id, true AS blocked
             FROM unnest($1::uuid[]) AS i(id)
             JOIN item_relationships r
               ON r.target_id = i.id AND r.relationship = 'blocks'
             UNION ALL
             SELECT i.id, r.target_id, false
             FROM unnest($1::uuid[]) AS i(id)
             JOIN item_relationships r
               ON r.source_id = i.id AND r.relationship = 'blocks'
         ) n
         JOIN entity_directory other
           ON other.id = n.other_id
          AND other.deleted_at IS NULL
         LEFT JOIN ({ITEM_COLUMNS_SQL}) own_place ON own_place.id = n.id
         LEFT JOIN board_columns own_column ON own_column.id = own_place.column_id
         LEFT JOIN ({ITEM_COLUMNS_SQL}) other_place ON other_place.id = other.id
         LEFT JOIN board_columns other_column ON other_column.id = other_place.column_id
         WHERE NOT COALESCE(own_column.is_done, false)
           AND (NOT COALESCE(other_column.is_done, false)
                OR (n.blocked
                    AND EXISTS (SELECT 1 FROM task_cancellations k
                                 WHERE k.task_id = other.id)))
         GROUP BY n.id"
    ))
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
/// are that function's supports predicate. If the definition of "this team's
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
         WHERE l.state = ANY($3) AND l.kind <> 'issue' \
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
         WHERE r.id = $1 AND l.state = ANY($2) AND l.kind <> 'issue' \
         ORDER BY l.forge_updated_at DESC \
         LIMIT $3",
    )
    .bind::<SqlUuid, _>(repository_id)
    .bind::<Array<Text>, _>(states.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .load(conn)
}

/// The SQL of [`repository_link_page`]: the rows of
/// [`repository_link_rollup`] with the count of all of them.
const REPOSITORY_LINKS_SQL: &str = "\
    SELECT l.id, l.kind, l.external_id, l.title, l.url, l.state, l.author, \
           l.forge_updated_at, \
           r.forge, r.repo_full_name, \
           d.short_code AS item_short_code, d.title AS item_title, \
           count(*) OVER () AS total \
      FROM item_links l \
      JOIN forge_connections c ON c.id = l.connection_id AND c.deleted_at IS NULL \
      JOIN repositories r ON r.id = c.repository_id \
      JOIN entity_directory d ON d.id = l.item_id AND d.deleted_at IS NULL \
     WHERE r.id = $1 AND l.state = ANY($2) AND l.kind <> 'issue' \
     ORDER BY l.forge_updated_at DESC, l.id \
     LIMIT $3 OFFSET $4";

/// A row of [`REPOSITORY_LINKS_SQL`].
#[derive(QueryableByName)]
struct PagedLinkRow {
    #[diesel(embed)]
    item: TeamLinkRow,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    total: i64,
}

/// One page of [`repository_link_rollup`] (KAIROS-T-0561): at most `limit`
/// rows after `offset`, and the count of all rows, so that `get_repository`
/// says when there are more. A page past the end runs the query once more
/// for the count.
pub fn repository_link_page(
    conn: &mut PgConnection,
    repository_id: Uuid,
    states: &[&str],
    limit: i64,
    offset: i64,
) -> Result<(Vec<TeamLinkRow>, i64), DieselError> {
    let states: Vec<String> = states.iter().map(|s| s.to_string()).collect();
    let mut page = |limit: i64, offset: i64| -> Result<Vec<PagedLinkRow>, DieselError> {
        sql_query(REPOSITORY_LINKS_SQL)
            .bind::<SqlUuid, _>(repository_id)
            .bind::<Array<Text>, _>(&states)
            .bind::<diesel::sql_types::BigInt, _>(limit)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn)
    };
    let rows = page(limit, offset)?;
    let total = match rows.first() {
        Some(row) => row.total,
        None if offset > 0 => page(1, 0)?.first().map_or(0, |row| row.total),
        None => 0,
    };
    Ok((rows.into_iter().map(|r| r.item).collect(), total))
}
