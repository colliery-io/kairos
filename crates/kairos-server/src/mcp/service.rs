//! The MCP service type behind `/mcp` (KAIROS-T-0026): per-request caller
//! resolution and the S-0006 error contract; the tool implementations live
//! in [`super::tools`].

use std::sync::OnceLock;

use axum::http::request::Parts;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
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

/// The router of the 71 tools, built once per process (KAIROS-T-0611): the
/// stateless `/mcp` makes a service per request, and building the router
/// each time was 70 boxed handlers and a map per call.
static TOOL_ROUTER: OnceLock<ToolRouter<KairosMcp>> = OnceLock::new();

/// The full tool list, built once per process from the router; each
/// `list_tools` filters it by the profile of the request (KAIROS-T-0611).
static ALL_TOOLS: OnceLock<Vec<rmcp::model::Tool>> = OnceLock::new();

impl KairosMcp {
    /// Build a service instance over the shared state.
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: TOOL_ROUTER.get_or_init(Self::tool_router).clone(),
        }
    }

    /// Every tool, in the order of `list_all`, once per process.
    fn all_tools(&self) -> &'static [rmcp::model::Tool] {
        ALL_TOOLS.get_or_init(|| self.tool_router.list_all())
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
    // KAIROS-T-0454: the profile of the URL chooses the list; each tool can
    // still be called.
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let profile = super::profiles::Profile::of(context.extensions.get::<Parts>())
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        Ok(ListToolsResult {
            tools: profile.filter(self.all_tools().to_vec()),
            meta: None,
            next_cursor: None,
        })
    }

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
        // KAIROS-T-0382: the unread notifications of the caller ride along
        // on each answer, but not on `get_events`, which lists them.
        let ride_along = (request.name != "get_events")
            .then(|| Self::caller(&context).ok())
            .flatten();
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
                if result.is_error != Some(true)
                    && let Some((auth, tenant)) = ride_along
                {
                    // KAIROS-T-0550: the unread ones of the caller: the agent
                    // key keeps its own read state, apart from the person.
                    let (user, key) = (auth.user_id, auth.agent_key);
                    let block = self
                        .state
                        .blocking
                        .run(&tenant.slug, move |conn| {
                            super::tools::ride_along(conn, user, key)
                        })
                        .await;
                    match block {
                        // KAIROS-T-0549: a content block of its own, after the
                        // answer. Appended to the answer's text, the block
                        // landed after the raw content of `get_item`, and an
                        // agent that copied "the content" into `update_item`
                        // wrote the inbox into the item.
                        Ok(Some(block)) => result.content.push(ContentBlock::text(block)),
                        Ok(None) => {}
                        // The answer of the tool stands: the notifications
                        // come with the next answer.
                        Err(e) => {
                            tracing::warn!(error = %e.message, "notifications ride-along failed")
                        }
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
                 it, and `set_repository` links a task to one. `/mcp` lists the work \
                 profile; `/mcp?profile=admin` the administration tools (teams, boards, \
                 grants, metadata, dispatch, team page writes, org read tokens, purge, \
                 board flow); `/mcp?profile=all` each tool. Every tool is callable under \
                 every profile.",
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
