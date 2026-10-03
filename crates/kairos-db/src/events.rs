//! Thin change-event emission over PostgreSQL `NOTIFY` (KAIROS-T-0022,
//! contract per KAIROS-A-0005 §5 / KAIROS-S-0005 "Event Push").
//!
//! Every mutating service (create/update/transition/delete, relationship
//! and metadata changes) emits one `NOTIFY kairos_events` carrying the
//! tenant-tagged thin payload:
//!
//! ```json
//! {"tenant": "acme", "event": "item_transitioned", "entity_type": "task",
//!  "short_code": "ACME-T-0123", "board_id": "uuid", "column_id": "uuid",
//!  "actor": "user-uuid", "occurred_at": "2026-07-10T12:00:00Z"}
//! ```
//!
//! The `tenant` field is fan-out routing metadata for the server's LISTEN
//! side (`kairos-server/src/ws.rs`) and is stripped before the event
//! reaches a client socket; the remaining fields are exactly the S-0005
//! client shape (`column_id` omitted when not applicable, `board_id`
//! `null` for off-board items — documents and unplaced ADRs).
//!
//! # Boards across a `blocks` edge (COLLIERY-T-0233)
//!
//! A card shows the count of its open blockers
//! ([`crate::graph::blocks_summary`]), and a blocker can be on a different
//! board. When the blocker moves to a done column, the count on the card
//! changes, but the event carries the board of the BLOCKER: a socket that
//! filters to the board of the card did not get it, and the count stayed
//! stale. So an event that can change a count also carries `blocks_boards`:
//! the boards of the live items at the other end of the item's `blocks`
//! edges, in the two directions ([`blocks_boards`]). Like `tenant`, this
//! field is routing metadata: the server delivers the event to a socket
//! filtered to one of these boards, and strips the field before the event
//! reaches a client. The client shape does not change.
//!
//! Only the kinds in [`EventKind::changes_blocks_counts`] carry it. The
//! other kinds cannot change a count, and a `relationship_changed` is
//! emitted for each end of the edge with its own board.
//!
//! # Post-commit semantics (A-0005 §5)
//!
//! [`emit_event`] runs INSIDE the service transaction, and that is the
//! point: PostgreSQL queues `pg_notify` payloads with the transaction and
//! delivers them to listeners only when the transaction COMMITS. A rolled
//! back write therefore emits nothing, and a delivered notification always
//! describes committed state — A-0005's "emitted post-commit" by
//! construction, with no application-side after-commit hook to forget.
//!
//! # Delivery is best-effort
//!
//! Events are UI-freshness hints, not a durable stream (A-0005 §5): no
//! replay, no ordering guarantee beyond what one LISTEN connection
//! observes. Clients reconcile by re-fetching through the REST API.
//!
//! # Tenant tagging
//!
//! The services run on connections whose `search_path` is pinned to the
//! tenant schema and never receive the slug as an argument, so the tenant
//! tag is derived from `current_schema()` (`org_{slug}` → `{slug}`) — the
//! same derivation [`crate::items::tenant_code_prefix`] uses for the
//! prefix of an item with no board.

use chrono::Utc;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Nullable, Text, Uuid as SqlUuid};
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// The one NOTIFY channel every Kairos event travels on (A-0005 §5).
pub const EVENT_CHANNEL: &str = "kairos_events";

/// The S-0005 event vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// An item was created.
    ItemCreated,
    /// An item's content changed (edit or rollback).
    ItemUpdated,
    /// An item moved to another board column.
    ItemTransitioned,
    /// A task moved to another delivery board (KAIROS-I-0012). Emitted
    /// once for the board it left (`board_id` = source, no column) and
    /// once for the board it joined, so both boards' subscribers refetch.
    ItemMoved,
    /// An item was soft-deleted.
    ItemDeleted,
    /// An archived item was put back (KAIROS-A-0020). Distinct from
    /// `ItemCreated`: the item and its history existed all along, so a
    /// client that treats this as a create would show a new card with an
    /// old version number.
    ItemRestored,
    /// A relationship edge touching the item was added or removed.
    RelationshipChanged,
    /// The item's metadata values changed.
    MetadataChanged,
    /// The item's forge links (branches, pull/merge requests) changed
    /// (KAIROS-T-0099).
    ItemLinksChanged,
}

impl EventKind {
    /// Whether this kind can change the `blocks` counts on the cards of
    /// OTHER items (COLLIERY-T-0233): the item changed column (a done
    /// column opens or closes the edge, COLLIERY-T-0214), or changed
    /// liveness (archived work does not count). A content, metadata or
    /// link change does not. An edge change does, but
    /// `relationship_changed` is emitted once for each end, so each board
    /// gets the event of its own item.
    pub fn changes_blocks_counts(self) -> bool {
        matches!(
            self,
            EventKind::ItemTransitioned
                | EventKind::ItemMoved
                | EventKind::ItemDeleted
                | EventKind::ItemRestored
        )
    }

    /// The wire name (`event` field).
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::ItemCreated => "item_created",
            EventKind::ItemUpdated => "item_updated",
            EventKind::ItemTransitioned => "item_transitioned",
            EventKind::ItemMoved => "item_moved",
            EventKind::ItemDeleted => "item_deleted",
            EventKind::ItemRestored => "item_restored",
            EventKind::RelationshipChanged => "relationship_changed",
            EventKind::MetadataChanged => "metadata_changed",
            EventKind::ItemLinksChanged => "item_links_changed",
        }
    }
}

/// One thin change event (S-0005 shape, before tenant tagging).
#[derive(Debug, Clone)]
pub struct ThinEvent {
    /// What happened.
    pub event: EventKind,
    /// `strategy|initiative|task|document|adr`.
    pub entity_type: String,
    /// The affected item's short code.
    pub short_code: String,
    /// The item's board (`None` for off-board items).
    pub board_id: Option<Uuid>,
    /// The item's (new) column, where applicable.
    pub column_id: Option<Uuid>,
    /// The acting user (`public.users.id`).
    pub actor: Uuid,
}

#[derive(QueryableByName)]
struct SchemaName {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct PlacementRow {
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    board_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    column_id: Option<Uuid>,
}

#[derive(QueryableByName)]
struct BoardRow {
    #[diesel(sql_type = SqlUuid)]
    board_id: Uuid,
}

/// The boards of the live items at the other end of the `blocks` edges of
/// the item `short_code`, in the two directions (COLLIERY-T-0233, module
/// docs). `own_board` is left out: the event already goes there. The item
/// itself can be archived (an `item_deleted` event); the other end must be
/// live, because an archived item has no card to refresh. Ordered, so the
/// payload is deterministic.
pub fn blocks_boards(
    conn: &mut PgConnection,
    short_code: &str,
    own_board: Option<Uuid>,
) -> Result<Vec<Uuid>, DieselError> {
    let rows: Vec<BoardRow> = sql_query(
        "SELECT DISTINCT other.board_id          FROM entity_directory me          JOIN item_relationships r            ON r.relationship = 'blocks'           AND (r.source_id = me.id OR r.target_id = me.id)          JOIN entity_directory other            ON other.id = CASE WHEN r.source_id = me.id                               THEN r.target_id ELSE r.source_id END           AND other.deleted_at IS NULL          WHERE me.short_code = $1            AND other.board_id IS NOT NULL            AND other.board_id IS DISTINCT FROM $2          ORDER BY other.board_id",
    )
    .bind::<Text, _>(short_code)
    .bind::<Nullable<SqlUuid>, _>(own_board)
    .load(conn)?;
    Ok(rows.into_iter().map(|row| row.board_id).collect())
}

/// Emit `event` on [`EVENT_CHANNEL`], tagged with the current connection's
/// tenant (see module docs). Call INSIDE the mutating transaction —
/// PostgreSQL delivers the notification on commit and drops it on
/// rollback, which is what makes emission post-commit (A-0005 §5).
pub fn emit_event(conn: &mut PgConnection, event: &ThinEvent) -> Result<(), DieselError> {
    let schema: SchemaName =
        sql_query("SELECT COALESCE(current_schema()::text, 'public') AS name").get_result(conn)?;
    let tenant = schema
        .name
        .strip_prefix("org_")
        .unwrap_or(schema.name.as_str());

    let mut payload = Map::new();
    payload.insert("tenant".into(), json!(tenant));
    payload.insert("event".into(), json!(event.event.as_str()));
    payload.insert("entity_type".into(), json!(event.entity_type));
    payload.insert("short_code".into(), json!(event.short_code));
    payload.insert("board_id".into(), json!(event.board_id));
    if let Some(column_id) = event.column_id {
        payload.insert("column_id".into(), json!(column_id));
    }
    payload.insert("actor".into(), json!(event.actor));
    payload.insert("occurred_at".into(), json!(Utc::now()));
    // COLLIERY-T-0233: routing metadata, stripped by the server like
    // `tenant`. Omitted when empty, which is the usual case.
    if event.event.changes_blocks_counts() {
        let boards = blocks_boards(conn, &event.short_code, event.board_id)?;
        if !boards.is_empty() {
            payload.insert("blocks_boards".into(), json!(boards));
        }
    }

    sql_query("SELECT pg_notify($1, $2)")
        .bind::<Text, _>(EVENT_CHANNEL)
        .bind::<Text, _>(Value::Object(payload).to_string())
        .execute(conn)?;
    Ok(())
}

/// [`item_placement`]'s row: `(short_code, board_id, column_id)`.
pub type ItemPlacement = (String, Option<Uuid>, Option<Uuid>);

/// `(short_code, board_id, column_id)` of an item by id, straight from its
/// entity table (soft-deleted rows included — `item_deleted` events need
/// the placement of the row that was just stamped). `None` for unknown
/// ids or unknown entity types.
pub fn item_placement(
    conn: &mut PgConnection,
    entity_type: &str,
    item_id: Uuid,
) -> Result<Option<ItemPlacement>, DieselError> {
    let sql = match entity_type {
        "strategy" => "SELECT short_code, board_id, column_id FROM strategies WHERE id = $1",
        "initiative" => "SELECT short_code, board_id, column_id FROM initiatives WHERE id = $1",
        "task" => "SELECT short_code, board_id, column_id FROM tasks WHERE id = $1",
        "document" => {
            "SELECT short_code, NULL::uuid AS board_id, NULL::uuid AS column_id \
             FROM documents WHERE id = $1"
        }
        "adr" => "SELECT short_code, board_id, column_id FROM adrs WHERE id = $1",
        _ => return Ok(None),
    };
    let row: Option<PlacementRow> = sql_query(sql)
        .bind::<SqlUuid, _>(item_id)
        .get_result(conn)
        .optional()?;
    Ok(row.map(|r| (r.short_code, r.board_id, r.column_id)))
}

/// Convenience for call sites that only hold an item id: look up the
/// item's short code and placement, then [`emit_event`]. Unknown ids emit
/// nothing (the mutation itself already failed or the row is gone).
pub fn emit_item_event_by_id(
    conn: &mut PgConnection,
    event: EventKind,
    entity_type: &str,
    item_id: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    if let Some((short_code, board_id, column_id)) = item_placement(conn, entity_type, item_id)? {
        emit_event(
            conn,
            &ThinEvent {
                event,
                entity_type: entity_type.to_string(),
                short_code,
                board_id,
                column_id,
                actor,
            },
        )?;
    }
    Ok(())
}
