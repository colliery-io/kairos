//! The S-0006 tool surface (KAIROS-T-0026): the 14 frozen tools, each a
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
use diesel::sql_types::{Text as SqlText, Uuid as SqlUuid};
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

use crate::api::meta::{manage_capability, require_org_admin, validate_metadata_value};
use crate::api::{
    map_abac_error, map_board_error, map_graph_error, map_item_error, parse_enum,
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
    /// Sort of the combined results (default created_at desc).
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
    /// Include soft-deleted items (default false).
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
    /// created_at | updated_at | title.
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
pub struct DeleteItemParams {
    /// The item's short code.
    pub short_code: String,
    /// Must be true: deletion soft-deletes the item AND cascades to its
    /// descendants via parent edges. The response lists the cascade.
    pub confirm: bool,
}

// ---------------------------------------------------------------------------
// The 14 tools
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

            let member_teams: Vec<(String, String, TeamType)> = team_members::table
                .inner_join(teams::table)
                .filter(team_members::user_id.eq(user_id))
                .filter(teams::deleted_at.is_null())
                .order(teams::slug.asc())
                .select((teams::slug, teams::name, teams::team_type))
                .load(conn)
                .map_err(ApiError::internal)?;

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
            for (slug, name, team_type) in member_teams {
                out.push_str(&format!("- {slug} — {name} ({team_type})\n"));
            }
            // KAIROS-T-0107: the repositories my teams own — where my
            // tickets are issued and executed (A-0019).
            {
                use kairos_db::schema::repositories as repos;
                let my_team_ids: Vec<Uuid> = team_members::table
                    .filter(team_members::user_id.eq(user_id))
                    .select(team_members::team_id)
                    .load(conn)
                    .map_err(ApiError::internal)?;
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
            let rendered = crate::api::org::repositories::render(conn, vec![repo])?.remove(0);
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
                rendered.delivery_board_id.as_deref().unwrap_or("(none)"),
                rendered.open_tasks,
                if rendered.has_webhook { "connected" } else { "not connected" },
            );
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
        description = "List the items on a board grouped by column: short code, type, and title. `board` is a slug or UUID; optional `column` (name or UUID) restricts to one column; optional `repository` (slug or UUID) narrows the tasks to one repository — pass the repository you are checked out in to see your queue."
    )]
    pub async fn board_items(
        &self,
        Parameters(params): Parameters<BoardItemsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            let board = board_by_ref(conn, &params.board)?;
            let mut columns = board_columns(conn, board.id)?;
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
            let items = board_item_rows(conn, board.id, repository)?;

            let mut out = format!(
                "# Board {} — {} ({})\n",
                board.slug, board.name, board.board_level
            );
            for column in &columns {
                let in_column: Vec<&BoardItemRow> =
                    items.iter().filter(|i| i.column_id == column.id).collect();
                out.push_str(&format!("\n## {} ({})\n", column.name, in_column.len()));
                for item in in_column {
                    out.push_str(&format!(
                        "- {} [{}] {}\n",
                        item.short_code, item.kind, item.title
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
            let item = load_item(conn, &params.short_code)?;

            let mut out = format!("# {} — {}\n", item.short_code, item.title);
            out.push_str(&format!("- type: {}", item.item_type));
            if let Some(task_type) = item.task_type {
                out.push_str(&format!(" ({task_type})"));
            }
            if let Some(work_class) = item.work_class {
                out.push_str(&format!(" · lane: {work_class}"));
            }
            if let Some(lifecycle) = item.lifecycle {
                out.push_str(&format!(" · lifecycle: {lifecycle}"));
            }
            out.push('\n');
            if let (Some(board_id), Some(column_id)) = (item.board_id, item.column_id) {
                let board = board_by_id(conn, board_id)?;
                let column = board_columns(conn, board_id)?
                    .into_iter()
                    .find(|c| c.id == column_id)
                    .map(|c| c.name)
                    .unwrap_or_default();
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
            let item = load_item(conn, &params.short_code)?;

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
            Ok(render_search_results(&results))
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
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            create_item_impl(conn, &slug, user, &params)
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
            let item = load_item(conn, &params.short_code)?;
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
                let item = load_item(conn, &params.short_code)?;
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
            let item = load_item(conn, &params.short_code)?;
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
            require_capability(conn, &slug, Some(board_id), user, "transition_items")?;
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
        description = "Create a relationship edge between two items (by short code): parent | supports | informs | supersedes | blocks. Type rules and cycle prevention are enforced; org-admin only (relationships are tenant-wide configuration)."
    )]
    pub async fn link_items(
        &self,
        Parameters(params): Parameters<LinkItemsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        if let Err(e) = require_org_admin(&tenant) {
            return Ok(tool_error(e));
        }
        let user = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            let relationship =
                parse_enum(&params.relationship, "relationship", RelationshipType::ALL)?;
            let source_id = require_live(conn, &params.source, "source")?;
            let target_id = require_live(conn, &params.target, "target")?;
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
        description = "Remove a relationship edge between two items (by short code and relationship type). Org-admin only."
    )]
    pub async fn unlink_items(
        &self,
        Parameters(params): Parameters<UnlinkItemsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        if let Err(e) = require_org_admin(&tenant) {
            return Ok(tool_error(e));
        }
        let user = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            let relationship =
                parse_enum(&params.relationship, "relationship", RelationshipType::ALL)?;
            let source_id = require_live(conn, &params.source, "source")?;
            let target_id = require_live(conn, &params.target, "target")?;
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

            let item = load_item(conn, &params.short_code)?;
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
            let item = load_item(conn, &params.short_code)?;
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
    /// Editorial lifecycle (KAIROS-T-0078; documents only).
    lifecycle: Option<DocumentLifecycle>,
    complexity: Option<Complexity>,
    hypothesis: Option<String>,
    bucket_type: Option<BucketType>,
    template_id: Option<Uuid>,
    decision_maker: Option<String>,
    decision_date: Option<NaiveDate>,
    updated_at: DateTime<Utc>,
}

/// Resolve a short code to a live item and load its [`ItemView`]; 404
/// `NOT_FOUND` otherwise (mirrors the REST 404 contract).
fn load_item(conn: &mut PgConnection, short_code: &str) -> Result<ItemView, ApiError> {
    use kairos_db::schema::{adrs, documents, initiatives, strategies, tasks};

    let (id, item_type) = resolve_short_code(conn, short_code)?.ok_or_else(|| {
        ApiError::not_found(format!("no live item with short code {short_code:?}"))
    })?;
    let missing = || ApiError::not_found(format!("no live item with short code {short_code:?}"));

    let view = match item_type {
        ItemType::Strategy => {
            let row: Strategy = strategies::table
                .filter(strategies::id.eq(id))
                .filter(strategies::deleted_at.is_null())
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
                lifecycle: None,
                complexity: None,
                hypothesis: row.hypothesis,
                bucket_type: None,
                template_id: None,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
            }
        }
        ItemType::Initiative => {
            let row: Initiative = initiatives::table
                .filter(initiatives::id.eq(id))
                .filter(initiatives::deleted_at.is_null())
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
                lifecycle: None,
                complexity: row.complexity,
                hypothesis: None,
                bucket_type: row.bucket_type,
                template_id: None,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
            }
        }
        ItemType::Task => {
            let row: Task = tasks::table
                .filter(tasks::id.eq(id))
                .filter(tasks::deleted_at.is_null())
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
                lifecycle: None,
                complexity: None,
                hypothesis: None,
                bucket_type: None,
                template_id: None,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
            }
        }
        ItemType::Document => {
            let row: Document = documents::table
                .filter(documents::id.eq(id))
                .filter(documents::deleted_at.is_null())
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
                lifecycle: Some(row.lifecycle),
                complexity: None,
                hypothesis: None,
                bucket_type: None,
                template_id: row.template_id,
                decision_maker: None,
                decision_date: None,
                updated_at: row.updated_at,
            }
        }
        ItemType::Adr => {
            let row: Adr = adrs::table
                .filter(adrs::id.eq(id))
                .filter(adrs::deleted_at.is_null())
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
                lifecycle: None,
                complexity: None,
                hypothesis: None,
                bucket_type: None,
                template_id: None,
                decision_maker: row.decision_maker,
                decision_date: row.decision_date,
                updated_at: row.updated_at,
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
    require_capability(conn, slug, board, user, manage_capability(item.item_type))
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

/// A board's columns in position order.
fn board_columns(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError> {
    use kairos_db::schema::board_columns;
    board_columns::table
        .filter(board_columns::board_id.eq(board_id))
        .order(board_columns::position.asc())
        .select(BoardColumn::as_select())
        .load(conn)
        .map_err(ApiError::internal)
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
    /// The type tag shown in listings (`task`/`bug`/`tech_debt` for tasks,
    /// the entity type otherwise).
    kind: String,
}

/// Every live item placed on a board (strategies, initiatives, tasks, and
/// on-board ADRs — documents have no placement), unified for listing.
/// `repository` (KAIROS-T-0107) narrows the TASKS only.
fn board_item_rows(
    conn: &mut PgConnection,
    board_id: Uuid,
    repository: Option<Uuid>,
) -> Result<Vec<BoardItemRow>, ApiError> {
    use kairos_db::schema::{adrs, initiatives, strategies, tasks};

    let mut rows: Vec<BoardItemRow> = Vec::new();
    let strategies: Vec<(Uuid, String, String)> = strategies::table
        .filter(strategies::board_id.eq(board_id))
        .filter(strategies::deleted_at.is_null())
        .order(strategies::short_code.asc())
        .select((
            strategies::column_id,
            strategies::short_code,
            strategies::title,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(
        strategies
            .into_iter()
            .map(|(column_id, short_code, title)| BoardItemRow {
                column_id,
                short_code,
                title,
                kind: "strategy".to_string(),
            }),
    );

    let initiatives: Vec<(Uuid, String, String, bool)> = initiatives::table
        .filter(initiatives::board_id.eq(board_id))
        .filter(initiatives::deleted_at.is_null())
        .order(initiatives::short_code.asc())
        .select((
            initiatives::column_id,
            initiatives::short_code,
            initiatives::title,
            initiatives::is_bucket,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(
        initiatives
            .into_iter()
            .map(|(column_id, short_code, title, is_bucket)| BoardItemRow {
                column_id,
                short_code,
                title,
                kind: if is_bucket { "bucket" } else { "initiative" }.to_string(),
            }),
    );

    let mut task_query = tasks::table
        .filter(tasks::board_id.eq(board_id))
        .filter(tasks::deleted_at.is_null())
        .into_boxed();
    if let Some(repository) = repository {
        task_query = task_query.filter(tasks::repository_id.eq(repository));
    }
    let tasks: Vec<(Uuid, String, String, TaskType, WorkClass)> = task_query
        .order(tasks::short_code.asc())
        .select((
            tasks::column_id,
            tasks::short_code,
            tasks::title,
            tasks::task_type,
            tasks::work_class,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(
        tasks
            .into_iter()
            .map(
                |(column_id, short_code, title, task_type, work_class)| BoardItemRow {
                    column_id,
                    short_code,
                    title,
                    // The Support lane rides in `kind` (KAIROS-T-0077); Planned
                    // stays unmarked as the default lane.
                    kind: match work_class {
                        WorkClass::Support => format!("{task_type} [support lane]"),
                        WorkClass::Planned => task_type.to_string(),
                    },
                },
            ),
    );

    let adrs: Vec<(Option<Uuid>, String, String)> = adrs::table
        .filter(adrs::board_id.eq(board_id))
        .filter(adrs::deleted_at.is_null())
        .order(adrs::short_code.asc())
        .select((adrs::column_id, adrs::short_code, adrs::title))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(
        adrs.into_iter()
            .filter_map(|(column_id, short_code, title)| {
                column_id.map(|column_id| BoardItemRow {
                    column_id,
                    short_code,
                    title,
                    kind: "adr".to_string(),
                })
            }),
    );

    Ok(rows)
}

/// Per-column live item counts for one board.
fn column_item_counts(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<HashMap<Uuid, i64>, ApiError> {
    let mut counts: HashMap<Uuid, i64> = HashMap::new();
    for row in board_item_rows(conn, board_id, None)? {
        *counts.entry(row.column_id).or_default() += 1;
    }
    Ok(counts)
}

/// A live item's id by short code, with the offending field named on
/// failure (422 `VALIDATION`, mirroring the REST relationship endpoints).
fn require_live(conn: &mut PgConnection, short_code: &str, field: &str) -> Result<Uuid, ApiError> {
    resolve_short_code(conn, short_code)?
        .map(|(id, _)| id)
        .ok_or_else(|| {
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
}

/// The item's ancestors via incoming `parent` edges, nearest first
/// (bounded — parent edges are acyclic by construction, this is
/// defense-in-depth).
fn parent_chain(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<ChainRow>, ApiError> {
    let mut chain = Vec::new();
    let mut current = item_id;
    for _ in 0..10 {
        let parent: Option<ChainRow> = sql_query(
            "SELECT d.id, d.short_code, d.title \
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
fn relationship_lines(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError> {
    let relationships = graph::relationships_for(conn, item_id).map_err(ApiError::internal)?;

    let mut groups: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut push =
        |label: &'static str, entry: String| groups.entry(label).or_default().push(entry);

    for neighbor in &relationships.outgoing {
        let entry = format!("{} — {}", neighbor.short_code, neighbor.title);
        match neighbor.relationship {
            RelationshipType::Parent => push("children", entry),
            RelationshipType::Supports => push("supporting docs", entry),
            RelationshipType::Informs => push("informs", entry),
            RelationshipType::Supersedes => push("supersedes", entry),
            RelationshipType::Blocks => push("blocks", entry),
        }
    }
    for neighbor in &relationships.incoming {
        let entry = format!("{} — {}", neighbor.short_code, neighbor.title);
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
            .map(|row| format!("{} ({})", row.short_code, row.title))
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
    slug: &str,
    user: Uuid,
    params: &CreateItemParams,
) -> Result<String, ApiError> {
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
        let (parent_id, parent_type) = resolve_short_code(conn, parent_code)?.ok_or_else(|| {
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
    if let Some(parent_code) = params.parent.as_deref() {
        let (parent_id, _) = resolve_short_code(conn, parent_code)?.ok_or_else(|| {
            ApiError::validation(format!("parent {parent_code:?} does not name a live item"))
        })?;
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
                field: enum_field(&sort.field, "sort.field", "created_at, updated_at, title")?,
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

/// Compact REQ-1.6 rendering: results grouped by type, one line per item
/// (short code + title + a key field), full content via `get_item`.
fn render_search_results(results: &SearchResults) -> String {
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
            out.push_str(&format!("- {} — {}\n", row.short_code, row.title));
        }
    }
    if !results.initiatives.is_empty() {
        out.push_str("\n## initiatives\n");
        for row in &results.initiatives {
            let bucket = if row.is_bucket { " [bucket]" } else { "" };
            out.push_str(&format!("- {} — {}{bucket}\n", row.short_code, row.title));
        }
    }
    if !results.tasks.is_empty() {
        out.push_str("\n## tasks\n");
        for row in &results.tasks {
            out.push_str(&format!(
                "- {} — {} [{}]\n",
                row.short_code, row.title, row.task_type
            ));
        }
    }
    if !results.documents.is_empty() {
        out.push_str("\n## documents\n");
        for row in &results.documents {
            out.push_str(&format!("- {} — {}\n", row.short_code, row.title));
        }
    }
    if !results.adrs.is_empty() {
        out.push_str("\n## adrs\n");
        for row in &results.adrs {
            out.push_str(&format!("- {} — {}\n", row.short_code, row.title));
        }
    }
    out
}
