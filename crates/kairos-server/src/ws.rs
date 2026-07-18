//! `GET /ws/events` — the tenant-scoped WebSocket event channel
//! (KAIROS-T-0022, contract per KAIROS-A-0005 §5 / KAIROS-S-0005 "Event
//! Push").
//!
//! # Fan-out
//!
//! One dedicated LISTEN connection per server process ([`spawn_listener`],
//! raw `tokio-postgres` — the pooled diesel connections cannot sit on a
//! long-lived `LISTEN`) receives every `NOTIFY kairos_events` the mutating
//! services emit (`kairos_db::events`, delivered by PostgreSQL on commit),
//! parses the tenant-tagged payload, and forwards it into a
//! `tokio::sync::broadcast` channel. Each connected socket subscribes to
//! the broadcast and forwards only the events of ITS tenant (bound at
//! upgrade time from the resolved [`TenantContext`]), optionally filtered
//! to one board by a client `{"subscribe": {"board_id": "..."}}` message.
//! If the LISTEN connection drops it is re-established with exponential
//! backoff (logged, with a reconnect counter on the [`EventHub`]).
//!
//! # Authentication (same middleware, same semantics)
//!
//! The upgrade request passes through the standard stack —
//! [`crate::middleware::auth::require_auth`] then
//! [`crate::middleware::tenant::require_tenant`] — so a missing/invalid
//! token is the usual 401, a non-member the usual 403, and an unknown
//! tenant the usual 404, all BEFORE the protocol upgrade. Tokens travel in
//! the `Authorization: Bearer` header; because browser `WebSocket` clients
//! cannot set request headers, the ONE supported fallback is the
//! `?access_token=<jwt>` query parameter, which [`promote_query_token`]
//! copies into the `Authorization` header ahead of the auth layer (browser
//! clients resolve their tenant via the `Host` subdomain, A-0005 §2).
//!
//! # Best-effort semantics (A-0005 §5)
//!
//! Events are thin UI-freshness hints, not a durable stream: NO replay, NO
//! ordering guarantee beyond per-connection FIFO. A socket that falls
//! behind the broadcast buffer simply skips the lagged events. Clients
//! reconcile by re-fetching through the REST API after (re)connecting.
//! Disconnects are clean by construction: the per-socket task is the only
//! resource, and it ends when the socket does.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Request};
use axum::http::HeaderValue;
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self as axum_middleware, Next};
use axum::response::Response;
use axum::routing::get;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::app::AppState;
use crate::middleware::tenant::TenantContext;
use crate::middleware::{auth, tenant};

/// The NOTIFY channel the db services emit on (`kairos_db::events`).
const EVENT_CHANNEL: &str = "kairos_events";

/// Broadcast buffer per server process. Sockets that fall more than this
/// many events behind skip the overwritten ones (best-effort, see module
/// docs).
const BROADCAST_CAPACITY: usize = 1024;

/// Reconnect backoff bounds for the LISTEN connection.
const BACKOFF_INITIAL: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// One parsed NOTIFY payload, ready for fan-out: the routing fields the
/// sockets filter on, plus the client-facing message (tenant stripped).
#[derive(Debug)]
struct BroadcastEvent {
    /// The emitting tenant's slug (fan-out routing; never sent to clients).
    tenant: String,
    /// The event's board, when it has one (client-side subscribe filter).
    board_id: Option<Uuid>,
    /// The serialized S-0005 thin event forwarded to matching sockets.
    message: String,
}

/// The per-process fan-out hub: LISTEN → broadcast → sockets.
#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<Arc<BroadcastEvent>>,
    /// How many times the LISTEN connection was (re-)established after the
    /// first attempt — the "reconnects" metric (logged on each increment).
    reconnects: Arc<AtomicU64>,
}

impl EventHub {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            tx,
            reconnects: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Build the `/ws/events` route behind the standard auth → tenant stack
/// (plus the browser token fallback) and start this process's LISTEN
/// task. Merged into the production router by [`crate::app::router`].
pub fn router(state: AppState) -> Router<AppState> {
    let hub = EventHub::new();
    spawn_listener(state.config.database_url.clone(), hub.clone());
    Router::new()
        .route("/ws/events", get(ws_events))
        .layer(Extension(hub))
        // route_layer wraps bottom-up (the layer added LAST runs FIRST):
        // promote_query_token → require_auth → require_tenant — the
        // A-0010 ordering with the browser fallback ahead of it.
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            tenant::require_tenant,
        ))
        .route_layer(axum_middleware::from_fn_with_state(
            state,
            auth::require_auth,
        ))
        .route_layer(axum_middleware::from_fn(promote_query_token))
}

// ---------------------------------------------------------------------------
// Browser token fallback (module docs: the ONE fallback is ?access_token=)
// ---------------------------------------------------------------------------

/// The `access_token` query parameter, if present and non-empty. JWTs are
/// URL-safe by construction, so no percent-decoding is needed.
fn query_access_token(query: &str) -> Option<&str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("access_token="))
        .filter(|token| !token.is_empty())
}

/// Copy `?access_token=<jwt>` into the `Authorization` header when the
/// header is absent, so the standard [`auth::require_auth`] layer (and its
/// 401 semantics) applies unchanged to browser WebSocket clients.
async fn promote_query_token(mut req: Request, next: Next) -> Response {
    if !req.headers().contains_key(AUTHORIZATION)
        && let Some(token) = req.uri().query().and_then(query_access_token)
        && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        req.headers_mut().insert(AUTHORIZATION, value);
    }
    next.run(req).await
}

// ---------------------------------------------------------------------------
// The socket side
// ---------------------------------------------------------------------------

/// The client subscribe message (`kairos_client::types_events`):
/// `{"subscribe": {"board_id": "uuid"}}` filters, `{"subscribe": {}}`
/// clears.
#[derive(Debug, serde::Deserialize)]
struct ClientMessage {
    subscribe: Option<SubscribeFilter>,
}

#[derive(Debug, serde::Deserialize)]
struct SubscribeFilter {
    board_id: Option<Uuid>,
}

/// `GET /ws/events`: upgrade the (already authenticated, tenant-resolved)
/// request and serve the connection until the client disconnects.
async fn ws_events(
    ws: WebSocketUpgrade,
    Extension(tenant): Extension<TenantContext>,
    Extension(hub): Extension<EventHub>,
) -> Response {
    let rx = hub.tx.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, tenant.slug, rx))
}

/// Forward this tenant's events to one socket until it closes. The task
/// owns nothing but the socket and its broadcast receiver, so ending it IS
/// the cleanup (no leaked tasks).
async fn handle_socket(
    mut socket: WebSocket,
    tenant: String,
    mut rx: broadcast::Receiver<Arc<BroadcastEvent>>,
) {
    let mut board_filter: Option<Uuid> = None;
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Ok(event) => {
                    if event.tenant != tenant {
                        continue;
                    }
                    if let Some(board) = board_filter
                        && event.board_id != Some(board)
                    {
                        continue;
                    }
                    if socket
                        .send(Message::Text(event.message.clone().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    // Best-effort: the client missed `skipped` events and
                    // will reconcile by re-fetching (module docs).
                    tracing::debug!(tenant, skipped, "event socket lagged; events dropped");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(parsed) = serde_json::from_str::<ClientMessage>(&text)
                        && let Some(filter) = parsed.subscribe
                    {
                        board_filter = filter.board_id;
                    }
                }
                // Ping/pong are answered by the protocol layer while we
                // keep reading; binary frames are ignored.
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The LISTEN side
// ---------------------------------------------------------------------------

/// Parse one NOTIFY payload (`kairos_db::events` shape): pull out the
/// routing fields and strip `tenant` so the forwarded message is exactly
/// the S-0005 client shape.
fn parse_payload(payload: &str) -> Option<BroadcastEvent> {
    let mut value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let object = value.as_object_mut()?;
    let tenant = match object.remove("tenant") {
        Some(serde_json::Value::String(tenant)) => tenant,
        _ => return None,
    };
    let board_id = object
        .get("board_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    Some(BroadcastEvent {
        tenant,
        board_id,
        message: value.to_string(),
    })
}

/// Start the per-process LISTEN task: connect, `LISTEN kairos_events`,
/// forward notifications into the hub, and reconnect with exponential
/// backoff when the connection drops. Requires a Tokio runtime; without
/// one (sync router construction in unit tests) the channel simply stays
/// silent.
fn spawn_listener(database_url: String, hub: EventHub) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("no tokio runtime at router construction; /ws/events listener not started");
        return;
    };
    handle.spawn(async move {
        let mut backoff = BACKOFF_INITIAL;
        loop {
            match run_listener(&database_url, &hub, &mut backoff).await {
                Ok(()) => tracing::warn!("event LISTEN connection closed"),
                Err(error) => tracing::warn!(%error, "event LISTEN connection failed"),
            }
            let reconnects = hub.reconnects.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::warn!(
                reconnects,
                backoff_ms = backoff.as_millis() as u64,
                "event listener reconnecting"
            );
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(BACKOFF_MAX);
        }
    });
}

/// One LISTEN connection lifetime: returns when the connection ends
/// (`Ok`) or errors. Resets `backoff` once the `LISTEN` is established.
async fn run_listener(
    database_url: &str,
    hub: &EventHub,
    backoff: &mut Duration,
) -> Result<(), tokio_postgres::Error> {
    let (client, mut connection) =
        tokio_postgres::connect(database_url, tokio_postgres::NoTls).await?;

    // `client` executes the LISTEN while `connection` is polled for it —
    // both live in this task, multiplexed by the select below. After the
    // LISTEN completes only notifications remain.
    let listen = client.batch_execute("LISTEN kairos_events");
    tokio::pin!(listen);
    let mut listening = false;

    loop {
        tokio::select! {
            result = listen.as_mut(), if !listening => {
                result?;
                listening = true;
                *backoff = BACKOFF_INITIAL;
                tracing::info!("event listener attached (LISTEN kairos_events)");
            }
            message = std::future::poll_fn(|cx| connection.poll_message(cx)) => {
                match message {
                    Some(Ok(tokio_postgres::AsyncMessage::Notification(n)))
                        if n.channel() == EVENT_CHANNEL =>
                    {
                        match parse_payload(n.payload()) {
                            // Send errors just mean no socket is connected.
                            Some(event) => drop(hub.tx.send(Arc::new(event))),
                            None => tracing::warn!(
                                payload = n.payload(),
                                "unparseable kairos_events payload dropped"
                            ),
                        }
                    }
                    Some(Ok(_)) => {} // other channels / notices
                    Some(Err(error)) => return Err(error),
                    None => return Ok(()), // connection closed
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_parse_and_strip_tenant() {
        let board = Uuid::new_v4();
        let event = parse_payload(&format!(
            r#"{{"tenant":"acme","event":"item_created","entity_type":"task",
                "short_code":"ACME-T-0001","board_id":"{board}",
                "actor":"7f9c24e5-2b12-4d6a-8f4e-0e1d2c3b4a59",
                "occurred_at":"2026-07-10T12:00:00Z"}}"#
        ))
        .expect("payload parses");
        assert_eq!(event.tenant, "acme");
        assert_eq!(event.board_id, Some(board));
        let forwarded: serde_json::Value =
            serde_json::from_str(&event.message).expect("forwarded message is JSON");
        assert!(forwarded.get("tenant").is_none(), "tenant is stripped");
        assert_eq!(forwarded["event"], "item_created");
        assert_eq!(forwarded["short_code"], "ACME-T-0001");
    }

    #[test]
    fn off_board_and_malformed_payloads() {
        let event = parse_payload(
            r#"{"tenant":"acme","event":"item_updated","entity_type":"document",
                "short_code":"ACME-D-0001","board_id":null,
                "actor":"7f9c24e5-2b12-4d6a-8f4e-0e1d2c3b4a59",
                "occurred_at":"2026-07-10T12:00:00Z"}"#,
        )
        .expect("off-board payload parses");
        assert_eq!(event.board_id, None);

        assert!(parse_payload("not json").is_none());
        assert!(
            parse_payload(r#"{"event":"item_created"}"#).is_none(),
            "no tenant tag"
        );
    }

    #[test]
    fn access_token_query_extraction() {
        assert_eq!(
            query_access_token("access_token=abc.def.ghi"),
            Some("abc.def.ghi")
        );
        assert_eq!(query_access_token("foo=1&access_token=t&bar=2"), Some("t"));
        assert_eq!(query_access_token("access_token="), None);
        assert_eq!(query_access_token("other=1"), None);
    }
}
