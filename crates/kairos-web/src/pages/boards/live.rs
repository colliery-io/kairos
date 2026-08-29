//! `/ws/events` live updates for the board view (KAIROS-T-0040, contract
//! per docs/api/events.md / KAIROS-A-0005 §5).
//!
//! The server pushes thin change notifications; the view reacts by
//! re-fetching through REST (never by patching local state — conventions
//! §4). Browser `WebSocket` clients cannot set request headers, so the
//! token travels via the ONE documented fallback, the `?access_token=`
//! query parameter (`kairos-server::ws::promote_query_token`).
//!
//! Semantics implemented here (best-effort, A-0005 §5):
//! - on open: send `{"subscribe": {"board_id": …}}` to filter the stream;
//! - on every event: run `refetch` (an event carries no payload — the
//!   REST re-fetch refreshes the affected cards);
//! - on close: reconnect with exponential backoff (1s → 15s cap), and on
//!   every RE-open run `refetch` once — the silent reconcile for whatever
//!   was missed while disconnected;
//! - on unmount (the returned guard drops): close the socket, cancel the
//!   backoff timer chain, and break the closure ↔ state `Rc` cycle.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::set_timeout;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{MessageEvent, WebSocket};

use super::data::ThinEvent;
use crate::auth::Auth;

/// Reconnect backoff bounds (milliseconds).
const BACKOFF_INITIAL_MS: u64 = 1_000;
const BACKOFF_MAX_MS: u64 = 15_000;

/// A stored wasm event-handler closure (present while a socket is live).
type Handler<T> = RefCell<Option<Closure<T>>>;

/// The per-connection state shared by the event handlers and the
/// reconnect timer chain.
struct Live {
    auth: Auth,
    /// `Some` filters the stream to one board; `None` subscribes to the
    /// whole tenant stream (`{"subscribe": {}}` clears the server filter —
    /// the graph view's mode, KAIROS-T-0090).
    board_id: Option<String>,
    /// Runs on every event and every reconnect. The argument is the
    /// event's `short_code` when one arrived (reconnect reconciles pass
    /// `None`) — consumers that only care THAT something changed ignore it.
    refetch: Box<dyn Fn(Option<&str>)>,
    /// Set by the drop guard: no further reconnects, handlers inert.
    closed: Cell<bool>,
    /// Consecutive failed/closed connections (drives the backoff).
    attempts: Cell<u32>,
    /// Whether any connection ever opened (re-opens trigger a reconcile
    /// re-fetch; the very first open does not — the view is loading).
    ever_opened: Cell<bool>,
    socket: RefCell<Option<WebSocket>>,
    // The wasm closures backing the socket's event handlers. Kept here so
    // they outlive the registration; cleared by the drop guard (this is
    // what breaks the `Rc` cycle state → closure → state).
    on_open: Handler<dyn FnMut()>,
    on_message: Handler<dyn FnMut(MessageEvent)>,
    on_close: Handler<dyn FnMut()>,
}

/// Dropping this closes the socket and stops the reconnect chain. Hold it
/// in the component (`on_cleanup(move || drop(guard))`).
pub struct LiveBoardGuard {
    state: Rc<Live>,
}

impl Drop for LiveBoardGuard {
    fn drop(&mut self) {
        self.state.closed.set(true);
        // Detach handlers first so close() doesn't schedule a reconnect.
        if let Some(socket) = self.state.socket.borrow_mut().take() {
            socket.set_onopen(None);
            socket.set_onmessage(None);
            socket.set_onclose(None);
            socket.set_onerror(None);
            let _ = socket.close();
        }
        self.state.on_open.borrow_mut().take();
        self.state.on_message.borrow_mut().take();
        self.state.on_close.borrow_mut().take();
    }
}

/// Subscribe the board view to `/ws/events`, filtered to `board_id`.
/// `refetch` runs on every board event and on every reconnect.
pub fn subscribe_board_events(
    auth: Auth,
    board_id: String,
    refetch: impl Fn() + 'static,
) -> LiveBoardGuard {
    subscribe(auth, Some(board_id), move |_| refetch())
}

/// Subscribe to the WHOLE tenant stream (no board filter): the graph
/// view's mode (KAIROS-T-0090). `on_event` receives the event's
/// short_code when present, `None` on reconnect reconciles.
pub fn subscribe_all_events(
    auth: Auth,
    on_event: impl Fn(Option<&str>) + 'static,
) -> LiveBoardGuard {
    subscribe(auth, None, on_event)
}

fn subscribe(
    auth: Auth,
    board_id: Option<String>,
    refetch: impl Fn(Option<&str>) + 'static,
) -> LiveBoardGuard {
    let state = Rc::new(Live {
        auth,
        board_id,
        refetch: Box::new(refetch),
        closed: Cell::new(false),
        attempts: Cell::new(0),
        ever_opened: Cell::new(false),
        socket: RefCell::new(None),
        on_open: RefCell::new(None),
        on_message: RefCell::new(None),
        on_close: RefCell::new(None),
    });
    connect(Rc::clone(&state));
    LiveBoardGuard { state }
}

/// The `ws(s)://…/ws/events?access_token=…` URL for the current origin.
/// Browser WS clients cannot set the Authorization header — the query
/// parameter is the documented fallback (docs/api/events.md).
fn events_url(token: &str) -> Option<String> {
    let location = web_sys::window()?.location();
    let protocol = location.protocol().ok()?;
    let host = location.host().ok()?;
    let scheme = if protocol == "https:" { "wss" } else { "ws" };
    Some(format!("{scheme}://{host}/ws/events?access_token={token}"))
}

/// Open one connection attempt and register its handlers.
fn connect(state: Rc<Live>) {
    if state.closed.get() {
        return;
    }
    // No session (mid-expiry): retry later; the shell guard owns re-auth.
    let Some(url) = state.auth.token().and_then(|t| events_url(&t)) else {
        schedule_reconnect(state);
        return;
    };
    let Ok(socket) = WebSocket::new(&url) else {
        schedule_reconnect(state);
        return;
    };

    let on_open = {
        let state = Rc::clone(&state);
        Closure::wrap(Box::new(move || {
            if state.closed.get() {
                return;
            }
            state.attempts.set(0);
            // Filter the stream to this board — or clear the filter for
            // the whole tenant stream (S-0005 subscribe message).
            let subscribe = match &state.board_id {
                Some(board_id) => {
                    serde_json::json!({ "subscribe": { "board_id": board_id } }).to_string()
                }
                None => serde_json::json!({ "subscribe": {} }).to_string(),
            };
            if let Some(socket) = state.socket.borrow().as_ref() {
                let _ = socket.send_with_str(&subscribe);
            }
            if state.ever_opened.replace(true) {
                // Reconnect: silently reconcile whatever was missed.
                (state.refetch)(None);
            }
        }) as Box<dyn FnMut()>)
    };
    let on_message = {
        let state = Rc::clone(&state);
        Closure::wrap(Box::new(move |event: MessageEvent| {
            if state.closed.get() {
                return;
            }
            let Some(text) = event.data().as_string() else {
                return;
            };
            // Thin event → re-fetch through REST (events carry routing
            // fields only; the payload is always refetched).
            if serde_json::from_str::<ThinEvent>(&text).is_ok() {
                let short_code = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("short_code")
                            .and_then(|code| code.as_str())
                            .map(str::to_string)
                    });
                (state.refetch)(short_code.as_deref());
            }
        }) as Box<dyn FnMut(MessageEvent)>)
    };
    let on_close = {
        let state = Rc::clone(&state);
        Closure::wrap(Box::new(move || {
            if state.closed.get() {
                return;
            }
            state.socket.borrow_mut().take();
            schedule_reconnect(Rc::clone(&state));
        }) as Box<dyn FnMut()>)
    };

    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    // An errored socket also fires `close`; one handler covers both.
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    state.socket.replace(Some(socket));
    state.on_open.replace(Some(on_open));
    state.on_message.replace(Some(on_message));
    state.on_close.replace(Some(on_close));
}

/// Queue the next connection attempt with exponential backoff.
fn schedule_reconnect(state: Rc<Live>) {
    if state.closed.get() {
        return;
    }
    let attempt = state.attempts.get();
    state.attempts.set(attempt.saturating_add(1));
    let delay = (BACKOFF_INITIAL_MS << attempt.min(4)).min(BACKOFF_MAX_MS);
    set_timeout(move || connect(state), Duration::from_millis(delay));
}

#[cfg(test)]
mod tests {
    /// Backoff doubles from 1s and caps at 15s (host-testable math for
    /// the wasm-only module).
    #[test]
    fn backoff_doubles_and_caps() {
        let delays: Vec<u64> = (0u32..7)
            .map(|attempt| (super::BACKOFF_INITIAL_MS << attempt.min(4)).min(super::BACKOFF_MAX_MS))
            .collect();
        assert_eq!(
            delays,
            vec![1_000, 2_000, 4_000, 8_000, 15_000, 15_000, 15_000]
        );
    }
}
