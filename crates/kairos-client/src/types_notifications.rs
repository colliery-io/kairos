//! Wire types of the notifications of a person (KAIROS-T-0381,
//! COLLIERY-I-0614): the settings that turn a kind off, the list, and the
//! mark as read (KAIROS-T-0382).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// `GET` and `PUT /api/me/notification-settings` response: each kind, and
/// whether it is on. A kind that is off makes no notification for the
/// person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationSettings {
    /// A person handed off the claim of a task to you.
    pub hand_off: bool,
    /// A blocker of an item that you have the claim on (or own) is done,
    /// archived or purged, or its `blocks` link is removed.
    pub blocker_cleared: bool,
    /// A blocker of an item that you have the claim on (or own, or made)
    /// left a done column, so it blocks again (KAIROS-T-0568).
    pub blocked_again: bool,
    /// A person cancelled a task that you created.
    pub cancelled: bool,
    /// A person cancelled a task that blocks an item that you have the
    /// claim on, own or made. The cancelled task still blocks
    /// (KAIROS-T-0567).
    pub blocker_cancelled: bool,
    /// A person wrote `@your_user_name` in a note or in the content of an
    /// item.
    pub mention: bool,
    /// A bug with the priority P0 (critical) is on the board of your team.
    pub p0_bug: bool,
    /// A webhook target that you own was turned off after failed
    /// deliveries (KAIROS-T-0384).
    pub delivery_failing: bool,
    /// An organization admin revoked an agent key of yours
    /// (KAIROS-T-0388).
    pub agent_key_revoked: bool,
    /// A person left the organization, so their claim on a task of the
    /// board of your team ended. The task is free (KAIROS-T-0531).
    pub claim_dropped: bool,
    /// A claim had no write and no state change for some days: your claim,
    /// or a claim on a task of an initiative that you own (KAIROS-T-0531).
    pub claim_stale: bool,
    /// An agent key of yours, or a key of a service account of your
    /// organization (you are an admin), expires within 7 days
    /// (KAIROS-T-0540).
    pub key_expiring: bool,
    /// A person confirmed or rejected an edge proposal that you made
    /// (KAIROS-T-0558).
    pub proposal_decided: bool,
    /// An item that you watch, or a child of an item that you watch with
    /// its children, moved, was edited, got a note, or is done
    /// (KAIROS-T-0736).
    pub watch: bool,
}

/// `PUT /api/me/notification-settings` body: the kinds to turn on (`true`)
/// or off (`false`). A kind that is not in the body does not change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNotificationSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand_off: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_cleared: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_again: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_cancelled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p0_bug: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_failing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_key_revoked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_dropped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_stale: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_expiring: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_decided: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<bool>,
}

/// One notification of the caller (KAIROS-T-0382).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Notification {
    /// The id (UUID). `POST /api/me/notifications/read` takes it.
    pub id: String,
    /// `hand_off|blocker_cleared|blocked_again|cancelled|blocker_cancelled|mention|p0_bug|delivery_failing|agent_key_revoked|claim_dropped|claim_stale|key_expiring|proposal_decided|watch`.
    pub kind: String,
    /// The code of the item now; `null` when the item is gone.
    pub short_code: Option<String>,
    /// `strategy|initiative|task|document|adr`; `null` when the item is
    /// gone.
    pub entity_type: Option<String>,
    /// The title of the item now; `null` when the item is gone.
    pub title: Option<String>,
    /// What happened, in one sentence or two.
    pub text: String,
    /// The person or the service account that made the change (UUID).
    pub actor_id: Option<String>,
    /// When Kairos made the notification (RFC 3339).
    pub created_at: String,
    /// When the person read it (RFC 3339); `null` while unread.
    pub read_at: Option<String>,
}

/// `GET /api/me/notifications` response: each unread notification (newest
/// first, 100 at most), then the 20 newest read ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationList {
    /// The count of all the unread notifications of the caller.
    pub unread: i64,
    pub items: Vec<Notification>,
}

/// `POST /api/me/notifications/read` body: `ids`, or `all: true` with
/// `up_to`. Not both.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkNotificationsReadRequest {
    /// The ids of notifications of the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    /// `true`: each unread notification of the caller up to `up_to`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    /// With `all: true`, required: the id of the newest notification that
    /// the caller read. Kairos marks it and each older unread one, and no
    /// newer one (KAIROS-T-0550).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_to: Option<String>,
}

/// `POST /api/me/notifications/read` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MarkNotificationsReadResponse {
    /// The ids that changed from unread to read.
    pub marked: Vec<String>,
    /// The count of the unread notifications after the change.
    pub unread: i64,
}

/// The watch of the caller on one item (KAIROS-T-0736): `GET`, `PUT` and
/// `DELETE /api/items/{short_code}/watch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WatchState {
    /// The code of the item.
    pub short_code: String,
    /// `true` when the caller watches the item.
    pub watching: bool,
    /// `true` when the watch includes the children of the item (its
    /// `parent` edges, one level). `false` when the caller does not watch.
    pub with_children: bool,
}

/// `PUT /api/items/{short_code}/watch` body. A second `PUT` changes
/// `with_children` of the watch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WatchRequest {
    /// `true`: a change of a child of the item tells you too. Default
    /// `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with_children: Option<bool>,
}

/// One watch of the caller, in `GET /api/me/watches`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WatchedItem {
    /// The code of the item now.
    pub short_code: String,
    /// `strategy|initiative|task|document|adr`.
    pub entity_type: String,
    /// The title of the item now.
    pub title: String,
    /// `true` when the item is archived.
    pub archived: bool,
    /// `true` when the watch includes the children of the item.
    pub with_children: bool,
    /// When the caller started the watch (RFC 3339).
    pub created_at: String,
}

/// `GET /api/me/watches` response: each watch of the caller, by code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WatchList {
    pub items: Vec<WatchedItem>,
}
