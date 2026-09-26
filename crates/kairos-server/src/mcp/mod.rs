//! The Kairos MCP server (KAIROS-T-0026, architecture per KAIROS-A-0011,
//! tool surface per KAIROS-S-0006): a remote streamable-HTTP MCP endpoint
//! at `/mcp` on the API binary, implemented with the official Rust SDK
//! (`rmcp`), whose tools call the same in-process `kairos-core`/`kairos-db`
//! services as the REST handlers — no HTTP loopback.
//!
//! # Mounting and authentication (A-0011)
//!
//! [`router`] mounts the rmcp [`StreamableHttpService`] at `/mcp` behind
//! the SAME `require_auth` → `require_tenant` middleware stack as `/api`:
//! every MCP HTTP request (initialize included) carries a bearer token
//! validated by [`crate::middleware::auth`], and the tenant resolves from
//! the connection host exactly as for `/api` (S-0006 REQ-1.2 — no tenant
//! parameter exists on any tool). The MCP session acts **as the
//! authenticated user** under full ABAC — the middleware's [`AuthContext`]
//! and [`TenantContext`] request extensions travel to the tool handlers
//! inside the `http::request::Parts` that rmcp injects into each MCP
//! request's extensions, so every tool call executes as whichever user
//! authenticated the HTTP request that delivered it.
//!
//! rmcp's own DNS-rebinding `Host` allowlist is disabled: Kairos resolves
//! tenants FROM the `Host` header (A-0005 §2), so host semantics belong to
//! the tenant middleware, and the endpoint is bearer-authenticated — the
//! rebinding protection that allowlist exists for does not apply.
//!
//! # OAuth protected resource (RFC 9728)
//!
//! The router also serves protected-resource metadata at
//! `/.well-known/oauth-protected-resource` and
//! `/.well-known/oauth-protected-resource/mcp` (the path-suffixed form for
//! the `/mcp` resource), pointing `authorization_servers` at the configured
//! OIDC issuer (A-0010). 401 responses from `/mcp` carry a
//! `WWW-Authenticate: Bearer resource_metadata="…"` challenge so MCP
//! clients can discover the metadata and drive the OAuth flow.
//!
//! # Sessions (NFR-1.2)
//!
//! The transport runs in stateful streamable-HTTP mode with the in-memory
//! [`LocalSessionManager`] — MCP protocol session state only. No Kairos
//! state (working board, team, …) is held server-side; agents discover
//! context via `whoami`/`my_boards` (A-0011 "Session context").

mod service;
mod tools;

use axum::extract::{Request, State};
use axum::http::header::{HOST, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router, middleware as axum_middleware};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::app::AppState;
use crate::middleware::{auth, tenant};

pub use service::KairosMcp;

/// The RFC 9728 well-known path for the `/mcp` protected resource.
const WELL_KNOWN_MCP: &str = "/.well-known/oauth-protected-resource/mcp";

/// The `/mcp` endpoint (behind auth → tenant) plus the open RFC 9728
/// metadata routes; merged into the production router by
/// [`crate::app::router`].
pub fn router(state: AppState) -> Router<AppState> {
    let mcp_service = StreamableHttpService::new(
        {
            let state = state.clone();
            move || Ok(KairosMcp::new(state.clone()))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().disable_allowed_hosts(),
    );

    Router::new()
        .route_service("/mcp", mcp_service)
        // route_layer wraps bottom-up (last added runs first): the
        // WWW-Authenticate mapper observes auth's 401s, then auth, then
        // tenant — the A-0010 ordering with RFC 9728 advertisement on top.
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            tenant::require_tenant,
        ))
        .route_layer(axum_middleware::from_fn_with_state(
            state,
            auth::require_auth,
        ))
        .route_layer(axum_middleware::from_fn(advertise_resource_metadata))
        // RFC 9728: metadata is served unauthenticated (clients fetch it
        // BEFORE they have a token) at both the root and the
        // path-suffixed well-known locations.
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(WELL_KNOWN_MCP, get(protected_resource_metadata))
}

/// `scheme://host` of the request as seen by the client: `Host` header
/// (always present on HTTP/1.1) with an `X-Forwarded-Proto` scheme when a
/// proxy provides one, `http` otherwise.
fn request_origin(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(HOST)?.to_str().ok()?;
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    Some(format!("{scheme}://{host}"))
}

/// Attach the RFC 9728 `WWW-Authenticate` challenge to 401 responses from
/// `/mcp`, pointing clients at the protected-resource metadata document.
async fn advertise_resource_metadata(req: Request, next: Next) -> Response {
    let origin = request_origin(req.headers());
    let mut response = next.run(req).await;
    if response.status() == StatusCode::UNAUTHORIZED
        && let Some(origin) = origin
        && let Ok(value) = HeaderValue::from_str(&format!(
            "Bearer resource_metadata=\"{origin}{WELL_KNOWN_MCP}\""
        ))
    {
        response.headers_mut().insert(WWW_AUTHENTICATE, value);
    }
    response
}

/// RFC 9728 protected-resource metadata: the `/mcp` resource identifier
/// (derived from the request host, like tenant resolution) and the
/// deployment's OIDC issuer as its authorization server (A-0010/A-0011).
async fn protected_resource_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<Value> {
    let origin = request_origin(&headers).unwrap_or_else(|| "http://localhost".to_string());
    Json(json!({
        "resource": format!("{origin}/mcp"),
        // KAIROS-T-0208: an EMPTY list when this deployment has no issuer, rather
        // than a list containing null. An MCP client reads this to find out where to
        // get a token; "there is nowhere" is the honest answer, and `[null]` would
        // send it to fetch a discovery document from the string "null".
        "authorization_servers": state
            .config
            .oidc_issuer_url
            .as_ref()
            .map(|issuer| vec![issuer.clone()])
            .unwrap_or_default(),
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["openid", "email", "profile"],
    }))
}
