//! Thin change events (KAIROS-T-0022, contract per KAIROS-A-0005 §5 /
//! KAIROS-S-0005 "Event Push"), sent through an outbox (KAIROS-T-0632).
//!
//! Every mutating service (create/update/transition/delete, relationship
//! and metadata changes) writes its events to `public.event_outbox`, in its
//! own transaction. An emitter sends them on the channel `kairos_events`
//! ([`send_outbox`]). One `NOTIFY` carries a batch: a JSON array of
//! tenant-tagged thin events:
//!
//! ```json
//! [{"tenant": "acme", "event": "item_transitioned", "entity_type": "task",
//!   "short_code": "ACME-T-0123", "board_id": "uuid", "column_id": "uuid",
//!   "actor": "user-uuid", "occurred_at": "2026-07-10T12:00:00Z"}]
//! ```
//!
//! `agent_key_id` is there only when an agent made the change with the
//! agent key of a person (KAIROS-T-0359): `actor` is the person, and
//! `agent_key_id` is the key ([`crate::agent_mark`]).
//!
//! The `tenant` field is fan-out routing metadata for the server's LISTEN
//! side (`kairos-server/src/ws.rs`) and is stripped before the event
//! reaches a client socket; the remaining fields are exactly the S-0005
//! client shape (`column_id` omitted when not applicable, `board_id`
//! `null` for off-board items — documents and unplaced ADRs).
//!
//! # The outbox (KAIROS-T-0632)
//!
//! At commit, PostgreSQL takes one database-wide lock for each transaction
//! that sent a `NOTIFY`, and holds it through the flush of the commit. Each
//! tenant is a schema of one database. When each write sent its own
//! `NOTIFY`, each write of the deployment waited on that lock. Now a write
//! inserts a row, and only the emitter notifies: one transaction for each
//! batch.
//!
//! - A write emits with one `INSERT` ([`emit_event`] and the others). The
//!   statement reads the schema (`current_schema()`), the agent key and the
//!   placement of the item itself: no lookup before it.
//! - [`send_outbox`] takes the unsent rows in id order with `FOR UPDATE
//!   SKIP LOCKED`, computes `blocks_boards` once for each tenant of the
//!   batch, sends one `NOTIFY` for each [`NOTIFY_LIMIT`] bytes of events,
//!   and sets `sent_at`. It is one transaction: the rows are sent and
//!   marked, or neither.
//! - [`prune_outbox`] deletes the rows sent more than a minute ago.
//!
//! The server runs one emitter in each process, next to its LISTEN
//! connection. With two processes, `SKIP LOCKED` gives each row to one
//! emitter, and its `NOTIFY` reaches the listeners of both. A crash after
//! the commit of a write loses nothing: the row waits, and the next pass
//! of an emitter sends it. A crash of an emitter before its commit rolls
//! back its marks, so the rows are sent again.
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
//! edges, in the two directions. Like `tenant`, this field is routing
//! metadata: the server delivers the event to a socket filtered to one of
//! these boards, and strips the field before the event reaches a client.
//! The client shape does not change.
//!
//! Only the kinds in [`EventKind::changes_blocks_counts`] carry it. The
//! other kinds cannot change a count, and a `relationship_changed` is
//! emitted for each end of the edge with its own board. The emitter
//! computes the field when it sends, with one query for each tenant of the
//! batch ([`send_outbox`]).
//!
//! # The events of a person (KAIROS-T-0381)
//!
//! A notification is for ONE person. [`emit_notification_event`] sends
//! `notification_created` (new notifications) or `notifications_read`
//! (the person read some) with the routing field `recipients`: the server
//! delivers the event only to a socket of a person in the list, and strips
//! the field as it strips `tenant`. One write that tells many persons (the
//! P0 fan-out to a team) sends one event with the list, in parts of at most
//! [`MAX_RECIPIENTS`] (KAIROS-T-0632). The event has the S-0005 shape:
//! `entity_type` is `notification`, `short_code` is the code of the item of
//! the notification (empty for `notifications_read`), and `board_id` is
//! `null`. The text of a notification is not in the event: the client
//! reads it through the REST API.
//!
//! # The precise kinds (KAIROS-T-0574)
//!
//! `task_state_changed`, `task_cancelled` and `claim_changed` come IN
//! ADDITION to the coarse kind of the same write: `item_updated` for a
//! write of the state, a claim, a hand-off and a release, and
//! `item_transitioned` for a cancel and a move. A subscriber that knows
//! only the coarse kinds keeps working. A subscriber that wants to know
//! what changed reads the precise kind. Both come in the same transaction.
//!
//! KAIROS-T-0630: `note_appended` and `note_promoted` come after the
//! `item_updated` of a note, with the id of the note as `change`. A note
//! writes no version of the content, so a client that shows the notes
//! reads the item again for them.
//!
//! `claim_changed` and `board_configured` carry the field `change`
//! ([`emit_event_with_change`]): what changed. For `claim_changed` it is
//! a [`ClaimChange`]; for `board_configured` it is the setting
//! (`column_add`, `transition_remove`, `board_settings`, ...).
//! `board_configured` has `entity_type` `board`, the slug of the board as
//! `short_code`, and the board as `board_id` ([`emit_board_configured`]).
//!
//! # Post-commit semantics (A-0005 §5)
//!
//! The emit functions run INSIDE the service transaction, and that is the
//! point: the outbox row commits with the write, and a rolled back write
//! leaves no row. The emitter reads only committed rows, so a delivered
//! event always describes committed state — A-0005's "emitted
//! post-commit" by construction, with no application-side after-commit
//! hook to forget.
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
//! tenant schema and never receive the slug as an argument, so the outbox
//! row keeps `current_schema()`, and the emitter derives the tag
//! (`org_{slug}` → `{slug}`) — the same derivation
//! [`crate::items::tenant_code_prefix`] uses for the prefix of an item
//! with no board.

use std::collections::{BTreeMap, HashMap};

use chrono::Utc;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Bool, Text, Uuid as SqlUuid};
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
    /// A run of the code index builder started or ended for a repository
    /// (KAIROS-T-0333). `entity_type` is `repository`, `short_code` is the
    /// slug of the repository, and there is no board. A reader of the
    /// repository reads its runs again.
    CodeIndexBuildChanged,
    /// The live state of a task changed (KAIROS-T-0574): a write of the
    /// state, or the clear of the next step when the claim changes. Comes
    /// after the `item_updated` of the same write.
    TaskStateChanged,
    /// A task was cancelled (KAIROS-T-0574). `column_id` is the done column.
    /// Comes after the `item_transitioned` of the cancel.
    TaskCancelled,
    /// The claim of a task changed (KAIROS-T-0574). `change` is a
    /// [`ClaimChange`].
    ClaimChanged,
    /// The configuration of a board changed (KAIROS-T-0574): its columns,
    /// its transitions, a flag or a setting. `entity_type` is `board`,
    /// `short_code` is the slug of the board, `change` names the setting.
    BoardConfigured,
    /// A note was added to the item (KAIROS-T-0630). `change` is the id of
    /// the note (`n-3`). Comes after the `item_updated` of the same write.
    NoteAppended,
    /// A note of the item became a draft ADR or document (KAIROS-T-0630).
    /// `change` is the id of the note. Comes after the `item_updated` of
    /// the same write.
    NotePromoted,
    /// The review state of a task changed (KAIROS-T-0718): `not_needed`,
    /// `needed` or `done`. Comes after the `item_updated` of the same
    /// write.
    TaskReviewChanged,
}

/// What happened to a claim: the `change` of a `claim_changed` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimChange {
    /// A person took the claim: a claim, or a move into Active.
    Taken,
    /// A hand-off gave the claim to a person.
    HandedOff,
    /// The holder or a manager released the claim. The task stays.
    Released,
    /// The claim ended for a different reason: a move out of Active, a
    /// cancel, an archive, a column flag, a person who left.
    Ended,
}

impl ClaimChange {
    /// The wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            ClaimChange::Taken => "taken",
            ClaimChange::HandedOff => "handed_off",
            ClaimChange::Released => "released",
            ClaimChange::Ended => "ended",
        }
    }
}

impl EventKind {
    /// Each kind, in the order of the enum. A test checks that the list is
    /// complete.
    pub const ALL: [EventKind; 17] = [
        EventKind::ItemCreated,
        EventKind::ItemUpdated,
        EventKind::ItemTransitioned,
        EventKind::ItemMoved,
        EventKind::ItemDeleted,
        EventKind::ItemRestored,
        EventKind::RelationshipChanged,
        EventKind::MetadataChanged,
        EventKind::ItemLinksChanged,
        EventKind::CodeIndexBuildChanged,
        EventKind::TaskStateChanged,
        EventKind::TaskCancelled,
        EventKind::ClaimChanged,
        EventKind::BoardConfigured,
        EventKind::NoteAppended,
        EventKind::NotePromoted,
        EventKind::TaskReviewChanged,
    ];

    /// The kind of a wire name, or `None` for a name of no kind (the events
    /// of a person).
    pub fn from_wire(name: &str) -> Option<EventKind> {
        EventKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == name)
    }

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

    /// Whether this kind can change the text that the vectors of the item
    /// are made of (KAIROS-T-0636): its title or content (a create, an edit,
    /// a note), its parent, or its metadata. [`emit_item_change_by_id`]
    /// marks the item for the embedding sweep in its outbox statement (the
    /// mark of [`crate::embeddings::mark_dirty`]).
    pub fn changes_embedded_text(self) -> bool {
        matches!(
            self,
            EventKind::ItemCreated
                | EventKind::ItemUpdated
                | EventKind::RelationshipChanged
                | EventKind::MetadataChanged
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
            EventKind::CodeIndexBuildChanged => "code_index_build_changed",
            EventKind::TaskStateChanged => "task_state_changed",
            EventKind::TaskCancelled => "task_cancelled",
            EventKind::ClaimChanged => "claim_changed",
            EventKind::BoardConfigured => "board_configured",
            EventKind::NoteAppended => "note_appended",
            EventKind::NotePromoted => "note_promoted",
            EventKind::TaskReviewChanged => "task_review_changed",
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

/// The SQL value of the schema of the write: the tenant schema of the
/// service connection.
const SCHEMA_SQL: &str = "COALESCE(current_schema()::text, 'public')";

/// The SQL value that adds `agent_key_id` when an agent made the change
/// (KAIROS-T-0359, [`crate::agent_mark`]), and nothing otherwise.
const AGENT_KEY_SQL: &str = "CASE WHEN NULLIF(current_setting('kairos.agent_key', true), '') IS NULL \
     THEN '{}'::jsonb \
     ELSE jsonb_build_object('agent_key_id', \
          NULLIF(current_setting('kairos.agent_key', true), '')::uuid) END";

/// The SQL value of the placement fields of an item row `p`: `short_code`,
/// `board_id`, and `column_id` when there is one.
const PLACEMENT_SQL: &str = "jsonb_build_object('short_code', p.short_code, 'board_id', p.board_id) \
     || CASE WHEN p.column_id IS NULL THEN '{}'::jsonb \
             ELSE jsonb_build_object('column_id', p.column_id) END";

/// The largest `NOTIFY` payload that the emitter sends. PostgreSQL refuses
/// a payload of 8000 bytes or more.
pub const NOTIFY_LIMIT: usize = 7900;

/// The most rows that one pass of [`send_outbox`] takes.
pub const SEND_BATCH: i64 = 500;

/// The most recipients of one event of a person (module docs, "The events
/// of a person"). 100 UUIDs keep the event under [`NOTIFY_LIMIT`].
pub const MAX_RECIPIENTS: usize = 100;

/// Insert one thin event into the outbox, in the transaction of the write.
/// `payload` has no `tenant`, no `agent_key_id` and no `blocks_boards`:
/// the statement adds the schema and the agent key, and the emitter adds
/// the routing fields.
fn enqueue(conn: &mut PgConnection, payload: &Map<String, Value>) -> Result<(), DieselError> {
    sql_query(format!(
        "INSERT INTO public.event_outbox (schema_name, payload) \
         VALUES ({SCHEMA_SQL}, $1::jsonb || {AGENT_KEY_SQL})"
    ))
    .bind::<Text, _>(Value::Object(payload.clone()).to_string())
    .execute(conn)?;
    Ok(())
}

/// The fields of an event that do not come from the item row.
fn base_payload(
    event: EventKind,
    entity_type: &str,
    actor: Uuid,
    change: Option<&str>,
) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert("event".into(), json!(event.as_str()));
    payload.insert("entity_type".into(), json!(entity_type));
    if let Some(change) = change {
        payload.insert("change".into(), json!(change));
    }
    payload.insert("actor".into(), json!(actor));
    payload.insert("occurred_at".into(), json!(Utc::now()));
    payload
}

/// Emit `event` (see module docs): one row in the outbox, sent after the
/// commit. Call INSIDE the mutating transaction — a rollback drops the
/// row, which is what makes emission post-commit (A-0005 §5).
pub fn emit_event(conn: &mut PgConnection, event: &ThinEvent) -> Result<(), DieselError> {
    emit_event_with_change(conn, event, None)
}

/// [`emit_event`] with the field `change` (module docs, "The precise
/// kinds"). `None` omits the field.
pub fn emit_event_with_change(
    conn: &mut PgConnection,
    event: &ThinEvent,
    change: Option<&str>,
) -> Result<(), DieselError> {
    let mut payload = base_payload(event.event, &event.entity_type, event.actor, change);
    payload.insert("short_code".into(), json!(event.short_code));
    payload.insert("board_id".into(), json!(event.board_id));
    if let Some(column_id) = event.column_id {
        payload.insert("column_id".into(), json!(column_id));
    }
    enqueue(conn, &payload)
}

/// The wire name of the event of a new notification (KAIROS-T-0381).
pub const NOTIFICATION_CREATED: &str = "notification_created";
/// The wire name of the event of notifications that the person read.
pub const NOTIFICATIONS_READ: &str = "notifications_read";

/// Emit an event for the persons in `recipients` (module docs, "The events
/// of a person"): `event` is [`NOTIFICATION_CREATED`] or
/// [`NOTIFICATIONS_READ`]. One row for each [`MAX_RECIPIENTS`] persons.
/// Call it in the transaction of the write, as [`emit_event`].
pub fn emit_notification_event(
    conn: &mut PgConnection,
    event: &str,
    recipients: &[Uuid],
    short_code: &str,
    actor: Uuid,
) -> Result<(), DieselError> {
    for part in recipients.chunks(MAX_RECIPIENTS) {
        let payload = json!({
            "recipients": part,
            "event": event,
            "entity_type": "notification",
            "short_code": short_code,
            "board_id": Value::Null,
            "actor": actor,
            "occurred_at": Utc::now(),
        });
        sql_query(format!(
            "INSERT INTO public.event_outbox (schema_name, payload) VALUES ({SCHEMA_SQL}, $1::jsonb)"
        ))
        .bind::<Text, _>(payload.to_string())
        .execute(conn)?;
    }
    Ok(())
}

/// The row of an item by id `$1` in its entity table: `short_code`,
/// `board_id`, `column_id`. Soft-deleted rows are included: an
/// `item_deleted` event needs the placement of the row that was just
/// stamped. `None` for an unknown entity type.
fn placement_by_id(entity_type: &str) -> Option<&'static str> {
    Some(match entity_type {
        "strategy" => "SELECT short_code, board_id, column_id FROM strategies WHERE id = $1",
        "initiative" => "SELECT short_code, board_id, column_id FROM initiatives WHERE id = $1",
        "task" => "SELECT short_code, board_id, column_id FROM tasks WHERE id = $1",
        "document" => {
            "SELECT short_code, NULL::uuid AS board_id, NULL::uuid AS column_id \
             FROM documents WHERE id = $1"
        }
        "adr" => "SELECT short_code, board_id, column_id FROM adrs WHERE id = $1",
        _ => return None,
    })
}

/// Emit `event` for an item by id. The one `INSERT` reads the short code
/// and the placement of the item (KAIROS-T-0632: no lookup before it).
/// Unknown ids emit nothing (the mutation itself already failed or the row
/// is gone).
pub fn emit_item_event_by_id(
    conn: &mut PgConnection,
    event: EventKind,
    entity_type: &str,
    item_id: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    emit_item_change_by_id(conn, event, entity_type, item_id, actor, None)
}

/// [`emit_item_event_by_id`] with the field `change` (KAIROS-T-0574).
///
/// KAIROS-T-0636: a kind that can change the embedded text
/// ([`EventKind::changes_embedded_text`]) also marks the item for the
/// embedding sweep, in the same transaction. Each item write emits its event
/// here, so each write path marks its item without a call of its own.
pub fn emit_item_change_by_id(
    conn: &mut PgConnection,
    event: EventKind,
    entity_type: &str,
    item_id: Uuid,
    actor: Uuid,
    change: Option<&str>,
) -> Result<(), DieselError> {
    let Some(placement) = placement_by_id(entity_type) else {
        return Ok(());
    };
    let payload = base_payload(event, entity_type, actor, change);
    // KAIROS-T-0636: a kind that can change the embedded text also marks the
    // item for the embedding sweep, in this statement.
    let mark = if event.changes_embedded_text() {
        format!("WITH {} ", crate::embeddings::mark_item_cte())
    } else {
        String::new()
    };
    sql_query(format!(
        "{mark}INSERT INTO public.event_outbox (schema_name, payload) \
         SELECT {SCHEMA_SQL}, $2::jsonb || {PLACEMENT_SQL} || {AGENT_KEY_SQL} \
         FROM ({placement}) p"
    ))
    .bind::<SqlUuid, _>(item_id)
    .bind::<Text, _>(Value::Object(payload).to_string())
    .execute(conn)?;
    Ok(())
}

/// Emit `claim_changed` for the task `task_id` (KAIROS-T-0574).
pub fn emit_claim_changed(
    conn: &mut PgConnection,
    task_id: Uuid,
    change: ClaimChange,
    actor: Uuid,
) -> Result<(), DieselError> {
    emit_item_change_by_id(
        conn,
        EventKind::ClaimChanged,
        "task",
        task_id,
        actor,
        Some(change.as_str()),
    )
}

/// Emit `event` for each item in `short_codes`, in that order, with one
/// `INSERT` (the descendants of an archive cascade, KAIROS-T-0574). Each
/// event has the board of its own item. An unknown code emits nothing.
pub fn emit_item_events_by_codes(
    conn: &mut PgConnection,
    event: EventKind,
    short_codes: &[String],
    actor: Uuid,
) -> Result<(), DieselError> {
    if short_codes.is_empty() {
        return Ok(());
    }
    let mut payload = base_payload(event, "", actor, None);
    payload.remove("entity_type");
    sql_query(format!(
        "INSERT INTO public.event_outbox (schema_name, payload) \
         SELECT {SCHEMA_SQL}, \
                $2::jsonb || jsonb_build_object('entity_type', p.entity_type) \
                || {PLACEMENT_SQL} || {AGENT_KEY_SQL} \
         FROM ( \
             SELECT 'strategy' AS entity_type, short_code, board_id, column_id \
               FROM strategies WHERE short_code = ANY($1) \
             UNION ALL SELECT 'initiative', short_code, board_id, column_id \
               FROM initiatives WHERE short_code = ANY($1) \
             UNION ALL SELECT 'task', short_code, board_id, column_id \
               FROM tasks WHERE short_code = ANY($1) \
             UNION ALL SELECT 'document', short_code, NULL::uuid, NULL::uuid \
               FROM documents WHERE short_code = ANY($1) \
             UNION ALL SELECT 'adr', short_code, board_id, column_id \
               FROM adrs WHERE short_code = ANY($1) \
         ) p \
         ORDER BY array_position($1, p.short_code)"
    ))
    .bind::<Array<Text>, _>(short_codes)
    .bind::<Text, _>(Value::Object(payload).to_string())
    .execute(conn)?;
    Ok(())
}

/// Emit `board_configured` for the board `board_id` (KAIROS-T-0574):
/// `entity_type` `board`, the slug of the board as `short_code`, and
/// `change` the setting that changed. Call it in the transaction of the
/// write. An unknown board emits nothing.
pub fn emit_board_configured(
    conn: &mut PgConnection,
    board_id: Uuid,
    actor: Uuid,
    change: &str,
) -> Result<(), DieselError> {
    let payload = base_payload(EventKind::BoardConfigured, "board", actor, Some(change));
    sql_query(format!(
        "INSERT INTO public.event_outbox (schema_name, payload) \
         SELECT {SCHEMA_SQL}, \
                $2::jsonb || jsonb_build_object('short_code', b.slug, 'board_id', b.id) \
                || {AGENT_KEY_SQL} \
         FROM boards b WHERE b.id = $1"
    ))
    .bind::<SqlUuid, _>(board_id)
    .bind::<Text, _>(Value::Object(payload).to_string())
    .execute(conn)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The emitter (KAIROS-T-0632)
// ---------------------------------------------------------------------------

#[derive(QueryableByName)]
struct OutboxRow {
    #[diesel(sql_type = BigInt)]
    id: i64,
    #[diesel(sql_type = Text)]
    schema_name: String,
    #[diesel(sql_type = Text)]
    payload: String,
}

#[derive(QueryableByName)]
struct BlocksRow {
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = SqlUuid)]
    board_id: Uuid,
}

#[derive(QueryableByName)]
struct PendingRow {
    #[diesel(sql_type = Bool)]
    pending: bool,
}

/// What one pass of [`send_outbox`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Sent {
    /// The rows that the pass took and marked sent.
    pub rows: usize,
    /// The `NOTIFY` statements that it sent.
    pub notifies: usize,
    /// The rows that it could not send: a payload that is not a JSON
    /// object, or an event over [`NOTIFY_LIMIT`] alone. They are marked
    /// sent, so they do not stop the outbox.
    pub dropped: usize,
}

/// Whether the outbox has an unsent row. A cheap check before the
/// transaction of [`send_outbox`].
pub fn outbox_pending(conn: &mut PgConnection) -> Result<bool, DieselError> {
    let row: PendingRow = sql_query(
        "SELECT EXISTS (SELECT 1 FROM public.event_outbox WHERE sent_at IS NULL) AS pending",
    )
    .get_result(conn)?;
    Ok(row.pending)
}

/// One pass of the emitter (module docs, "The outbox"): in one
/// transaction, take at most `limit` unsent rows in id order (`FOR UPDATE
/// SKIP LOCKED`), add `tenant` and `blocks_boards`, send the events in
/// batches of at most [`NOTIFY_LIMIT`] bytes with one `NOTIFY` each, and
/// mark the rows sent.
pub fn send_outbox(conn: &mut PgConnection, limit: i64) -> Result<Sent, DieselError> {
    conn.transaction(|conn| {
        let rows: Vec<OutboxRow> = sql_query(
            "SELECT id, schema_name, payload::text AS payload FROM public.event_outbox \
             WHERE sent_at IS NULL ORDER BY id LIMIT $1 FOR UPDATE SKIP LOCKED",
        )
        .bind::<BigInt, _>(limit)
        .load(conn)?;
        if rows.is_empty() {
            return Ok(Sent::default());
        }
        let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
        let mut dropped = 0usize;
        let mut events: Vec<(String, Map<String, Value>)> = Vec::with_capacity(rows.len());
        for row in rows {
            match serde_json::from_str::<Value>(&row.payload) {
                Ok(Value::Object(payload)) => events.push((row.schema_name, payload)),
                _ => dropped += 1,
            }
        }

        // `blocks_boards`: one query for each tenant of the batch.
        let mut wanted: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (schema, payload) in &events {
            if needs_blocks_boards(payload)
                && let Some(code) = payload.get("short_code").and_then(Value::as_str)
            {
                wanted
                    .entry(schema.clone())
                    .or_default()
                    .push(code.to_string());
            }
        }
        let mut across: HashMap<(String, String), Vec<Uuid>> = HashMap::new();
        for (schema, codes) in &wanted {
            for (code, boards) in blocks_boards_of(conn, schema, codes) {
                across.insert((schema.clone(), code), boards);
            }
        }

        let mut texts = Vec::with_capacity(events.len());
        for (schema, mut payload) in events {
            let boards = payload
                .get("short_code")
                .and_then(Value::as_str)
                .filter(|_| needs_blocks_boards(&payload))
                .and_then(|code| across.get(&(schema.clone(), code.to_string())));
            let text = finish_event(&schema, &mut payload, boards.map(Vec::as_slice));
            match text {
                Some(text) => texts.push(text),
                None => dropped += 1,
            }
        }

        let batches = pack(&texts, NOTIFY_LIMIT);
        for batch in &batches {
            sql_query("SELECT pg_notify($1, $2)")
                .bind::<Text, _>(EVENT_CHANNEL)
                .bind::<Text, _>(batch)
                .execute(conn)?;
        }
        sql_query("UPDATE public.event_outbox SET sent_at = now() WHERE id = ANY($1)")
            .bind::<Array<BigInt>, _>(&ids)
            .execute(conn)?;
        Ok(Sent {
            rows: ids.len(),
            notifies: batches.len(),
            dropped,
        })
    })
}

/// Delete the rows that were sent more than one minute ago. Returns the
/// count. A row that is not sent stays: the emitter sends it first.
pub fn prune_outbox(conn: &mut PgConnection) -> Result<usize, DieselError> {
    sql_query("DELETE FROM public.event_outbox WHERE sent_at < now() - interval '1 minute'")
        .execute(conn)
}

/// Whether the event in `payload` carries `blocks_boards`
/// ([`EventKind::changes_blocks_counts`]).
fn needs_blocks_boards(payload: &Map<String, Value>) -> bool {
    payload
        .get("event")
        .and_then(Value::as_str)
        .and_then(EventKind::from_wire)
        .is_some_and(EventKind::changes_blocks_counts)
}

/// The tenant tag of a schema: `org_acme` gives `acme`.
fn tenant_of(schema: &str) -> &str {
    schema.strip_prefix("org_").unwrap_or(schema)
}

/// Add the routing fields to one event and serialize it: `tenant`, and
/// `blocks_boards` without the board of the event itself (omitted when
/// empty). An event over [`NOTIFY_LIMIT`] loses `blocks_boards`; one that
/// is still over it gives `None`.
fn finish_event(
    schema: &str,
    payload: &mut Map<String, Value>,
    boards: Option<&[Uuid]>,
) -> Option<String> {
    payload.insert("tenant".into(), json!(tenant_of(schema)));
    let own = payload
        .get("board_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok());
    let boards: Vec<Uuid> = boards
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|board| Some(*board) != own)
        .collect();
    if !boards.is_empty() {
        payload.insert("blocks_boards".into(), json!(boards));
    }
    // Two bytes for the brackets of the array.
    let fits = |text: &String| text.len() + 2 <= NOTIFY_LIMIT;
    let text = Value::Object(payload.clone()).to_string();
    if fits(&text) {
        return Some(text);
    }
    payload.remove("blocks_boards")?;
    Some(Value::Object(payload.clone()).to_string()).filter(fits)
}

/// Pack serialized events into JSON arrays of at most `limit` bytes, in
/// order. Each event fits alone ([`finish_event`]).
fn pack(texts: &[String], limit: usize) -> Vec<String> {
    let mut batches = Vec::new();
    let mut current = String::from("[");
    for text in texts {
        let extra = text.len() + usize::from(current.len() > 1);
        if current.len() > 1 && current.len() + extra + 1 > limit {
            current.push(']');
            batches.push(std::mem::replace(&mut current, String::from("[")));
        }
        if current.len() > 1 {
            current.push(',');
        }
        current.push_str(text);
    }
    if current.len() > 1 {
        current.push(']');
        batches.push(current);
    }
    batches
}

/// Quote a schema name as an SQL identifier.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The boards of the live items at the other end of the `blocks` edges of
/// each item in `codes`, in the two directions (COLLIERY-T-0233, module
/// docs), in the tenant `schema`. One query for the batch. The item itself
/// can be archived (an `item_deleted` event); the other end must be live,
/// because an archived item has no card to refresh. Ordered, so the
/// payload is deterministic.
///
/// The query runs in a savepoint. When it fails (a tenant schema that is
/// gone), the events go without the field: they still reach their own
/// board, and the outbox does not stop.
fn blocks_boards_of(
    conn: &mut PgConnection,
    schema: &str,
    codes: &[String],
) -> HashMap<String, Vec<Uuid>> {
    let rows = conn.transaction::<Vec<BlocksRow>, DieselError, _>(|conn| {
        sql_query(format!(
            "SET LOCAL search_path TO {}, public",
            quote_ident(schema)
        ))
        .execute(conn)?;
        // KAIROS-T-0628: one UNION ALL arm per direction, so each arm probes
        // its relationship index (source or target); the CHECK source_id !=
        // target_id keeps the arms disjoint, so no edge is counted twice.
        sql_query(
            "SELECT DISTINCT me.short_code, other.board_id \
             FROM entity_directory me \
             CROSS JOIN LATERAL ( \
                 SELECT r.target_id AS other_id FROM item_relationships r \
                     WHERE r.source_id = me.id AND r.relationship = 'blocks' \
                 UNION ALL \
                 SELECT r.source_id FROM item_relationships r \
                     WHERE r.target_id = me.id AND r.relationship = 'blocks' \
             ) n \
             JOIN entity_directory other \
               ON other.id = n.other_id \
              AND other.deleted_at IS NULL \
             WHERE me.short_code = ANY($1) \
               AND other.board_id IS NOT NULL \
             ORDER BY me.short_code, other.board_id",
        )
        .bind::<Array<Text>, _>(codes)
        .load(conn)
    });
    let mut boards: HashMap<String, Vec<Uuid>> = HashMap::new();
    match rows {
        Ok(rows) => {
            for row in rows {
                boards.entry(row.short_code).or_default().push(row.board_id);
            }
        }
        Err(error) => tracing::warn!(
            %error,
            schema,
            "event outbox: blocks_boards failed; the events go without it"
        ),
    }
    boards
}

/// The setting of a board configuration record: the text before the
/// first `:` of the `board_config` details (`column_add:Review@3` gives
/// `column_add`).
pub fn board_config_change(details: &str) -> &str {
    details.split(':').next().unwrap_or(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_new_kinds_have_their_wire_names() {
        assert_eq!(EventKind::TaskStateChanged.as_str(), "task_state_changed");
        assert_eq!(EventKind::TaskCancelled.as_str(), "task_cancelled");
        assert_eq!(EventKind::ClaimChanged.as_str(), "claim_changed");
        assert_eq!(EventKind::BoardConfigured.as_str(), "board_configured");
        assert_eq!(EventKind::NoteAppended.as_str(), "note_appended");
        assert_eq!(EventKind::NotePromoted.as_str(), "note_promoted");
        assert_eq!(EventKind::TaskReviewChanged.as_str(), "task_review_changed");
        let changes: Vec<&str> = [
            ClaimChange::Taken,
            ClaimChange::HandedOff,
            ClaimChange::Released,
            ClaimChange::Ended,
        ]
        .into_iter()
        .map(ClaimChange::as_str)
        .collect();
        assert_eq!(changes, ["taken", "handed_off", "released", "ended"]);
    }

    #[test]
    fn the_new_kinds_change_no_blocks_count() {
        for kind in [
            EventKind::TaskStateChanged,
            EventKind::TaskCancelled,
            EventKind::ClaimChanged,
            EventKind::BoardConfigured,
            EventKind::NoteAppended,
            EventKind::NotePromoted,
            EventKind::TaskReviewChanged,
        ] {
            assert!(!kind.changes_blocks_counts(), "{}", kind.as_str());
        }
    }

    #[test]
    fn the_change_of_a_board_record_is_its_setting() {
        assert_eq!(board_config_change("column_add:Review@3"), "column_add");
        assert_eq!(
            board_config_change("transition_remove:Todo->Active"),
            "transition_remove"
        );
        assert_eq!(board_config_change("board_settings"), "board_settings");
    }

    /// KAIROS-T-0632: `ALL` names each kind once. The match has no
    /// wildcard, so a new kind does not compile until it is here.
    #[test]
    fn all_lists_each_kind_once() {
        let position = |kind: EventKind| match kind {
            EventKind::ItemCreated => 0,
            EventKind::ItemUpdated => 1,
            EventKind::ItemTransitioned => 2,
            EventKind::ItemMoved => 3,
            EventKind::ItemDeleted => 4,
            EventKind::ItemRestored => 5,
            EventKind::RelationshipChanged => 6,
            EventKind::MetadataChanged => 7,
            EventKind::ItemLinksChanged => 8,
            EventKind::CodeIndexBuildChanged => 9,
            EventKind::TaskStateChanged => 10,
            EventKind::TaskCancelled => 11,
            EventKind::ClaimChanged => 12,
            EventKind::BoardConfigured => 13,
            EventKind::NoteAppended => 14,
            EventKind::NotePromoted => 15,
            EventKind::TaskReviewChanged => 16,
        };
        for (n, kind) in EventKind::ALL.into_iter().enumerate() {
            assert_eq!(position(kind), n, "{}", kind.as_str());
            assert_eq!(EventKind::from_wire(kind.as_str()), Some(kind));
        }
        assert_eq!(EventKind::from_wire(NOTIFICATION_CREATED), None);
        assert_eq!(EventKind::from_wire("item_vanished"), None);
    }

    /// KAIROS-T-0632: the batches of one pass. Each batch is a JSON array
    /// under the limit, in order, and no event is lost or split.
    #[test]
    fn events_pack_into_arrays_under_the_limit() {
        let texts: Vec<String> = (0..50)
            .map(|n| json!({"event": "item_updated", "n": n, "pad": "x".repeat(300)}).to_string())
            .collect();
        let batches = pack(&texts, NOTIFY_LIMIT);
        assert!(batches.len() > 1, "300 bytes x 50 do not fit in one");
        let mut seen = Vec::new();
        for batch in &batches {
            assert!(batch.len() <= NOTIFY_LIMIT, "{} bytes", batch.len());
            let Value::Array(events) = serde_json::from_str(batch).expect("JSON array") else {
                panic!("not an array: {batch}");
            };
            seen.extend(events.into_iter().map(|e| e["n"].as_i64().expect("n")));
        }
        assert_eq!(seen, (0..50).collect::<Vec<i64>>(), "in order, each once");

        assert!(pack(&[], NOTIFY_LIMIT).is_empty(), "no event, no NOTIFY");
        let one = vec![json!({"n": 1}).to_string()];
        assert_eq!(pack(&one, NOTIFY_LIMIT), vec![r#"[{"n":1}]"#.to_string()]);
        // A limit of exactly two events of 7 bytes: `[a,b]` is 17 bytes.
        let two = vec![json!({"n": 1}).to_string(), json!({"n": 2}).to_string()];
        assert_eq!(pack(&two, 17).len(), 1);
        assert_eq!(pack(&two, 16).len(), 2);
    }

    /// KAIROS-T-0632: the emitter adds `tenant`, and `blocks_boards`
    /// without the board of the event. An event too big for a NOTIFY
    /// loses `blocks_boards`, or is dropped.
    #[test]
    fn the_emitter_adds_the_routing_fields() {
        let (own, across) = (Uuid::new_v4(), Uuid::new_v4());
        let mut payload = json!({"event": "item_transitioned", "short_code": "ACME-T-0001",
                                 "board_id": own})
        .as_object()
        .cloned()
        .expect("object");
        assert!(needs_blocks_boards(&payload));
        let text = finish_event("org_acme", &mut payload, Some(&[own, across])).expect("fits");
        let event: Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(event["tenant"], "acme");
        assert_eq!(event["blocks_boards"], json!([across]));

        let mut only_own = json!({"event": "item_deleted", "board_id": own})
            .as_object()
            .cloned()
            .expect("object");
        let text = finish_event("org_acme", &mut only_own, Some(&[own])).expect("fits");
        let event: Value = serde_json::from_str(&text).expect("JSON");
        assert!(event.get("blocks_boards").is_none(), "omitted when empty");

        let many: Vec<Uuid> = (0..300).map(|_| Uuid::new_v4()).collect();
        let mut big = json!({"event": "item_moved", "board_id": own})
            .as_object()
            .cloned()
            .expect("object");
        let text = finish_event("org_acme", &mut big, Some(&many)).expect("fits without");
        let event: Value = serde_json::from_str(&text).expect("JSON");
        assert!(event.get("blocks_boards").is_none(), "{} bytes", text.len());
        assert_eq!(event["event"], "item_moved");

        let mut huge = json!({"event": "item_updated", "pad": "x".repeat(NOTIFY_LIMIT)})
            .as_object()
            .cloned()
            .expect("object");
        assert!(!needs_blocks_boards(&huge));
        assert_eq!(finish_event("org_acme", &mut huge, None), None);

        assert_eq!(tenant_of("org_acme"), "acme");
        assert_eq!(tenant_of("public"), "public");
        assert_eq!(quote_ident(r#"org_a"b"#), r#""org_a""b""#);
    }

    /// KAIROS-T-0632: 100 recipients and the other fields of the event
    /// stay under the limit of a NOTIFY.
    #[test]
    fn a_full_list_of_recipients_fits_in_a_notify() {
        let recipients: Vec<Uuid> = (0..MAX_RECIPIENTS).map(|_| Uuid::new_v4()).collect();
        let mut payload = json!({
            "recipients": recipients,
            "event": NOTIFICATION_CREATED,
            "entity_type": "notification",
            "short_code": "LONGPREFIXX-T-123456",
            "board_id": Value::Null,
            "actor": Uuid::new_v4(),
            "occurred_at": Utc::now(),
        })
        .as_object()
        .cloned()
        .expect("object");
        let schema = format!("org_{}", "a".repeat(63));
        assert!(finish_event(&schema, &mut payload, None).is_some());
    }
}
