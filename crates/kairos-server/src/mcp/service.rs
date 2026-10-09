//! The MCP service type behind `/mcp` (KAIROS-T-0026): per-request caller
//! resolution and the S-0006 error contract; the tool implementations live
//! in [`super::tools`].

use axum::http::request::Parts;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ServerCapabilities,
    ServerInfo,
};
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
//
// `call_tool` is by hand (COLLIERY-T-0256): the refusal of the arguments
// comes from the extractor as an error, and the agent gets it as a tool
// error, in the form of each other tool error (`super::arguments`).
#[tool_handler(router = self.tool_router)]
impl ServerHandler for KairosMcp {
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // KAIROS-T-0359: rmcp runs the tool in the task of the MCP session,
        // not in the task of the HTTP request, so the scope of the tenant
        // middleware does not reach here. Set it again from the request.
        let agent_key = context
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<AuthContext>())
            .and_then(|auth| auth.agent_key);
        let call = ToolCallContext::new(self, request, context);
        // KAIROS-T-0359: a write to a task that a different person has the
        // claim on gets a warning line (`crate::claims`).
        let (result, warnings) = crate::claims::collect(crate::blocking::with_agent_key(
            agent_key,
            self.tool_router.call(call),
        ))
        .await;
        match result {
            Ok(mut result) => {
                if result.is_error != Some(true) && !warnings.is_empty() {
                    let lines: Vec<String> = warnings.iter().map(|w| w.mcp_text()).collect();
                    match result.content.first_mut() {
                        Some(ContentBlock::Text(text)) => {
                            text.text.push_str("\n\n");
                            text.text.push_str(&lines.join("\n"));
                        }
                        _ => result.content.push(ContentBlock::text(lines.join("\n"))),
                    }
                }
                Ok(result)
            }
            Err(e) => super::arguments::tool_error_of(e),
        }
    }

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
                 versioning. Repositories: `list_repositories` and `get_repository` \
                 read the directory, `add_repository` and `update_repository` write \
                 it, and `set_repository` links a task to one.",
            )
    }
}

/// Render an [`ApiError`] as the S-0006 REQ-1.1 typed tool-error text:
/// first line `CODE: message` (the same stable codes as the REST
/// envelope), then a `details:` JSON line when the error carries
/// structured extras (e.g. `allowed_targets` on `INVALID_TRANSITION`,
/// `current` on `CONFLICT`).
pub(super) fn tool_error(e: ApiError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(tool_error_text(&e))])
}

/// The text of [`tool_error`].
pub(super) fn tool_error_text(e: &ApiError) -> String {
    let mut text = format!("{}: {}", e.code, e.message);
    if e.details != json!({}) {
        text.push_str("\ndetails: ");
        text.push_str(&e.details.to_string());
    }
    text
}

/// A successful tool result: one compact markdown/text content block
/// (REQ-1.6).
pub(super) fn tool_text(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}
