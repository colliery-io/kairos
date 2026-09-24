//! The S-0006 tool surface (KAIROS-T-0026): 18 tools — the 14 frozen by
//! S-0006, plus the two repository tools (KAIROS-T-0107), `move_item`
//! (KAIROS-I-0012) and `restore_item` (KAIROS-A-0020) — each a
//! thin wrapper over the same `kairos-core`/`kairos-db` services the REST
//! handlers call — through [`crate::blocking::BlockingTenantPool`], never
//! HTTP (A-0011). Contracts:
//!
//! - **REQ-1.1**: every tool runs as the authenticated user under full
//!   ABAC ([`require_capability`] / [`require_org_admin`], identical to
//!   the REST handlers); errors surface as tool errors carrying the same
//!   stable codes as the S-0005 envelope ([`tool_error`]).
//! - **REQ-1.2**: no tenant parameter exists — the tenant is the one the
//!   middleware resolved from the connection host.
//! - **REQ-1.3**: short codes identify items in every input and output;
//!   a `board` may be a slug or UUID.
//! - **REQ-1.4/1.5**: invalid transitions enumerate allowed target
//!   columns; version conflicts return the current version + content.
//! - **REQ-1.6**: listings are compact (short code + title + key fields);
//!   full content ships only via `get_item` (and `get_history` snapshots).
//! - **NFR-1.3**: audit rows come from the services themselves (the same
//!   `activity_log` writes as the API path).

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, NaiveDate, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{
    Nullable as SqlNullable, Text as SqlText, Timestamptz as SqlTimestamptz, Uuid as SqlUuid,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use kairos_core::search as core_search;
use kairos_core::short_code::ItemType;
use kairos_db::models::boards::{Board, BoardColumn};
use kairos_db::models::enums::{
    BoardLevel, BucketType, Complexity, DocumentLifecycle, OrgRole, RelationshipType, TaskType,
    TeamType, WorkClass,
};
use kairos_db::models::items::{Adr, Document, Initiative, Strategy, Task};
use kairos_db::models::templates::Template;
use kairos_db::search::{SearchError, SearchResults};
use kairos_db::{GraphError, abac, boards, graph, items, repositories, search};

use crate::api::meta::{manage_capability, require_edge_capability, validate_metadata_value};
use crate::api::{
    Liveness, map_abac_error, map_board_error, map_graph_error, map_item_error, parse_enum,
    require_capability, resolve_short_code,
};
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

use super::service::{KairosMcp, tool_error, tool_text};

// ---------------------------------------------------------------------------
// Tool inputs (frozen names/shapes per S-0006's inventory tables)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListRepositoriesParams {
    /// Only this team's repositories (slug or UUID).
    pub team: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetRepositoryParams {
    /// The repository, by slug (e.g. "payments-api") or UUID.
    pub repository: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct MyBoardsParams {
    /// Restrict to one board level: strategy | initiative | delivery | adr.
    pub level: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct BoardItemsParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// Restrict to one column, by name (e.g. "In Progress") or UUID.
    pub column: Option<String>,
    /// Restrict the TASKS to those issued against this repository (slug
    /// or UUID). Your repo's queue on a multi-repo team board.
    pub repository: Option<String>,
    /// Include archived (put-away) cards, each marked `[archived]`.
    /// Default false — the board as it stands. Ask for this when the
    /// question is historical ("what was in Done last quarter?"), never
    /// to decide what to work on next.
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetItemParams {
    /// The item's short code (e.g. "ACME-T-0012").
    pub short_code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetHistoryParams {
    /// The item's short code.
    pub short_code: String,
    /// Max versions to list (default 20, newest first).
    pub limit: Option<i64>,
    /// Return this version's full title + content instead of the list.
    pub version: Option<i32>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchParams {
    /// Full-text query (websearch syntax: quoted phrases, OR, -negation).
    pub q: Option<String>,
    /// Structured filter; fields AND together.
    pub filter: Option<SearchFilterParams>,
    /// Graph traversal from a starting item.
    pub traverse: Option<SearchTraverseParams>,
    /// Sort of the combined results. Default: relevance desc when `q` is
    /// given, created_at desc otherwise (KAIROS-T-0186) — so a text search is
    /// ranked without asking.
    pub sort: Option<SearchSortParams>,
    /// Page size (default 25, max 100).
    pub limit: Option<i64>,
    /// Offset into the combined result set.
    pub offset: Option<i64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchFilterParams {
    /// Restrict to entity types: strategy | initiative | task | document | adr.
    pub entity_type: Option<Vec<String>>,
    /// Restrict to items on this board (UUID).
    pub board_id: Option<String>,
    /// Restrict to items in this column (UUID).
    pub column_id: Option<String>,
    /// Restrict to tasks of this team (UUID).
    pub team_id: Option<String>,
    /// Restrict to tasks issued against this repository (slug or UUID).
    pub repository: Option<String>,
    /// Restrict to task types: task | bug | tech_debt | support.
    pub task_type: Option<Vec<String>>,
    /// Restrict to Planned/Support lanes: planned | support
    /// (KAIROS-T-0077).
    pub work_class: Option<Vec<String>>,
    /// Restrict to (non-)bucket initiatives.
    pub is_bucket: Option<bool>,
    /// Metadata conditions keyed by definition slug; trailing-* globs allowed.
    pub metadata: Option<BTreeMap<String, String>>,
    /// Only items created strictly after this RFC 3339 instant.
    pub created_after: Option<String>,
    /// Only items created strictly before this RFC 3339 instant.
    pub created_before: Option<String>,
    /// Include archived (soft-deleted) items (default false). Composes with
    /// everything, `q` and `traverse` included; archived hits come back
    /// marked `[archived]`.
    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchTraverseParams {
    /// The starting item's short code (e.g. "ACME-S-0001").
    pub from: String,
    /// Relationship types to follow: parent | supports | informs | supersedes | blocks.
    pub relationships: Vec<String>,
    /// Edge direction: outbound | inbound | both.
    pub direction: String,
    /// Traversal depth, 1..=10.
    pub depth: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchSortParams {
    /// created_at | updated_at | title | relevance (relevance requires `q`).
    pub field: String,
    /// asc | desc.
    pub order: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateItemParams {
    /// What to create: strategy | initiative | task | document | adr.
    pub item_type: String,
    /// The item's title.
    pub title: String,
    /// Target board, by slug or UUID. Optional when the tenant has exactly
    /// one board of the matching level (strategy/initiative/adr boards,
    /// or the single delivery board for tasks). Ignored for documents.
    pub board: Option<String>,
    /// Parent item's short code: creates the `parent` edge (or the
    /// `supports` edge for documents, where a parent is REQUIRED).
    pub parent: Option<String>,
    /// Template (id or name) — documents only (KAIROS-A-0003).
    pub template: Option<String>,
    /// Initial markdown content (defaults to empty / the template's).
    pub content: Option<String>,
    /// Tasks only: task | bug | tech_debt | support (default task).
    pub task_type: Option<String>,
    /// Tasks only: the repository to issue the task against (slug or
    /// UUID, KAIROS-A-0019). ROUTES the task to the repository's owning
    /// team's delivery board, so `board` becomes optional (and must agree
    /// when given). Any tenant member may create a task against ANOTHER
    /// team's repository: it lands in that team's Backlog behind their
    /// triage gate (the computed `file_backlog` capability).
    pub repository: Option<String>,
    /// Tasks only: Planned/Support lane planned | support (KAIROS-T-0077;
    /// defaults to support for support-type tasks, else planned).
    pub work_class: Option<String>,
    /// Strategies only: the strategy's hypothesis.
    pub hypothesis: Option<String>,
    /// Initiatives only: t-shirt complexity xs | s | m | l | xl.
    pub complexity: Option<String>,
    /// ADRs only: who makes/made the decision.
    pub decision_maker: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UpdateItemParams {
    /// The item's short code.
    pub short_code: String,
    /// New title (omit to keep the current one).
    pub title: Option<String>,
    /// The FULL replacement markdown content.
    pub content: String,
    /// The version this edit is based on (from get_item). A stale version
    /// returns CONFLICT with the current version + content to reconcile.
    pub version: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct EditItemParams {
    /// The item's short code.
    pub short_code: String,
    /// Exact text to find in the current content.
    pub search: String,
    /// Replacement text.
    pub replace: String,
    /// Replace every occurrence (default false: the single occurrence must
    /// be unique, otherwise the edit is rejected as ambiguous).
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct TransitionItemParams {
    /// The item's short code.
    pub short_code: String,
    /// Target column, by name (e.g. "In Progress") or UUID, on the item's
    /// own board.
    pub to_column: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct MoveItemParams {
    /// The task's short code (e.g. "ACME-T-0012").
    pub short_code: String,
    /// The delivery board to move it to, by slug (e.g. "web-delivery") or
    /// UUID. It lands in that board's entry column.
    pub to_board: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct LinkItemsParams {
    /// Source item's short code (edge direction: source -> target).
    pub source: String,
    /// Target item's short code.
    pub target: String,
    /// parent | supports | informs | supersedes | blocks.
    pub relationship: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UnlinkItemsParams {
    /// Source item's short code.
    pub source: String,
    /// Target item's short code.
    pub target: String,
    /// parent | supports | informs | supersedes | blocks.
    pub relationship: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SetMetadataParams {
    /// The item's short code.
    pub short_code: String,
    /// Values keyed by metadata-definition slug; null clears a value.
    /// Values are validated against the definition (enum membership,
    /// YYYY-MM-DD dates).
    pub values: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct RestoreItemParams {
    /// The archived item's short code.
    pub short_code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct DeleteItemParams {
    /// The item's short code.
    pub short_code: String,
    /// Must be true: deletion soft-deletes the item AND cascades to its
    /// descendants via parent edges. The response lists the cascade.
    pub confirm: bool,
}

// ---------------------------------------------------------------------------
// The tools (18; the count is asserted in tests/mcp.rs)
// ---------------------------------------------------------------------------

#[tool_router(vis = "pub(super)")]
impl KairosMcp {
    /// Run one closure on a tenant-pinned sync connection (the T-0018
    /// blocking bridge); service errors become S-0006 tool errors.
    async fn run_tool<F>(&self, tenant: &TenantContext, f: F) -> Result<CallToolResult, ErrorData>
    where
        F: FnOnce(&mut PgConnection) -> Result<String, ApiError> + Send + 'static,
    {
        match self.state.blocking.run(&tenant.slug, f).await {
            Ok(text) => Ok(tool_text(text)),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Who am I in this Kairos organization: identity, org role, teams, my teams' repositories, the boards where I hold write capabilities, and the implicit capabilities every member has. Call this first to establish working context."
    )]
    pub async fn whoami(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let is_admin = tenant.role == OrgRole::Admin;
        let (org_slug, role) = (tenant.slug.clone(), tenant.role.as_str());
        let (display_name, email, user_id) = (auth.display_name, auth.email, auth.user_id);
        self.run_tool(&tenant, move |conn| {
            use kairos_db::schema::{
                board_member_capabilities as caps, boards, team_members, teams,
            };

            let member_teams: Vec<(Uuid, String, String, TeamType)> = team_members::table
                .inner_join(teams::table)
                .filter(team_members::user_id.eq(user_id))
                .filter(teams::deleted_at.is_null())
                .order(teams::slug.asc())
                .select((teams::id, teams::slug, teams::name, teams::team_type))
                .load(conn)
                .map_err(ApiError::internal)?;
            let my_team_ids: Vec<Uuid> = member_teams.iter().map(|(id, ..)| *id).collect();

            let grants: Vec<(String, String, String)> = caps::table
                .inner_join(boards::table)
                .filter(caps::user_id.eq(user_id))
                .filter(boards::deleted_at.is_null())
                .order((boards::slug.asc(), caps::capability.asc()))
                .select((boards::slug, boards::name, caps::capability))
                .load(conn)
                .map_err(ApiError::internal)?;

            let mut out = format!("# {display_name} <{email}>\n");
            out.push_str(&format!("- organization: {org_slug} (role: {role})\n"));
            out.push_str("\n## Teams\n");
            if member_teams.is_empty() {
                out.push_str("(none)\n");
            }
            for (_, slug, name, team_type) in member_teams {
                out.push_str(&format!("- {slug} — {name} ({team_type})\n"));
            }
            // KAIROS-T-0107: the repositories my teams own — where my
            // tickets are issued and executed (A-0019).
            {
                use kairos_db::schema::repositories as repos;
                let mine: Vec<(String, String, String)> = repos::table
                    .inner_join(teams::table)
                    .filter(repos::team_id.eq_any(&my_team_ids))
                    .filter(repos::deleted_at.is_null())
                    .order(repos::slug.asc())
                    .select((repos::slug, repos::repo_full_name, teams::slug))
                    .load(conn)
                    .map_err(ApiError::internal)?;
                out.push_str("\n## My teams' repositories\n");
                if mine.is_empty() {
                    out.push_str("(none — see list_repositories for the whole directory)\n");
                }
                for (slug, full_name, team) in mine {
                    out.push_str(&format!("- {slug} — {full_name} (owner: {team})\n"));
                }
            }
            out.push_str("\n## Board capabilities\n");
            if is_admin {
                out.push_str("- org admin: implicit full access on every board\n");
            }
            let mut by_board: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
            for (slug, name, capability) in grants {
                by_board.entry((slug, name)).or_default().push(capability);
            }
            if by_board.is_empty() && !is_admin {
                out.push_str("(no capability grants — reads are open, writes need grants)\n");
            }
            for ((slug, name), capabilities) in by_board {
                out.push_str(&format!("- {slug} ({name}): {}\n", capabilities.join(", ")));
            }
            // KAIROS-T-0105: computed capabilities every member holds.
            out.push_str(&format!(
                "- implicit (every member, every delivery board): {}\n  \
                 file_backlog = create a task against another team's repository; \
                 it lands in their Backlog for triage.\n",
                kairos_core::abac::COMPUTED_CAPABILITIES.join(", ")
            ));
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Directory of repositories in this organization (KAIROS-A-0019): for each, its slug, forge and full name, the ONE owning team, the delivery board tasks filed against it land on, open task count, and whether webhooks are connected. Call before filing work against a codebase you are not checked out in, or to find who owns a repo. Optional `team` (slug or UUID) narrows to one team's repositories."
    )]
    pub async fn list_repositories(
        &self,
        Parameters(params): Parameters<ListRepositoriesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            let team_id = params
                .team
                .as_deref()
                .map(|reference| team_by_ref(conn, reference).map(|t| t.id))
                .transpose()?;
            let rows = repositories::list(conn, team_id)
                .map_err(crate::api::org::repositories::map_error)?;
            let rendered = crate::api::org::repositories::render(conn, rows)?;
            let mut out = String::from("# Repositories\n");
            if rendered.is_empty() {
                out.push_str(
                    "(none registered — an org admin or a team member registers one with \
                     POST /api/repositories or `kairos repos create`)\n",
                );
            }
            for repo in rendered {
                out.push_str(&format!(
                    "- {} — {} {} · owner: {} · board: {} · open tasks: {}{}\n",
                    repo.slug,
                    repo.forge,
                    repo.repo_full_name,
                    repo.team.slug,
                    repo.delivery_board_id
                        .as_deref()
                        .unwrap_or("(no delivery board)"),
                    repo.open_tasks,
                    if repo.has_webhook {
                        " · webhooks connected"
                    } else {
                        ""
                    }
                ));
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "One repository in full: owner team, delivery board, default branch, the team's `description` of how to work in it (READ THIS before working in or filing against an unfamiliar repo), and its in-flight branches and pull requests with the work items they belong to. `repository` is a slug or UUID."
    )]
    pub async fn get_repository(
        &self,
        Parameters(params): Parameters<GetRepositoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            let repo = repositories::resolve(conn, &params.repository)
                .map_err(crate::api::org::repositories::map_error)?;
            let repo_id = repo.id;
            let stale = repositories::stale_tasks(conn, &repo)
                .map_err(crate::api::org::repositories::map_error)?;
            let rendered = crate::api::org::repositories::render(conn, vec![repo])?.remove(0);
            // Slugs everywhere (KAIROS-T-0123): the delivery board is what
            // the skills pass to `board_items`, so print it the way they
            // will use it.
            let delivery_board = match rendered.delivery_board_id.as_deref() {
                Some(id) => board_by_ref(conn, id)
                    .map(|b| format!("{} ({})", b.slug, b.name))
                    .unwrap_or_else(|_| id.to_string()),
                None => "(none)".to_string(),
            };
            let in_flight =
                graph::repository_link_rollup(conn, repo_id, &["open", "draft"], 50)
                    .map_err(ApiError::internal)?;
            let mut out = format!(
                "# Repository {} — {} {}\n- url: {}\n- default branch: {}\n- owner team: {} ({})\n- delivery board: {}\n- open tasks: {}\n- webhooks: {}\n",
                rendered.slug,
                rendered.forge,
                rendered.repo_full_name,
                rendered.repo_url,
                rendered.default_branch,
                rendered.team.slug,
                rendered.team.name,
                delivery_board,
                rendered.open_tasks,
                if rendered.has_webhook { "connected" } else { "not connected" },
            );
            if stale > 0 {
                out.push_str(&format!(
                    "- STALE: {stale} task(s) bound here sit on another team's board (a re-home left them); rebind or move them\n"
                ));
            }
            out.push_str("\n## How to work here\n");
            if rendered.description.trim().is_empty() {
                out.push_str("(no description yet)\n");
            } else {
                out.push_str(&rendered.description);
                out.push('\n');
            }
            out.push_str("\n## In flight\n");
            if in_flight.is_empty() {
                out.push_str("(nothing open)\n");
            }
            for link in in_flight {
                out.push_str(&format!(
                    "- {} {} [{}] {} — {} ({} {})\n",
                    link.kind,
                    link.external_id,
                    link.state,
                    link.title,
                    link.item_short_code,
                    link.item_title,
                    link.url
                ));
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "List boards in this organization grouped by level (strategy/initiative/delivery/adr), with column names and per-column item counts for my delivery boards. Optional `level` filter."
    )]
    pub async fn my_boards(
        &self,
        Parameters(params): Parameters<MyBoardsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user_id = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            use kairos_db::schema::{board_member_capabilities as caps, boards, team_members};

            let level_filter = params
                .level
                .as_deref()
                .map(|v| parse_enum(v, "level", BoardLevel::ALL))
                .transpose()?;

            let mut query = boards::table
                .filter(boards::deleted_at.is_null())
                .order((boards::board_level.asc(), boards::slug.asc()))
                .select(Board::as_select())
                .into_boxed();
            if let Some(level) = level_filter {
                query = query.filter(boards::board_level.eq(level));
            }
            let all_boards: Vec<Board> = query.load(conn).map_err(ApiError::internal)?;

            // "The user's delivery boards": their teams' boards plus any
            // board they hold a capability grant on.
            let my_teams: Vec<Uuid> = team_members::table
                .filter(team_members::user_id.eq(user_id))
                .select(team_members::team_id)
                .load(conn)
                .map_err(ApiError::internal)?;
            let my_grant_boards: Vec<Uuid> = caps::table
                .filter(caps::user_id.eq(user_id))
                .select(caps::board_id)
                .distinct()
                .load(conn)
                .map_err(ApiError::internal)?;

            let mut out = String::from("# Boards\n");
            let mut current_level: Option<BoardLevel> = None;
            if all_boards.is_empty() {
                out.push_str("(none)\n");
            }
            for board in all_boards {
                if current_level != Some(board.board_level) {
                    out.push_str(&format!("\n## {}\n", board.board_level));
                    current_level = Some(board.board_level);
                }
                out.push_str(&format!("- {} — {}", board.slug, board.name));
                let mine = board.board_level == BoardLevel::Delivery
                    && (board.team_id.is_some_and(|t| my_teams.contains(&t))
                        || my_grant_boards.contains(&board.id));
                if mine {
                    out.push_str(" [mine]\n");
                    let columns = board_columns(conn, board.id)?;
                    let counts = column_item_counts(conn, board.id)?;
                    let rendered: Vec<String> = columns
                        .iter()
                        .map(|c| {
                            format!("{} ({})", c.name, counts.get(&c.id).copied().unwrap_or(0))
                        })
                        .collect();
                    out.push_str(&format!("  columns: {}\n", rendered.join(" | ")));
                } else {
                    out.push('\n');
                }
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "List the items on a board grouped by column: short code, type, and title. `board` is a slug or UUID; optional `column` (name or UUID) restricts to one column; optional `repository` (slug or UUID) narrows the tasks to one repository — pass the repository you are checked out in to see your queue. Live cards only unless `include_deleted` is true, which adds the archived ones back in the column they were put away in, each marked [archived]."
    )]
    pub async fn board_items(
        &self,
        Parameters(params): Parameters<BoardItemsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            let board = board_by_ref(conn, &params.board)?;
            let liveness = if params.include_deleted.unwrap_or(false) {
                Liveness::IncludeArchived
            } else {
                Liveness::LiveOnly
            };
            // Removed columns come back only with the archived cards that
            // still point at them (KAIROS-T-0161) — otherwise those cards
            // would be fetched and then silently never rendered.
            let mut columns = board_columns_including_removed(conn, board.id, liveness)?;
            if let Some(wanted) = &params.column {
                let target = resolve_column(&columns, wanted)?;
                columns.retain(|c| c.id == target);
            }
            let repository = params
                .repository
                .as_deref()
                .map(|reference| {
                    repositories::resolve(conn, reference)
                        .map(|r| r.id)
                        .map_err(crate::api::tasks::map_repository_error)
                })
                .transpose()?;
            let items = board_item_rows(conn, board.id, repository, liveness)?;
            let repo_ids: Vec<Uuid> = items.iter().filter_map(|i| i.repository_id).collect();
            let repo_slugs = repo_slug_map(conn, &repo_ids)?;

            let mut out = format!(
                "# Board {} — {} ({})\n",
                board.slug, board.name, board.board_level
            );
            for column in &columns {
                let in_column: Vec<&BoardItemRow> =
                    items.iter().filter(|i| i.column_id == column.id).collect();
                out.push_str(&format!("\n## {} ({})\n", column.name, in_column.len()));
                for item in in_column {
                    // The marker is not decoration: an agent that cannot
                    // tell put-away work from live work will pick one up
                    // and start on it (KAIROS-A-0020 rule 2).
                    out.push_str(&format!(
                        "- {} [{}] {}{}{}\n",
                        item.short_code,
                        item.kind,
                        item.title,
                        item.repository_id
                            .and_then(|id| repo_slugs.get(&id))
                            .map(|slug| format!(" [repo:{slug}]"))
                            .unwrap_or_default(),
                        if item.archived { " [archived]" } else { "" }
                    ));
                }
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Full detail of one item by short code: type, board/column, version, full markdown content, metadata values, and relationships (parent chain, children, blockers, supporting docs)."
    )]
    pub async fn get_item(
        &self,
        Parameters(params): Parameters<GetItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::IncludeArchived)?;

            let mut out = format!("# {} — {}\n", item.short_code, item.title);
            // Before anything else: an agent that cannot tell retired work
            // from live work will try to act on it and be refused by every
            // write path, with no idea why (KAIROS-A-0020).
            if let Some(archived_at) = item.archived_at {
                out.push_str(&format!(
                    "\n> **ARCHIVED** {} — this work has been put away. It is \
                     readable for reference and audit, but it is not on a \
                     board and every write to it will be refused. Restore it \
                     first if it needs to move.\n\n",
                    archived_at.format("%Y-%m-%dT%H:%M:%SZ")
                ));
            }
            out.push_str(&format!("- type: {}", item.item_type));
            if let Some(task_type) = item.task_type {
                out.push_str(&format!(" ({task_type})"));
            }
            if let Some(work_class) = item.work_class {
                out.push_str(&format!(" · lane: {work_class}"));
            }
            if item.item_type == ItemType::Task {
                out.push_str(&format!(
                    " · repository: {}",
                    repo_label(conn, item.repository_id)?
                ));
            }
            if let Some(lifecycle) = item.lifecycle {
                out.push_str(&format!(" · lifecycle: {lifecycle}"));
            }
            out.push('\n');
            if let (Some(board_id), Some(column_id)) = (item.board_id, item.column_id) {
                let board = board_by_id(conn, board_id)?;
                // `column_label`, not `board_columns`: an archived card may
                // sit in a column that has since been removed, and the
                // column it was put away in is exactly what this line is
                // for (KAIROS-T-0161).
                let column = column_label(conn, column_id)?;
                out.push_str(&format!("- board: {} / column: {column}\n", board.slug));
            }
            out.push_str(&format!("- version: {}\n", item.version));
            out.push_str(&format!(
                "- updated: {}\n",
                item.updated_at.format("%Y-%m-%dT%H:%M:%SZ")
            ));
            if let Some(hypothesis) = &item.hypothesis {
                out.push_str(&format!("- hypothesis: {hypothesis}\n"));
            }
            if let Some(complexity) = item.complexity {
                out.push_str(&format!("- complexity: {complexity}\n"));
            }
            if let Some(bucket_type) = item.bucket_type {
                out.push_str(&format!("- bucket: {bucket_type}\n"));
            }
            if let Some(decision_maker) = &item.decision_maker {
                out.push_str(&format!("- decision_maker: {decision_maker}\n"));
            }
            if let Some(decision_date) = item.decision_date {
                out.push_str(&format!("- decision_date: {decision_date}\n"));
            }
            if let Some(template_id) = item.template_id {
                use kairos_db::schema::templates;
                let name: Option<String> = templates::table
                    .filter(templates::id.eq(template_id))
                    .select(templates::name)
                    .first(conn)
                    .optional()
                    .map_err(ApiError::internal)?;
                if let Some(name) = name {
                    out.push_str(&format!("- template: {name}\n"));
                }
            }

            // Metadata values.
            let metadata = metadata_lines(conn, item.id)?;
            if !metadata.is_empty() {
                out.push_str("\n## Metadata\n");
                out.push_str(&metadata);
            }

            // Relationships (both directions, agent-oriented labels).
            let relationships = relationship_lines(conn, item.id)?;
            if !relationships.is_empty() {
                out.push_str("\n## Relationships\n");
                out.push_str(&relationships);
            }

            // Forge links (KAIROS-T-0123, UAT finding #3): an agent must see
            // PR state here, not only as a PR leaving `get_repository`'s
            // in-flight list. Same query and order as the links API.
            if item.item_type != ItemType::Document {
                let links = kairos_db::forge::links_for_item(conn, item.id)
                    .map_err(crate::api::org::forge::map_error)?;
                if !links.is_empty() {
                    out.push_str("\n## Development\n");
                    for link in links {
                        out.push_str(&format!(
                            "- {} {} [{}] {} — {} ({}/{})\n",
                            link.link.kind,
                            link.link.external_id,
                            link.link.state,
                            link.link.title,
                            link.link.url,
                            link.forge,
                            link.repo_full_name
                        ));
                    }
                }
            }

            out.push_str("\n## Content\n");
            out.push_str(&item.content);
            out.push('\n');
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "An item's content version history (version, editor, timestamp; newest first). Pass `version` to fetch that snapshot's full title + content instead."
    )]
    pub async fn get_history(
        &self,
        Parameters(params): Parameters<GetHistoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            use kairos_db::schema::{item_history, users};
            let item = load_item(conn, &params.short_code, Liveness::IncludeArchived)?;

            if let Some(version) = params.version {
                let snapshot: Option<(String, String)> = item_history::table
                    .filter(item_history::item_id.eq(item.id))
                    .filter(item_history::version.eq(version))
                    .select((item_history::title, item_history::content))
                    .first(conn)
                    .optional()
                    .map_err(ApiError::internal)?;
                let (title, content) = snapshot.ok_or_else(|| {
                    ApiError::not_found(format!(
                        "no history snapshot for {} at version {version}",
                        item.short_code
                    ))
                })?;
                return Ok(format!(
                    "# {} v{version} — {title}\n\n{content}\n",
                    item.short_code
                ));
            }

            let limit = params.limit.unwrap_or(20).clamp(1, 200);
            let rows: Vec<(i32, Uuid, DateTime<Utc>)> = item_history::table
                .filter(item_history::item_id.eq(item.id))
                .order(item_history::version.desc())
                .limit(limit)
                .select((
                    item_history::version,
                    item_history::edited_by,
                    item_history::edited_at,
                ))
                .load(conn)
                .map_err(ApiError::internal)?;
            let editor_ids: Vec<Uuid> = rows.iter().map(|(_, editor, _)| *editor).collect();
            let editors: HashMap<Uuid, String> = users::table
                .filter(users::id.eq_any(editor_ids))
                .select((users::id, users::display_name))
                .load::<(Uuid, String)>(conn)
                .map_err(ApiError::internal)?
                .into_iter()
                .collect();

            let mut out = format!(
                "# History of {} (current version {})\n",
                item.short_code, item.version
            );
            if let Some(archived_at) = item.archived_at {
                out.push_str(&format!(
                    "\n> **ARCHIVED** {} — this is the history of work that \
                     has been put away. The versions below are what it said; \
                     it has not changed since.\n\n",
                    archived_at.format("%Y-%m-%dT%H:%M:%SZ")
                ));
            }
            for (version, editor, edited_at) in rows {
                let editor = editors.get(&editor).map_or("unknown", String::as_str);
                out.push_str(&format!(
                    "- v{version} — {} by {editor}\n",
                    edited_at.format("%Y-%m-%dT%H:%M:%SZ")
                ));
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Unified search: full-text `q`, structured `filter` (types, board, column, team, task_type, metadata, dates), and graph `traverse` compose freely (at least one required). Compact results grouped by type; use get_item for full content."
    )]
    pub async fn search(
        &self,
        Parameters(params): Parameters<SearchParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        let mut request = match search_to_core(&params) {
            Ok(request) => request,
            Err(e) => return Ok(tool_error(e)),
        };
        let repository = params.filter.as_ref().and_then(|f| f.repository.clone());
        if repository.is_none()
            && let Err(e) = core_search::validate(&request)
        {
            // Invalid requests never cost a connection checkout.
            return Ok(tool_error(ApiError::validation(e.to_string())));
        }
        self.run_tool(&tenant, move |conn| {
            // KAIROS-T-0107: the slug-or-UUID repository filter needs a conn
            // to resolve, so it joins the core filter here — then validate.
            if let Some(reference) = repository.as_deref() {
                let repo = repositories::resolve(conn, reference)
                    .map_err(crate::api::tasks::map_repository_error)?;
                request
                    .filter
                    .get_or_insert_with(Default::default)
                    .repository_id = Some(repo.id);
                core_search::validate(&request).map_err(|e| ApiError::validation(e.to_string()))?;
            }
            let results = search::execute_search(conn, &request).map_err(map_search_error)?;
            let repo_ids: Vec<Uuid> = results
                .tasks
                .iter()
                .filter_map(|t| t.repository_id)
                .collect();
            let repo_slugs = repo_slug_map(conn, &repo_ids)?;
            Ok(render_search_results(&results, &repo_slugs))
        })
        .await
    }

    #[tool(
        description = "Create a work item: strategy | initiative | task | document | adr. Boards resolve by slug/UUID (defaulted when unambiguous); `parent` (short code) creates the parent edge — REQUIRED for documents (supports edge). Tasks: pass `repository` (slug/UUID) to issue the task against a codebase; it routes to the owning team's delivery board. Any member may create a task against another team's repository — it lands in that board's Backlog for their triage. Returns the new short code."
    )]
    pub async fn create_item(
        &self,
        Parameters(params): Parameters<CreateItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let tenant_ctx = tenant.clone();
        self.run_tool(&tenant, move |conn| {
            create_item_impl(conn, &tenant_ctx, user, &params)
        })
        .await
    }

    #[tool(
        description = "Replace an item's full content (and optionally title) under optimistic concurrency: pass the `version` you read. A stale version returns CONFLICT with the current version + content so you can reconcile. For small targeted edits prefer edit_item."
    )]
    pub async fn update_item(
        &self,
        Parameters(params): Parameters<UpdateItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            authorize_item_write(conn, &slug, user, &item)?;
            let update = items::ContentUpdate {
                new_title: params.title.as_deref(),
                new_content: &params.content,
                expected_version: params.version,
            };
            match items::update_item_content(conn, item.item_type, item.id, update, user) {
                Ok(new_version) => Ok(format!(
                    "Updated {} to version {new_version}.",
                    item.short_code
                )),
                Err(e) => Err(map_update_error(conn, &item, e)?),
            }
        })
        .await
    }

    #[tool(
        description = "Targeted server-side edit of an item's content: exact search/replace applied to the CURRENT version (retries once on a concurrent-edit race). Fails if `search` is not found, or is ambiguous when replace_all is false."
    )]
    pub async fn edit_item(
        &self,
        Parameters(params): Parameters<EditItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            if params.search.is_empty() {
                return Err(ApiError::validation("search must not be empty"));
            }
            // One retry on a version race (S-0006 edit_item semantics):
            // the read-modify-write below re-reads on the second attempt.
            for attempt in 0..2 {
                let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
                authorize_item_write(conn, &slug, user, &item)?;
                let occurrences = item.content.matches(&params.search).count();
                if occurrences == 0 {
                    return Err(ApiError::validation(format!(
                        "search string not found in {} (version {})",
                        item.short_code, item.version
                    )));
                }
                if occurrences > 1 && !params.replace_all {
                    return Err(ApiError::validation(format!(
                        "search string is ambiguous in {}: {occurrences} occurrences; \
                         pass replace_all=true or a more specific search",
                        item.short_code
                    )));
                }
                let new_content = if params.replace_all {
                    item.content.replace(&params.search, &params.replace)
                } else {
                    item.content.replacen(&params.search, &params.replace, 1)
                };
                let update = items::ContentUpdate {
                    new_title: None,
                    new_content: &new_content,
                    expected_version: item.version,
                };
                match items::update_item_content(conn, item.item_type, item.id, update, user) {
                    Ok(new_version) => {
                        return Ok(format!(
                            "Edited {} to version {new_version} ({occurrences} replacement{}).",
                            item.short_code,
                            if occurrences == 1 { "" } else { "s" }
                        ));
                    }
                    Err(items::ItemError::VersionConflict { .. }) if attempt == 0 => continue,
                    Err(e) => return Err(map_update_error(conn, &item, e)?),
                }
            }
            unreachable!("edit_item loop returns within two attempts");
        })
        .await
    }

    #[tool(
        description = "Move a TASK to another delivery board (`to_board` is a board slug or UUID) — what you do when work belongs to a different team, instead of recreating it there. It lands in that board's entry column and follows its team. Needs `manage_tasks` on both the task's current board and the target. A task bound to a repository may only move to that repository's owning team's board: unbind it first (`kairos repos unbind`) or pick that board. To move an item between COLUMNS of its own board, use `transition_item`."
    )]
    pub async fn move_item(
        &self,
        Parameters(params): Parameters<MoveItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if item.item_type != ItemType::Task {
                return Err(ApiError::validation(format!(
                    "{} {} is not a task; only tasks live on per-team delivery boards. \
                     Use transition_item to move an item between columns of its own board",
                    item.item_type, item.short_code
                )));
            }
            let from_board_id = item.board_id.ok_or_else(|| {
                ApiError::unprocessable(
                    "ITEM_NOT_ON_BOARD",
                    format!("task {} is not placed on a board", item.short_code),
                )
            })?;
            let target = board_by_ref(conn, &params.to_board)?;
            // Two-sided, like link_items and re-homing: the work leaves one
            // team's board and lands on another's.
            require_capability_explained(
                conn,
                &slug,
                Some(from_board_id),
                user,
                "manage_tasks",
                &item.short_code,
            )?;
            require_capability_explained(
                conn,
                &slug,
                Some(target.id),
                user,
                "manage_tasks",
                &item.short_code,
            )?;
            let from_board = board_by_ref(conn, &from_board_id.to_string())?;
            let moved =
                boards::move_task(conn, item.id, target.id, user).map_err(map_board_error)?;
            let column = board_columns(conn, target.id)?
                .into_iter()
                .find(|c| c.id == moved.column_id)
                .map(|c| c.name)
                .unwrap_or_default();
            Ok(format!(
                "Moved {}: {} -> {} / {}.",
                item.short_code, from_board.slug, target.slug, column
            ))
        })
        .await
    }

    #[tool(
        description = "Move an item to another column on its board (`to_column` is a column name or UUID). An invalid move fails with the allowed target columns enumerated — pick one and retry."
    )]
    pub async fn transition_item(
        &self,
        Parameters(params): Parameters<TransitionItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            let (board_id, from_column_id) = match (item.board_id, item.column_id) {
                (Some(board_id), Some(column_id)) => (board_id, column_id),
                _ => {
                    return Err(ApiError::unprocessable(
                        "ITEM_NOT_ON_BOARD",
                        format!(
                            "{} {} is not placed on a board, so it cannot be transitioned",
                            item.item_type, item.short_code
                        ),
                    ));
                }
            };
            require_capability_explained(
                conn,
                &slug,
                Some(board_id),
                user,
                "transition_items",
                &item.short_code,
            )?;
            let columns = board_columns(conn, board_id)?;
            let to_column_id = resolve_column(&columns, &params.to_column)?;

            match item.item_type {
                ItemType::Strategy => {
                    boards::transition_strategy(conn, item.id, to_column_id, user)
                }
                ItemType::Initiative => {
                    boards::transition_initiative(conn, item.id, to_column_id, user)
                }
                ItemType::Task => boards::transition_task(conn, item.id, to_column_id, user),
                ItemType::Adr => boards::transition_adr(conn, item.id, to_column_id, user),
                ItemType::Document => unreachable!("documents have no board placement"),
            }
            .map_err(map_board_error)?;

            let name_of = |id: Uuid| {
                columns
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            };
            Ok(format!(
                "Transitioned {}: {} -> {}.",
                item.short_code,
                name_of(from_column_id),
                name_of(to_column_id)
            ))
        })
        .await
    }

    #[tool(
        description = "Create a relationship edge between two items (by short code): parent | supports | informs | supersedes | blocks. Type rules and cycle prevention are enforced. `parent` and `blocks` may be written by anyone who manages either item's board or created the source item (so a task you filed against another team's repository can block your own item); the other types are org-admin only."
    )]
    pub async fn link_items(
        &self,
        Parameters(params): Parameters<LinkItemsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let tenant_ctx = tenant.clone();
        self.run_tool(&tenant, move |conn| {
            let relationship =
                parse_enum(&params.relationship, "relationship", RelationshipType::ALL)?;
            let (source_id, source_type) = require_live_typed(conn, &params.source, "source")?;
            let (target_id, target_type) = require_live_typed(conn, &params.target, "target")?;
            require_edge_capability(
                conn,
                &tenant_ctx,
                user,
                relationship.as_str(),
                (source_id, source_type),
                (target_id, target_type),
            )?;
            graph::link_items(conn, source_id, target_id, relationship, user)
                .map_err(map_link_error)?;
            Ok(format!(
                "Linked {} -[{relationship}]-> {}.",
                params.source, params.target
            ))
        })
        .await
    }

    #[tool(
        description = "Remove a relationship edge between two items (by short code and relationship type). Gated exactly like link_items."
    )]
    pub async fn unlink_items(
        &self,
        Parameters(params): Parameters<UnlinkItemsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let tenant_ctx = tenant.clone();
        self.run_tool(&tenant, move |conn| {
            let relationship =
                parse_enum(&params.relationship, "relationship", RelationshipType::ALL)?;
            let (source_id, source_type) = require_live_typed(conn, &params.source, "source")?;
            let (target_id, target_type) = require_live_typed(conn, &params.target, "target")?;
            require_edge_capability(
                conn,
                &tenant_ctx,
                user,
                relationship.as_str(),
                (source_id, source_type),
                (target_id, target_type),
            )?;
            graph::unlink_items(conn, source_id, target_id, relationship, user)
                .map_err(map_link_error)?;
            Ok(format!(
                "Unlinked {} -[{relationship}]-> {}.",
                params.source, params.target
            ))
        })
        .await
    }

    #[tool(
        description = "Set, update, or clear (null) metadata values on an item, keyed by metadata-definition slug. Values validate against the definition (enum membership, YYYY-MM-DD dates). Returns the item's resulting metadata set."
    )]
    pub async fn set_metadata(
        &self,
        Parameters(params): Parameters<SetMetadataParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            use kairos_db::models::templates::{MetadataDefinition, NewItemMetadata};
            use kairos_db::schema::{item_metadata, metadata_definitions as definitions};

            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            authorize_item_write(conn, &slug, user, &item)?;

            // Phase 1 — resolve + validate every entry (no writes yet); a
            // bad entry rejects the whole call (A-0003, same as the API).
            let mut ops: Vec<(Uuid, Option<String>)> = Vec::with_capacity(params.values.len());
            for (definition_slug, value) in &params.values {
                let definition: MetadataDefinition = definitions::table
                    .filter(definitions::slug.eq(definition_slug))
                    .select(MetadataDefinition::as_select())
                    .first(conn)
                    .optional()
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| {
                        ApiError::validation(format!(
                            "unknown metadata definition slug {definition_slug:?}"
                        ))
                    })?;
                if let Some(value) = value {
                    validate_metadata_value(conn, &definition, value)?;
                }
                ops.push((definition.id, value.clone()));
            }

            // Phase 2 — apply atomically (same shape as the API handler).
            conn.transaction::<_, diesel::result::Error, _>(|conn| {
                for (definition_id, value) in &ops {
                    match value {
                        Some(value) => {
                            diesel::insert_into(item_metadata::table)
                                .values(NewItemMetadata {
                                    item_id: item.id,
                                    metadata_definition_id: *definition_id,
                                    value: value.clone(),
                                })
                                .on_conflict((
                                    item_metadata::item_id,
                                    item_metadata::metadata_definition_id,
                                ))
                                .do_update()
                                .set(item_metadata::value.eq(value))
                                .execute(conn)?;
                        }
                        None => {
                            diesel::delete(
                                item_metadata::table
                                    .filter(item_metadata::item_id.eq(item.id))
                                    .filter(
                                        item_metadata::metadata_definition_id.eq(*definition_id),
                                    ),
                            )
                            .execute(conn)?;
                        }
                    }
                }
                Ok(())
            })
            .map_err(ApiError::internal)?;

            kairos_db::events::emit_item_event_by_id(
                conn,
                kairos_db::events::EventKind::MetadataChanged,
                item.item_type.entity_type(),
                item.id,
                user,
            )
            .map_err(ApiError::internal)?;

            let lines = metadata_lines(conn, item.id)?;
            Ok(format!(
                "# Metadata of {}\n{}",
                item.short_code,
                if lines.is_empty() {
                    "(none)\n".to_string()
                } else {
                    lines
                }
            ))
        })
        .await
    }

    #[tool(
        description = "Soft-delete an item by short code. Requires confirm=true because deletion CASCADES to descendants via parent edges; the response lists everything that was cascade-deleted."
    )]
    pub async fn delete_item(
        &self,
        Parameters(params): Parameters<DeleteItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            if !params.confirm {
                return Err(ApiError::validation(
                    "delete_item requires confirm=true: the delete soft-deletes the item \
                     AND cascades to all descendants reachable via parent edges",
                ));
            }
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            authorize_item_write(conn, &slug, user, &item)?;
            let outcome = items::soft_delete_item(conn, item.item_type, item.id, user)
                .map_err(map_item_error)?;
            let mut out = format!("Deleted {} (soft delete).\n", outcome.root_short_code);
            if outcome.cascaded_short_codes.is_empty() {
                out.push_str("Cascade: none.\n");
            } else {
                out.push_str(&format!(
                    "Cascade deleted {} descendant(s): {}\n",
                    outcome.cascaded_short_codes.len(),
                    outcome.cascaded_short_codes.join(", ")
                ));
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Put an archived item back on its board by short code. Restores ONLY the named item: a cascade delete was an act on a subtree, so archived descendants stay archived and are listed in the response for you to restore separately. Refused (RESTORE_BLOCKED) when the item's board, column, owning team or repository has since been removed — the response names what is missing, and the item must be moved somewhere that still exists."
    )]
    pub async fn restore_item(
        &self,
        Parameters(params): Parameters<RestoreItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::IncludeArchived)?;
            if item.archived_at.is_none() {
                return Err(ApiError::validation(format!(
                    "{} is not archived; there is nothing to restore",
                    item.short_code
                )));
            }
            authorize_item_write(conn, &slug, user, &item)?;
            match items::restore_item(conn, item.item_type, item.id, user).map_err(map_item_error)?
            {
                Ok(outcome) => {
                    let mut out = format!("Restored {}; it is on its board again.\n", outcome.short_code);
                    if outcome.still_archived_descendants.is_empty() {
                        out.push_str("Nothing below it is still archived.\n");
                    } else {
                        out.push_str(&format!(
                            "Still archived below it ({}): {}\nRestore them separately if you need them.\n",
                            outcome.still_archived_descendants.len(),
                            outcome.still_archived_descendants.join(", ")
                        ));
                    }
                    Ok(out)
                }
                Err(blocked) => Err(ApiError::unprocessable(
                    "RESTORE_BLOCKED",
                    format!(
                        "{} cannot be restored because {} is gone; move it \
                         somewhere that still exists, or restore what it needs \
                         first",
                        item.short_code,
                        blocked.missing.join(" and ")
                    ),
                )),
            }
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// Shared item projection + lookups (sync, on the blocking connection)
// ---------------------------------------------------------------------------

/// A uniform projection of any live item, whatever its table.
struct ItemView {
    id: Uuid,
    item_type: ItemType,
    short_code: String,
    title: String,
    content: String,
    version: i32,
    board_id: Option<Uuid>,
    column_id: Option<Uuid>,
    task_type: Option<TaskType>,
    /// Planned/Support lane (KAIROS-T-0077; tasks only).
    work_class: Option<WorkClass>,
    /// The bound repository (KAIROS-T-0111, A-0019; tasks only).
    repository_id: Option<Uuid>,
    /// Editorial lifecycle (KAIROS-T-0078; documents only).
    lifecycle: Option<DocumentLifecycle>,
    complexity: Option<Complexity>,
    hypothesis: Option<String>,
    bucket_type: Option<BucketType>,
    template_id: Option<Uuid>,
    decision_maker: Option<String>,
    decision_date: Option<NaiveDate>,
    updated_at: DateTime<Utc>,
    /// Set when this work has been put away (KAIROS-A-0020). Rendered as a
    /// banner so an agent knows not to try to act on it.
    archived_at: Option<DateTime<Utc>>,
}

/// Resolve a short code and load its [`ItemView`]; 404 `NOT_FOUND`
/// otherwise (mirrors the REST 404 contract).
///
/// `liveness` is enforced by the resolution step alone — it is authoritative,
/// so the per-table loads below carry no `deleted_at` filter of their own. A
/// `LiveOnly` caller never reaches them for an archived row, and a second
/// filter would only be a place for the two to disagree.
fn load_item(
    conn: &mut PgConnection,
    short_code: &str,
    liveness: Liveness,
) -> Result<ItemView, ApiError> {
    use kairos_db::schema::{adrs, documents, initiatives, strategies, tasks};

    let missing = || match liveness {
        Liveness::LiveOnly => {
            ApiError::not_found(format!("no live item with short code {short_code:?}"))
        }
        Liveness::IncludeArchived => {
            ApiError::not_found(format!("no item with short code {short_code:?}"))
        }
    };
    let (id, item_type) = resolve_short_code(conn, short_code, liveness)?.ok_or_else(missing)?;

    let view = match item_type {
        ItemType::Strategy => {
            let row: Strategy = strategies::table
                .filter(strategies::id.eq(id))
                .select(Strategy::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?
                .ok_or_else(missing)?;
            ItemView {
                id: row.id,
                item_type,
                short_code: row.short_code,
                title: row.title,
                content: row.content,
                version: row.version,
                board_id: Some(row.board_id),
                column_id: Some(row.column_id),
                task_type: None,
                work_class: None,
                repository_id: None,
                lifecycle: None,
                complexity: None,
                hypothesis: row.hypothesis,
                bucket_type: None,
                template_id: None,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
                archived_at: row.deleted_at,
            }
        }
        ItemType::Initiative => {
            let row: Initiative = initiatives::table
                .filter(initiatives::id.eq(id))
                .select(Initiative::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?
                .ok_or_else(missing)?;
            ItemView {
                id: row.id,
                item_type,
                short_code: row.short_code,
                title: row.title,
                content: row.content,
                version: row.version,
                board_id: Some(row.board_id),
                column_id: Some(row.column_id),
                task_type: None,
                work_class: None,
                repository_id: None,
                lifecycle: None,
                complexity: row.complexity,
                hypothesis: None,
                bucket_type: row.bucket_type,
                template_id: None,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
                archived_at: row.deleted_at,
            }
        }
        ItemType::Task => {
            let row: Task = tasks::table
                .filter(tasks::id.eq(id))
                .select(Task::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?
                .ok_or_else(missing)?;
            ItemView {
                id: row.id,
                item_type,
                short_code: row.short_code,
                title: row.title,
                content: row.content,
                version: row.version,
                board_id: Some(row.board_id),
                column_id: Some(row.column_id),
                task_type: Some(row.task_type),
                work_class: Some(row.work_class),
                repository_id: row.repository_id,
                lifecycle: None,
                complexity: None,
                hypothesis: None,
                bucket_type: None,
                template_id: None,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
                archived_at: row.deleted_at,
            }
        }
        ItemType::Document => {
            let row: Document = documents::table
                .filter(documents::id.eq(id))
                .select(Document::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?
                .ok_or_else(missing)?;
            ItemView {
                id: row.id,
                item_type,
                short_code: row.short_code,
                title: row.title,
                content: row.content,
                version: row.version,
                board_id: None,
                column_id: None,
                task_type: None,
                work_class: None,
                repository_id: None,
                lifecycle: Some(row.lifecycle),
                complexity: None,
                hypothesis: None,
                bucket_type: None,
                template_id: row.template_id,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
                archived_at: row.deleted_at,
            }
        }
        ItemType::Adr => {
            let row: Adr = adrs::table
                .filter(adrs::id.eq(id))
                .select(Adr::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?
                .ok_or_else(missing)?;
            ItemView {
                id: row.id,
                item_type,
                short_code: row.short_code,
                title: row.title,
                content: row.content,
                version: row.version,
                board_id: row.board_id,
                column_id: row.column_id,
                task_type: None,
                work_class: None,
                repository_id: None,
                lifecycle: None,
                complexity: None,
                hypothesis: None,
                bucket_type: None,
                template_id: None,
                decision_maker: row.decision_maker,
                decision_date: row.decision_date,
                updated_at: row.updated_at,
                archived_at: row.deleted_at,
            }
        }
    };
    Ok(view)
}

/// The A-0006 write gate for an item: `manage_<type>` on the item's
/// authorization board (own board; documents inherit the parent's via
/// `supports`; no board context → org-admin-only fallback).
fn authorize_item_write(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    item: &ItemView,
) -> Result<(), ApiError> {
    let board = abac::resolve_authorization_board(conn, item.id).map_err(map_abac_error)?;
    require_capability_explained(
        conn,
        slug,
        board,
        user,
        manage_capability(item.item_type),
        &item.short_code,
    )
}

/// `require_capability`, but when the caller is a cross-team filer — no
/// grant on this board, yet `file_backlog` would let them file into it —
/// the refusal explains the Backlog-only rule the plugin recipe teaches
/// instead of the bare capability name (KAIROS-T-0123, UAT finding #5).
/// The HTTP API keeps its generic envelope; this is agent-facing text.
fn require_capability_explained(
    conn: &mut PgConnection,
    slug: &str,
    board_id: Option<Uuid>,
    user: Uuid,
    capability: &str,
    short_code: &str,
) -> Result<(), ApiError> {
    let err = match require_capability(conn, slug, board_id, user, capability) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    let Some(board_id) = board_id else {
        return Err(err);
    };
    if err.code != "FORBIDDEN"
        || !abac::check_file_backlog(conn, slug, board_id, user).unwrap_or(false)
    {
        return Err(err);
    }
    let owner = {
        use kairos_db::schema::{boards, teams};
        boards::table
            .inner_join(teams::table)
            .filter(boards::id.eq(board_id))
            .select(teams::slug)
            .first::<String>(conn)
            .optional()
            .map_err(ApiError::internal)?
    };
    let whose = owner
        .map(|team| format!("{team}'s"))
        .unwrap_or_else(|| "the owning team's".to_string());
    Err(ApiError::forbidden(format!(
        "{short_code} sits in {whose} Backlog for their triage; a cross-team filer may create \
         and link it (file_backlog), not move, edit or delete it — that needs {capability:?} \
         on their board"
    ))
    .with_details(json!({
        "required_capability": capability,
        "board_id": board_id,
        "held": "file_backlog",
    })))
}

/// Resolve a board by UUID or slug; 404 `NOT_FOUND` otherwise.
fn board_by_ref(conn: &mut PgConnection, reference: &str) -> Result<Board, ApiError> {
    use kairos_db::schema::boards;
    let mut query = boards::table
        .filter(boards::deleted_at.is_null())
        .select(Board::as_select())
        .into_boxed();
    query = match Uuid::parse_str(reference) {
        Ok(id) => query.filter(boards::id.eq(id)),
        Err(_) => query.filter(boards::slug.eq(reference)),
    };
    query
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("no live board {reference:?} (slug or UUID)")))
}

/// Resolve a team by UUID or slug; 422 otherwise (a filter value).
fn team_by_ref(
    conn: &mut PgConnection,
    reference: &str,
) -> Result<kairos_db::models::teams::Team, ApiError> {
    use kairos_db::models::teams::Team;
    use kairos_db::schema::teams;
    let mut query = teams::table
        .filter(teams::deleted_at.is_null())
        .select(Team::as_select())
        .into_boxed();
    query = match Uuid::parse_str(reference) {
        Ok(id) => query.filter(teams::id.eq(id)),
        Err(_) => query.filter(teams::slug.eq(reference)),
    };
    query
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::validation(format!("team {reference:?} does not exist")))
}

/// A board row by id (must exist — callers hold a FK to it).
fn board_by_id(conn: &mut PgConnection, board_id: Uuid) -> Result<Board, ApiError> {
    use kairos_db::schema::boards;
    boards::table
        .filter(boards::id.eq(board_id))
        .select(Board::as_select())
        .first(conn)
        .map_err(ApiError::internal)
}

/// A board's LIVE columns in position order — what the board is now, so
/// a removed column (KAIROS-T-0161) is neither listed nor resolvable as a
/// transition target.
fn board_columns(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError> {
    board_columns_including_removed(conn, board_id, Liveness::LiveOnly)
}

/// A board's columns in position order, removed ones included when the
/// caller is showing archived cards (KAIROS-T-0159).
///
/// The only reason to pass [`Liveness::IncludeArchived`] is that archived
/// cards keep a `NOT NULL` FK to the column they were put away in, and
/// that column may since have been removed. Anything that renders or
/// validates a LIVE board calls [`board_columns`] instead.
fn board_columns_including_removed(
    conn: &mut PgConnection,
    board_id: Uuid,
    liveness: Liveness,
) -> Result<Vec<BoardColumn>, ApiError> {
    use kairos_db::schema::board_columns;
    let mut query = board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .into_boxed();
    if liveness == Liveness::LiveOnly {
        query = query.filter(board_columns::deleted_at.is_null());
    }
    query
        .order(board_columns::position.asc())
        .select(BoardColumn::as_select())
        .load(conn)
        .map_err(ApiError::internal)
}

/// The name of ANY column, removed ones included — the audit answer, not
/// the board view. An archived card keeps a `NOT NULL` FK to the column it
/// was put away in (KAIROS-T-0161), and "which column was this in?" has to
/// stay answerable for it, so this deliberately does not filter on
/// `deleted_at`. Use [`board_columns`] for anything that renders or
/// validates a live board.
fn column_label(conn: &mut PgConnection, column_id: Uuid) -> Result<String, ApiError> {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::id.eq(column_id))
        .select(board_columns::name)
        .first(conn)
        .optional()
        .map_err(ApiError::internal)
        .map(Option::unwrap_or_default)
}

/// Resolve a column reference (UUID or case-insensitive name) against a
/// board's columns; failure names the available columns (agent-corrective).
fn resolve_column(columns: &[BoardColumn], reference: &str) -> Result<Uuid, ApiError> {
    if let Ok(id) = Uuid::parse_str(reference)
        && columns.iter().any(|c| c.id == id)
    {
        return Ok(id);
    }
    if let Some(column) = columns
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(reference))
    {
        return Ok(column.id);
    }
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    Err(ApiError::validation(format!(
        "no column {reference:?} on this board; columns are [{}]",
        names.join(", ")
    )))
}

/// One compact row of a board listing.
struct BoardItemRow {
    column_id: Uuid,
    short_code: String,
    title: String,
    /// The bound repository (tasks only; KAIROS-T-0111 renders it).
    repository_id: Option<Uuid>,
    /// The type tag shown in listings (`task`/`bug`/`tech_debt` for tasks,
    /// the entity type otherwise).
    kind: String,
    /// Put away (KAIROS-A-0020). Only ever true when the caller asked for
    /// archived rows, and the renderer MUST mark it.
    archived: bool,
}

/// What each family's board listing selects. The trailing
/// `Option<DateTime<Utc>>` is `deleted_at`, selected in BOTH modes rather
/// than only the widened one: the live mode must still be able to prove
/// it served nothing archived, and a marker that is only fetched when it
/// might be set is a marker nobody checks (KAIROS-T-0159).
type BoardStrategySelect = (Uuid, String, String, Option<DateTime<Utc>>);
type BoardInitiativeSelect = (Uuid, String, String, bool, Option<DateTime<Utc>>);
type BoardTaskSelect = (
    Uuid,
    String,
    String,
    TaskType,
    WorkClass,
    Option<Uuid>,
    Option<DateTime<Utc>>,
);
type BoardAdrSelect = (Option<Uuid>, String, String, Option<DateTime<Utc>>);

/// Every item placed on a board (strategies, initiatives, tasks, and
/// on-board ADRs — documents have no placement), unified for listing.
/// `repository` (KAIROS-T-0107) narrows the TASKS only.
///
/// `liveness` is a parameter rather than a constant so that the one caller
/// that wants the audit view — MCP `board_items` with `include_deleted` —
/// can have it without `list_boards`' per-column counts silently widening
/// too: those count live work (ADR-20 rule 5) and pass
/// [`Liveness::LiveOnly`].
fn board_item_rows(
    conn: &mut PgConnection,
    board_id: Uuid,
    repository: Option<Uuid>,
    liveness: Liveness,
) -> Result<Vec<BoardItemRow>, ApiError> {
    use kairos_db::schema::{adrs, initiatives, strategies, tasks};

    let live_only = liveness == Liveness::LiveOnly;
    let mut rows: Vec<BoardItemRow> = Vec::new();
    let mut strategy_query = strategies::table
        .filter(strategies::board_id.eq(board_id))
        .into_boxed();
    if live_only {
        strategy_query = strategy_query.filter(strategies::deleted_at.is_null());
    }
    let strategies: Vec<BoardStrategySelect> = strategy_query
        .order(strategies::short_code.asc())
        .select((
            strategies::column_id,
            strategies::short_code,
            strategies::title,
            strategies::deleted_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(
        strategies
            .into_iter()
            .map(|(column_id, short_code, title, deleted_at)| BoardItemRow {
                column_id,
                short_code,
                title,
                repository_id: None,
                kind: "strategy".to_string(),
                archived: deleted_at.is_some(),
            }),
    );

    let mut initiative_query = initiatives::table
        .filter(initiatives::board_id.eq(board_id))
        .into_boxed();
    if live_only {
        initiative_query = initiative_query.filter(initiatives::deleted_at.is_null());
    }
    let initiatives: Vec<BoardInitiativeSelect> = initiative_query
        .order(initiatives::short_code.asc())
        .select((
            initiatives::column_id,
            initiatives::short_code,
            initiatives::title,
            initiatives::is_bucket,
            initiatives::deleted_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(initiatives.into_iter().map(
        |(column_id, short_code, title, is_bucket, deleted_at)| BoardItemRow {
            column_id,
            short_code,
            title,
            repository_id: None,
            kind: if is_bucket { "bucket" } else { "initiative" }.to_string(),
            archived: deleted_at.is_some(),
        },
    ));

    let mut task_query = tasks::table
        .filter(tasks::board_id.eq(board_id))
        .into_boxed();
    if live_only {
        task_query = task_query.filter(tasks::deleted_at.is_null());
    }
    if let Some(repository) = repository {
        task_query = task_query.filter(tasks::repository_id.eq(repository));
    }
    let tasks: Vec<BoardTaskSelect> = task_query
        .order(tasks::short_code.asc())
        .select((
            tasks::column_id,
            tasks::short_code,
            tasks::title,
            tasks::task_type,
            tasks::work_class,
            tasks::repository_id,
            tasks::deleted_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(tasks.into_iter().map(
        |(column_id, short_code, title, task_type, work_class, repository_id, deleted_at)| {
            BoardItemRow {
                column_id,
                short_code,
                title,
                repository_id,
                // The Support lane rides in `kind` (KAIROS-T-0077); Planned
                // stays unmarked as the default lane.
                kind: match work_class {
                    WorkClass::Support => format!("{task_type} [support lane]"),
                    WorkClass::Planned => task_type.to_string(),
                },
                archived: deleted_at.is_some(),
            }
        },
    ));

    let mut adr_query = adrs::table.filter(adrs::board_id.eq(board_id)).into_boxed();
    if live_only {
        adr_query = adr_query.filter(adrs::deleted_at.is_null());
    }
    let adrs: Vec<BoardAdrSelect> = adr_query
        .order(adrs::short_code.asc())
        .select((
            adrs::column_id,
            adrs::short_code,
            adrs::title,
            adrs::deleted_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(
        adrs.into_iter()
            .filter_map(|(column_id, short_code, title, deleted_at)| {
                column_id.map(|column_id| BoardItemRow {
                    column_id,
                    short_code,
                    title,
                    repository_id: None,
                    kind: "adr".to_string(),
                    archived: deleted_at.is_some(),
                })
            }),
    );

    Ok(rows)
}

/// Per-column LIVE item counts for one board — what `list_boards` prints
/// beside each column name. Archived work is not live work (ADR-20 rule
/// 5), so this stays live-only however `board_items` is asked for.
fn column_item_counts(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<HashMap<Uuid, i64>, ApiError> {
    let mut counts: HashMap<Uuid, i64> = HashMap::new();
    for row in board_item_rows(conn, board_id, None, Liveness::LiveOnly)? {
        *counts.entry(row.column_id).or_default() += 1;
    }
    Ok(counts)
}

/// Slugs for a set of repository ids, one query (KAIROS-T-0111): what the
/// compact listings print after a task.
fn repo_slug_map(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<BTreeMap<Uuid, String>, ApiError> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    use kairos_db::schema::repositories::dsl;
    let rows: Vec<(Uuid, String)> = dsl::repositories
        .filter(dsl::id.eq_any(ids))
        .filter(dsl::deleted_at.is_null())
        .select((dsl::id, dsl::slug))
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(rows.into_iter().collect())
}

/// `slug (owner team)` for one task's repository, or `(none)`.
fn repo_label(conn: &mut PgConnection, repository_id: Option<Uuid>) -> Result<String, ApiError> {
    let Some(id) = repository_id else {
        return Ok("(none)".to_string());
    };
    use kairos_db::schema::{repositories, teams};
    let row: Option<(String, String)> = repositories::table
        .inner_join(teams::table)
        .filter(repositories::id.eq(id))
        .filter(repositories::deleted_at.is_null())
        .select((repositories::slug, teams::slug))
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?;
    Ok(row.map_or_else(
        || "(none)".to_string(),
        |(slug, team)| format!("{slug} (owner: {team})"),
    ))
}

/// A live item by short code WITH its type (the edge-permission check needs
/// it); 422 `VALIDATION` naming the field otherwise, mirroring REST.
fn require_live_typed(
    conn: &mut PgConnection,
    short_code: &str,
    field: &str,
) -> Result<(Uuid, ItemType), ApiError> {
    resolve_short_code(conn, short_code, Liveness::LiveOnly)?.ok_or_else(|| {
        ApiError::validation(format!("{field} {short_code:?} does not name a live item"))
    })
}

/// An item's metadata values as compact `- slug: value` lines (ordered by
/// definition slug), empty string when it has none.
fn metadata_lines(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError> {
    use kairos_db::schema::{item_metadata, metadata_definitions};
    let rows: Vec<(String, String)> = item_metadata::table
        .inner_join(metadata_definitions::table)
        .filter(item_metadata::item_id.eq(item_id))
        .order(metadata_definitions::slug.asc())
        .select((metadata_definitions::slug, item_metadata::value))
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|(slug, value)| format!("- {slug}: {value}\n"))
        .collect())
}

#[derive(QueryableByName)]
struct ChainRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlText)]
    short_code: String,
    #[diesel(sql_type = SqlText)]
    title: String,
    #[diesel(sql_type = SqlNullable<SqlTimestamptz>)]
    deleted_at: Option<DateTime<Utc>>,
}

/// The item's ancestors via incoming `parent` edges, nearest first
/// (bounded — parent edges are acyclic by construction, this is
/// defense-in-depth).
///
/// Archived ancestors are reported, tagged (KAIROS-T-0158). This chain
/// IS the rendering of the item's incoming `parent` edges — those are
/// skipped in [`relationship_lines`] and drawn here instead — so a
/// live-only join here would have put the widened `relationships_for`
/// back behind a filter for exactly one relationship type, and the
/// nearest archived ancestor would have truncated the chain above it
/// too, hiding live grandparents along with it.
fn parent_chain(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<ChainRow>, ApiError> {
    let mut chain = Vec::new();
    let mut current = item_id;
    for _ in 0..10 {
        let parent: Option<ChainRow> = sql_query(
            "SELECT d.id, d.short_code, d.title, d.deleted_at \
             FROM item_relationships r \
             JOIN entity_directory d ON d.id = r.source_id \
             WHERE r.target_id = $1 AND r.relationship = 'parent' \
             ORDER BY r.created_at ASC LIMIT 1",
        )
        .bind::<SqlUuid, _>(current)
        .get_result(conn)
        .optional()
        .map_err(ApiError::internal)?;
        match parent {
            Some(row) => {
                current = row.id;
                chain.push(row);
            }
            None => break,
        }
    }
    Ok(chain)
}

/// Agent-oriented relationship lines for `get_item`: parent chain,
/// children, blockers, supporting docs, informs/supersedes — both
/// directions with direction-aware labels.
///
/// `relationships_for` reports archived neighbours since KAIROS-T-0158,
/// so every rendered entry carries an `[archived]` tag. An agent reading
/// "children: ACME-T-0007" and trying to move it would be refused by
/// every write path with no idea why; the tag is what tells it that the
/// row is history rather than work in flight.
fn relationship_lines(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError> {
    let relationships = graph::relationships_for(conn, item_id).map_err(ApiError::internal)?;

    let mut groups: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut push =
        |label: &'static str, entry: String| groups.entry(label).or_default().push(entry);

    /// One neighbour line, tagged when the neighbour is archived.
    fn line(neighbor: &kairos_db::graph::Neighbor) -> String {
        let mark = if neighbor.archived_at.is_some() {
            " [archived]"
        } else {
            ""
        };
        format!("{} — {}{mark}", neighbor.short_code, neighbor.title)
    }

    for neighbor in &relationships.outgoing {
        let entry = line(neighbor);
        match neighbor.relationship {
            RelationshipType::Parent => push("children", entry),
            RelationshipType::Supports => push("supporting docs", entry),
            RelationshipType::Informs => push("informs", entry),
            RelationshipType::Supersedes => push("supersedes", entry),
            RelationshipType::Blocks => push("blocks", entry),
        }
    }
    for neighbor in &relationships.incoming {
        let entry = line(neighbor);
        match neighbor.relationship {
            RelationshipType::Parent => {} // rendered as the parent chain below
            RelationshipType::Supports => push("supports", entry),
            RelationshipType::Informs => push("informed by", entry),
            RelationshipType::Supersedes => push("superseded by", entry),
            RelationshipType::Blocks => push("blocked by", entry),
        }
    }

    let mut out = String::new();
    let chain = parent_chain(conn, item_id)?;
    if !chain.is_empty() {
        let rendered: Vec<String> = chain
            .iter()
            .map(|row| {
                let mark = if row.deleted_at.is_some() {
                    " [archived]"
                } else {
                    ""
                };
                format!("{} ({}){mark}", row.short_code, row.title)
            })
            .collect();
        out.push_str(&format!("- parent chain: {}\n", rendered.join(" <- ")));
    }
    for (label, entries) in groups {
        out.push_str(&format!("- {label}: {}\n", entries.join("; ")));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// update/edit error shaping (REQ-1.5)
// ---------------------------------------------------------------------------

/// Map an [`items::ItemError`] from a content update to the S-0006 tool
/// error: a version conflict carries the CURRENT version, title, and
/// content so the agent can reconcile without another call (REQ-1.5);
/// everything else takes the standard REST mapping. Returns `Ok(error)` so
/// callers can `Err(map_update_error(...)?)` inside the blocking closure.
fn map_update_error(
    conn: &mut PgConnection,
    item: &ItemView,
    e: items::ItemError,
) -> Result<ApiError, ApiError> {
    match e {
        items::ItemError::VersionConflict {
            expected_version,
            current_version,
            current_title,
            current_content,
            ..
        } => {
            let _ = conn; // the typed error already carries the current row
            Ok(ApiError::conflict(format!(
                "version mismatch on {}: expected {expected_version}, current is {current_version}",
                item.short_code
            ))
            .with_details(json!({
                "current": {
                    "version": current_version,
                    "title": current_title,
                    "content": current_content,
                }
            })))
        }
        other => Ok(map_item_error(other)),
    }
}

// ---------------------------------------------------------------------------
// link/unlink error shaping (mirrors the REST relationship endpoints)
// ---------------------------------------------------------------------------

/// [`GraphError`] → the same codes the REST relationship endpoints emit:
/// `RELATIONSHIP_RULE`, `CYCLE_DETECTED`, `ALREADY_LINKED` (422),
/// `NOT_FOUND` for a missing edge, `VALIDATION` for self-links/unknown
/// endpoints.
fn map_link_error(e: GraphError) -> ApiError {
    match e {
        GraphError::Rule(rule) => ApiError::unprocessable("RELATIONSHIP_RULE", rule.to_string()),
        e @ GraphError::CycleDetected { .. } => {
            ApiError::unprocessable("CYCLE_DETECTED", e.to_string())
        }
        e @ GraphError::AlreadyLinked { .. } => {
            ApiError::unprocessable("ALREADY_LINKED", e.to_string())
        }
        e @ (GraphError::SelfLink(_) | GraphError::ItemNotFound(_)) => {
            ApiError::validation(e.to_string())
        }
        e @ GraphError::NotLinked { .. } => ApiError::not_found(e.to_string()),
        GraphError::Database(e) => ApiError::internal(e),
    }
}

// ---------------------------------------------------------------------------
// create_item (S-0006 write-tool semantics over the T-0012 services)
// ---------------------------------------------------------------------------

/// The board level whose boards host this item type.
fn level_of(item_type: ItemType) -> BoardLevel {
    match item_type {
        ItemType::Strategy => BoardLevel::Strategy,
        ItemType::Initiative => BoardLevel::Initiative,
        ItemType::Task => BoardLevel::Delivery,
        ItemType::Adr => BoardLevel::Adr,
        ItemType::Document => unreachable!("documents have no board level"),
    }
}

/// The tenant's single live board of `level`, or a 422 asking the agent to
/// name one (listing the candidates).
fn default_board_for(conn: &mut PgConnection, level: BoardLevel) -> Result<Board, ApiError> {
    use kairos_db::schema::boards;
    let candidates: Vec<Board> = boards::table
        .filter(boards::board_level.eq(level))
        .filter(boards::deleted_at.is_null())
        .order(boards::slug.asc())
        .select(Board::as_select())
        .load(conn)
        .map_err(ApiError::internal)?;
    match candidates.len() {
        1 => Ok(candidates.into_iter().next().expect("len checked")),
        0 => Err(ApiError::validation(format!(
            "no live {level} board exists; pass `board`"
        ))),
        _ => {
            let slugs: Vec<&str> = candidates.iter().map(|b| b.slug.as_str()).collect();
            Err(ApiError::validation(format!(
                "multiple {level} boards exist; pass `board` as one of [{}]",
                slugs.join(", ")
            )))
        }
    }
}

/// Resolve a template reference (UUID, slug, or name) to its id.
fn resolve_template(conn: &mut PgConnection, reference: &str) -> Result<Uuid, ApiError> {
    use kairos_db::schema::templates;
    if let Ok(id) = Uuid::parse_str(reference) {
        let exists: Option<Uuid> = templates::table
            .filter(templates::id.eq(id))
            .select(templates::id)
            .first(conn)
            .optional()
            .map_err(ApiError::internal)?;
        if exists.is_some() {
            return Ok(id);
        }
    }
    let by_name: Vec<Template> = templates::table
        .filter(
            templates::slug
                .eq(reference)
                .or(templates::name.eq(reference)),
        )
        .select(Template::as_select())
        .load(conn)
        .map_err(ApiError::internal)?;
    match by_name.len() {
        1 => Ok(by_name.into_iter().next().expect("len checked").id),
        0 => Err(ApiError::validation(format!(
            "no template {reference:?} (id, slug, or name)"
        ))),
        _ => Err(ApiError::validation(format!(
            "template name {reference:?} is ambiguous; pass its id or slug"
        ))),
    }
}

/// Reject a type-specific field supplied for the wrong item type (agents
/// get corrected instead of silently losing input).
fn reject_field(
    field: &str,
    value: Option<&String>,
    item_type: ItemType,
    applies_to: &str,
) -> Result<(), ApiError> {
    match value {
        Some(_) => Err(ApiError::validation(format!(
            "{field} applies to {applies_to}, not {item_type}"
        ))),
        None => Ok(()),
    }
}

/// The create_item body: resolve the target board (or parent, for
/// documents), enforce `manage_<type>` (A-0006), create through the T-0012
/// service, then write the `parent`/`supports` edge when `parent` is given.
fn create_item_impl(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    params: &CreateItemParams,
) -> Result<String, ApiError> {
    let slug = tenant.slug.as_str();
    let item_type = match params.item_type.as_str() {
        "strategy" => ItemType::Strategy,
        "initiative" => ItemType::Initiative,
        "task" => ItemType::Task,
        "document" => ItemType::Document,
        "adr" => ItemType::Adr,
        other => {
            return Err(ApiError::validation(format!(
                "item_type must be one of [strategy, initiative, task, document, adr], \
                 got {other:?}"
            )));
        }
    };
    if item_type != ItemType::Document {
        reject_field("template", params.template.as_ref(), item_type, "documents")?;
    }
    if item_type != ItemType::Task {
        reject_field("task_type", params.task_type.as_ref(), item_type, "tasks")?;
    }
    if item_type != ItemType::Strategy {
        reject_field(
            "hypothesis",
            params.hypothesis.as_ref(),
            item_type,
            "strategies",
        )?;
    }
    if item_type != ItemType::Initiative {
        reject_field(
            "complexity",
            params.complexity.as_ref(),
            item_type,
            "initiatives",
        )?;
    }
    if item_type != ItemType::Adr {
        reject_field(
            "decision_maker",
            params.decision_maker.as_ref(),
            item_type,
            "ADRs",
        )?;
    }
    let content = params.content.as_deref().unwrap_or("");

    // Documents: no board; parent REQUIRED; supports edge; authorization
    // inherits from the parent's board (the REST create_document contract).
    if item_type == ItemType::Document {
        let parent_code = params.parent.as_deref().ok_or_else(|| {
            ApiError::validation(
                "documents require `parent` (a strategy, initiative, or task short code); \
                 the document is attached via a supports edge",
            )
        })?;
        let (parent_id, parent_type) = resolve_short_code(conn, parent_code, Liveness::LiveOnly)?
            .ok_or_else(|| {
            ApiError::validation(format!("parent {parent_code:?} does not name a live item"))
        })?;
        if !matches!(
            parent_type,
            ItemType::Strategy | ItemType::Initiative | ItemType::Task
        ) {
            return Err(ApiError::validation(format!(
                "parent {parent_code:?} is a {parent_type}; documents attach to a \
                 strategy, initiative, or task"
            )));
        }
        let board = abac::resolve_authorization_board(conn, parent_id).map_err(map_abac_error)?;
        require_capability(conn, slug, board, user, manage_capability(item_type))?;
        let template_id = params
            .template
            .as_deref()
            .map(|t| resolve_template(conn, t))
            .transpose()?;
        let created = items::create_document(
            conn,
            items::CreateDocument {
                title: &params.title,
                content: params.content.as_deref(),
                template_id,
            },
            user,
        )
        .map_err(map_item_error)?;
        graph::link_items(
            conn,
            parent_id,
            created.id,
            RelationshipType::Supports,
            user,
        )
        .map_err(map_graph_error)?;
        return Ok(format!(
            "Created document {} — {} (version 1), supports {parent_code}.",
            created.short_code, created.title
        ));
    }

    if item_type != ItemType::Task {
        reject_field("repository", params.repository.as_ref(), item_type, "tasks")?;
    }

    // Board items: resolve the board (explicit slug/UUID or the single
    // board of the matching level), then manage_<type> on it. Tasks go
    // through the SAME routing + capability helpers as POST /api/tasks
    // (KAIROS-T-0104/T-0105): a repository routes the task to its owning
    // team's delivery board, and a non-member may still file into that
    // board's Backlog.
    let (board, route) = if item_type == ItemType::Task {
        let explicit = params
            .board
            .as_deref()
            .map(|reference| board_by_ref(conn, reference))
            .transpose()?;
        let route = match (explicit.as_ref(), params.repository.as_deref()) {
            (None, None) => {
                let board = default_board_for(conn, level_of(item_type))?;
                crate::api::tasks::TaskRoute {
                    board_id: board.id,
                    team_id: None,
                    repository_id: None,
                }
            }
            (board, repository) => {
                crate::api::tasks::resolve_routing(conn, board.map(|b| b.id), None, repository)?
            }
        };
        crate::api::tasks::require_task_create_capability(conn, slug, user, &route, None)?;
        let board = match explicit {
            Some(board) if board.id == route.board_id => board,
            _ => board_by_ref(conn, &route.board_id.to_string())?,
        };
        (board, Some(route))
    } else {
        let board = match params.board.as_deref() {
            Some(reference) => board_by_ref(conn, reference)?,
            None => default_board_for(conn, level_of(item_type))?,
        };
        require_capability(
            conn,
            slug,
            Some(board.id),
            user,
            manage_capability(item_type),
        )?;
        (board, None)
    };

    // KAIROS-T-0111: the `parent` edge is gated BEFORE the item is written
    // (a refusal must leave no orphan), by the same rule as link_items —
    // the target is the board the new item will sit on.
    let parent = params
        .parent
        .as_deref()
        .map(|parent_code| {
            let (parent_id, parent_type) =
                resolve_short_code(conn, parent_code, Liveness::LiveOnly)?.ok_or_else(|| {
                    ApiError::validation(format!(
                        "parent {parent_code:?} does not name a live item"
                    ))
                })?;
            crate::api::meta::require_edge_capability_on(
                conn,
                tenant,
                user,
                RelationshipType::Parent.as_str(),
                (parent_id, parent_type),
                (Some(board.id), item_type),
            )?;
            Ok::<_, ApiError>((parent_code, parent_id))
        })
        .transpose()?;

    let (created_code, created_title, created_id) = match item_type {
        ItemType::Strategy => {
            let created = items::create_strategy(
                conn,
                items::CreateStrategy {
                    board_id: board.id,
                    column_id: None,
                    title: &params.title,
                    content,
                    hypothesis: params.hypothesis.as_deref(),
                },
                user,
            )
            .map_err(map_item_error)?;
            (created.short_code, created.title, created.id)
        }
        ItemType::Initiative => {
            let complexity = params
                .complexity
                .as_deref()
                .map(|v| parse_enum(v, "complexity", Complexity::ALL))
                .transpose()?;
            let created = items::create_initiative(
                conn,
                items::CreateInitiative {
                    board_id: board.id,
                    column_id: None,
                    title: &params.title,
                    content,
                    complexity,
                    bucket_type: None,
                },
                user,
            )
            .map_err(map_item_error)?;
            (created.short_code, created.title, created.id)
        }
        ItemType::Task => {
            let task_type = params
                .task_type
                .as_deref()
                .map(|v| parse_enum(v, "task_type", TaskType::ALL))
                .transpose()?
                .unwrap_or(TaskType::Task);
            // KAIROS-T-0077: same default rule as the REST create — a
            // support-type ticket is born in the Support lane.
            let work_class = params
                .work_class
                .as_deref()
                .map(|v| parse_enum(v, "work_class", WorkClass::ALL))
                .transpose()?
                .unwrap_or(if task_type == TaskType::Support {
                    WorkClass::Support
                } else {
                    WorkClass::Planned
                });
            let route = route.expect("tasks always resolve a route");
            let created = items::create_task(
                conn,
                items::CreateTask {
                    board_id: route.board_id,
                    column_id: None,
                    title: &params.title,
                    content,
                    task_type,
                    work_class,
                    team_id: route.team_id,
                    repository_id: route.repository_id,
                },
                user,
            )
            .map_err(map_item_error)?;
            (created.short_code, created.title, created.id)
        }
        ItemType::Adr => {
            let created = items::create_adr(
                conn,
                items::CreateAdr {
                    board_id: Some(board.id),
                    column_id: None,
                    title: &params.title,
                    content,
                    decision_maker: params.decision_maker.as_deref(),
                    decision_date: None,
                },
                user,
            )
            .map_err(map_item_error)?;
            (created.short_code, created.title, created.id)
        }
        ItemType::Document => unreachable!("handled above"),
    };

    let mut out = format!(
        "Created {item_type} {created_code} — {created_title} (version 1) on board {}.",
        board.slug
    );
    if let Some((parent_code, parent_id)) = parent {
        graph::link_items(conn, parent_id, created_id, RelationshipType::Parent, user)
            .map_err(map_graph_error)?;
        out.push_str(&format!("\nparent: {parent_code} (parent edge created)."));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// search (mirrors POST /api/search — the A-0007 pipeline, REQ per S-0006)
// ---------------------------------------------------------------------------

/// A field-level 422 `VALIDATION` for the search input (the tool-error
/// mirror of the REST endpoint's 400s).
fn field_invalid(field: &str, message: impl Into<String>) -> ApiError {
    ApiError::validation(message).with_details(json!({ "field": field }))
}

fn uuid_field(value: &str, field: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value)
        .map_err(|_| field_invalid(field, format!("{field} must be a UUID, got {value:?}")))
}

fn timestamp_field(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| {
            field_invalid(
                field,
                format!("{field} must be an RFC 3339 timestamp, got {value:?}"),
            )
        })
}

/// Parse a closed-vocabulary value through the core model's serde
/// vocabulary (one source of truth for the allowed strings).
fn enum_field<T: serde::de::DeserializeOwned>(
    value: &str,
    field: &str,
    allowed: &str,
) -> Result<T, ApiError> {
    serde_json::from_value(json!(value)).map_err(|_| {
        field_invalid(
            field,
            format!("{field} must be one of [{allowed}], got {value:?}"),
        )
    })
}

/// Convert the tool input into the typed `kairos_core::search` request and
/// validate it — exactly the conversions the REST endpoint applies.
fn search_to_core(params: &SearchParams) -> Result<core_search::SearchRequest, ApiError> {
    let filter = params
        .filter
        .as_ref()
        .map(|filter| -> Result<core_search::SearchFilter, ApiError> {
            Ok(core_search::SearchFilter {
                entity_type: filter
                    .entity_type
                    .as_ref()
                    .map(|types| {
                        types
                            .iter()
                            .map(|t| {
                                enum_field(
                                    t,
                                    "filter.entity_type",
                                    "strategy, initiative, task, document, adr",
                                )
                            })
                            .collect::<Result<Vec<core_search::SearchEntityType>, _>>()
                    })
                    .transpose()?,
                board_id: filter
                    .board_id
                    .as_deref()
                    .map(|v| uuid_field(v, "filter.board_id"))
                    .transpose()?,
                column_id: filter
                    .column_id
                    .as_deref()
                    .map(|v| uuid_field(v, "filter.column_id"))
                    .transpose()?,
                team_id: filter
                    .team_id
                    .as_deref()
                    .map(|v| uuid_field(v, "filter.team_id"))
                    .transpose()?,
                // Resolved from the slug-or-UUID `repository` inside the
                // tool (a conn is needed); see `search`.
                repository_id: None,
                task_type: filter
                    .task_type
                    .as_ref()
                    .map(|types| {
                        types
                            .iter()
                            .map(|t| {
                                enum_field(t, "filter.task_type", "task, bug, tech_debt, support")
                            })
                            .collect::<Result<Vec<core_search::SearchTaskType>, _>>()
                    })
                    .transpose()?,
                work_class: filter
                    .work_class
                    .as_ref()
                    .map(|classes| {
                        classes
                            .iter()
                            .map(|c| enum_field(c, "filter.work_class", "planned, support"))
                            .collect::<Result<Vec<core_search::SearchWorkClass>, _>>()
                    })
                    .transpose()?,
                is_bucket: filter.is_bucket,
                metadata: filter.metadata.clone(),
                created_after: filter
                    .created_after
                    .as_deref()
                    .map(|v| timestamp_field(v, "filter.created_after"))
                    .transpose()?,
                created_before: filter
                    .created_before
                    .as_deref()
                    .map(|v| timestamp_field(v, "filter.created_before"))
                    .transpose()?,
                include_deleted: filter.include_deleted,
            })
        })
        .transpose()?;
    let traverse = params
        .traverse
        .as_ref()
        .map(|traverse| -> Result<core_search::Traverse, ApiError> {
            Ok(core_search::Traverse {
                from: core_search::TraverseFrom {
                    short_code: Some(traverse.from.clone()),
                    id: None,
                },
                relationships: traverse
                    .relationships
                    .iter()
                    .map(|r| {
                        enum_field(
                            r,
                            "traverse.relationships",
                            "parent, supports, informs, supersedes, blocks",
                        )
                    })
                    .collect::<Result<Vec<core_search::SearchRelationship>, _>>()?,
                direction: enum_field(
                    &traverse.direction,
                    "traverse.direction",
                    "outbound, inbound, both",
                )?,
                depth: traverse.depth,
            })
        })
        .transpose()?;
    let sort = params
        .sort
        .as_ref()
        .map(|sort| -> Result<core_search::Sort, ApiError> {
            Ok(core_search::Sort {
                field: enum_field(
                    &sort.field,
                    "sort.field",
                    "created_at, updated_at, title, relevance",
                )?,
                order: enum_field(&sort.order, "sort.order", "asc, desc")?,
            })
        })
        .transpose()?;

    let request = core_search::SearchRequest {
        q: params.q.clone(),
        filter,
        traverse,
        sort,
        limit: params.limit,
        offset: params.offset,
    };
    // Validation happens in the `search` tool: a slug-or-UUID `repository`
    // filter (KAIROS-T-0107) is resolved with a connection first, and a
    // filter carrying only that must not be rejected as unconstraining.
    Ok(request)
}

/// [`SearchError`] → tool error (validation was pre-checked, so this is
/// the traverse-root 404 or a real failure).
fn map_search_error(e: SearchError) -> ApiError {
    match e {
        SearchError::Invalid(e) => ApiError::validation(e.to_string()),
        SearchError::TraverseRootNotFound { reference } => {
            ApiError::not_found(format!("traverse root {reference:?} does not exist"))
        }
        SearchError::Database(e) => ApiError::internal(e),
    }
}

/// `" [archived]"` for a row that has been put away, empty otherwise
/// (KAIROS-A-0020, KAIROS-T-0157). `include_deleted` mixes archived hits in
/// with live ones, and an agent that cannot tell them apart will try to
/// write to retired work and be refused with no idea why.
fn archived_marker(deleted_at: Option<DateTime<Utc>>) -> &'static str {
    if deleted_at.is_some() {
        " [archived]"
    } else {
        ""
    }
}

/// Compact REQ-1.6 rendering: results grouped by type, one line per item
/// (short code + title + a key field), full content via `get_item`.
fn render_search_results(results: &SearchResults, repo_slugs: &BTreeMap<Uuid, String>) -> String {
    let shown = results.strategies.len()
        + results.initiatives.len()
        + results.tasks.len()
        + results.documents.len()
        + results.adrs.len();
    let mut out = format!(
        "{} match(es), showing {} (limit {}, offset {})\n",
        results.total, shown, results.limit, results.offset
    );
    if !results.strategies.is_empty() {
        out.push_str("\n## strategies\n");
        for row in &results.strategies {
            out.push_str(&format!(
                "- {} — {}{}\n",
                row.short_code,
                row.title,
                archived_marker(row.deleted_at)
            ));
        }
    }
    if !results.initiatives.is_empty() {
        out.push_str("\n## initiatives\n");
        for row in &results.initiatives {
            let bucket = if row.is_bucket { " [bucket]" } else { "" };
            out.push_str(&format!(
                "- {} — {}{bucket}{}\n",
                row.short_code,
                row.title,
                archived_marker(row.deleted_at)
            ));
        }
    }
    if !results.tasks.is_empty() {
        out.push_str("\n## tasks\n");
        for row in &results.tasks {
            out.push_str(&format!(
                "- {} — {} [{}]{}{}\n",
                row.short_code,
                row.title,
                row.task_type,
                row.repository_id
                    .and_then(|id| repo_slugs.get(&id))
                    .map(|slug| format!(" [repo:{slug}]"))
                    .unwrap_or_default(),
                archived_marker(row.deleted_at)
            ));
        }
    }
    if !results.documents.is_empty() {
        out.push_str("\n## documents\n");
        for row in &results.documents {
            out.push_str(&format!(
                "- {} — {}{}\n",
                row.short_code,
                row.title,
                archived_marker(row.deleted_at)
            ));
        }
    }
    if !results.adrs.is_empty() {
        out.push_str("\n## adrs\n");
        for row in &results.adrs {
            out.push_str(&format!(
                "- {} — {}{}\n",
                row.short_code,
                row.title,
                archived_marker(row.deleted_at)
            ));
        }
    }
    out
}
