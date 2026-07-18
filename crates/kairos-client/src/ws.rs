//! The `/ws/events` client helper (KAIROS-T-0024, channel contract per
//! KAIROS-A-0005 §5 / KAIROS-S-0005 "Event Push"): connect with the
//! client's bearer token + tenant, apply/clear the `board_id` subscribe
//! filter, and read the stream as typed [`ThinEvent`]s.
//!
//! The channel is thin and best-effort — no payloads, no replay; clients
//! re-fetch through the REST methods on [`KairosClient`]. Protocol-level
//! handshake probes (missing/garbage tokens, the `?access_token=` browser
//! fallback) stay outside this helper by design: it always authenticates
//! via the `Authorization` header.

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite};

use crate::client::KairosClient;
use crate::error::Error;
use crate::types::ErrorEnvelope;
use crate::types_events::{SubscribeFilter, SubscribeRequest, ThinEvent};

/// An open `/ws/events` connection.
pub struct EventStream {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl std::fmt::Debug for EventStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}

impl KairosClient {
    /// Open the tenant-scoped `/ws/events` channel with this client's
    /// bearer token (and `X-Tenant`, if configured). A rejected upgrade
    /// with an S-0005 body maps to the typed [`Error`] variants, exactly
    /// like a REST call.
    pub async fn connect_events(&self) -> Result<EventStream, Error> {
        let token = self.token.bearer_token().await?;
        let ws_base = ws_base_url(&self.base_url)?;
        let mut request = format!("{ws_base}/ws/events")
            .into_client_request()
            .map_err(|e| Error::WebSocket(e.to_string()))?;
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| Error::WebSocket(e.to_string()))?,
        );
        if let Some(tenant) = &self.tenant {
            request.headers_mut().insert(
                "x-tenant",
                HeaderValue::from_str(tenant).map_err(|e| Error::WebSocket(e.to_string()))?,
            );
        }
        match connect_async(request).await {
            Ok((socket, _)) => Ok(EventStream { socket }),
            Err(tungstenite::Error::Http(response)) => {
                let status = response.status().as_u16();
                let body = response
                    .body()
                    .as_deref()
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                    .unwrap_or_default();
                match serde_json::from_str::<ErrorEnvelope>(&body) {
                    Ok(envelope) => Err(Error::from_envelope(status, envelope)),
                    Err(_) => Err(Error::UnexpectedResponse { status, body }),
                }
            }
            Err(e) => Err(Error::WebSocket(e.to_string())),
        }
    }
}

impl EventStream {
    /// Filter subsequent events to one board
    /// (`{"subscribe": {"board_id": ...}}`).
    pub async fn subscribe_board(&mut self, board_id: &str) -> Result<(), Error> {
        self.send_subscribe(SubscribeFilter {
            board_id: Some(board_id.to_string()),
        })
        .await
    }

    /// Clear the board filter (`{"subscribe": {}}` — deliver every tenant
    /// event again).
    pub async fn subscribe_all(&mut self) -> Result<(), Error> {
        self.send_subscribe(SubscribeFilter::default()).await
    }

    async fn send_subscribe(&mut self, filter: SubscribeFilter) -> Result<(), Error> {
        let message =
            serde_json::to_string(&SubscribeRequest { subscribe: filter }).map_err(|source| {
                Error::Decode {
                    context: "subscribe request".to_string(),
                    source,
                }
            })?;
        self.socket
            .send(Message::text(message))
            .await
            .map_err(|e| Error::WebSocket(e.to_string()))
    }

    /// The next event as a typed [`ThinEvent`], skipping ping/pong frames.
    /// Callers own timeout policy (`tokio::time::timeout`).
    pub async fn next_event(&mut self) -> Result<ThinEvent, Error> {
        let raw = self.next_event_raw().await?;
        serde_json::from_value(raw.clone()).map_err(|source| Error::Decode {
            context: format!("thin event {raw}"),
            source,
        })
    }

    /// The next event as raw JSON — for wire-shape assertions the typed
    /// DTO would mask (e.g. "no tenant tag ever reaches a client").
    pub async fn next_event_raw(&mut self) -> Result<Value, Error> {
        loop {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str(&text).map_err(|source| Error::Decode {
                        context: format!("event frame {text:?}"),
                        source,
                    });
                }
                Some(Ok(Message::Close(frame))) => {
                    return Err(Error::WebSocket(format!("closed by server: {frame:?}")));
                }
                Some(Ok(_)) => continue, // ping/pong/binary
                Some(Err(e)) => return Err(Error::WebSocket(e.to_string())),
                None => return Err(Error::WebSocket("stream ended".to_string())),
            }
        }
    }

    /// Send a close frame and drain the socket.
    pub async fn close(mut self) -> Result<(), Error> {
        self.socket
            .close(None)
            .await
            .map_err(|e| Error::WebSocket(e.to_string()))
    }
}

/// `http(s)://…` → `ws(s)://…`.
fn ws_base_url(base_url: &str) -> Result<String, Error> {
    if let Some(rest) = base_url.strip_prefix("http://") {
        Ok(format!("ws://{rest}"))
    } else if let Some(rest) = base_url.strip_prefix("https://") {
        Ok(format!("wss://{rest}"))
    } else {
        Err(Error::WebSocket(format!(
            "base URL {base_url:?} is not http(s)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_base_url_scheme_mapping() {
        assert_eq!(
            ws_base_url("http://127.0.0.1:8080").expect("ws"),
            "ws://127.0.0.1:8080"
        );
        assert_eq!(
            ws_base_url("https://kairos.example").expect("wss"),
            "wss://kairos.example"
        );
        assert!(ws_base_url("ftp://nope").is_err());
    }
}
