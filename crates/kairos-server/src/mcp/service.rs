//! The MCP service type behind `/mcp` (KAIROS-T-0026): per-request caller
//! resolution and the S-0006 error contract; the tool implementations live
//! in [`super::tools`].

use std::sync::{Arc, Mutex, OnceLock};

use axum::http::request::Parts;
use diesel::pg::PgConnection;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool_handler};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The instructions of the server (`initialize`). KAIROS-T-0692 checks
/// that each tool name in it is served.
pub(super) const INSTRUCTIONS: &str = "Kairos work-item tools. Start with `whoami` (identity, teams, board \
         capabilities) and `my_boards` (boards + columns). Items are identified \
         by short code (e.g. ACME-T-0012) everywhere. Use `search` to find \
         items, `get_item` for full content, and the write tools \
         (create/update/edit/transition/link/set_metadata/delete) to work \
         them; writes require board capabilities and edits use optimistic \
         versioning. Repositories: `list_repositories` and `get_repository` \
         read the directory, `add_repository` and `update_repository` write \
         it, and `set_repository` links a task to one. `/mcp` lists the work \
         profile: items, repositories, members, claims, notifications, board \
         flow and dispatch rules to read. `/mcp?profile=admin` lists the \
         administration tools: teams, boards, grants, metadata, dispatch rule \
         writes, team page writes, org read tokens, purge. `/mcp?profile=all` \
         lists each tool. The server runs a call to a tool that the profile does \
         not list. A client such as Claude Code calls only the listed tools. With \
         the Claude Code plugin, the user gets the admin tools with one command: \
         `claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile \
         admin`, then a new session; the CLI is the fallback (`kairos teams \
         create`). Do not add an MCP server yourself: tell the user. The plugin \
         skill `/kairos:onboard-team` sets up a team; only the user can start it.";

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

/// The router of the 79 tools, built once per process (KAIROS-T-0611): the
/// stateless `/mcp` makes a service per request, and building the router
/// each time was 70 boxed handlers and a map per call.
static TOOL_ROUTER: OnceLock<ToolRouter<KairosMcp>> = OnceLock::new();

/// The full tool list, built once per process from the router; each
/// `list_tools` filters it by the profile of the request (KAIROS-T-0611).
static ALL_TOOLS: OnceLock<Vec<rmcp::model::Tool>> = OnceLock::new();

/// The notification ride-along of the current tool call (KAIROS-T-0612).
///
/// The ride-along ran as a second `blocking.run` after the tool: a second
/// `spawn_blocking`, checkout, `SET` and reset on each answer. Now
/// [`KairosMcp::call_tool`] opens this slot in a task-local (the pattern of
/// the agent key, `crate::blocking`), and `run_tool` fills it on the
/// connection of the tool, in the same checkout, after the closure of the
/// tool succeeds ([`RideAlong::fill`]). A tool that does not go through
/// `run_tool` leaves the slot empty, and `call_tool` reads the
/// notifications in a checkout of their own, as before.
#[derive(Clone)]
pub(super) struct RideAlong {
    user: Uuid,
    agent_key: Option<Uuid>,
    /// `None` until filled; then the block, or `None` for no block.
    block: Arc<Mutex<Option<Option<String>>>>,
}

tokio::task_local! {
    /// The ride-along slot of the tool call in scope, or `None` when the
    /// answer gets no ride-along (`get_events`).
    static RIDE_ALONG: Option<RideAlong>;
}

impl RideAlong {
    fn new(user: Uuid, agent_key: Option<Uuid>) -> Self {
        Self {
            user,
            agent_key,
            block: Arc::new(Mutex::new(None)),
        }
    }

    /// The slot of the tool call in scope. Read in the task of the request,
    /// before the closure moves to the blocking thread.
    pub(super) fn current() -> Option<RideAlong> {
        RIDE_ALONG.try_with(Clone::clone).ok().flatten()
    }

    /// Read the notifications on `conn` into the slot, once per tool call.
    /// A failure is logged and gives no block: the answer of the tool
    /// stands, and the notifications come with the next answer.
    pub(super) fn fill(&self, conn: &mut PgConnection) {
        let mut slot = self.block.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return;
        }
        *slot = Some(
            super::tools::ride_along(conn, self.user, self.agent_key).unwrap_or_else(|e| {
                tracing::warn!(error = %e.message, "notifications ride-along failed");
                None
            }),
        );
    }

    /// The filled slot, or `None` when no `run_tool` filled it.
    fn take(&self) -> Option<Option<String>> {
        self.block.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

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
    // KAIROS-T-0454: the profile of the URL chooses the list; `call_tool`
    // runs each tool under each profile.
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
        let request_auth = context
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<AuthContext>());
        let agent_key = request_auth.and_then(|auth| auth.agent_key);
        // KAIROS-T-0536: the scope of the key goes with it.
        let key_scope =
            request_auth.and_then(|auth| auth.key_scope.clone().map(|scope| (auth.user_id, scope)));
        // KAIROS-T-0382: the unread notifications of the caller ride along
        // on each answer, but not on `get_events`, which lists them.
        let ride_along = (request.name != "get_events")
            .then(|| Self::caller(&context).ok())
            .flatten();
        // KAIROS-T-0612: `run_tool` reads them in the checkout of the tool.
        let slot = ride_along
            .as_ref()
            .map(|(auth, _)| RideAlong::new(auth.user_id, auth.agent_key));
        // KAIROS-T-0694: the tool and the caller of a refusal, for its count.
        let tool_name = request.name.to_string();
        let refusal_caller = Self::caller(&context)
            .ok()
            .map(|(auth, tenant)| (auth.user_id, auth.agent_key, tenant.slug));
        let call = ToolCallContext::new(self, request, context);
        // KAIROS-T-0359: a write to a task that a different person has the
        // claim on gets a warning line (`crate::claims`).
        let (result, warnings) = crate::claims::collect(crate::blocking::with_agent_key(
            agent_key,
            key_scope,
            RIDE_ALONG.scope(slot.clone(), self.tool_router.call(call)),
        ))
        .await;
        let outcome = match result {
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
                    && let (Some((_, tenant)), Some(slot)) = (ride_along, slot)
                {
                    // KAIROS-T-0550: the unread ones of the caller: the agent
                    // key keeps its own read state, apart from the person.
                    let block = match slot.take() {
                        Some(block) => block,
                        // No `run_tool` in this tool: a checkout of its own.
                        None => {
                            let filler = slot.clone();
                            self.state
                                .blocking
                                .run(&tenant.slug, move |conn| {
                                    filler.fill(conn);
                                    Ok(())
                                })
                                .await
                                .unwrap_or_else(|e| {
                                    tracing::warn!(
                                        error = %e.message,
                                        "notifications ride-along failed"
                                    )
                                });
                            slot.take().flatten()
                        }
                    };
                    // KAIROS-T-0549: a content block of its own, after the
                    // answer. Appended to the answer's text, the block
                    // landed after the raw content of `get_item`, and an
                    // agent that copied "the content" into `update_item`
                    // wrote the inbox into the item.
                    if let Some(block) = block {
                        result.content.push(ContentBlock::text(block));
                    }
                }
                Ok(result)
            }
            Err(e) => super::arguments::tool_error_of(e),
        };
        if let (Ok(result), Some(caller)) = (&outcome, refusal_caller)
            && result.is_error == Some(true)
        {
            self.record_refusal(&tool_name, result, caller).await;
        }
        outcome
    }

    fn get_info(&self) -> ServerInfo {
        // REQ-1.7: `initialize` reports the server version (tool schemas
        // are versioned with the server).
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("kairos", env!("CARGO_PKG_VERSION")).with_title("Kairos"),
            )
            .with_instructions(INSTRUCTIONS)
    }
}

impl KairosMcp {
    /// KAIROS-T-0694 (KAIROS-A-0068): write the row of a refused call: the
    /// tool, the code and the `field` of the text of the tool error, and
    /// the kind of the caller. A failed write is logged; the answer stands.
    async fn record_refusal(
        &self,
        tool: &str,
        result: &CallToolResult,
        (user_id, agent_key, slug): (Uuid, Option<Uuid>, String),
    ) {
        let text = match result.content.first() {
            Some(ContentBlock::Text(text)) => text.text.clone(),
            _ => return,
        };
        let Some((code, field)) = refusal_parts(&text) else {
            return;
        };
        let tool = tool.to_string();
        let written = self
            .state
            .blocking
            .run(&slug, move |conn| {
                kairos_db::tool_refusals::record(
                    conn,
                    &tool,
                    &code,
                    field.as_deref(),
                    user_id,
                    agent_key,
                )
                .map_err(ApiError::internal)
            })
            .await;
        if let Err(e) = written {
            tracing::warn!(error = %e.message, "the count of a refusal was not written");
        }
    }
}

/// The code and the `field` of the text of a tool error (`CODE: message`,
/// then an optional `details: {...}` line). `None` for a text that does
/// not start with a code.
pub(super) fn refusal_parts(text: &str) -> Option<(String, Option<String>)> {
    let (code, _) = text.split_once(": ")?;
    if code.is_empty() || !code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
        return None;
    }
    let field = text
        .rsplit_once("\ndetails: ")
        .and_then(|(_, details)| serde_json::from_str::<Value>(details.lines().next()?).ok())
        .and_then(|details| details.get("field")?.as_str().map(str::to_string));
    Some((code.to_string(), field))
}

/// The instructions of `initialize`. KAIROS-T-0691: a unit test in
/// `profiles` checks that each tool that they name is in the work profile.
pub(super) const INSTRUCTIONS: &str = "Kairos work-item tools. Start with `whoami` (identity, teams, board \
     capabilities) and `my_boards` (boards + columns). Items are identified \
     by short code (e.g. ACME-T-0012) everywhere. Use `search` to find \
     items, `get_item` for full content, and the write tools \
     (create/update/edit/transition/link/set_metadata/delete) to work \
     them; writes require board capabilities and edits use optimistic \
     versioning. Repositories: `list_repositories` and `get_repository` \
     read the directory, `add_repository` and `update_repository` write \
     it, and `set_repository` links a task to one. `/mcp` lists the work \
     profile: items, repositories, members, claims, notifications, board \
     flow and dispatch rules to read. `/mcp?profile=admin` lists the \
     administration tools: teams, boards, grants, metadata, dispatch rule \
     writes, team page writes, org read tokens, purge. `/mcp?profile=all` \
     lists each tool. The server runs a call to a tool that the profile does \
     not list. A client such as Claude Code calls only the listed tools. With \
     the Claude Code plugin, the user gets the admin tools with one command: \
     `claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile \
     admin`, then a new session; the CLI is the fallback (`kairos teams \
     create`). Do not add an MCP server yourself: tell the user. The plugin \
     skill `/kairos:onboard-team` sets up a team; only the user can start it.";

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

#[cfg(test)]
mod t0694_tests {
    use super::refusal_parts;

    #[test]
    fn the_code_and_the_field_of_a_tool_error() {
        assert_eq!(
            refusal_parts(
                "VALIDATION: The call has the argument \"colour\".\n\
                 details: {\"allowed\":[\"title\"],\"field\":\"colour\"}"
            ),
            Some(("VALIDATION".into(), Some("colour".into())))
        );
        assert_eq!(
            refusal_parts("NOT_FOUND: No item has the short code ACME-T-0009."),
            Some(("NOT_FOUND".into(), None))
        );
        assert_eq!(
            refusal_parts("FORBIDDEN: No.\ndetails: {\"capability\":\"manage_tasks\"}"),
            Some(("FORBIDDEN".into(), None))
        );
        assert_eq!(refusal_parts("The server stopped: no."), None);
        assert_eq!(refusal_parts("no code"), None);
    }
}
