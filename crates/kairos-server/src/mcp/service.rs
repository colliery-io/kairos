//! The MCP service type behind `/mcp` (KAIROS-T-0026): per-request caller
//! resolution and the S-0006 error contract; the tool implementations live
//! in [`super::tools`].

use axum::http::request::Parts;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool_handler};
use serde_json::json;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// One MCP service instance (rmcp builds one per session via the service
/// factory in [`super::router`]). Holds the shared [`AppState`]; the
/// caller identity is NOT stored here — it is read per tool call from the
/// HTTP request that delivered it (see [`KairosMcp::caller`]), so a
/// session always acts as whoever authenticated the current request.
#[derive(Clone)]
pub struct KairosMcp {
    pub(super) state: AppState,
    pub(super) tool_router: ToolRouter<Self>,
}

impl KairosMcp {
    /// Build a service instance over the shared state.
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    /// The authenticated caller of the CURRENT tool call: the
    /// [`AuthContext`] + [`TenantContext`] the middleware stack attached to
    /// the HTTP request, which rmcp forwards as `http::request::Parts`
    /// inside the MCP request extensions. Absence is a server bug (the
    /// middleware rejects unauthenticated requests before rmcp runs).
    pub(super) fn caller(
        context: &RequestContext<RoleServer>,
    ) -> Result<(AuthContext, TenantContext), ErrorData> {
        let parts = context.extensions.get::<Parts>().ok_or_else(|| {
            ErrorData::internal_error("no HTTP request parts on the MCP request", None)
        })?;
        let auth = parts.extensions.get::<AuthContext>().cloned();
        let tenant = parts.extensions.get::<TenantContext>().cloned();
        match (auth, tenant) {
            (Some(auth), Some(tenant)) => Ok((auth, tenant)),
            _ => Err(ErrorData::internal_error(
                "MCP request reached the handler without auth/tenant context",
                None,
            )),
        }
    }
}

// `#[tool_handler]` generates `call_tool`/`list_tools`/`get_tool` over
// `self.tool_router` (built by the `#[tool_router]` block in
// `super::tools`); `get_info` is provided by hand for REQ-1.7.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for KairosMcp {
    fn get_info(&self) -> ServerInfo {
        // REQ-1.7: `initialize` reports the server version (tool schemas
        // are versioned with the server).
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("kairos", env!("CARGO_PKG_VERSION")).with_title("Kairos"),
            )
            .with_instructions(
                "Kairos work-item tools. Start with `whoami` (identity, teams, board \
                 capabilities) and `my_boards` (boards + columns). Items are identified \
                 by short code (e.g. ACME-T-0012) everywhere. Use `search` to find \
                 items, `get_item` for full content, and the write tools \
                 (create/update/edit/transition/link/set_metadata/delete) to work \
                 them; writes require board capabilities and edits use optimistic \
                 versioning.",
            )
    }
}

/// Render an [`ApiError`] as the S-0006 REQ-1.1 typed tool-error text:
/// first line `CODE: message` (the same stable codes as the REST
/// envelope), then a `details:` JSON line when the error carries
/// structured extras (e.g. `allowed_targets` on `INVALID_TRANSITION`,
/// `current` on `CONFLICT`).
pub(super) fn tool_error(e: ApiError) -> CallToolResult {
    let mut text = format!("{}: {}", e.code, e.message);
    if e.details != json!({}) {
        text.push_str("\ndetails: ");
        text.push_str(&e.details.to_string());
    }
    CallToolResult::error(vec![ContentBlock::text(text)])
}

/// A successful tool result: one compact markdown/text content block
/// (REQ-1.6).
pub(super) fn tool_text(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}
