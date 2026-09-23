//! Wire types for the `GET /ws/events` WebSocket channel (KAIROS-T-0022,
//! contract per KAIROS-A-0005 §5 / KAIROS-S-0005 "Event Push").
//!
//! Same dependency discipline as [`crate::types`]: serde/utoipa only, ids
//! travel as canonical UUID strings and timestamps as RFC 3339.
//!
//! The channel is tenant-scoped (bound at upgrade time) and best-effort:
//! events are thin change hints with NO payloads and NO replay — clients
//! re-fetch details through the REST API, and reconcile by re-fetching
//! after a reconnect. Authentication is the standard bearer token; browser
//! clients (which cannot set WebSocket headers) pass it as an
//! `?access_token=` query parameter instead.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One thin change event pushed by the server (S-0005 shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ThinEvent {
    /// `item_created|item_updated|item_transitioned|item_moved|item_deleted|`
    /// `relationship_changed|metadata_changed`.
    pub event: String,
    /// `strategy|initiative|task|document|adr`.
    pub entity_type: String,
    /// The affected item's short code — re-fetch it via the REST API.
    pub short_code: String,
    /// The item's board (UUID); `null` for off-board items (documents,
    /// unplaced ADRs).
    pub board_id: Option<String>,
    /// The item's (new) column (UUID); omitted when not applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_id: Option<String>,
    /// The acting user's id (UUID).
    pub actor: String,
    /// When the change happened (RFC 3339).
    pub occurred_at: String,
}

/// Client → server message: `{"subscribe": {"board_id": "uuid"}}` filters
/// subsequent events to one board; `{"subscribe": {}}` clears the filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SubscribeRequest {
    /// The filter to apply from this message on.
    pub subscribe: SubscribeFilter,
}

/// The [`SubscribeRequest`] filter body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SubscribeFilter {
    /// Only deliver events for this board (UUID); absent/`null` delivers
    /// every event of the connection's tenant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
}
