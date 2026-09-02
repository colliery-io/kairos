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
//! same derivation [`crate::items::next_short_code`] uses for the
//! short-code prefix.

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
    /// An item was soft-deleted.
    ItemDeleted,
    /// A relationship edge touching the item was added or removed.
    RelationshipChanged,
    /// The item's metadata values changed.
    MetadataChanged,
    /// The item's forge links (branches, pull/merge requests) changed
    /// (KAIROS-T-0099).
    ItemLinksChanged,
}

impl EventKind {
    /// The wire name (`event` field).
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::ItemCreated => "item_created",
            EventKind::ItemUpdated => "item_updated",
            EventKind::ItemTransitioned => "item_transitioned",
            EventKind::ItemDeleted => "item_deleted",
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
