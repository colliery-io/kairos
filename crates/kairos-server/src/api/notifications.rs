//! The notifications of a person (COLLIERY-I-0614):
//!
//! - `/api/me/notification-settings` (KAIROS-T-0381): the kinds of
//!   notification that a person turned off;
//! - `GET /api/me/notifications` and `POST /api/me/notifications/read`
//!   (KAIROS-T-0382): the list, and the mark as read. The read state is
//!   of the caller: the person, or the agent key of the request
//!   (KAIROS-T-0550). The MCP tools
//!   `get_events` and `mark_events_read` call the same functions
//!   ([`list_for`], [`mark_for`]).
//!
//! Each person reads and changes only their own notifications and settings.
//! A service account gets no notifications, so it has no settings (403).

use axum::extract::{Extension, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use kairos_client::types_notifications::{
    MarkNotificationsReadRequest, MarkNotificationsReadResponse, MutedKey, Notification,
    NotificationList, NotificationSettings, UpdateNotificationSettingsRequest,
};
use kairos_core::notifications::NotificationKind;
use kairos_db::notifications::{Mark, Reader};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/me/notification-settings",
            get(get_notification_settings).put(update_notification_settings),
        )
        .route("/api/me/notifications", get(list_notifications))
        .route("/api/me/notifications/read", post(mark_notifications_read))
}

/// The most unread notifications in a list, and in a page.
pub(crate) const LIST_UNREAD: i64 = 100;
/// The read notifications in a list.
pub(crate) const LIST_READ: i64 = 20;
/// The most read notifications in a page of `read_before` (KAIROS-T-0584).
pub(crate) const LIST_READ_PAGE: i64 = 100;

/// The reader of a request (KAIROS-T-0550): the person, or the agent key
/// of the request. Each keeps its own read state.
pub(crate) fn reader_for(
    conn: &mut diesel::pg::PgConnection,
    user: Uuid,
    agent_key: Option<Uuid>,
) -> Result<Reader, ApiError> {
    Reader::of(conn, user, agent_key).map_err(ApiError::internal)
}

/// Parse `ids` as notifications of the person of `reader`. An id that is
/// not a UUID, or not a notification of the person, is refused and named.
/// `field` names the input of the ids in a refusal.
fn notification_ids(
    conn: &mut diesel::pg::PgConnection,
    reader: &Reader,
    ids: &[String],
    field: &str,
) -> Result<Vec<Uuid>, ApiError> {
    let mut parsed = Vec::with_capacity(ids.len());
    let mut bad = Vec::new();
    for id in ids {
        match Uuid::parse_str(id.trim()) {
            Ok(uuid) => parsed.push(uuid),
            Err(_) => bad.push(id.clone()),
        }
    }
    let unknown =
        kairos_db::notifications::not_of(conn, reader.user, &parsed).map_err(ApiError::internal)?;
    bad.extend(unknown.iter().map(Uuid::to_string));
    if !bad.is_empty() {
        return Err(ApiError::not_found(format!(
            "You have no notification with the id {}. Kairos changed nothing. \
             get_events and GET /api/me/notifications give the ids.",
            bad.join(", ")
        ))
        .with_details(serde_json::json!({ "field": field })));
    }
    Ok(parsed)
}

/// The list of the reader: the unread notifications (newest first,
/// [`LIST_UNREAD`] at most), then the [`LIST_READ`] newest read ones, and
/// the count of the unread ones. With `before` (an id), the next page: the
/// unread ones older than that one, and no read ones. With `read_before`
/// (the id of a read notification, KAIROS-T-0584), the next page of the
/// read ones: [`LIST_READ_PAGE`] at most, read before that one, and no
/// unread ones.
pub(crate) fn list_for(
    conn: &mut diesel::pg::PgConnection,
    reader: &Reader,
    before: Option<&str>,
    read_before: Option<&str>,
) -> Result<(Vec<kairos_db::notifications::Notification>, i64), ApiError> {
    if let Some(read_before) = read_before {
        if before.is_some() {
            return Err(ApiError::validation(
                "Send `before` or `read_before`, not both. `before` gives the next page of \
                 the unread notifications. `read_before` gives the next page of the read ones.",
            )
            .with_details(serde_json::json!({
                "field": "before",
                "fields": ["before", "read_before"],
            })));
        }
        let id = notification_ids(conn, reader, &[read_before.to_string()], "read_before")?[0];
        let items = kairos_db::notifications::read_page(conn, reader, id, LIST_READ_PAGE)
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                ApiError::validation(format!(
                    "The notification {id} is unread for you. `read_before` takes a read \
                         notification. Use `before` for the next page of the unread ones."
                ))
                .with_details(serde_json::json!({ "field": "read_before" }))
            })?;
        let unread =
            kairos_db::notifications::unread_count(conn, reader).map_err(ApiError::internal)?;
        return Ok((items, unread));
    }
    let before = match before {
        Some(id) => notification_ids(conn, reader, &[id.to_string()], "before")?
            .first()
            .copied(),
        None => None,
    };
    let items = kairos_db::notifications::list(conn, reader, before, LIST_UNREAD, LIST_READ)
        .map_err(ApiError::internal)?;
    let unread =
        kairos_db::notifications::unread_count(conn, reader).map_err(ApiError::internal)?;
    Ok((items, unread))
}

/// Mark notifications read for the reader: `ids`, or `all: true` with
/// `up_to` (KAIROS-T-0550). `up_to` is the id of the newest notification
/// that the caller read: Kairos marks it and each older unread one, and no
/// newer one. `all` without `up_to` is refused, so a notification that
/// came after the caller's list stays unread. An id that is not a UUID, or
/// not a notification of the person, is refused and named, and nothing
/// changes. Returns the ids that changed, and the count of the unread ones
/// after.
pub(crate) fn mark_for(
    conn: &mut diesel::pg::PgConnection,
    reader: &Reader,
    ids: Option<Vec<String>>,
    all: Option<bool>,
    up_to: Option<String>,
) -> Result<(Vec<Uuid>, i64), ApiError> {
    let ids = ids.filter(|ids| !ids.is_empty());
    let all = all == Some(true);
    let changed = match (ids, all, up_to) {
        (Some(_), true, _) => {
            return Err(
                ApiError::validation("Send `ids` or `all: true` with `up_to`, not both.")
                    .with_details(serde_json::json!({ "field": "ids", "fields": ["ids", "all"] })),
            );
        }
        (Some(_), false, Some(_)) => {
            return Err(
                ApiError::validation("`up_to` goes with `all: true`, not with `ids`.")
                    .with_details(serde_json::json!({ "field": "up_to" })),
            );
        }
        (Some(ids), false, None) => {
            let parsed = notification_ids(conn, reader, &ids, "ids")?;
            kairos_db::notifications::mark_read(conn, reader, Mark::Ids(&parsed))
        }
        (None, true, Some(up_to)) => {
            let parsed = notification_ids(conn, reader, &[up_to], "up_to")?;
            kairos_db::notifications::mark_read(conn, reader, Mark::UpTo(parsed[0]))
        }
        (None, true, None) => {
            return Err(ApiError::validation(
                "`all: true` needs `up_to`: the id of the newest notification that you read. \
                 get_events gives it first. Kairos marks that one and each older one, and no \
                 newer one.",
            )
            .with_details(serde_json::json!({ "field": "up_to" })));
        }
        (None, false, _) => {
            return Err(ApiError::validation(
                "Send `ids`, the ids of your notifications. Or send `all: true` with `up_to`, \
                 the id of the newest notification that you read.",
            )
            .with_details(serde_json::json!({ "field": "ids", "fields": ["ids", "all"] })));
        }
    }
    .map_err(ApiError::internal)?;
    let unread =
        kairos_db::notifications::unread_count(conn, reader).map_err(ApiError::internal)?;
    Ok((changed, unread))
}

fn notification_view(n: kairos_db::notifications::Notification) -> Notification {
    Notification {
        id: n.id.to_string(),
        kind: n.kind,
        short_code: n.short_code,
        entity_type: n.entity_type,
        title: n.title,
        text: n.text,
        actor_id: n.actor_id.map(|id| id.to_string()),
        created_at: n
            .created_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        read_at: n
            .read_at
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        agent_key_id: n.agent_key_id.map(|id| id.to_string()),
        agent_key_name: n.agent_key_name,
    }
}

/// Query parameters of [`list_notifications`].
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    /// The id of a notification: the page has the unread ones older than it.
    pub before: Option<String>,
    /// The id of a read notification: the page has the read ones read
    /// before it (KAIROS-T-0584).
    pub read_before: Option<String>,
}

/// The caller's notifications: each unread one (newest first, 100 at most),
/// then the 20 newest read ones. `before` gives the next page of unread
/// ones. `read_before` gives the next page of read ones: 100 at most, read
/// before that one (KAIROS-T-0584). The read state is the caller's: the person, or the agent key of
/// the request (KAIROS-T-0550). A service account has none.
#[utoipa::path(
    get,
    path = "/api/me/notifications",
    tag = "me",
    params(
        ("before" = Option<String>, Query, description = "The id of a notification. The page has the unread notifications older than it, and no read ones"),
        ("read_before" = Option<String>, Query, description = "The id of a read notification. The page has the read notifications that the caller read before it, 100 at most, and no unread ones"),
    ),
    responses(
        (status = 200, description = "The unread notifications, then the newest read ones", body = NotificationList),
        (status = 404, description = "`before` or `read_before` is not a notification of the caller", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "`before` and `read_before` together, or `read_before` is unread for the caller", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_notifications(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> Result<Json<NotificationList>, ApiError> {
    let (user, key) = (auth.user_id, auth.agent_key);
    let (items, unread) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let reader = reader_for(conn, user, key)?;
            list_for(
                conn,
                &reader,
                query.before.as_deref(),
                query.read_before.as_deref(),
            )
        })
        .await?;
    Ok(Json(NotificationList {
        unread,
        items: items.into_iter().map(notification_view).collect(),
    }))
}

/// Mark notifications of the caller read: `ids`, or `all: true` with
/// `up_to`. The read is the caller's: the person, or the agent key of the
/// request (KAIROS-T-0550). An id that is not a notification of the caller
/// is a 404 that names it, and nothing changes. An id that is read already
/// does not change.
#[utoipa::path(
    post,
    path = "/api/me/notifications/read",
    tag = "me",
    request_body = MarkNotificationsReadRequest,
    responses(
        (status = 200, description = "The ids that changed, and the unread count after", body = MarkNotificationsReadResponse),
        (status = 404, description = "An id is not a notification of the caller", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Neither ids nor all, or both, or all without up_to", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn mark_notifications_read(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<MarkNotificationsReadRequest>,
) -> Result<Json<MarkNotificationsReadResponse>, ApiError> {
    let (user, key) = (auth.user_id, auth.agent_key);
    let (changed, unread) = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let reader = reader_for(conn, user, key)?;
            mark_for(conn, &reader, body.ids, body.all, body.up_to)
        })
        .await?;
    Ok(Json(MarkNotificationsReadResponse {
        marked: changed.iter().map(Uuid::to_string).collect(),
        unread,
    }))
}

/// Refuse a service account: notifications are for people.
pub(crate) fn require_person(
    conn: &mut diesel::pg::PgConnection,
    user: Uuid,
) -> Result<(), ApiError> {
    if kairos_db::task_claims::is_person(conn, user).map_err(ApiError::internal)? {
        return Ok(());
    }
    Err(ApiError::forbidden(
        "A service account gets no notifications. Notifications are for people.",
    ))
}

fn settings_view(
    settings: &BTreeMap<NotificationKind, bool>,
    muted: Vec<kairos_db::notices::MutedKey>,
) -> NotificationSettings {
    let on = |kind| settings.get(&kind).copied().unwrap_or(true);
    NotificationSettings {
        hand_off: on(NotificationKind::HandOff),
        blocker_cleared: on(NotificationKind::BlockerCleared),
        blocked_again: on(NotificationKind::BlockedAgain),
        cancelled: on(NotificationKind::Cancelled),
        blocker_cancelled: on(NotificationKind::BlockerCancelled),
        mention: on(NotificationKind::Mention),
        p0_bug: on(NotificationKind::P0Bug),
        delivery_failing: on(NotificationKind::DeliveryFailing),
        agent_key_revoked: on(NotificationKind::AgentKeyRevoked),
        claim_dropped: on(NotificationKind::ClaimDropped),
        claim_stale: on(NotificationKind::ClaimStale),
        key_expiring: on(NotificationKind::KeyExpiring),
        proposal_decided: on(NotificationKind::ProposalDecided),
        watch: on(NotificationKind::Watch),
        notice: on(NotificationKind::Notice),
        muted_keys: muted
            .into_iter()
            .map(|k| MutedKey {
                id: k.id.to_string(),
                name: k.name,
                prefix: k.prefix,
                owner_id: k.owner_id.to_string(),
                owner_name: k.owner_name,
            })
            .collect(),
    }
}

/// The settings and the muted keys of `user`.
fn settings_of(
    conn: &mut diesel::pg::PgConnection,
    user: Uuid,
) -> Result<NotificationSettings, ApiError> {
    let settings = kairos_db::notifications::settings(conn, user).map_err(ApiError::internal)?;
    let muted = kairos_db::notices::muted_keys(conn, user).map_err(ApiError::internal)?;
    Ok(settings_view(&settings, muted))
}

/// Parse the ids of agent keys of `field` (KAIROS-T-0737). An id that is
/// not a UUID, or not an agent key of a person, is refused and named.
fn agent_key_ids(
    conn: &mut diesel::pg::PgConnection,
    ids: &[String],
    field: &str,
) -> Result<Vec<Uuid>, ApiError> {
    let mut parsed = Vec::with_capacity(ids.len());
    let mut bad = Vec::new();
    for id in ids {
        match Uuid::parse_str(id.trim()) {
            Ok(uuid) => parsed.push(uuid),
            Err(_) => bad.push(id.clone()),
        }
    }
    let unknown = kairos_db::notices::not_agent_keys(conn, &parsed).map_err(ApiError::internal)?;
    bad.extend(unknown.iter().map(Uuid::to_string));
    if !bad.is_empty() {
        return Err(ApiError::not_found(format!(
            "No agent key has the id {}. Kairos changed nothing. A notification of a notice \
             from an agent key gives the id of the key (`agent_key_id`).",
            bad.join(", ")
        ))
        .with_details(serde_json::json!({ "field": field })));
    }
    Ok(parsed)
}

/// The caller's notification settings: each kind, and whether it is on.
#[utoipa::path(
    get,
    path = "/api/me/notification-settings",
    tag = "me",
    responses(
        (status = 200, description = "Each kind, and whether it is on", body = NotificationSettings),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_notification_settings(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<NotificationSettings>, ApiError> {
    let user = auth.user_id;
    let settings = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_person(conn, user)?;
            settings_of(conn, user)
        })
        .await?;
    Ok(Json(settings))
}

/// Turn kinds of notification on or off for the caller, and mute or unmute
/// agent keys (KAIROS-T-0737). A kind that is not in the body does not
/// change. Kairos refuses a body with no kind and no key.
#[utoipa::path(
    put,
    path = "/api/me/notification-settings",
    tag = "me",
    request_body = UpdateNotificationSettingsRequest,
    responses(
        (status = 200, description = "The settings after the change", body = NotificationSettings),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "An id of `mute_keys` or `unmute_keys` is not an agent key", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "The body names no kind and no key, or a key is in `mute_keys` and `unmute_keys`", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_notification_settings(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<UpdateNotificationSettingsRequest>,
) -> Result<Json<NotificationSettings>, ApiError> {
    let changes: Vec<(NotificationKind, bool)> = [
        (NotificationKind::HandOff, body.hand_off),
        (NotificationKind::BlockerCleared, body.blocker_cleared),
        (NotificationKind::BlockedAgain, body.blocked_again),
        (NotificationKind::Cancelled, body.cancelled),
        (NotificationKind::BlockerCancelled, body.blocker_cancelled),
        (NotificationKind::Mention, body.mention),
        (NotificationKind::P0Bug, body.p0_bug),
        (NotificationKind::DeliveryFailing, body.delivery_failing),
        (NotificationKind::AgentKeyRevoked, body.agent_key_revoked),
        (NotificationKind::ClaimDropped, body.claim_dropped),
        (NotificationKind::ClaimStale, body.claim_stale),
        (NotificationKind::KeyExpiring, body.key_expiring),
        (NotificationKind::ProposalDecided, body.proposal_decided),
        (NotificationKind::Watch, body.watch),
        (NotificationKind::Notice, body.notice),
    ]
    .into_iter()
    .filter_map(|(kind, value)| value.map(|v| (kind, v)))
    .collect();
    let mute = body.mute_keys.unwrap_or_default();
    let unmute = body.unmute_keys.unwrap_or_default();
    if let Some(both) = mute.iter().find(|id| unmute.contains(id)) {
        return Err(ApiError::validation(format!(
            "The key {both} is in `mute_keys` and in `unmute_keys`. Send it in one of them."
        ))
        .with_details(serde_json::json!({ "field": "mute_keys" })));
    }
    if changes.is_empty() && mute.is_empty() && unmute.is_empty() {
        return Err(ApiError::validation(
            "The body names no kind. Send one or more kinds, each with true or false. The \
             kinds: hand_off, blocker_cleared, blocked_again, cancelled, blocker_cancelled, \
             mention, p0_bug, delivery_failing, agent_key_revoked, claim_dropped, claim_stale, \
             key_expiring, proposal_decided, watch, notice. Or send mute_keys or unmute_keys.",
        )
        .with_details(serde_json::json!({
            "field": "hand_off",
            "fields": [
                "hand_off",
                "blocker_cleared",
                "blocked_again",
                "cancelled",
                "blocker_cancelled",
                "mention",
                "p0_bug",
                "delivery_failing",
                "agent_key_revoked",
                "claim_dropped",
                "claim_stale",
                "key_expiring",
                "proposal_decided",
                "watch",
                "notice",
                "mute_keys",
                "unmute_keys",
            ],
        })));
    }
    let user = auth.user_id;
    let settings = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_person(conn, user)?;
            crate::api::atomically(conn, |conn| {
                let mute = agent_key_ids(conn, &mute, "mute_keys")?;
                let unmute = agent_key_ids(conn, &unmute, "unmute_keys")?;
                kairos_db::notifications::set_settings(conn, user, &changes)
                    .map_err(ApiError::internal)?;
                kairos_db::notices::set_mutes(conn, user, &mute, &unmute)
                    .map_err(ApiError::internal)?;
                settings_of(conn, user)
            })
        })
        .await?;
    Ok(Json(settings))
}
