//! The S-0006 tool surface (KAIROS-T-0026): the 14 tools frozen by
//! S-0006, plus the two repository tools (KAIROS-T-0107), `move_item`
//! (KAIROS-I-0012), `restore_item` (KAIROS-A-0020), `related_work`
//! (KAIROS-T-0191), `propose_edge` (KAIROS-T-0192), `set_repository`
//! (COLLIERY-T-0220), and `add_repository` and `update_repository`
//! (COLLIERY-T-0266); later tasks added more (79 in all, in the profiles
//! of `profiles.rs`, KAIROS-T-0454) — each a
//! thin wrapper over the same `kairos-core`/`kairos-db` services the REST
//! handlers call — through [`crate::blocking::BlockingTenantPool`], never
//! HTTP (A-0011). Contracts:
//!
//! - **REQ-1.1**: every tool runs as the authenticated user under full
//!   ABAC, identical to the REST handlers; errors surface as tool errors
//!   carrying the same stable codes as the S-0005 envelope
//!   ([`tool_error`]). Since COLLIERY-T-0228 the check is one of three,
//!   and each is the function that the REST handler calls: an EDIT asks
//!   [`require_item_edit`], an EDGE asks [`require_edge_write`], and a MOVE
//!   or a create asks [`require_capability`]. The REMOVE of an edge asks
//!   [`crate::api::remove_edge`] (COLLIERY-T-0235): the same rule, but for
//!   the `supports` edge of a document. COLLIERY-T-0269 added no tool: the
//!   owner board of a document is an argument of `create_item` and of
//!   `move_item`, and the relationship `impacts` is a relationship of
//!   `link_items` and of `unlink_items`. Each calls the function that the
//!   REST handler calls ([`crate::api::documents`],
//!   [`crate::api::meta::impacts`]).
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
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use kairos_client::types_org::{
    BOARD_ITEMS_DEFAULT_LIMIT, BOARD_ITEMS_MAX_LIMIT, incomplete_board_note,
};
use kairos_core::search as core_search;
use kairos_core::short_code::ItemType;
use kairos_db::board_items::{BoardItemFilter, board_item_page};
use kairos_db::models::boards::{Board, BoardColumn};
use kairos_db::models::enums::{
    BoardLevel, BucketType, Complexity, DocumentLifecycle, OrgRole, RelationshipType, TaskType,
    TeamType, WorkClass,
};
use kairos_db::models::templates::Template;
use kairos_db::search::{SearchError, SearchResults};
use kairos_db::{GraphError, abac, boards, graph, items, repositories, search};

use crate::api::meta::{apply_metadata_ops, manage_capability, validated_metadata_ops};
use crate::api::{
    Liveness, atomically, map_board_error, map_graph_error, map_item_error, parse_enum,
    require_capability, require_edge_write, require_item_edit, resolve_short_code,
};
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

use super::arguments::Parameters;
use super::service::{KairosMcp, tool_error, tool_text};

// ---------------------------------------------------------------------------
// Tool inputs (frozen names/shapes per S-0006's inventory tables)
// ---------------------------------------------------------------------------
//
// COLLIERY-T-0249, COLLIERY-T-0256: the input of each tool has
// `deny_unknown_fields`, the rule of the routes of the API. A call with an
// argument that the tool does not know is refused, and the refusal is a
// tool error that names the argument: "VALIDATION: The call has the
// argument "column". This tool does not accept that argument. ..."
// (`super::arguments`). The schema of the tool shows
// `additionalProperties: false`. The rule is for the tools that read too:
// an argument that does nothing tells the agent that it did something.

/// `whoami` has no arguments. The type is there for the rule.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct WhoamiParams {}

/// `list_members` (KAIROS-T-0556): the people of the organization, of a
/// team or of a board.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListMembersParams {
    /// Only the members of this team (slug or UUID). Do not send it with
    /// `board`.
    pub team: Option<String>,
    /// Only the members of this board, with their capabilities (slug or
    /// UUID). Do not send it with `team`.
    pub board: Option<String>,
    /// The number of members in the result (default 50, maximum 200).
    pub limit: Option<i64>,
    /// The number of members to skip (default 0). The result gives the
    /// offset of the next part.
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListRepositoriesParams {
    /// Only this team's repositories (slug or UUID).
    pub team: Option<String>,
    /// The number of repositories in the result (default 50, maximum 200).
    pub limit: Option<i64>,
    /// The number of repositories to skip (default 0). The result says
    /// which offset gives the next part of the directory.
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct GetRepositoryParams {
    /// The repository, by slug (e.g. "payments-api") or UUID.
    pub repository: String,
    /// Include the archived documents and ADRs that impact the
    /// repository, each marked `[archived]`. Default false.
    pub include_deleted: Option<bool>,
    /// The rows to skip in "In flight" and in "Not linked to an item"
    /// (each shows 50 at most). Default 0.
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct MyBoardsParams {
    /// Restrict to one board level: strategy | initiative | delivery | adr.
    pub level: Option<String>,
}

/// `board_flow` (KAIROS-T-0372).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct BoardFlowParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// The first day of the range, YYYY-MM-DD. Default: 89 days before
    /// `to`.
    pub from: Option<String>,
    /// The last day of the range, YYYY-MM-DD. Default: today (UTC).
    pub to: Option<String>,
}

/// `list_dispatch_rules` (KAIROS-T-0385).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListDispatchRulesParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
}

/// `create_dispatch_rule` (KAIROS-T-0385).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CreateDispatchRuleParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// When the rule fires: `entered_column` (default: a task enters
    /// `column`) or `unblocked` (the last open blocker of a task is done,
    /// archived or cancelled).
    pub trigger: Option<String>,
    /// The column, by name (e.g. "Todo") or UUID. Required for
    /// `entered_column`; for `unblocked`, the column the task must be in
    /// (none: each column that is not done).
    pub column: Option<String>,
    /// The id of a webhook target of the board. Send it, or send `url`
    /// (and `target_name`) to make a new target.
    pub target_id: Option<String>,
    /// The URL of a new webhook target (http or https).
    pub url: Option<String>,
    /// The name of the new target. Default: "Agent runner".
    pub target_name: Option<String>,
    /// Only tasks of this type: task | bug | tech_debt | support.
    pub task_type: Option<String>,
    /// Only tasks with this metadata: the slug of a definition. Send it with
    /// `metadata_value`.
    pub metadata_slug: Option<String>,
    /// The value of `metadata_slug` that matches (no case).
    pub metadata_value: Option<String>,
    /// Only tasks of this repository: its slug or UUID.
    pub repository: Option<String>,
}

/// `set_agent_triage` (KAIROS-T-0428).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetAgentTriageParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// True: an agent decides the triage of intake. False: an agent
    /// proposes it with a note and a person decides.
    pub agents_decide: bool,
}

/// `set_board_policy` (KAIROS-T-0641).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetBoardPolicyParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// A delivery board only. True: a task moves into a claims column
    /// (Active) only while its parent initiative is active. False: no check.
    pub parent_gate: Option<bool>,
    /// An initiative board only. True: an initiative moves into its Active
    /// column only with an owner, and its owner cannot release it in Active
    /// or Monitoring. False: no check.
    pub owner_gate: Option<bool>,
    /// A delivery board only. True: a task with an open blocker does not
    /// move into a claims column or a done column (BLOCKED). False: the
    /// move goes on with a warning.
    pub blockers_gate: Option<bool>,
}

/// `delete_dispatch_rule` (KAIROS-T-0385).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct DeleteDispatchRuleParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// The id of the rule (from `list_dispatch_rules`).
    pub rule_id: String,
}

/// `remove_webhook_target`, `enable_webhook_target` and
/// `test_webhook_target` (KAIROS-T-0558).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct WebhookTargetParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// The id of the webhook target (from `list_dispatch_rules`).
    pub target_id: String,
}

/// `list_org_read_tokens` (KAIROS-T-0431).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListOrgReadTokensParams {}

/// `set_org_read_token` (KAIROS-T-0431).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetOrgReadTokenParams {
    /// The forge host, for example "github.com" (a port only when it is not
    /// 443).
    pub host: String,
    /// The read token. Kairos encrypts it; no tool gives it back.
    #[schemars(with = "String")]
    pub token: kairos_client::types_auth::Secret,
}

/// `remove_org_read_token` (KAIROS-T-0431).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct RemoveOrgReadTokenParams {
    /// The forge host, for example "github.com".
    pub host: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct BoardItemsParams {
    /// The board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// Restrict to one column, by name (e.g. "In Progress") or UUID.
    pub column: Option<String>,
    /// Restrict the TASKS to those that link to this repository (slug
    /// or UUID). Your repo's queue on a multi-repo team board.
    pub repository: Option<String>,
    /// Include archived cards, marked `[archived]` (default false). For
    /// history, not to pick work.
    pub include_deleted: Option<bool>,
    /// The number of items in the result (default 200, maximum 1000).
    pub limit: Option<i64>,
    /// The number of items to skip (default 0). The result says which
    /// offset gives the next part of the board.
    pub offset: Option<i64>,
    /// Restrict the STRATEGIES and the INITIATIVES to those of this team
    /// (slug or UUID), from tasks or set by hand. Not with `no_team`.
    pub team: Option<String>,
    /// Restrict the STRATEGIES and the INITIATIVES to those with no team.
    /// Not with `team`.
    pub no_team: Option<bool>,
    /// Add the last change and the metadata of each card, e.g.
    /// `{updated 2026-10-08; priority: high}` (default false).
    pub details: Option<bool>,
}

/// Parameters for `list_metadata_definitions` (KAIROS-T-0368).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListMetadataDefinitionsParams {
    /// Only the definitions that apply to this entity type: strategy |
    /// initiative | task | document | adr.
    pub entity_type: Option<String>,
}

/// Parameters for `list_templates` (KAIROS-T-0368).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListTemplatesParams {
    /// Show this one template (by slug) with its full content.
    pub slug: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct GetItemParams {
    /// The item's short code (e.g. "ACME-T-0012").
    pub short_code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct GetHistoryParams {
    /// The item's short code.
    pub short_code: String,
    /// Max rows of each list, the versions and the activity (default 20,
    /// newest first).
    pub limit: Option<i64>,
    /// The versions to skip, newest first (default 0). The activity starts
    /// at its newest row.
    pub offset: Option<i64>,
    /// Return this version's full title + content instead of the list.
    pub version: Option<i32>,
}

/// Parameters for `propose_edge` (KAIROS-T-0192).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ProposeEdgeParams {
    /// The edge's source, by short code. For `parent`, this is the parent.
    pub source: String,
    /// The edge's target, by short code.
    pub target: String,
    /// `parent` or `blocks`. Nothing else may be proposed.
    pub relationship: String,
    /// Why you think so, in your own words. Kept verbatim and shown to whoever
    /// decides, so write what convinced you rather than a label.
    pub why: String,
}

/// Parameters for `related_work` (KAIROS-T-0191).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct RelatedWorkParams {
    /// The item to find related work for, by short code.
    pub short_code: String,
    /// How many proposals to return (default 5, max 10). Deliberately small:
    /// forty related items is a research project, not an answer.
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct SearchFilterParams {
    /// Restrict to entity types: strategy | initiative | task | document | adr.
    pub entity_type: Option<Vec<String>>,
    /// Restrict to items on this board (UUID). Not with `board`.
    pub board_id: Option<String>,
    /// Restrict to items on this board, by slug or UUID. Not with `board_id`.
    pub board: Option<String>,
    /// Restrict to items in this column (UUID). Not with `column`.
    pub column_id: Option<String>,
    /// Restrict to items in this column, by name (with `board`) or UUID
    /// (KAIROS-T-0552). Not with `column_id`.
    pub column: Option<String>,
    /// Restrict to tasks of this team (UUID). Not with `team`.
    pub team_id: Option<String>,
    /// Restrict to tasks of this team, by slug or UUID. Not with `team_id`.
    pub team: Option<String>,
    /// Restrict to the items of this repository (slug or UUID): the tasks
    /// that link to it, and the documents and the ADRs that impact it.
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
    /// Only items last changed (updated_at) strictly after this RFC 3339
    /// instant.
    pub updated_after: Option<String>,
    /// Only items last changed (updated_at) strictly before this RFC 3339
    /// instant. "Untouched for 90 days": now minus 90 days.
    pub updated_before: Option<String>,
    /// Only items with NO value for each of these metadata definitions
    /// (slugs), e.g. ["priority"]. An unknown slug is refused.
    pub metadata_missing: Option<Vec<String>>,
    /// Only the tasks that this person has the claim on, also through an
    /// agent key: `me`, a user id, an email or a user name.
    pub claimed_by: Option<String>,
    /// Only the strategies and the initiatives that this person owns: `me`,
    /// a user id, an email or a user name.
    pub owned_by: Option<String>,
    /// Only the items that this person created: `me`, a user id, an email
    /// or a user name.
    pub created_by: Option<String>,
    /// Include archived (soft-deleted) items (default false). Composes with
    /// everything, `q` and `traverse` included; archived hits come back
    /// marked `[archived]`.
    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct SearchSortParams {
    /// created_at | updated_at | title | relevance (relevance requires `q`).
    pub field: String,
    /// asc | desc.
    pub order: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CreateItemParams {
    /// What to create: strategy | initiative | task | document | adr.
    pub item_type: String,
    /// The item's title.
    pub title: String,
    /// Target board, by slug or UUID; optional when only one board of the
    /// level exists. For a document: the OWNER board, required, and its
    /// prefix gives the document's code.
    pub board: Option<String>,
    /// Parent item's short code: the `parent` edge (for a document or an
    /// ADR, the `supports` edge).
    pub parent: Option<String>,
    /// Template (id or name) — documents only (KAIROS-A-0003).
    pub template: Option<String>,
    /// Initial markdown content (defaults to empty / the template's).
    pub content: Option<String>,
    /// Tasks only: task | bug | tech_debt | support (default task).
    pub task_type: Option<String>,
    /// Tasks only: the repository the task links to (slug or UUID), of any
    /// team. It does not choose the board.
    pub repository: Option<String>,
    /// Tasks only: the lane, planned | support. Default: support for a
    /// support task, else planned; a request is always support.
    pub work_class: Option<String>,
    /// Tasks only: the team that sends the task (slug or UUID). Not the
    /// board's team: the task is a request, whoever sends it. It chooses
    /// the lane and is not kept on the task: name the sending team in the
    /// content.
    pub from_team: Option<String>,
    /// Strategies only: the strategy's hypothesis.
    pub hypothesis: Option<String>,
    /// Initiatives only: t-shirt complexity xs | s | m | l | xl.
    /// `set_complexity` changes it later.
    pub complexity: Option<String>,
    /// Initiatives only: tech_debt | bug | ad_hoc makes a bucket, not a
    /// dated initiative.
    pub bucket_type: Option<String>,
    /// ADRs only: who makes/made the decision.
    pub decision_maker: Option<String>,
    /// ADRs only: the decision's date, YYYY-MM-DD.
    pub decision_date: Option<String>,
    //
    // There is deliberately NO `column` / `column_id` here (KAIROS-T-0178).
    // Every item lands in its board's entry column, and `transition_item` is
    // the only way work moves. Placing an item directly would bypass the
    // transition graph the board exists to enforce — an agent could skip
    // states a human cannot. The absence is documented in the tool description
    // rather than left silent, because an agent reading the schema cannot ask
    // whether a missing field is a restriction or an omission.
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// Parameters for `get_events` (KAIROS-T-0382, KAIROS-T-0550).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct GetEventsParams {
    /// The next page: the id of the oldest unread notification of the last
    /// page. The page has the unread ones older than it, and no read ones.
    #[serde(default)]
    pub before: Option<String>,
}

/// `my_work` has no arguments (KAIROS-T-0489).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct MyWorkParams {}

/// Parameters for `mark_events_read` (KAIROS-T-0382): `ids`, or `all: true`
/// with `up_to` (KAIROS-T-0550).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct MarkEventsReadParams {
    /// The ids of your notifications, as `get_events` gives them.
    pub ids: Option<Vec<String>>,
    /// `true`: each of your unread notifications up to `up_to`.
    pub all: Option<bool>,
    /// With `all: true`, required: the id of the newest notification that
    /// you read (`get_events` gives it first). Kairos marks it and each
    /// older unread one, and no newer one.
    pub up_to: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct AppendNoteParams {
    /// The item's short code.
    pub short_code: String,
    /// The markdown text of the note.
    pub text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct TransitionItemParams {
    /// The item's short code.
    pub short_code: String,
    /// Target column, by name (e.g. "In Progress") or UUID, on the item's
    /// own board.
    pub to_column: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct MoveItemParams {
    /// The short code of the task (e.g. "ACME-T-0012") or of the document
    /// (e.g. "ACME-D-0004").
    pub short_code: String,
    /// The board to move it to, by slug (e.g. "web-delivery") or UUID. A
    /// task must have it: a delivery board, and the task lands in the
    /// entry column of that board. For a document it is the new owner
    /// board, of each level. A document must have it too: the owner board
    /// of a document cannot be removed.
    pub to_board: Option<String>,
    /// Give the item the next code of the target board (default false).
    /// The old code is retired: a read with it finds the item. Each
    /// reference to the old code in the text of the items changes to the
    /// new code, one time. A code in a URL or a path does not change. A
    /// document needs `to_board` for a rename.
    #[serde(default)]
    pub rename: bool,
}

/// Parameters for `set_repository` (COLLIERY-T-0220).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetRepositoryParams {
    /// The short code of the task (e.g. "ACME-T-0012").
    pub short_code: String,
    /// The repository to link the task to, by slug (e.g. "payments-api") or
    /// UUID. It can be any live repository, of any team. To clear the link,
    /// omit this argument, or send null or an empty string.
    pub repository: Option<String>,
}

/// Parameters for `set_parent` (KAIROS-T-0572).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetParentParams {
    /// The short code of the initiative or the task (e.g. "ACME-T-0012").
    pub short_code: String,
    /// The short code of the new parent: a strategy for an initiative, an
    /// initiative for a task. Send null to remove the parent. Required: a
    /// call without it changes nothing and is refused.
    #[serde(default, deserialize_with = "present_value")]
    pub parent: Option<Option<String>>,
}

/// `Some(value)` for an argument that is in the call, also when it is null:
/// absent is `None` (the `#[serde(default)]`), null is `Some(None)`.
fn present_value<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

/// Parameters for `set_work_class` (KAIROS-T-0433).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetWorkClassParams {
    /// The short code of the task (e.g. "ACME-T-0012").
    pub short_code: String,
    /// The lane: planned | support.
    pub work_class: String,
}

/// Parameters for `set_complexity`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetComplexityParams {
    /// The short code of the initiative (e.g. "ACME-I-0012").
    pub short_code: String,
    /// The complexity: xs | s | m | l | xl, or none to clear it. Required:
    /// a call that leaves it out clears nothing.
    pub complexity: String,
}

/// Parameters for `set_lifecycle` (KAIROS-T-0555).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetLifecycleParams {
    /// The short code of the document (e.g. "ACME-D-0012").
    pub short_code: String,
    /// The lifecycle: draft | review | published | archived.
    pub lifecycle: String,
}

/// Parameters for `promote_note` (KAIROS-T-0505).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct PromoteNoteParams {
    /// The short code of the item that has the note (e.g. "ACME-T-0012").
    pub short_code: String,
    /// The id of the note, as `get_item` lists it: "n-3".
    pub note_id: String,
    /// What the note becomes: adr | document.
    pub kind: String,
    /// The board that gets the new item (slug or UUID): an ADR board for
    /// an adr, the owner board for a document.
    pub board: String,
    /// The title of the new item. Default: the first line of the note.
    #[serde(default)]
    pub title: Option<String>,
}

/// `None` when the field is absent, `Some(None)` for `null`, `Some(Some(v))`
/// for a value (KAIROS-T-0533): the three states of a patch field.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Parameters for `set_state` (KAIROS-T-0506).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetStateParams {
    /// The short code of the task (e.g. "ACME-T-0012").
    pub short_code: String,
    /// The branch of the work. Not sent: kept. `null`: cleared.
    #[serde(default, deserialize_with = "present")]
    pub branch: Option<Option<String>>,
    /// The last commit (a hash or a short hash). Not sent: kept. `null`: cleared.
    #[serde(default, deserialize_with = "present")]
    pub commit: Option<Option<String>>,
    /// True when the last commit is pushed. Not sent: kept.
    #[serde(default)]
    pub pushed: Option<bool>,
    /// The URL of the PR. Not sent: kept. `null`: cleared.
    #[serde(default, deserialize_with = "present")]
    pub pr_url: Option<Option<String>>,
    /// The `updated_at` of the state you read (RFC 3339). A state that
    /// changed since refuses the write with CONFLICT and the current state.
    #[serde(default)]
    pub expected_updated_at: Option<String>,
    /// What comes next. Not sent: kept. `null`: cleared.
    #[serde(default, deserialize_with = "present")]
    pub next_step: Option<Option<String>>,
}

/// Parameters for `set_team` and `clear_team` (KAIROS-T-0321).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ItemTeamParams {
    /// The short code of the initiative or the strategy (e.g.
    /// "ACME-I-0012").
    pub short_code: String,
    /// The team, by slug (e.g. "payments") or UUID.
    pub team: String,
}

/// Parameters for `add_repository` (COLLIERY-T-0266). The names are those
/// of the body of `POST /api/repositories`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct AddRepositoryParams {
    /// The slug of the repository in Kairos (e.g. "payments-api"):
    /// lowercase letters, digits and hyphens, 2 to 63 characters. Omit it
    /// and Kairos makes the slug from `repo_full_name`
    /// ("acme/payments-api" gives "acme-payments-api").
    pub slug: Option<String>,
    /// github | gitlab | other.
    pub forge: String,
    /// The name on the forge: "owner/repo" (github), "group/.../project"
    /// (gitlab); no space, no ".git".
    pub repo_full_name: String,
    /// The URL for a browser: absolute http or https, no credentials.
    pub repo_url: String,
    /// The default branch, set by hand. Omit it to follow the forge's
    /// default branch.
    pub default_branch: Option<String>,
    /// The ONE owner team (slug or UUID).
    pub team: String,
    /// How to work in the repository, for the agents that read it with
    /// `get_repository`: the task runner, the tests, the rules for a
    /// branch.
    pub description: Option<String>,
}

/// Parameters for `update_repository` (COLLIERY-T-0266). KAIROS-T-0443 added
/// `slug` and `team`: see the description of the tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct UpdateRepositoryParams {
    /// The repository, by slug (e.g. "payments-api") or UUID.
    pub repository: String,
    /// The new text on how to work in the repository. It replaces the
    /// full text. An empty string removes the text.
    pub description: Option<String>,
    /// The new default branch: a branch name that git accepts. It is set
    /// by hand: the code index builder indexes it, whatever the forge says.
    pub default_branch: Option<String>,
    /// "forge" clears the branch set by hand: the code index builder
    /// follows the default branch of the forge again. "hand" keeps the
    /// current branch by hand. Do not send "forge" with `default_branch`.
    pub default_branch_source: Option<String>,
    /// The new URL of the repository for a browser: an absolute http or
    /// https URL, with no user name and no password.
    pub repo_url: Option<String>,
    /// The code index builder for the repository: "on" or "off" ("off"
    /// cancels the waiting builds).
    pub code_index_build: Option<String>,
    /// The new slug (KAIROS-T-0443). Each checkout keeps the slug in
    /// `.claude/kairos.local.md`: a new slug breaks that reference.
    pub slug: Option<String>,
    /// The new owner team, by slug or UUID (KAIROS-T-0443). You need the
    /// right on the old team and on the new team.
    pub team: Option<String>,
    /// The forge filters (KAIROS-T-0479), org admin only: per field, an
    /// `include` and an `exclude` Rust regular expression (512 bytes at
    /// most). Fields: branch_name, pr_head_branch, pr_title, pr_author,
    /// issue_title, issue_author. Send only what changes, for example
    /// `{"pr_author": {"exclude": "^dependabot\\[bot\\]$"}}`.
    pub forge_filters: Option<McpForgeFilters>,
    /// How often the repository takes its open issues with no webhook
    /// (KAIROS-T-0670), org admin only: "off", or a number with a unit
    /// ("90s", "15m", "1h", "1d"). The shortest is 1 minute. The longest is
    /// 30 days. The repository needs a read token.
    pub issue_import_interval: Option<String>,
}

/// One field of `forge_filters` (KAIROS-T-0479).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct McpForgeFilterRule {
    /// The include pattern. Default `.*`.
    pub include: Option<String>,
    /// The exclude pattern. Default empty: exclude nothing.
    pub exclude: Option<String>,
}

/// The `forge_filters` argument of `update_repository` (KAIROS-T-0479).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct McpForgeFilters {
    pub branch_name: Option<McpForgeFilterRule>,
    pub pr_head_branch: Option<McpForgeFilterRule>,
    pub pr_title: Option<McpForgeFilterRule>,
    pub pr_author: Option<McpForgeFilterRule>,
    pub issue_title: Option<McpForgeFilterRule>,
    pub issue_author: Option<McpForgeFilterRule>,
}

impl From<McpForgeFilters> for kairos_client::types_repositories::ForgeFilters {
    fn from(value: McpForgeFilters) -> Self {
        let rule = |r: Option<McpForgeFilterRule>| {
            r.map(|r| kairos_client::types_repositories::ForgeFilterRule {
                include: r.include,
                exclude: r.exclude,
            })
        };
        Self {
            branch_name: rule(value.branch_name),
            pr_head_branch: rule(value.pr_head_branch),
            pr_title: rule(value.pr_title),
            pr_author: rule(value.pr_author),
            issue_title: rule(value.issue_title),
            issue_author: rule(value.issue_author),
        }
    }
}

/// Parameters for `rebuild_code_index` (KAIROS-T-0332).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct RebuildCodeIndexParams {
    /// The repository, by slug (e.g. "payments-api") or UUID.
    pub repository: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct HandOffItemParams {
    /// The short code of a task in Active, or of a strategy or an
    /// initiative.
    pub short_code: String,
    /// The person who gets the claim (a task) or who becomes the owner (a
    /// strategy or an initiative): a user id, an email or a user name of a
    /// person of the organization, or `me`. A new owner must be a member
    /// of the board of the item. A new claim holder must hold
    /// `transition_items` on the board of the task.
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ReleaseItemParams {
    /// The short code of a task in Active, or of a strategy or an
    /// initiative.
    pub short_code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ClaimItemParams {
    /// The short code of a task in Active with no claim, or of a strategy
    /// or an initiative with no owner.
    pub short_code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct LinkItemsParams {
    /// Source item's short code (edge direction: source -> target).
    pub source: String,
    /// Target item's short code. For `impacts`: the slug or the UUID of a
    /// repository. For `external`: a URI (http, https or mailto).
    pub target: String,
    /// parent | supports | informs | supersedes | blocks | impacts | external.
    pub relationship: String,
    /// For `external` only: the label of the reference, at most 200
    /// characters ("RFC: zen as the policy engine, accepted 2026-10-05").
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct UnlinkItemsParams {
    /// Source item's short code.
    pub source: String,
    /// Target item's short code. For `impacts`: the slug or the UUID of a
    /// repository. For `external`: the URI of the reference.
    pub target: String,
    /// parent | supports | informs | supersedes | blocks | impacts | external.
    pub relationship: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct RestoreItemParams {
    /// The archived item's short code.
    pub short_code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct DeleteItemParams {
    /// The item's short code.
    pub short_code: String,
    /// True to archive: the call archives the item AND each descendant that
    /// a parent edge connects to it. The response lists the cascade.
    pub confirm: Option<bool>,
    /// True to see the cascade first (KAIROS-T-0558): the answer lists what
    /// an archive by you would take and leave. Nothing changes. Not with
    /// `confirm: true`.
    pub preview: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CancelItemParams {
    /// The task's short code. The task must not be in a done column.
    pub short_code: String,
    /// Why the task is cancelled. Must not be empty.
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ReopenItemParams {
    /// The task's short code. The task must be in a done column.
    pub short_code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct PurgeTaskParams {
    /// The task's current short code (a retired code is not followed).
    pub short_code: String,
    /// Must be true: the call deletes the task for good. You cannot undo
    /// it.
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ArchiveCompletedParams {
    /// The delivery board, by slug (e.g. "platform-delivery") or UUID.
    pub board: String,
    /// Must be true: the call archives each task in the done columns of
    /// the board.
    pub confirm: bool,
    /// Archive only the done tasks that entered their done column at
    /// least this many days ago (1 to 36500). Omit it to archive each
    /// done task.
    #[serde(default)]
    pub older_than_days: Option<i64>,
}

/// Parameters for `create_team` (KAIROS-T-0337). The names are those of the
/// body of `POST /api/teams`, and `adr_board`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CreateTeamParams {
    /// The name of the team (e.g. "Home operations").
    pub name: String,
    /// The slug of the team (e.g. "home-ops"): it must match
    /// ^[a-z][a-z0-9_-]{1,62}$. The delivery board gets the slug
    /// "{slug}-delivery", and the ADR board (with `adr_board`) "{slug}-adrs".
    pub slug: String,
    /// The short-code prefix of the boards of the team (e.g. "OPS"): a
    /// capital letter, then 1 to 9 capital letters or digits. It does not
    /// change later. Tasks get OPS-T-0001, ADRs OPS-A-0001.
    pub code_prefix: String,
    /// stream_aligned | platform | enabling | complicated_subsystem. The
    /// default is stream_aligned.
    pub team_type: Option<String>,
    /// Also make the ADR board of the team. The default is false: the
    /// team gets its delivery board only, as with REST and the CLI
    /// (KAIROS-T-0448).
    pub adr_board: Option<bool>,
}

/// Parameters for `create_board` (KAIROS-T-0337). The names are those of
/// the body of `POST /api/boards`, with `team` for `team_id`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CreateBoardParams {
    /// The name of the board.
    pub name: String,
    /// The slug of the board: it must match ^[a-z][a-z0-9_-]{1,62}$.
    pub slug: String,
    /// strategy | initiative | delivery | adr.
    pub board_level: String,
    /// The owner team (slug or UUID). A delivery board must have one. A
    /// strategy or initiative board with no team gets its own empty team,
    /// `<level>-<slug>`. An ADR board with a team is the ADR board of that
    /// team.
    pub team: Option<String>,
    /// The short-code prefix of the board: a capital letter, then 1 to 9
    /// capital letters or digits. The ADR board of a team takes the prefix
    /// of the team.
    pub code_prefix: String,
}

/// Parameters for `add_team_member` (KAIROS-T-0337).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct AddTeamMemberParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// The member of the organization: a user id, an email, a user name, or
    /// the name of a service account.
    pub user: String,
}

/// Parameters for `grant_capability` (KAIROS-T-0337).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct GrantCapabilityParams {
    /// The member of the organization: a user id, an email, a user name, or
    /// the name of a service account.
    pub user: String,
    /// A board capability (manage_tasks, manage_documents, manage_adrs,
    /// manage_initiatives, manage_strategies, transition_items,
    /// configure_boards, administer_members, pages, purge, member,
    /// approve_proposals, or a glob: *, manage_*, configure_*,
    /// transition_*), or manage_structure.
    pub capability: String,
    /// The board, by slug or UUID. A board capability needs it. For
    /// manage_structure, omit it for a grant at the organization.
    pub board: Option<String>,
}

/// Parameters for `update_team` (KAIROS-T-0443). The names are those of the
/// body of `PATCH /api/teams/{id}`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct UpdateTeamParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// The new name.
    pub name: Option<String>,
    /// The new slug.
    pub slug: Option<String>,
    /// The new type: stream_aligned | platform | enabling |
    /// complicated_subsystem.
    pub team_type: Option<String>,
    /// The published site of the team: true (on) or false (off).
    pub site_enabled: Option<bool>,
}

/// Parameters for `team_pages` (KAIROS-T-0434).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct TeamPagesParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// One page or folder to read in full: its path of slugs from the root
    /// (e.g. "documentation/how-to-guides/deploy") or its UUID. Omit it to
    /// get the tree.
    pub page: Option<String>,
}

/// Parameters for `create_team_page` (KAIROS-T-0434).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CreateTeamPageParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// page | folder. A folder holds folders and pages; a page holds nothing.
    pub kind: String,
    /// The title.
    pub title: String,
    /// The slug, unique among its siblings. The default comes from the title.
    pub slug: Option<String>,
    /// The folder to create it in: a path of slugs or a UUID. Omit it for
    /// the root.
    pub parent: Option<String>,
    /// The markdown content of a page. A folder has none.
    pub content: Option<String>,
    /// The position among its siblings. The default is after the last one.
    pub position: Option<i32>,
}

/// Parameters for `edit_team_page` (KAIROS-T-0434).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct EditTeamPageParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// The page: a path of slugs (e.g. "charter") or a UUID.
    pub page: String,
    /// The new markdown content (the whole page).
    pub content: String,
    /// The version that you read. A newer version refuses the edit.
    pub version: i32,
    /// A new title.
    pub title: Option<String>,
}

/// Parameters for `move_team_page` (KAIROS-T-0434).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct MoveTeamPageParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// The page or folder: a path of slugs or a UUID. Or the short code of a
    /// document or an ADR of the team, to place it in the tree.
    pub page: String,
    /// The folder to move it to: a path of slugs or a UUID.
    pub parent: Option<String>,
    /// true moves it to the root. Not with `parent`.
    pub to_root: Option<bool>,
    /// A new slug.
    pub slug: Option<String>,
    /// A new position among its siblings.
    pub position: Option<i32>,
}

/// Parameters for `delete_team_page` (KAIROS-T-0434).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct DeleteTeamPageParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// The page or the folder: a path of slugs or a UUID.
    pub page: String,
}

/// Parameters for `remove_team_member` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct RemoveTeamMemberParams {
    /// The team, by slug or UUID.
    pub team: String,
    /// The member: a user id, an email, a user name, or the name of a
    /// service account.
    pub user: String,
}

/// Parameters for `set_board_capabilities` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetBoardCapabilitiesParams {
    /// The board, by slug or UUID.
    pub board: String,
    /// The member of the organization: a user id, an email, a user name, or
    /// the name of a service account.
    pub user: String,
    /// The full set of board capabilities of the user on the board. It
    /// replaces the set. An empty list revokes each capability.
    pub capabilities: Vec<String>,
}

/// Parameters for `add_column` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct AddColumnParams {
    /// The board, by slug or UUID.
    pub board: String,
    /// The name of the new column.
    pub name: String,
    /// The position of the new column, from 0 (the first). Default: the
    /// last.
    pub position: Option<i32>,
}

/// Parameters for `update_column` (KAIROS-T-0443). The names are those of
/// the body of `PATCH /api/boards/{id}/columns/{col_id}`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct UpdateColumnParams {
    /// The board, by slug or UUID.
    pub board: String,
    /// The column, by name or UUID.
    pub column: String,
    /// The new name.
    pub name: Option<String>,
    /// The new position, from 0 (the first).
    pub position: Option<i32>,
    /// True: the column is a done column.
    pub is_done: Option<bool>,
    /// True: a task in the column has a claim.
    pub claims: Option<bool>,
    /// True: a task with a claim keeps it in the column, and gets none
    /// there (Blocked, KAIROS-T-0529).
    pub keeps_claims: Option<bool>,
    /// The WIP limit, 1 or more.
    pub wip_limit: Option<i32>,
    /// True: remove the WIP limit. Not with `wip_limit`.
    #[serde(default)]
    pub no_wip_limit: bool,
}

/// Parameters for `remove_column` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct RemoveColumnParams {
    /// The board, by slug or UUID.
    pub board: String,
    /// The column, by name or UUID. It must hold no live item.
    pub column: String,
}

/// Parameters for `set_transition` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetTransitionParams {
    /// The board, by slug or UUID.
    pub board: String,
    /// The column the move starts in, by name or UUID.
    pub from: String,
    /// The column the move ends in, by name or UUID.
    pub to: String,
    /// True: allow the move. False: stop it.
    pub allowed: bool,
}

/// Parameters for `delete_repository` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct DeleteRepositoryParams {
    /// The repository, by slug or UUID.
    pub repository: String,
}

/// Parameters for `save_metadata_definition` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SaveMetadataDefinitionParams {
    /// The definition to change, by slug or UUID. Omit it to make a new
    /// definition.
    pub definition: Option<String>,
    /// The name. A new definition needs it.
    pub name: Option<String>,
    /// The slug. A new definition needs it.
    pub slug: Option<String>,
    /// The type of a new definition: text | number | date | enum | boolean.
    /// It does not change later.
    pub field_type: Option<String>,
    /// The values of an enum, in their order (the scale). It replaces the
    /// list.
    pub enum_options: Option<Vec<String>>,
    /// The entity types that the definition applies to (strategy,
    /// initiative, task, document, adr). Empty: each type. It replaces the
    /// list.
    pub entity_types: Option<Vec<String>>,
}

/// Parameters for `delete_metadata_definition` and `delete_template`
/// (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct DeleteMetaParams {
    /// The definition or the template, by slug or UUID.
    pub reference: String,
}

/// A metadata field of a template (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct TemplateFieldParams {
    /// The slug of a metadata definition.
    pub definition_slug: String,
    /// The default value.
    pub default_value: Option<String>,
    /// True: an item of the template must have a value.
    #[serde(default)]
    pub required: bool,
}

/// Parameters for `save_template` (KAIROS-T-0443).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SaveTemplateParams {
    /// The template to change, by slug or UUID. Omit it to make a new
    /// template.
    pub template: Option<String>,
    /// The name. A new template needs it.
    pub name: Option<String>,
    /// The slug. A new template needs it.
    pub slug: Option<String>,
    /// The markdown content of a new item of the template.
    pub content: Option<String>,
    /// The metadata fields. It replaces the list.
    pub metadata: Option<Vec<TemplateFieldParams>>,
}

// ---------------------------------------------------------------------------
// The tools (79; the list of each profile is asserted in tests/mcp.rs)
// ---------------------------------------------------------------------------

#[tool_router(vis = "pub(super)")]
impl KairosMcp {
    /// Run one closure on a tenant-pinned sync connection (the T-0018
    /// blocking bridge); service errors become S-0006 tool errors.
    ///
    /// KAIROS-T-0612: after the closure succeeds, the notification
    /// ride-along of the call is read on the same connection, in the same
    /// checkout (`super::service::RideAlong`).
    async fn run_tool<F>(&self, tenant: &TenantContext, f: F) -> Result<CallToolResult, ErrorData>
    where
        F: FnOnce(&mut PgConnection) -> Result<String, ApiError> + Send + 'static,
    {
        let ride_along = super::service::RideAlong::current();
        let run = move |conn: &mut PgConnection| {
            let text = f(conn)?;
            if let Some(ride_along) = ride_along {
                ride_along.fill(conn);
            }
            Ok(text)
        };
        match self.state.blocking.run(&tenant.slug, run).await {
            Ok(text) => Ok(tool_text(text)),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Who am I in this Kairos organization: identity (display name, email, user name), org role, teams, my teams' repositories, the boards where I hold write capabilities, the implicit capabilities every member has, and the scope of the agent key in use (KAIROS-T-0536). Call this first to establish working context."
    )]
    pub async fn whoami(
        &self,
        Parameters(WhoamiParams {}): Parameters<WhoamiParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let is_admin = tenant.role == OrgRole::Admin;
        let (org_slug, role) = (tenant.slug.clone(), tenant.role.as_str());
        let (display_name, email, user_id) = (auth.display_name, auth.email, auth.user_id);
        let (agent_key, key_scope) = (auth.agent_key, auth.key_scope);
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

            let grants: Vec<(String, String, BoardLevel, String)> = caps::table
                .inner_join(boards::table)
                .filter(caps::user_id.eq(user_id))
                .filter(boards::deleted_at.is_null())
                .order((boards::slug.asc(), caps::capability.asc()))
                .select((
                    boards::slug,
                    boards::name,
                    boards::board_level,
                    caps::capability,
                ))
                .load(conn)
                .map_err(ApiError::internal)?;

            // KAIROS-T-0556: the user name is what `hand_off_item`, the
            // `claimed_by` filter of `search` and an `@mention` take.
            let user_name: String = kairos_db::schema::users::table
                .filter(kairos_db::schema::users::id.eq(user_id))
                .select(kairos_db::schema::users::user_name)
                .first(conn)
                .map_err(ApiError::internal)?;

            let mut out = format!("# {display_name} <{email}>\n");
            out.push_str(&format!("- user name: {user_name}\n"));
            out.push_str(&format!("- organization: {org_slug} (role: {role})\n"));
            out.push_str("\n## Teams\n");
            if member_teams.is_empty() {
                out.push_str("(none)\n");
            }
            for (_, slug, name, team_type) in member_teams {
                out.push_str(&format!("- {slug} — {name} ({team_type})\n"));
            }
            // KAIROS-T-0107: the repositories my teams own. A task on any
            // board may link to any of them (COLLIERY-A-0023).
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
                    out.push_str("(none. Use list_repositories to see the full directory.)\n");
                }
                for (slug, full_name, team) in mine {
                    out.push_str(&format!("- {slug} — {full_name} (owner: {team})\n"));
                }
            }
            out.push_str("\n## Board capabilities\n");
            if is_admin {
                out.push_str("- organization admin: you have each capability on each board.\n");
            }
            let mut by_board: BTreeMap<(String, String), (BoardLevel, Vec<String>)> =
                BTreeMap::new();
            for (slug, name, level, capability) in grants {
                by_board
                    .entry((slug, name))
                    .or_insert_with(|| (level, Vec::new()))
                    .1
                    .push(capability);
            }
            // COLLIERY-T-3108: the capabilities the team rule gives on each
            // board of my teams, with no grant (KAIROS-T-0072). The ADR
            // board of a team adds `manage_adrs`.
            let team_boards: Vec<(String, String, BoardLevel, String)> = boards::table
                .inner_join(teams::table)
                .filter(boards::team_id.eq_any(&my_team_ids))
                .filter(boards::deleted_at.is_null())
                .order(boards::slug.asc())
                .select((boards::slug, boards::name, boards::board_level, teams::slug))
                .load(conn)
                .map_err(ApiError::internal)?;
            if by_board.is_empty() && team_boards.is_empty() && !is_admin {
                out.push_str("(none. Each member can read. A write needs a capability.)\n");
            }
            for ((slug, name), (level, capabilities)) in by_board {
                out.push_str(&format!("- {slug} ({name}): {}\n", capabilities.join(", ")));
                // KAIROS-T-0410: the `member` grant on a strategy or
                // initiative board (KAIROS-T-0541) gives the edit and the
                // move of each item on it. No other grant does.
                let member = member_capabilities(level, &capabilities);
                if !member.is_empty() {
                    out.push_str(&format!(
                        "- {slug} ({name}): {} (board member, no grant)\n",
                        member.join(", ")
                    ));
                }
            }
            for (slug, name, level, team) in team_boards {
                out.push_str(&format!(
                    "- {slug} ({name}): {} (team {team}, no grant)\n",
                    team_capabilities(level).join(", ")
                ));
            }
            // KAIROS-T-0105: computed capabilities every member holds.
            out.push_str(&format!(
                "- implicit (every member, every delivery board): {}\n  \
                 file_backlog = you can send a request to each team. The request is a \
                 task in the entry column of its delivery board, in the support lane.\n",
                kairos_core::abac::COMPUTED_CAPABILITIES.join(", ")
            ));
            // KAIROS-T-0337: the grants of `manage_structure`.
            let structure: Vec<(Option<Uuid>, Option<String>)> = {
                use kairos_db::schema::structure_grants as sg;
                sg::table
                    .left_join(boards::table)
                    .filter(sg::user_id.eq(user_id))
                    .filter(sg::board_id.is_null().or(boards::deleted_at.is_null()))
                    .order(boards::slug.asc().nulls_first())
                    .select((sg::board_id, boards::slug.nullable()))
                    .load(conn)
                    .map_err(ApiError::internal)?
            };
            out.push_str("\n## Structure (manage_structure)\n");
            if is_admin {
                out.push_str("- organization admin: you can change each team, board and grant.\n");
            }
            if structure.is_empty() && !is_admin {
                out.push_str("(none. An organization admin grants it.)\n");
            }
            for (board_id, board_slug) in structure {
                match (board_id, board_slug) {
                    (None, _) => out.push_str(
                        "- on the organization: teams, team members, boards, the columns, \
                         transitions, members and grants of each board, repositories\n",
                    ),
                    (Some(_), Some(board)) => out.push_str(&format!(
                        "- on the board {board}: its columns, transitions, members and grants\n"
                    )),
                    (Some(board_id), None) => out.push_str(&format!(
                        "- on the board {board_id}: its columns, transitions, members and grants\n"
                    )),
                }
            }
            // KAIROS-T-0536: the agent key in use, and its scope.
            if agent_key.is_some() {
                out.push_str(&key_scope_section(conn, key_scope.as_deref())?);
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "The members of the organization, of a team or of a board. Each line gives the user name, the display name, the email and the kind: person, service account or system actor. With no argument, the tool lists the organization and the role of each member. `team` (slug or UUID) lists the members of the team. `board` (slug or UUID) lists the members of the board and their capabilities: by a grant, by membership of the board, or by the team of the board. Send `team` or `board`, not both. Send the user name to `hand_off_item` or to the `claimed_by` filter of `search`, or write `@<user name>` to mention a person. At most 50 by default (`limit` up to 200, `offset`); a partial result says so and gives the next `offset`."
    )]
    pub async fn list_members(
        &self,
        Parameters(params): Parameters<ListMembersParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        let (org_id, org_slug) = (tenant.org_id, tenant.slug.clone());
        self.run_tool(&tenant, move |conn| {
            let (limit, offset) = crate::api::clamp_pagination(&kairos_client::types::Pagination {
                limit: params.limit,
                offset: params.offset,
            });
            let (heading, members) = match (params.team.as_deref(), params.board.as_deref()) {
                (Some(_), Some(_)) => {
                    return Err(ApiError::validation(
                        "Send `team` or `board`, not both. `team` lists the members of a team. \
                         `board` lists the members of a board and their capabilities.",
                    )
                    .with_details(json!({ "arguments": ["team", "board"] })));
                }
                (Some(reference), None) => {
                    let team = team_by_ref(conn, reference)?;
                    (
                        format!("# Members of the team {} ({})\n", team.slug, team.name),
                        team_member_lines(conn, team.id)?,
                    )
                }
                (None, Some(reference)) => {
                    let board = board_by_ref(conn, reference)?;
                    (
                        format!(
                            "# Members of the board {} ({})\nAn organization admin holds each \
                             capability on each board. `list_members` with no argument gives \
                             the role of each member.\n",
                            board.slug, board.name
                        ),
                        board_member_lines(conn, &board)?,
                    )
                }
                (None, None) => (
                    format!("# Members of the organization {org_slug}\n"),
                    org_member_lines(conn, org_id)?,
                ),
            };
            Ok(render_members(&heading, members, limit, offset))
        })
        .await
    }

    #[tool(
        description = "The repositories of the organization: slug, forge and full name, the owner team and its delivery board, the open task count, and whether webhooks are connected. Use it to find the repository to link a task to, or who owns one. Optional `team` (slug or UUID). At most 50 by default (`limit` up to 200, `offset`); a partial result says so and gives the next `offset`."
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
            // KAIROS-T-0268: the same page as `GET /api/repositories`.
            let (limit, offset) = crate::api::clamp_pagination(&kairos_client::types::Pagination {
                limit: params.limit,
                offset: params.offset,
            });
            let page = crate::api::org::repositories::render_page(conn, rows, limit, offset)?;
            let rendered = page.items;
            let mut out = String::from("# Repositories\n");
            let shown = rendered.len() as i64;
            if offset > 0 || offset + shown < page.total {
                out.push_str(&format!(
                    "Repositories in the directory: {}. Shown: {shown} (limit {limit}, offset \
                     {offset}).",
                    page.total
                ));
                if offset + shown < page.total {
                    out.push_str(&format!(
                        " Call again with offset {} for the next part.",
                        offset + shown
                    ));
                }
                out.push('\n');
            }
            if page.total == 0 {
                out.push_str(
                    "(none. An organization admin or a member of the owner team adds a \
                     repository with add_repository.)\n",
                );
            }
            for repo in rendered {
                out.push_str(&format!(
                    "- {} — {} {} · owner: {} · owner's board: {} · open tasks: {}{}\n",
                    repo.slug,
                    repo.forge,
                    repo.repo_full_name,
                    repo.team.slug,
                    repo.delivery_board_slug
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
        description = "One repository: owner team and its delivery board, open task count, default branch and where it comes from, the team's `description` of how to work in it (read it before working in an unfamiliar repository), the documents and ADRs that impact it, the other ADRs of the owner team (newest first, 50 at most, none superseded; read each with `get_item` before planning), its open branches and pull requests with their items and those not linked to an item (50 of each at most; `offset` gives the next part), the forge filters, and the lines read token, write-back, code index build and code index breaker (a tripped breaker stops builds until `rebuild_code_index`). `repository` is a slug or UUID; `include_deleted: true` adds archived documents and ADRs."
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
            let team_id = repo.team_id;
            let rendered = crate::api::org::repositories::render(conn, vec![repo])?.remove(0);
            // COLLIERY-T-0269: the same read as `GET /api/repositories/{slug}`.
            let include_deleted = params.include_deleted.unwrap_or(false);
            let impacted_by =
                crate::api::org::repositories::impacted_by(conn, repo_id, include_deleted)?;
            // KAIROS-T-0599: the ADRs of the owner team that no `impacts`
            // link names. Most ADRs have no link, and the grounding rule
            // reads this tool, so it must find them here.
            let (team_adrs, team_adrs_total) =
                crate::api::org::repositories::owner_team_adrs(conn, team_id, include_deleted)?;
            // Slugs everywhere (KAIROS-T-0123): the delivery board is what
            // the skills pass to `board_items`, so print it the way they
            // will use it. It is the board of the OWNER, and the line says
            // so: a task that links here can be on any team's board
            // (COLLIERY-T-0219, COLLIERY-A-0023).
            let delivery_board = match rendered.delivery_board_id.as_deref() {
                Some(id) => board_by_ref(conn, id)
                    .map(|b| format!("{} ({})", b.slug, b.name))
                    .unwrap_or_else(|_| id.to_string()),
                None => "(none)".to_string(),
            };
            // KAIROS-T-0561: one page of each open list, with its count.
            let offset = params.offset.unwrap_or(0).max(0);
            let (in_flight, in_flight_total) = graph::repository_link_page(
                conn,
                repo_id,
                &["open", "draft"],
                REPOSITORY_LIST_LIMIT,
                offset,
            )
            .map_err(ApiError::internal)?;
            let mut out = format!(
                "# Repository {} — {} {}\n- url: {}\n- default branch: {} ({})\n- owner team: {} ({})\n- owner's delivery board: {}\n- open tasks (all boards): {}\n- webhooks: {}\n- read token: {}\n- write-back: {}\n- code index build: {}\n- code index breaker: {}\n- code index summaries: {}\n- forge filters: {}\n- issue import: {}\n",
                rendered.slug,
                rendered.forge,
                rendered.repo_full_name,
                rendered.repo_url,
                rendered.default_branch,
                // KAIROS-T-0430: where the indexed branch comes from.
                rendered.default_branch_summary(),
                rendered.team.slug,
                rendered.team.name,
                delivery_board,
                rendered.open_tasks,
                if rendered.has_webhook { "connected" } else { "not connected" },
                // COLLIERY-T-3105: the status only. No tool sets or gives
                // the token.
                rendered.credential.summary(),
                // KAIROS-T-0386: the GitHub write-back, on or off. No tool
                // sets or gives the write token.
                rendered.write_back.summary(),
                // KAIROS-T-0398: on or off; `update_repository` sets it.
                rendered.code_index_build.as_str(),
                // KAIROS-T-0399: a build job that stopped its server trips
                // it; a requested build resets it.
                rendered.code_index_breaker.summary(),
                // KAIROS-T-0340: where the summaries of the checkout come
                // from. On `hosted`, the CLI makes none (KAIROS-T-0343).
                // KAIROS-T-0358: `organization` shows what it resolves to.
                if rendered.code_index_summaries
                    == kairos_client::types_repositories::CodeIndexSummaries::Organization
                {
                    format!(
                        "organization ({})",
                        rendered.code_index_summaries_resolved
                    )
                } else {
                    rendered.code_index_summaries.to_string()
                },
                // KAIROS-T-0479: what the forge webhook picks up.
                rendered.forge_filters.summary(),
                // KAIROS-T-0670: the cadence of the issue import.
                rendered.issue_import_summary(),
            );
            // KAIROS-T-0668: a language whose edges are by name, and why.
            for warning in &rendered.code_index_scip_warnings {
                out.push_str(&format!("- code index edges: {}\n", warning.note()));
            }
            // No line about "stale" tasks (COLLIERY-T-0219). It counted the
            // linked tasks on a board of a team that does not own the
            // repository, and told the reader to rebind or move them.
            // COLLIERY-A-0023 makes those tasks normal work, and `open tasks`
            // above already counts them.
            out.push_str("\n## How to work here\n");
            if rendered.description.trim().is_empty() {
                out.push_str("(no description yet)\n");
            } else {
                out.push_str(&rendered.description);
                out.push('\n');
            }
            // COLLIERY-T-0269. Before the work in flight: what the
            // repository is for comes before what is open in it.
            out.push_str("\n## Documents and ADRs that impact this repository\n");
            if impacted_by.is_empty() {
                out.push_str("(none)\n");
            }
            for item in &impacted_by {
                out.push_str(&impacting_item_line(item));
            }
            out.push_str(&format!(
                "\n## ADRs of the owner team\nThe ADRs on the boards of the team {} that no `impacts` link names, newest first. A superseded ADR is not listed.\n",
                rendered.team.slug
            ));
            if team_adrs.is_empty() {
                out.push_str("(none)\n");
            }
            for item in &team_adrs {
                out.push_str(&impacting_item_line(item));
            }
            let not_listed = team_adrs_total - team_adrs.len() as i64;
            if not_listed > 0 {
                out.push_str(&format!(
                    "- {not_listed} more: `team_pages {}` lists each ADR of the team.\n",
                    rendered.team.slug
                ));
            }
            out.push_str("\n## In flight\n");
            if in_flight.is_empty() {
                out.push_str(&empty_page_text(
                    "(nothing open)",
                    in_flight_total,
                    offset,
                ));
            }
            let in_flight_shown = in_flight.len() as i64;
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
            out.push_str(&more_line(in_flight_total, offset, in_flight_shown));
            // KAIROS-T-0480: what is open and has no item. The forge filter
            // decides what is recorded.
            let (unlinked, unlinked_total) = kairos_db::forge::repository_unlinked_refs(
                conn,
                repo_id,
                &["open", "draft"],
                REPOSITORY_LIST_LIMIT,
                offset,
            )
            .map_err(ApiError::internal)?;
            out.push_str("\n## Not linked to an item\n");
            if unlinked.is_empty() {
                out.push_str(&empty_page_text("(none)", unlinked_total, offset));
            }
            let unlinked_shown = unlinked.len() as i64;
            for row in unlinked {
                out.push_str(&format!(
                    "- {} {} [{}] {} by {} ({})\n",
                    row.kind, row.external_id, row.state, row.title, row.author, row.url
                ));
            }
            out.push_str(&more_line(unlinked_total, offset, unlinked_shown));
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "The boards of the organization by level (strategy, initiative, delivery, ADR), with the columns and item counts of your delivery boards, the team of each ADR board, the capabilities that your teams or your board memberships give you with no grant, and the boards where you approve edge proposals. Optional `level`."
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
            let my_grants: Vec<(Uuid, String)> = caps::table
                .filter(caps::user_id.eq(user_id))
                .select((caps::board_id, caps::capability))
                .load(conn)
                .map_err(ApiError::internal)?;
            let mut my_grant_boards: Vec<Uuid> = my_grants.iter().map(|(b, _)| *b).collect();
            my_grant_boards.sort_unstable();
            my_grant_boards.dedup();
            // KAIROS-T-0541: the grants of each board, to tell a member
            // (the `member` grant) from a holder of other grants.
            let grants_on = |board_id: Uuid| -> Vec<&str> {
                my_grants
                    .iter()
                    .filter(|(b, _)| *b == board_id)
                    .map(|(_, c)| c.as_str())
                    .collect()
            };

            let is_mine = |board: &Board| {
                board.board_level == BoardLevel::Delivery
                    && (board.team_id.is_some_and(|t| my_teams.contains(&t))
                        || my_grant_boards.contains(&board.id))
            };
            // COLLIERY-T-0265: the columns and the counts of all my boards
            // come from 1 query, and the database counts the cards. Before,
            // the tool read each row of each board (5 queries for a board).
            let mine: Vec<Uuid> = all_boards
                .iter()
                .filter(|board| is_mine(board))
                .map(|board| board.id)
                .collect();
            let mut columns_of: HashMap<Uuid, Vec<String>> = HashMap::new();
            let mut counts_of: HashMap<Uuid, Vec<kairos_db::board_items::ColumnCardCount>> =
                HashMap::new();
            for column in kairos_db::board_items::live_column_counts(conn, &mine)
                .map_err(ApiError::internal)?
            {
                columns_of
                    .entry(column.board_id)
                    .or_default()
                    .push(format!("{} ({})", column.name, column.count));
                counts_of.entry(column.board_id).or_default().push(column);
            }
            // KAIROS-T-0554: the rules of each board of mine, so an agent
            // does not learn them by failing: each live column with its
            // flags (entry, done, claims, WIP) and the columns a card can
            // move to from it. One query for the transitions of every board
            // of mine; the columns come with the counts above.
            let transitions = kairos_db::board_items::transitions_of_boards(conn, &mine)
                .map_err(ApiError::internal)?;
            let mut moves_of: HashMap<Uuid, String> = HashMap::new();
            for (board_id, columns) in &counts_of {
                let edges: Vec<(Uuid, Uuid)> = transitions
                    .iter()
                    .filter(|(board, _, _)| board == board_id)
                    .map(|(_, from, to)| (*from, *to))
                    .collect();
                moves_of.insert(*board_id, moves_line(columns, &edges));
            }

            // COLLIERY-T-3102: an ADR board can have a team. The list names
            // the team of each team ADR board, so that an agent can find the
            // ADR board of its team.
            let team_slugs: HashMap<Uuid, String> = {
                use kairos_db::schema::teams;
                let ids: Vec<Uuid> = all_boards
                    .iter()
                    .filter(|board| board.board_level == BoardLevel::Adr)
                    .filter_map(|board| board.team_id)
                    .collect();
                teams::table
                    .filter(teams::id.eq_any(ids))
                    .select((teams::id, teams::slug))
                    .load::<(Uuid, String)>(conn)
                    .map_err(ApiError::internal)?
                    .into_iter()
                    .collect()
            };

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
                if board.board_level == BoardLevel::Adr {
                    match board.team_id.and_then(|id| team_slugs.get(&id)) {
                        Some(team) => out.push_str(&format!(" (ADR board of the team {team})")),
                        None => out.push_str(" (ADR board of the organization)"),
                    }
                }
                if is_mine(&board) {
                    out.push_str(" [mine]\n");
                    let rendered = columns_of.remove(&board.id).unwrap_or_default();
                    out.push_str(&format!("  columns: {}\n", rendered.join(" | ")));
                    if let Some(moves) = moves_of.remove(&board.id) {
                        out.push_str(&format!("  moves: {moves}\n"));
                    }
                } else {
                    out.push('\n');
                }
                // KAIROS-T-0641: the policies of the board.
                if let Some(policy) = policy_text(&board) {
                    out.push_str(&format!("  policy: {policy}\n"));
                }
                // COLLIERY-T-3108: what the team rule gives me on a board of
                // my team, with no grant.
                if board.team_id.is_some_and(|t| my_teams.contains(&t)) {
                    out.push_str(&format!(
                        "  team capabilities: {}\n",
                        team_capabilities(board.board_level).join(", ")
                    ));
                }
                // KAIROS-T-0410: what membership of a strategy or initiative
                // board (the `member` grant, KAIROS-T-0541) gives me.
                let grants = grants_on(board.id);
                let member = member_capabilities(board.board_level, &grants);
                if !member.is_empty() {
                    out.push_str(&format!("  member capabilities: {}\n", member.join(", ")));
                }
                // KAIROS-T-0659: a holder of `approve_proposals` decides the
                // edge proposals of the items of this board.
                if grants.iter().any(|grant| {
                    kairos_core::abac::capability_matches(
                        grant,
                        kairos_core::abac::APPROVE_PROPOSALS,
                    )
                }) {
                    out.push_str(
                        "  approve_proposals: you confirm or reject the edge proposals of \
                         its items\n",
                    );
                }
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "The items of a board by column: short code, type, title and tags ([blocked by N], [claimed by NAME], [owner: NAME], [teams: a, b], [WIP 2/3]). `board` is a slug or UUID. Optional: `column` (name or UUID); `repository` (slug or UUID) for the tasks of one repository, your queue; `team` or `no_team: true` for strategies and initiatives; `include_deleted` adds archived cards, marked [archived]; `details: true` adds the last change and the metadata of each card. At most 200 items by default (`limit` up to 1000, `offset`); a partial result says so and gives the next `offset`."
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
            // KAIROS-T-0553: a delivery board has one team; a team filter
            // there would be read and dropped, so it is refused.
            if board.board_level == BoardLevel::Delivery
                && (params.team.is_some() || params.no_team.unwrap_or(false))
            {
                return Err(ApiError::validation(format!(
                    "The board {} is a delivery board: its tasks are all of its team, so \
                     `team` and `no_team` do not apply. Send them for a strategy or an \
                     initiative board.",
                    board.slug
                ))
                .with_details(json!({ "field": "team" })));
            }
            // KAIROS-T-0321: the team filter, the rule of the REST route.
            let team_filter = crate::api::meta::item_teams::team_filter(
                conn,
                params.team.as_deref(),
                params.no_team.unwrap_or(false),
            )?;
            // COLLIERY-T-0261: one page of the board. The filters apply
            // before the page, and the order is that of the REST route.
            let limit = params
                .limit
                .unwrap_or(BOARD_ITEMS_DEFAULT_LIMIT)
                .clamp(1, BOARD_ITEMS_MAX_LIMIT);
            let offset = params.offset.unwrap_or(0).max(0);
            let column_ids: Vec<Uuid> = columns.iter().map(|c| c.id).collect();
            let page = board_item_page(
                conn,
                &BoardItemFilter {
                    board_id: board.id,
                    column_ids: &column_ids,
                    include_archived: liveness == Liveness::IncludeArchived,
                    repository_id: repository,
                    team: team_filter,
                    done_limit: None,
                },
                limit,
                offset,
            )
            .map_err(ApiError::internal)?;
            let page_ids: Vec<Uuid> = page.cards.iter().map(|(_, id)| *id).collect();
            let mut items = board_item_rows(conn, board.id, repository, liveness, Some(&page_ids))?;
            let place: HashMap<Uuid, usize> = page_ids
                .iter()
                .enumerate()
                .map(|(place, id)| (*id, place))
                .collect();
            items.sort_by_key(|item| place.get(&item.id).copied().unwrap_or(usize::MAX));
            let filtered = params.column.is_some()
                || params.repository.is_some()
                || team_filter.is_some()
                || liveness == Liveness::IncludeArchived;
            let subject = if filtered {
                format!(
                    "The board has {} items that agree with the filters.",
                    page.total
                )
            } else {
                format!("The board has {} items.", page.total)
            };
            let incomplete = incomplete_board_note(
                &subject,
                page.total,
                i64::try_from(items.len()).unwrap_or(i64::MAX),
                limit,
                offset,
                "call the tool with offset",
            );
            let repo_ids: Vec<Uuid> = items.iter().filter_map(|i| i.repository_id).collect();
            let repo_slugs = repo_slug_map(conn, &repo_ids)?;
            // The same rollup the board card shows, in the same one
            // grouped query (COLLIERY-T-0214). Until this, the listing said
            // nothing about blocking, so an agent that was told to pick
            // work "with no unresolved blocks edge" had to read every card
            // with `get_item`, and could not tell a resolved blocker from
            // an open one when it did. The counts are of OPEN edges: done
            // work does not block and is not blocked.
            let item_ids: Vec<Uuid> = items.iter().map(|i| i.id).collect();
            let blocks = graph::blocks_summary(conn, &item_ids).map_err(ApiError::internal)?;
            // KAIROS-T-0321: the teams of the strategies and the
            // initiatives of the page, one query (KAIROS-T-0627).
            let teams = kairos_db::item_teams::page_teams(conn, board.id, &item_ids)
                .map_err(ApiError::internal)?;
            // KAIROS-T-0359: the claims of the tasks, one query.
            let claims = crate::claims::claim_texts(conn, &item_ids)?;
            // KAIROS-T-0411: the owners of the strategies and the
            // initiatives, two queries.
            let owners = crate::owners::owner_names(conn, &item_ids)?;
            // KAIROS-T-0362: the cancelled tasks, one query.
            let cancelled = kairos_db::task_cancellations::marks_of(conn, &item_ids)
                .map_err(ApiError::internal)?;
            // KAIROS-T-0506: the live states of the tasks, two queries.
            let states = crate::task_state::state_dtos(conn, &item_ids)?;
            // KAIROS-T-0368: with `details`, the metadata of the cards, one
            // query.
            let details = params.details.unwrap_or(false);
            let metadata = if details {
                kairos_db::board_items::metadata_of_items(conn, &item_ids)
                    .map_err(ApiError::internal)?
            } else {
                HashMap::new()
            };

            // KAIROS-T-0371: the live count of each column, for the WIP
            // limit. 1 query.
            let live_counts: HashMap<Uuid, i64> =
                kairos_db::board_items::live_column_counts(conn, &[board.id])
                    .map_err(ApiError::internal)?
                    .into_iter()
                    .map(|c| (c.column_id, c.count))
                    .collect();

            let mut out = format!(
                "# Board {} — {} ({})\n",
                board.slug, board.name, board.board_level
            );
            // Before the columns, so that an agent reads it first: a part
            // of a board must not look like the board.
            if let Some(note) = &incomplete {
                out.push_str(&format!("\n{note}\n"));
            }
            for column in &columns {
                let in_column: Vec<&BoardItemRow> =
                    items.iter().filter(|i| i.column_id == column.id).collect();
                // The number is that of the column only when the result
                // is the full board.
                let wip = wip_marker(
                    live_counts.get(&column.id).copied().unwrap_or(0),
                    column.wip_limit,
                );
                out.push_str(&match incomplete {
                    Some(_) => format!(
                        "\n## {} ({} in this result){wip}\n",
                        column.name,
                        in_column.len()
                    ),
                    None => format!("\n## {} ({}){wip}\n", column.name, in_column.len()),
                });
                for item in in_column {
                    // The marker is not decoration: an agent that cannot
                    // tell put-away work from live work will pick one up
                    // and start on it (KAIROS-A-0020 rule 2).
                    out.push_str(&format!(
                        "- {} [{}] {}{}{}{}{}{}{}{}{}\n",
                        item.short_code,
                        item.kind,
                        item.title,
                        if cancelled.contains_key(&item.id) {
                            " [cancelled]"
                        } else {
                            ""
                        },
                        claims
                            .get(&item.id)
                            .map(|(holder, _)| format!(" [claimed by {holder}]"))
                            .or_else(|| owners
                                .get(&item.id)
                                .map(|owner| format!(" [owner: {owner}]")))
                            .unwrap_or_default(),
                        item.repository_id
                            .and_then(|id| repo_slugs.get(&id))
                            .map(|slug| format!(" [repo:{slug}]"))
                            .unwrap_or_default(),
                        teams
                            .get(&item.id)
                            .map(|teams| format!(
                                " [teams: {}]",
                                teams
                                    .iter()
                                    .map(|t| t.slug.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ))
                            .unwrap_or_default(),
                        blocks
                            .get(&item.id)
                            .map(|counts| blocks_marker(*counts))
                            .unwrap_or_default(),
                        states
                            .get(&item.id)
                            .map(|s| crate::task_state::state_tag(s, chrono::Utc::now()))
                            .unwrap_or_default(),
                        if item.archived { " [archived]" } else { "" },
                        if details {
                            card_details(item.updated_at, metadata.get(&item.id))
                        } else {
                            String::new()
                        }
                    ));
                }
            }
            Ok(out)
        })
        .await
    }

    // KAIROS-T-0372: the flow metrics of a board.
    #[tool(
        description = "Flow metrics of a board for a range of days (UTC): cycle time (count, mean, median, 85th percentile, and each item), throughput per week (done, and cancelled apart), the age of each item in progress, and the cumulative flow of the last 14 days. `board` is a slug or UUID; optional `from` and `to` (YYYY-MM-DD; default the last 90 days; 366 days at most)."
    )]
    pub async fn board_flow(
        &self,
        Parameters(params): Parameters<BoardFlowParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            let flow = crate::api::org::flow::board_flow_of(
                conn,
                &params.board,
                params.from.as_deref(),
                params.to.as_deref(),
            )?;
            Ok(flow_text(&flow))
        })
        .await
    }

    // KAIROS-T-0385: the agent dispatch rules of a board.
    #[tool(
        description = "The dispatch rules of a board (when each fires, its filter and target), its webhook targets (on or off, failed tries in a row), the 20 newest deliveries (status, tries, response code, error), and who decides intake triage. A person who belongs to the board can read them. `board` is a slug or UUID."
    )]
    pub async fn list_dispatch_rules(
        &self,
        Parameters(params): Parameters<ListDispatchRulesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        self.run_tool(&tenant, move |conn| {
            use crate::api::org::dispatch as d;
            let rules = d::list_rules_as(conn, &slug, user, &params.board)?;
            let targets = d::list_targets_as(conn, &slug, user, &params.board)?;
            let log = d::list_deliveries_as(
                conn,
                &slug,
                user,
                &params.board,
                &kairos_client::types_dispatch::WebhookDeliveriesQuery {
                    limit: Some(20),
                    ..Default::default()
                },
            )?;
            let triage = d::agent_triage_as(conn, &slug, user, &params.board)?;
            let mut text = dispatch_text(&params.board, &rules, &targets, &log);
            text.push_str(&format!(
                "Triage of intake (KAIROS-T-0428): {}.\n",
                if triage.agents_decide {
                    "an agent decides"
                } else {
                    "an agent proposes with a note, a person decides"
                }
            ));
            Ok(text)
        })
        .await
    }

    #[tool(
        description = "Make a dispatch rule on a board: when a task that matches the filter enters `column` (event task.entered_column), or with `trigger: unblocked` when its last open blocker closes (event task.unblocked; `column` optional), Kairos sends one signed delivery to a webhook target. Send `target_id`, or `url` (and `target_name`) to make a target: the answer shows its secret once, for the receiver to check X-Kairos-Signature. Optional filter, each part must match: `task_type`, `metadata_slug` with `metadata_value`, `repository`. Needs configure_boards."
    )]
    pub async fn create_dispatch_rule(
        &self,
        Parameters(params): Parameters<CreateDispatchRuleParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        let key = self.state.config.secrets_key.clone();
        let allow_private = self.state.config.webhooks.allow_private;
        self.run_tool(&tenant, move |conn| {
            use crate::api::org::dispatch as d;
            use kairos_client::types_dispatch as dto;
            crate::api::org::run_in_transaction(conn, |conn| {
                let (target_id, created) =
                    match (params.target_id.as_deref(), params.url.as_deref()) {
                        (Some(id), None) => (id.to_string(), None),
                        (None, Some(url)) => {
                            let created = d::create_target_as(
                                conn,
                                &slug,
                                user,
                                &params.board,
                                &dto::CreateWebhookTargetRequest {
                                    name: params
                                        .target_name
                                        .clone()
                                        .unwrap_or_else(|| "Agent runner".to_string()),
                                    url: url.to_string(),
                                },
                                key.as_ref(),
                                allow_private,
                            )?;
                            (created.target.id.clone(), Some(created))
                        }
                        _ => {
                            return Err(ApiError::validation(
                                "Send target_id (a webhook target of the board) or url (a new \
                             target), and not both.",
                            )
                            .with_details(serde_json::json!({ "field": "target_id" })));
                        }
                    };
                let rule = d::create_rule_as(
                    conn,
                    &slug,
                    user,
                    &params.board,
                    &dto::CreateDispatchRuleRequest {
                        trigger: params.trigger.clone(),
                        column: params.column.clone(),
                        target_id,
                        task_type: params.task_type.clone(),
                        metadata_slug: params.metadata_slug.clone(),
                        metadata_value: params.metadata_value.clone(),
                        repository: params.repository.clone(),
                    },
                )?;
                let mut out = format!(
                    "Made the dispatch rule {}: {} {}goes to the target {} ({}).",
                    rule.id,
                    rule_when_text(&rule),
                    rule_filter_text(&rule),
                    rule.target,
                    rule.target_id
                );
                if let Some(created) = created {
                    out.push_str(&format!(
                        "\nMade the webhook target {} ({}) for {}.\nsecret: {}\nKairos shows the \
                         secret this one time. Keep it in the receiver: X-Kairos-Signature is \
                         sha256=<hex HMAC-SHA256 of the body with this secret>.",
                        created.target.name, created.target.id, created.target.url, created.secret
                    ));
                }
                Ok(out)
            })
        })
        .await
    }

    #[tool(
        description = "Set who decides the triage of intake (a task in the support lane) on a board. `agents_decide: false` (the default): an agent (agent key or service account) that triages intake (a support-lane task in the entry column: into the planned lane, out of the entry column, to another board, or cancelled) is refused with AGENT_TRIAGE_OFF and writes its proposal with `append_note`. A task a person triaged out of the entry column moves as any other. `true`: the agent's move works and the activity log names it. A person's triage always works. Needs configure_boards; `list_dispatch_rules` shows it."
    )]
    pub async fn set_agent_triage(
        &self,
        Parameters(params): Parameters<SetAgentTriageParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        self.run_tool(&tenant, move |conn| {
            let setting = crate::api::org::dispatch::set_agent_triage_as(
                conn,
                &slug,
                user,
                &params.board,
                params.agents_decide,
            )?;
            Ok(format!(
                "Triage of intake on {}: {}.",
                params.board,
                if setting.agents_decide {
                    "an agent decides"
                } else {
                    "an agent proposes with a note, a person decides"
                }
            ))
        })
        .await
    }

    #[tool(
        description = "Set the policies of a board. `parent_gate` (a delivery board): when true (the default), a task moves into a claims column (Active) by a transition, a board move or a create only while its parent initiative is in Active or Monitoring, or is a bucket; else PARENT_NOT_ACTIVE. A task with no parent initiative moves freely. `owner_gate` (an initiative board): when true (the default), an initiative moves into Active only with an owner, and the owner of an initiative in Active or Monitoring cannot release it (hand_off_item changes it); else OWNER_REQUIRED. `blockers_gate` (a delivery board): when true, a task with an open blocker (a cancelled blocker is open) does not move into a claims column or a done column by a transition, a reopen or a board move; else BLOCKED, with the blockers. When false (the default), the move goes on with a warning that names each open blocker. Send at least one policy; a policy of a different level is VALIDATION. Needs configure_boards; `my_boards` shows the policies."
    )]
    pub async fn set_board_policy(
        &self,
        Parameters(params): Parameters<SetBoardPolicyParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        self.run_tool(&tenant, move |conn| {
            let board = crate::api::org::boards::set_board_policy_as(
                conn,
                &slug,
                user,
                &params.board,
                crate::api::org::boards::PolicyChange {
                    parent_gate: params.parent_gate,
                    owner_gate: params.owner_gate,
                    blockers_gate: params.blockers_gate,
                },
            )?;
            Ok(format!(
                "Policies of {}: {}.",
                board.slug,
                policy_text(&board).unwrap_or_else(|| "none".to_string())
            ))
        })
        .await
    }

    #[tool(
        description = "Remove an agent dispatch rule of a board. Its deliveries stay in the log, and its webhook target stays. `board` is a slug or UUID; `rule_id` comes from list_dispatch_rules. Requires configure_boards on the board."
    )]
    pub async fn delete_dispatch_rule(
        &self,
        Parameters(params): Parameters<DeleteDispatchRuleParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        self.run_tool(&tenant, move |conn| {
            let rule = crate::api::org::dispatch::delete_rule_as(
                conn,
                &slug,
                user,
                &params.board,
                &params.rule_id,
            )?;
            Ok(format!(
                "Removed the dispatch rule {} ({}, target {}).",
                rule.id,
                rule_when_text(&rule),
                rule.target
            ))
        })
        .await
    }

    // KAIROS-T-0558: the webhook targets that create_dispatch_rule makes.
    #[tool(
        description = "Remove a webhook target of a board, with its dispatch rules and its delivery log. You cannot undo it. `board` is a slug or UUID; `target_id` comes from list_dispatch_rules. Needs configure_boards on the board."
    )]
    pub async fn remove_webhook_target(
        &self,
        Parameters(params): Parameters<WebhookTargetParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        self.run_tool(&tenant, move |conn| {
            let target = crate::api::org::dispatch::delete_target_as(
                conn,
                &slug,
                user,
                &params.board,
                &params.target_id,
            )?;
            Ok(format!(
                "Removed the webhook target {} ({}), with its dispatch rules and its delivery \
                 log.",
                target.name, target.id
            ))
        })
        .await
    }

    #[tool(
        description = "Turn a webhook target of a board on again, with its failed tries in a row at 0. Kairos queues the skipped deliveries of the last 30 days again, oldest first; the answer counts them. `board` is a slug or UUID; `target_id` comes from list_dispatch_rules. Needs configure_boards on the board."
    )]
    pub async fn enable_webhook_target(
        &self,
        Parameters(params): Parameters<WebhookTargetParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        let result = self
            .run_tool(&tenant, move |conn| {
                let target = crate::api::org::dispatch::enable_target_as(
                    conn,
                    &slug,
                    user,
                    &params.board,
                    &params.target_id,
                )?;
                Ok(format!(
                    "The webhook target {} ({}) is on. Kairos queued {} skipped deliveries \
                     again.",
                    target.name,
                    target.id,
                    target.redelivered.unwrap_or(0)
                ))
            })
            .await;
        // As REST: the worker sends the queued deliveries now.
        self.state.jobs.wake();
        result
    }

    #[tool(
        description = "Send a test delivery (the event `ping`) to a webhook target of a board. A target that is off gets a skipped delivery. The answer gives the id of the delivery; list_dispatch_rules shows its status. `board` is a slug or UUID; `target_id` comes from list_dispatch_rules. Needs configure_boards on the board."
    )]
    pub async fn test_webhook_target(
        &self,
        Parameters(params): Parameters<WebhookTargetParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        let result = self
            .run_tool(&tenant, move |conn| {
                let delivery = crate::api::org::dispatch::test_target_as(
                    conn,
                    &slug,
                    user,
                    &params.board,
                    &params.target_id,
                )?;
                Ok(format!(
                    "Queued the test delivery {} (event ping) to the webhook target {}. Its \
                     status: {}.",
                    delivery.id, delivery.target_id, delivery.status
                ))
            })
            .await;
        // As REST: the worker sends the delivery now.
        self.state.jobs.wake();
        result
    }

    // KAIROS-T-0431: the read token of the organization for each forge host.
    #[tool(
        description = "The read tokens of the organization for the code index builder: for each forge host (for example github.com), who set the token and when. Never the token. The builder uses the token of the host of a repository before the token of the repository. Only an organization admin."
    )]
    pub async fn list_org_read_tokens(
        &self,
        Parameters(ListOrgReadTokensParams {}): Parameters<ListOrgReadTokensParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        if let Err(e) = crate::api::meta::require_org_admin(&tenant) {
            return Ok(tool_error(e));
        }
        self.run_tool(&tenant, |conn| {
            Ok(org_read_tokens_text(
                &crate::api::org::org_read_tokens::list(conn)?,
            ))
        })
        .await
    }

    #[tool(
        description = "Set or replace the read token of the organization for a forge host. The code index builder then fetches each repository on that host with this token, before the token of the repository. `host` is the host only (github.com, or git.example.org:8443); `token` is a read-only token. Kairos encrypts it with KAIROS_SECRETS_KEY, and no tool gives it back. Only an organization admin."
    )]
    pub async fn set_org_read_token(
        &self,
        Parameters(params): Parameters<SetOrgReadTokenParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        if let Err(e) = crate::api::meta::require_org_admin(&tenant) {
            return Ok(tool_error(e));
        }
        let (user, slug) = (auth.user_id, tenant.slug.clone());
        let key = crate::api::org::repository_credentials::secrets_key(&self.state);
        self.run_tool(&tenant, move |conn| {
            let status = crate::api::org::org_read_tokens::set(
                conn,
                &slug,
                user,
                key,
                &params.host,
                &params.token,
            )?;
            Ok(format!(
                "Set the read token of the organization for {}. The builder uses it for each \
                 repository on this host.",
                status.host
            ))
        })
        .await
    }

    #[tool(
        description = "Remove the read token of the organization for a forge host. The next fetch of a repository on the host uses the token of the repository, or no token. `host` is the host only (github.com). Only an organization admin."
    )]
    pub async fn remove_org_read_token(
        &self,
        Parameters(params): Parameters<RemoveOrgReadTokenParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        if let Err(e) = crate::api::meta::require_org_admin(&tenant) {
            return Ok(tool_error(e));
        }
        let user = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            let left = crate::api::org::org_read_tokens::remove(conn, user, &params.host)?;
            Ok(format!(
                "Removed the read token of the organization for {}.\n{}",
                kairos_core::repositories::normalize_host(&params.host)
                    .unwrap_or_else(|_| params.host.clone()),
                org_read_tokens_text(&left)
            ))
        })
        .await
    }

    // KAIROS-T-0368: the metadata definitions and the templates, so that an
    // agent knows the scales (the values of `priority`, in order) and the
    // slugs that `set_metadata` and `search` take.
    #[tool(
        description = "The metadata definitions of the organization: slug, name, type, the values of an enum in their order (the scale), and the entity types it applies to. Optional `entity_type`. Use the slugs in `set_metadata` and in `search`."
    )]
    pub async fn list_metadata_definitions(
        &self,
        Parameters(params): Parameters<ListMetadataDefinitionsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            use crate::api::meta::definitions::ENTITY_TYPES;
            use kairos_db::models::templates::MetadataDefinition;
            use kairos_db::schema::metadata_definitions as definitions;
            if let Some(entity_type) = params.entity_type.as_deref()
                && !ENTITY_TYPES.contains(&entity_type)
            {
                return Err(ApiError::validation(format!(
                    "The value {entity_type:?} of entity_type is not an entity type. The entity \
                     types are: {}.",
                    ENTITY_TYPES.join(", ")
                )));
            }
            let rows: Vec<MetadataDefinition> = definitions::table
                .order(definitions::slug.asc())
                .select(MetadataDefinition::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let mut out = String::from("# Metadata definitions\n\n");
            let mut shown = 0;
            for row in rows {
                let definition = crate::api::meta::definitions::hydrate(conn, row)?;
                if let Some(entity_type) = params.entity_type.as_deref()
                    && !definition.entity_types.is_empty()
                    && !definition.entity_types.iter().any(|t| t == entity_type)
                {
                    continue;
                }
                shown += 1;
                out.push_str(&format!(
                    "- `{}` {} ({})",
                    definition.slug, definition.name, definition.field_type
                ));
                if !definition.enum_options.is_empty() {
                    out.push_str(&format!(
                        " — values, in order: {}",
                        definition.enum_options.join(", ")
                    ));
                }
                out.push_str(&if definition.entity_types.is_empty() {
                    " — applies to: all types".to_string()
                } else {
                    format!(" — applies to: {}", definition.entity_types.join(", "))
                });
                out.push('\n');
            }
            if shown == 0 {
                out.push_str("The organization has no metadata definition for this.\n");
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "List the templates of the organization: slug, name, and the metadata that each template stamps on an item (the default value, and whether the field is required). With `slug`, show that one template with its full content."
    )]
    pub async fn list_templates(
        &self,
        Parameters(params): Parameters<ListTemplatesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            use kairos_db::models::templates::Template;
            use kairos_db::schema::templates;
            let mut query = templates::table
                .order(templates::slug.asc())
                .select(Template::as_select())
                .into_boxed();
            if let Some(slug) = params.slug.as_deref() {
                query = query.filter(templates::slug.eq(slug));
            }
            let rows: Vec<Template> = query.load(conn).map_err(ApiError::internal)?;
            if rows.is_empty()
                && let Some(slug) = params.slug.as_deref()
            {
                return Err(ApiError::not_found(format!(
                    "The organization has no template with the slug {slug:?}."
                )));
            }
            let mut out = String::from("# Templates\n");
            for row in rows {
                let template = crate::api::meta::templates::detail(conn, row)?;
                out.push_str(&format!("\n## `{}` {}\n", template.slug, template.name));
                if template.metadata.is_empty() {
                    out.push_str("- metadata: none\n");
                }
                for field in &template.metadata {
                    out.push_str(&format!(
                        "- `{}` ({}){}{}\n",
                        field.slug,
                        field.field_type,
                        field
                            .default_value
                            .as_deref()
                            .map(|v| format!(" default: {v}"))
                            .unwrap_or_default(),
                        if field.required { " required" } else { "" }
                    ));
                }
                if params.slug.is_some() {
                    out.push_str(&format!("\n{}\n", template.content));
                }
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "One item by short code: type, board and column, who created it and when, version, markdown content and then its notes (each one under `## Note n-3 (date, name)`), metadata, relationships (parent chain, children, blockers and blocked, supporting documents; a done end is marked [done], a cancelled one [cancelled]), and as it applies: the claim of a task in Active, the owner and teams of a strategy or initiative, the owner board and impacted repositories of a document or ADR, the cancel mark of a task, the external references (URIs with a label) of any item, and its edge proposals (each pending one and the 5 newest decided ones, with the state). A retired code finds the item and names the current code."
    )]
    pub async fn get_item(
        &self,
        Parameters(params): Parameters<GetItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| get_item_text(conn, &params.short_code))
            .await
    }

    #[tool(
        description = "An item's history, newest first: the content versions (version, editor, time), then the activity (moves, claims, hand-offs, releases, lane, metadata, state, links and more, with who and when). `limit` caps each list (default 20, up to 200); `offset` pages the versions, and a partial list says so and gives the next `offset`. Pass `version` to fetch that snapshot's full title + content instead."
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

            // KAIROS-T-0553: `limit` is for the list; with `version` it would
            // be read and dropped.
            if params.version.is_some() && params.limit.is_some() {
                return Err(ApiError::validation(
                    "Send version for one snapshot, or limit for the list, not both.",
                )
                .with_details(json!({ "field": "limit" })));
            }
            if params.version.is_some() && params.offset.is_some() {
                return Err(ApiError::validation(
                    "Send version for one snapshot, or offset for the list, not both.",
                )
                .with_details(json!({ "field": "offset" })));
            }

            if let Some(version) = params.version {
                // KAIROS-T-0651: a diff row gets its text from its base row.
                let snapshot = kairos_db::history::version_row(conn, item.id, version)
                    .map_err(ApiError::internal)?
                    .map(|row| (row.title, row.content));
                let (title, content) = snapshot.ok_or_else(|| {
                    ApiError::not_found(format!(
                        "{} has no version {version} in its history.",
                        item.short_code
                    ))
                })?;
                return Ok(format!(
                    "# {} v{version} — {title}\n\n{content}\n",
                    item.short_code
                ));
            }

            let limit = params.limit.unwrap_or(20).clamp(1, 200);
            let offset = params.offset.unwrap_or(0).max(0);
            // KAIROS-T-0561: the count, so that a cut list says so.
            let total: i64 = item_history::table
                .filter(item_history::item_id.eq(item.id))
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<(i32, Uuid, DateTime<Utc>, Option<Uuid>)> = item_history::table
                .filter(item_history::item_id.eq(item.id))
                .order(item_history::version.desc())
                .limit(limit)
                .offset(offset)
                .select((
                    item_history::version,
                    item_history::edited_by,
                    item_history::edited_at,
                    item_history::agent_key_id,
                ))
                .load(conn)
                .map_err(ApiError::internal)?;
            let editor_ids: Vec<Uuid> = rows.iter().map(|(_, editor, _, _)| *editor).collect();
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
                    "\n> **ARCHIVED** {}. This item is in the archive. The list \
                     shows its versions. The item did not change after that \
                     time.\n\n",
                    archived_at.format("%Y-%m-%dT%H:%M:%SZ")
                ));
            }
            let shown = rows.len() as i64;
            if shown == 0 && offset > 0 && total > 0 {
                out.push_str(&empty_page_text("", total, offset));
            }
            for (version, editor, edited_at, agent_key) in rows {
                let editor = editors.get(&editor).map_or("unknown", String::as_str);
                out.push_str(&format!(
                    "- v{version} — {} by {}\n",
                    edited_at.format("%Y-%m-%dT%H:%M:%SZ"),
                    actor_text(editor, agent_key)
                ));
            }
            out.push_str(&more_line(total, offset, shown));
            out.push_str(&rename_history(conn, item.id)?);
            out.push_str(&activity_section(conn, item.id, &item.short_code, limit)?);
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Propose a `parent` or `blocks` edge between two items. The proposal is not an edge until someone confirms it. Use it when `related_work` shows a real dependency, and say why: the reason is shown to who decides. A holder of `approve_proposals` on the board of one of the two items confirms or rejects it, or an organization admin. The holder can be you, your agent key or a service account. The answer names the two boards."
    )]
    pub async fn propose_edge(
        &self,
        Parameters(params): Parameters<ProposeEdgeParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            let relationship = match params.relationship.as_str() {
                "parent" => kairos_db::models::enums::RelationshipType::Parent,
                "blocks" => kairos_db::models::enums::RelationshipType::Blocks,
                other => {
                    return Err(ApiError::validation(format!(
                        "You can propose only a parent edge or a blocks edge. {other:?} \
                         is not one of them."
                    )));
                }
            };
            let source = load_item(conn, &params.source, Liveness::LiveOnly)?;
            let target = load_item(conn, &params.target, Liveness::LiveOnly)?;
            let proposal = kairos_db::proposals::propose(
                conn,
                kairos_db::proposals::NewProposal {
                    source_id: source.id,
                    target_id: target.id,
                    relationship,
                    claim: "proposed by an agent",
                    why: &params.why,
                    score: 0.0,
                },
                user,
            )
            .map_err(crate::api::proposals::map_proposal_error)?;
            // KAIROS-T-0659: the answer says who decides.
            let boards = kairos_db::proposals::approval_boards(conn, source.id, target.id)
                .map_err(crate::api::proposals::map_proposal_error)?;
            Ok(format!(
                "Proposed {} -[{}]-> {} ({}). It is NOT an edge yet. {}",
                source.short_code,
                params.relationship,
                target.short_code,
                proposal.id,
                boards.deciders()
            ))
        })
        .await
    }

    #[tool(
        description = "Find work related to an item — possible duplicates, prior art in finished or put-away work, and dependencies nobody drew. Returns a HANDFUL of PROPOSALS, each with what is claimed and why, never assertions: check before acting on one. Ranked across text and meaning together; a deployment with no vectors answers from text alone and says so. Ends with the external references (URIs) of the item and of the proposals. Ask this before starting a ticket, and before filing one."
    )]
    pub async fn related_work(
        &self,
        Parameters(params): Parameters<RelatedWorkParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        let Some(service) = self.state.embedding.clone() else {
            // Not an error: a deployment may have embeddings switched off
            // (KAIROS-A-0021 rule 7). Say what happened and what still works.
            return Ok(tool_text(
                "This deployment does not have the search for related work. Use `search`."
                    .to_string(),
            ));
        };
        let mut config = kairos_core::retrieval::RetrievalConfig::default();
        if let Some(limit) = params.limit {
            config.limit = limit.clamp(1, 10);
        }
        let short_code = params.short_code.clone();
        self.run_tool(&tenant, move |conn| {
            // KAIROS-T-0561: each error keeps its code.
            let found = service
                .related_work(conn, &short_code, &config)
                .map_err(crate::api::search::related_work_error)?;
            let external = external_refs_of_related(conn, &found)?;
            Ok(render_related_work(&found, &external))
        })
        .await
    }

    #[tool(
        description = "Search items: full-text `q`, a structured `filter` (types, board, column, team, repository, task_type, metadata, metadata_missing, updated_before, updated_after, claimed_by, owned_by, created_by) and a graph `traverse`, in any mix; at least one. Board and team take a slug or UUID; `column` takes a name with `board`, or a UUID. claimed_by, owned_by and created_by take `me`, a user id, an email or a user name; a claim by an agent key counts for its person. \"Untouched for 90 days\" is `updated_before` = now minus 90 days; \"bugs without a priority\" is `{entity_type: [task], task_type: [bug], metadata_missing: [priority]}`. `filter.repository` gives the tasks linked to it and the documents and ADRs that impact it. Compact results by type; a task line has its board/column, the claim, the open blockers and the repository. A partial result gives the next `offset`. `get_item` gives the content."
    )]
    pub async fn search(
        &self,
        Parameters(params): Parameters<SearchParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (slug, caller) = (tenant.slug.clone(), auth.user_id);
        let mut request = match search_to_core(&params) {
            Ok(request) => request,
            Err(e) => return Ok(tool_error(e)),
        };
        let refs = match params.filter.as_ref().map(|f| {
            crate::api::search::FilterRefs::new(
                f.repository.clone(),
                (f.board.clone(), f.board_id.is_some()),
                (f.team.clone(), f.team_id.is_some()),
            )
            .and_then(|refs| refs.with_column(f.column.clone(), f.column_id.is_some()))
            .map(|refs| {
                refs.with_people(
                    f.claimed_by.clone(),
                    f.owned_by.clone(),
                    f.created_by.clone(),
                )
            })
        }) {
            None => crate::api::search::FilterRefs::default(),
            Some(Ok(refs)) => refs,
            Some(Err(e)) => return Ok(tool_error(e)),
        };
        if refs.is_empty()
            && let Err(e) = core_search::validate(&request)
        {
            // Invalid requests never cost a connection checkout.
            return Ok(tool_error(ApiError::validation(e.to_string())));
        }
        self.run_tool(&tenant, move |conn| {
            // KAIROS-T-0107, KAIROS-T-0368: the slug-or-UUID repository,
            // board and team need a conn to resolve, so they join the core
            // filter here — then validate.
            if !refs.is_empty() {
                refs.resolve_into(conn, &slug, caller, &mut request)?;
                core_search::validate(&request).map_err(|e| ApiError::validation(e.to_string()))?;
            }
            let results = search::execute_search(conn, &request).map_err(map_search_error)?;
            // KAIROS-T-0561: the board, the column, the claim, the open
            // blockers and the repository of each task, one statement for
            // the page.
            let task_ids: Vec<Uuid> = results.tasks.iter().map(|t| t.id).collect();
            let facts = search::task_line_facts(conn, &task_ids).map_err(ApiError::internal)?;
            let mut out = String::new();
            // COLLIERY-T-3100: a query that is a retired code finds the
            // item; say which code is current.
            if let Some(q) = request.q.as_deref() {
                let code = q.trim().to_ascii_uppercase();
                if let Some(current) = crate::api::retired_to_current(conn, &code)? {
                    out.push_str(&format!(
                        "> The code {code} is retired. The current code of the item is \
                         {current}.\n\n"
                    ));
                }
            }
            out.push_str(&render_search_results(&results, &facts));
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Create a work item: strategy, initiative, task, document or ADR. There is no `column` argument: the item goes to the entry column of its board, and only `transition_item` moves it. `board` is a slug or UUID; it defaults when only one board of the level exists. `parent` (short code) makes the parent edge; for a document or an ADR, the supports edge. Tasks: the board decides the team; `repository` is an optional link and chooses no board. When you split work into tasks, `link_items` `blocks` from each blocker to each task that must wait; parallel tasks get no edge. On a board you do not manage the task is a request (support lane; `work_class: planned` refused). `from_team` names the sending team: when it is not the board's team the task is a request whoever sends it, so an agent in a board session sends its own team. Initiatives: `bucket_type` makes a bucket; `set_complexity` changes the complexity later. Documents: `board` is required: the owner board, which needs `manage_documents` and gives the right to edit; to say a document is about a repository, `link_items` with `impacts` after. ADRs: `board` is an ADR board. Returns the new short code."
    )]
    pub async fn create_item(
        &self,
        Parameters(params): Parameters<CreateItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let tenant_ctx = tenant.clone();
        // COLLIERY-T-0227: one transaction for the item and its edge. Each
        // service opens a transaction of its own, which becomes a savepoint
        // in this one, so an edge that fails takes the item with it.
        self.run_tool(&tenant, move |conn| {
            atomically(conn, |conn| {
                create_item_impl(conn, &tenant_ctx, user, &params)
            })
        })
        .await
    }

    #[tool(
        description = "Replace an item's full content (and optionally title) under optimistic concurrency: pass the `version` you read. A stale version returns CONFLICT with the current version + content so you can reconcile. For small targeted edits prefer edit_item. You can edit an item that you created, or with `manage_<type>` on its board."
    )]
    pub async fn update_item(
        &self,
        Parameters(params): Parameters<UpdateItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            update_item_text(conn, role, user, &params)
        })
        .await
    }

    #[tool(
        description = "Targeted server-side edit of an item's content: exact search/replace applied to the CURRENT version (retries once on a concurrent-edit race). Fails if `search` is not found, or is ambiguous when replace_all is false. You can edit an item that you created, or with `manage_<type>` on its board."
    )]
    pub async fn edit_item(
        &self,
        Parameters(params): Parameters<EditItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            edit_item_text(conn, role, user, &params)
        })
        .await
    }

    // KAIROS-T-0367: a note in one call, with no version and no anchor.
    #[tool(
        description = "Append a note to an item. The note gets the next id, and `get_item` shows it after the content under a heading with the date and your name: `## Note n-3 (2026-10-11, Robin)`. The result names the id: cite it as `note n-3 on ACME-T-0012`. Send no version and no anchor text. A note is not part of the content: it writes no version, and an edit of the content does not change it. Two notes at the same time both stay. The text is markdown, and headings are allowed. Use it to record work, a finding or a decision on an item. You can append to an item that you created, or with `manage_<type>` on its board."
    )]
    pub async fn append_note(
        &self,
        Parameters(params): Parameters<AppendNoteParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            append_note_text(conn, role, user, &params)
        })
        .await
    }

    // KAIROS-T-0489: the work of the person of the key.
    #[tool(
        description = "Your work, in two lanes. Initiatives: the strategies and initiatives you own. Tasks: the tasks you or your agent keys claim, the Todo, Active and Blocked tasks under initiatives you own (claimed by anyone or nobody), and the tasks these block. Each item has a state (Todo, Active, Blocked), its board and column, its claimant or owner, and its open blockers. Done items are not shown. `search` with `claimed_by`, `owned_by` or `created_by` finds more."
    )]
    pub async fn my_work(
        &self,
        Parameters(MyWorkParams {}): Parameters<MyWorkParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            let work = crate::api::work::work_for(conn, user)?;
            Ok(crate::api::work::render_text(&work))
        })
        .await
    }

    // KAIROS-T-0382: the notifications of the person of the key. The read
    // state is of the caller (KAIROS-T-0550): the agent key, or the person.
    #[tool(
        description = "Your notifications: a claim handed to you, a blocker of your claimed task closed, a task you created cancelled, a mention (`@name`), a P0 bug on your team's board. Unread first (newest first, 100 at most), then the 20 newest read ones, each with its id. `before` (an id) gives the next page of unread ones. An agent key keeps its own read state: a read by the key does not clear the person's bell. While some are unread, each other tool's answer ends with the 5 newest. Mark them read with `mark_events_read`."
    )]
    pub async fn get_events(
        &self,
        Parameters(params): Parameters<GetEventsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, key) = (auth.user_id, auth.agent_key);
        self.run_tool(&tenant, move |conn| {
            let reader = crate::api::notifications::reader_for(conn, user, key)?;
            let before = params.before.as_deref();
            let (items, unread) = crate::api::notifications::list_for(conn, &reader, before, None)?;
            Ok(events_text(&items, unread, before.is_some()))
        })
        .await
    }

    #[tool(
        description = "Mark your notifications read: `ids` (the ids that `get_events` gives), or `all: true` with `up_to` (the id of the newest notification that you read). `all` marks that one and each older unread one, and no newer one; `all` without `up_to` is refused. A read with an agent key is the key's: the person's bell keeps its own state. A read notification no longer comes at the end of each answer. An id that is not one of your notifications is refused and named, and nothing changes."
    )]
    pub async fn mark_events_read(
        &self,
        Parameters(params): Parameters<MarkEventsReadParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let (user, key) = (auth.user_id, auth.agent_key);
        self.run_tool(&tenant, move |conn| {
            let reader = crate::api::notifications::reader_for(conn, user, key)?;
            let (changed, unread) = crate::api::notifications::mark_for(
                conn,
                &reader,
                params.ids,
                params.all,
                params.up_to,
            )?;
            Ok(format!(
                "Marked {} notification(s) read. You have {unread} unread.",
                changed.len()
            ))
        })
        .await
    }

    // COLLIERY-T-0269: the change of the owner board of a document is in
    // this tool, and not in `update_item`. `update_item` replaces content
    // under a version check, and the edit rule is its gate. The change of
    // an owner is no content: it writes no version. Its gate is the gate
    // of a move: the capability on the board that the item leaves and on
    // the board that it goes to, and nothing for the creator.
    #[tool(
        description = "Move a TASK to another team's delivery board, or a DOCUMENT to another owner board (`to_board`: slug or UUID), instead of recreating it. A task lands in the target's entry column and gets its team; it keeps its repository. Needs `manage_tasks` (a document: `manage_documents`) on both boards; creating the item gives no right. `rename: true` gives the item the next code of the target board; the old code is retired, still finds the item, and is replaced once in the text of items. Between columns of one board, use `transition_item`."
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
            let to_board = crate::api::documents::board_to_set(params.to_board.as_deref());
            if item.item_type == ItemType::Document {
                let to_board = to_board
                    .ok_or_else(|| crate::api::documents::board_is_required("field", "to_board"))?;
                return move_document(conn, &slug, user, &item, to_board, params.rename);
            }
            if item.item_type != ItemType::Task {
                return Err(ApiError::validation(format!(
                    "The {} {} is not a task and not a document. Only a task can move to \
                     a different delivery board, and only a document can move to a \
                     different owner board. Use transition_item to move an item between \
                     the columns of its board.",
                    item.item_type, item.short_code
                )));
            }
            let to_board = to_board.ok_or_else(|| {
                ApiError::validation(format!(
                    "The call has no `to_board`. Send the slug or the id of the delivery \
                     board that the task {} moves to.",
                    item.short_code
                ))
                .with_details(json!({ "field": "to_board" }))
            })?;
            let from_board_id = item.board_id.ok_or_else(|| {
                ApiError::unprocessable(
                    "ITEM_NOT_ON_BOARD",
                    format!("The task {} is not on a board.", item.short_code),
                )
            })?;
            let target = board_by_ref(conn, to_board)?;
            // Two-sided: the work leaves one team's board and lands on
            // another's, so the caller needs the capability on the two.
            //
            // NOT an edit (COLLIERY-T-0228): creation grants no right here.
            // The creator of a task needs the capability on the two boards
            // as all others do, because a team controls its own plan
            // (COLLIERY-T-0218).
            require_capability_explained(
                conn,
                &slug,
                Some(from_board_id),
                user,
                "manage_tasks",
                &item,
            )?;
            require_capability_explained(
                conn,
                &slug,
                Some(target.id),
                user,
                "manage_tasks",
                &item,
            )?;
            // KAIROS-T-0456: a move to another team is a triage of intake.
            let triaged = crate::agent_triage::check(
                conn,
                &auth,
                item.id,
                crate::agent_triage::TriageMove::Board(target.id),
            )?;
            crate::claims::note(conn, item.id, user)?;
            let from_board = board_by_ref(conn, &from_board_id.to_string())?;
            let moved = boards::move_task_with(conn, item.id, target.id, user, params.rename)
                .map_err(map_board_error)?;
            crate::agent_triage::record(conn, &auth, item.id, triaged)?;
            // KAIROS-T-0371: the entry column of the target over its limit.
            let moved_code = moved
                .rename
                .as_ref()
                .map_or(item.short_code.clone(), |rename| rename.new_code.clone());
            // KAIROS-T-0565: and the open blockers.
            crate::claims::note_task_move(conn, item.id, moved.column_id, &moved_code)?;
            let column = board_columns(conn, target.id)?
                .into_iter()
                .find(|c| c.id == moved.column_id)
                .map(|c| c.name)
                .unwrap_or_default();
            let mut out = format!(
                "Moved {}: {} -> {} / {}.",
                item.short_code, from_board.slug, target.slug, column
            );
            if let Some(rename) = &moved.rename {
                out.push_str(&rename_lines(rename));
            }
            Ok(out)
        })
        .await
    }

    // COLLIERY-T-0220 (COLLIERY-A-0023): the repository of a task is a link,
    // and REST, the CLI and the GUI could set it while MCP could not. An
    // agent that found that a task belongs to the codebase it stands in had
    // to leave MCP for `kairos repos bind`.
    //
    // It is a tool of its own and not an argument of `update_item`. That
    // tool replaces content under a version check. A link is not content: it
    // writes no new version, and it must not fail on a stale one.
    #[tool(
        description = "Set or clear the repository of a TASK: a link that says where the code is; it changes neither the board nor the team. `repository` is a slug or UUID of any live repository; omit it, or send null or \"\", to clear. You can set it on a task you created, or with `manage_tasks` on its board."
    )]
    pub async fn set_repository(
        &self,
        Parameters(params): Parameters<SetRepositoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if item.item_type != ItemType::Task {
                return Err(ApiError::validation(format!(
                    "The {} {} is not a task. set_repository applies to tasks only.",
                    item.item_type, item.short_code
                )));
            }
            // The same gate as PUT /api/tasks/{short_code}/repository, and
            // at the same point: before the repository is resolved. A caller
            // who cannot write the task does not learn from the answer which
            // repositories exist. The gate is the edit rule
            // (COLLIERY-T-0228): the repository is a fact about the task,
            // and it moves nothing.
            require_item_edit(conn, role, user, item.id, item.item_type)?;
            // An agent that builds its arguments from a template sends ""
            // for "none" as often as it sends null. Both clear the link, as
            // an absent argument does. The shared function decides that, for
            // REST too (COLLIERY-T-0231).
            let updated = crate::api::tasks::link_task_to_repository(
                conn,
                item.id,
                params.repository.as_deref(),
                user,
            )?;
            Ok(match updated.repository_id {
                Some(_) => format!(
                    "Set the repository of {}: {}.",
                    updated.short_code,
                    repo_label(conn, updated.repository_id)?
                ),
                None => format!(
                    "Cleared the repository of {}: the task has no repository.",
                    updated.short_code
                ),
            })
        })
        .await
    }

    // KAIROS-T-0572: an item has one parent. A change of parent was an
    // unlink and a link, two calls, with a moment of no parent between
    // them, or of two. One tool, one transaction, by the function that
    // PUT /api/{family}/{short_code}/parent calls.
    #[tool(
        description = "Set the parent of an initiative (a strategy) or a task (an initiative), or remove it with `parent: null`. An item has one parent: the new parent replaces the old one in one transaction. The same parent again changes nothing. You need to be able to edit the item, or the parent at each end that changes. A change of parent is not a move: the board rules do not apply."
    )]
    pub async fn set_parent(
        &self,
        Parameters(params): Parameters<SetParentParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let Some(parent) = params.parent else {
                return Err(ApiError::validation(
                    "The argument parent is required. Send the short code of the new parent, \
                     or null to remove the parent.",
                )
                .with_details(json!({ "field": "parent" })));
            };
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            let (change, renamed) = crate::api::meta::relationships::change_parent(
                conn,
                role,
                user,
                (item.id, item.item_type),
                parent.as_deref(),
                "set_parent",
            )?;
            let was = change.previous_parent.as_deref().unwrap_or("none");
            let mut out = match (&change.parent, change.changed) {
                (Some(now), true) => format!(
                    "Set the parent of {}: {now} (was {was}).",
                    change.short_code
                ),
                (None, true) => format!(
                    "Removed the parent of {}: the item has no parent (was {was}).",
                    change.short_code
                ),
                (Some(now), false) => format!(
                    "{} has the parent {now} already. Nothing changed.",
                    change.short_code
                ),
                (None, false) => format!("{} has no parent. Nothing changed.", change.short_code),
            };
            if let Some(line) = renamed {
                out.push_str(&format!("\n{line}"));
            }
            Ok(out)
        })
        .await
    }

    // KAIROS-T-0433: the lane of a task could change only on the GUI and
    // REST. A tool of its own, as `set_repository`: the lane is not
    // content, and it must not fail on a stale version. REST and this tool
    // call one function (`crate::api::tasks::change_task_lane`).
    #[tool(
        description = "Change the lane of a TASK: `work_class` is planned or support. Moving a support request to the planned backlog is a lane change. The task stays in its column. A lane the task already has changes nothing. You need `transition_items` on the board of the task; creating the task gives no right here. `create_item` sets the lane of a new task. An agent's move of intake to planned is refused with AGENT_TRIAGE_OFF while the board lets a person decide: write the proposal with `append_note`."
    )]
    pub async fn set_work_class(
        &self,
        Parameters(params): Parameters<SetWorkClassParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let work_class = parse_enum(&params.work_class, "work_class", WorkClass::ALL)?;
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if item.item_type != ItemType::Task {
                return Err(ApiError::validation(format!(
                    "The {} {} is not a task. set_work_class applies to tasks only.",
                    item.item_type, item.short_code
                )));
            }
            let (Some(board_id), Some(before)) = (item.board_id, item.work_class) else {
                return Err(ApiError::internal("a task has a board and a lane"));
            };
            let updated = crate::api::tasks::change_task_lane(
                conn, &slug, &auth, item.id, board_id, work_class,
            )?;
            Ok(if before == updated.work_class {
                format!(
                    "{} is already in the {} lane: nothing changed.",
                    updated.short_code, updated.work_class
                )
            } else {
                format!(
                    "Moved {} to the {} lane (was {}).",
                    updated.short_code, updated.work_class, before
                )
            })
        })
        .await
    }

    // The complexity of an initiative could only be set at creation. A
    // tool of its own, as `set_lifecycle`: complexity is not content, and
    // it must not fail on a stale version. REST and this tool call one
    // function (`crate::api::initiatives::change_complexity`).
    #[tool(
        description = "Set the complexity of an INITIATIVE: `complexity` is xs, s, m, l or xl, or none to clear it; it is required. A value that the initiative already has changes nothing. The edit rule applies: the creator of the initiative, a member of its board, `manage_initiatives` on its board, or an organization admin. `create_item` sets the complexity of a new initiative."
    )]
    pub async fn set_complexity(
        &self,
        Parameters(params): Parameters<SetComplexityParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if item.item_type != ItemType::Initiative {
                return Err(ApiError::validation(format!(
                    "The {} {} is not an initiative. set_complexity applies to initiatives only.",
                    item.item_type, item.short_code
                )));
            }
            let (before, updated) = crate::api::initiatives::change_complexity(
                conn,
                role,
                user,
                &item.short_code,
                &params.complexity,
            )?;
            let name = |c: Option<Complexity>| c.map_or("none".to_string(), |c| c.to_string());
            Ok(match (before == updated.complexity, updated.complexity) {
                (true, Some(now)) => format!(
                    "{} already has the complexity {now}: nothing changed.",
                    updated.short_code
                ),
                (true, None) => {
                    format!("{} has no complexity: nothing changed.", updated.short_code)
                }
                (false, Some(now)) => format!(
                    "Set the complexity of {} to {now} (was {}).",
                    updated.short_code,
                    name(before)
                ),
                (false, None) => format!(
                    "Cleared the complexity of {} (was {}).",
                    updated.short_code,
                    name(before)
                ),
            })
        })
        .await
    }

    // KAIROS-T-0505: a note becomes a draft ADR or document.
    #[tool(
        description = "Turn a note into a draft ADR or document in one call: `short_code` of the item, `note_id` as `get_item` lists it (n-3), `kind` adr | document, `board` (slug or UUID) that gets the new item, optional `title` (default: the first line of the note). The new item has the text of the note and a line `From note n-3 on CODE (author, date)`; it `supports` the item when the item is a strategy, an initiative or a task. The note gets the mark `Promoted to <code>`. A note is promoted one time only: a second promote is refused with CONFLICT and the code. Needs the edit rule on the item and the right to create on the board (manage_adrs or manage_documents)."
    )]
    pub async fn promote_note(
        &self,
        Parameters(params): Parameters<PromoteNoteParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            let promoted = atomically(conn, |conn| {
                crate::promote::promote_note(
                    conn,
                    &slug,
                    role,
                    user,
                    crate::promote::PromoteInput {
                        source: (item.id, item.item_type, &item.short_code),
                        note_id: &params.note_id,
                        kind: &params.kind,
                        board: &params.board,
                        title: params.title.as_deref(),
                    },
                    crate::api::documents::CreateWording {
                        request: "call",
                        details_key: "field",
                        board: "`board`",
                        parent: "`board`",
                    },
                )
            })?;
            let mut out = format!(
                "Promoted note {} on {} to the draft {} {}.",
                promoted.note_id, promoted.source_code, promoted.kind, promoted.created_code
            );
            if promoted.supports {
                out.push_str(&format!(
                    " {} supports {}.",
                    promoted.created_code, promoted.source_code
                ));
            } else {
                out.push_str(&format!(
                    " No edge: {} is a {}, and a supports edge joins a strategy, an initiative or a task.",
                    promoted.source_code, item.item_type
                ));
            }
            out.push_str(&format!(
                " The note has the line \"Promoted to {}.\"",
                promoted.created_code
            ));
            Ok(out)
        })
        .await
    }

    // KAIROS-T-0555: a document moves between draft, review, published
    // and archived. A document is not on a board (KAIROS-A-0018), so
    // `transition_item` cannot move it. REST and this tool call one
    // function (`crate::api::documents::change_lifecycle`).
    #[tool(
        description = "Set the lifecycle of a DOCUMENT: `lifecycle` is draft, review, published or archived. You can move a document from each value to each other value. A document is not on a board, so `transition_item` does not move it. A value that the document already has changes nothing. The edit rule applies: the creator of the document, or `manage_documents` on its owner board. `promote_note` makes a draft: publish it with this tool."
    )]
    pub async fn set_lifecycle(
        &self,
        Parameters(params): Parameters<SetLifecycleParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let lifecycle = crate::api::documents::parse_lifecycle(&params.lifecycle)?;
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if item.item_type != ItemType::Document {
                return Err(ApiError::validation(format!(
                    "The {} {} is not a document. set_lifecycle applies to documents only.",
                    item.item_type, item.short_code
                )));
            }
            let (before, updated) = crate::api::documents::change_lifecycle(
                conn,
                role,
                user,
                &item.short_code,
                lifecycle,
            )?;
            Ok(if before == lifecycle {
                format!(
                    "{} is already {}: nothing changed.",
                    updated.short_code, updated.lifecycle
                )
            } else {
                format!(
                    "Set the lifecycle of {} to {} (was {}).",
                    updated.short_code, updated.lifecycle, before
                )
            })
        })
        .await
    }

    // KAIROS-T-0506: the live state of a task.
    #[tool(
        description = "Write the live state of a TASK: `branch`, `commit`, `pushed`, `pr_url`, `next_step` (send at least one). The write is a patch: a field that you send is set, `null` clears it, a field that you do not send is kept. `expected_updated_at` (the `updated_at` you read) refuses a write over a state that changed since, with CONFLICT and the current state. A hand-off or a release clears `next_step`. The state is not a note: it is one current line that `get_item`, `board_items` and the card show, with who wrote it when. Write it after each commit, each push, a PR, and before you stop. The edit rule applies: the creator of the task, or `manage_tasks` on its board."
    )]
    pub async fn set_state(
        &self,
        Parameters(params): Parameters<SetStateParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if item.item_type != ItemType::Task {
                return Err(ApiError::validation(format!(
                    "The {} {} is not a task. set_state applies to tasks only.",
                    item.item_type, item.short_code
                )));
            }
            let body = kairos_client::types::SetTaskStateRequest {
                branch: params.branch,
                commit: params.commit,
                pushed: params.pushed,
                pr_url: params.pr_url,
                next_step: params.next_step,
                expected_updated_at: params.expected_updated_at,
            };
            let state = crate::task_state::write_state(conn, role, user, item.id, &body)?;
            Ok(format!(
                "State of {}: {}",
                item.short_code,
                crate::task_state::state_text(&state, chrono::Utc::now())
            ))
        })
        .await
    }

    // KAIROS-T-0321: set and clear a team of an initiative or a strategy by
    // hand. Tools of their own, as `set_repository`: a link is not content,
    // and it must not fail on a stale version. The rules are those of
    // `POST` and `DELETE /api/{entity_type}/{short_code}/teams`: the two
    // call one function (`crate::api::meta::item_teams`).
    #[tool(
        description = "Set a team by hand on an INITIATIVE or STRATEGY (for example before it has tasks); it adds to the teams that come from its tasks. `team` is a slug or UUID. A team gives no right. You can set it on an item you created, or with `manage_<type>` on its board. A task, document or ADR is refused."
    )]
    pub async fn set_team(
        &self,
        Parameters(params): Parameters<ItemTeamParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            let team = crate::api::meta::item_teams::add(
                conn,
                role,
                user,
                (item.id, item.item_type),
                &params.team,
            )?;
            Ok(format!(
                "Set the team {} on {} by hand.",
                team.slug, item.short_code
            ))
        })
        .await
    }

    #[tool(
        description = "Clear a team that is set on an INITIATIVE or a STRATEGY by hand (`set_team`). A team that the item gets from its tasks stays: to remove it, move or archive those tasks. `team` is a slug or UUID; the team can be archived. You can clear it on an item that you created, or with `manage_<type>` on its board."
    )]
    pub async fn clear_team(
        &self,
        Parameters(params): Parameters<ItemTeamParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            let team = crate::api::meta::item_teams::remove(
                conn,
                role,
                user,
                (item.id, item.item_type),
                &params.team,
            )?;
            Ok(format!(
                "Cleared the team {team} set by hand on {}.",
                item.short_code
            ))
        })
        .await
    }

    // COLLIERY-T-0266: REST, the CLI and the GUI could add a repository to
    // the directory while MCP could not. The bootstrap of a checkout found
    // that the tenant did not have its repository, and it had to leave MCP
    // for `kairos repos create`.
    //
    // The rule, the defaults and the refusals are those of
    // `POST /api/repositories`: the two call one function
    // (`crate::api::org::repositories::add`).
    #[tool(
        description = "Add a repository to the directory (once: call `list_repositories` first). `forge`: github, gitlab or other. `repo_full_name`: the name on the forge (owner/repo for github; group/…/project for gitlab), no .git. `repo_url`: absolute http or https, no credentials. `team` (slug or UUID): the one owner team. Optional: `slug` (default from the full name: acme/payments-api gives acme-payments-api), `default_branch` (set by hand; with none the code index follows the forge), `description` (how to work in it: task runner, tests, branch rules). Needs membership of the owner team, or the org admin role. `set_repository` links a task; `update_repository` changes the slug and the owner team; `delete_repository` (admin profile) removes it."
    )]
    pub async fn add_repository(
        &self,
        Parameters(params): Parameters<AddRepositoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let added = crate::api::org::repositories::add(
                conn,
                &slug,
                user,
                kairos_client::types_repositories::CreateRepositoryRequest {
                    slug: params.slug,
                    forge: params.forge,
                    repo_full_name: params.repo_full_name,
                    repo_url: params.repo_url,
                    default_branch: params.default_branch,
                    team: params.team,
                    description: params.description,
                },
            )?;
            Ok(format!(
                "Added repository {}: {} {} (owner: {}, default branch {}).",
                added.slug,
                added.forge,
                added.repo_full_name,
                added.team.slug,
                added.default_branch
            ))
        })
        .await
    }

    // COLLIERY-T-0266. KAIROS-T-0443 added `slug` and `team`, as the REST
    // route has them. A checkout keeps the slug in
    // `.claude/kairos.local.md`, so a new slug breaks each checkout. A new
    // owner team needs the right on the two teams.
    //
    // The gate and the write are those of `PATCH /api/repositories/{slug}`
    // (`crate::api::org::repositories::change`). That function compares the
    // values of the call with the row, and it writes only when a value is
    // different. An agent sends the same description again more often than
    // a person does, and each write is a row in the activity log. Until
    // COLLIERY-T-0267 the tool made the comparison, and the REST route
    // always wrote.
    #[tool(
        description = "Change a repository of the directory: `description` (how to work in it; replaces the full text, so read it with `get_repository` first), `default_branch` (set by hand; the code index builder indexes it), `default_branch_source: forge` (follow the forge's default branch again; not with `default_branch`), `repo_url` (absolute http or https, no credentials), `code_index_build` (`on` or `off`; off cancels waiting builds), `slug` (breaks `.claude/kairos.local.md` in each checkout: run /kairos:bootstrap there), the owner `team` (you need the right on both teams), `forge_filters` (org admin: include and exclude regex per field for the branches, pull requests and issues that the forge webhook picks up), and `issue_import_interval` (org admin: `off`, or `15m`, `1h`, `1d`: the repository takes its open issues on that cadence, with a read token and no webhook). Send one or more; a value it has already changes nothing. `repository` is a slug or UUID. Needs membership of the owner team, or the org admin role. `set_repository` links a task; `delete_repository` removes a repository."
    )]
    pub async fn update_repository(
        &self,
        Parameters(params): Parameters<UpdateRepositoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            // Before the gate: the fault is in the call, and the answer is
            // the same for each caller and each repository.
            if params.description.is_none()
                && params.default_branch.is_none()
                && params.default_branch_source.is_none()
                && params.repo_url.is_none()
                && params.code_index_build.is_none()
                && params.slug.is_none()
                && params.team.is_none()
                && params.forge_filters.is_none()
                && params.issue_import_interval.is_none()
            {
                return Err(ApiError::validation(
                    "The call has nothing to change. Send one or more of these arguments: \
                     description, default_branch, default_branch_source, repo_url, \
                     code_index_build, slug, team, forge_filters, issue_import_interval.",
                ));
            }
            // KAIROS-T-0430: refuse an unknown value, and name it.
            let default_branch_source = params
                .default_branch_source
                .as_deref()
                .map(|value| {
                    kairos_client::types_repositories::DefaultBranchSource::parse(value).ok_or_else(
                        || {
                            ApiError::validation(format!(
                                "The argument default_branch_source has the value {value:?}. \
                                 Send \"forge\" or \"hand\"."
                            ))
                            .with_details(json!({ "field": "default_branch_source" }))
                        },
                    )
                })
                .transpose()?;
            // KAIROS-T-0398: refuse an unknown value, and name it.
            let code_index_build = params
                .code_index_build
                .as_deref()
                .map(|value| {
                    kairos_client::types_repositories::CodeIndexBuild::parse(value).ok_or_else(
                        || {
                            ApiError::validation(format!(
                                "The argument code_index_build has the value {value:?}. \
                                 Send \"on\" or \"off\"."
                            ))
                            .with_details(json!({ "field": "code_index_build" }))
                        },
                    )
                })
                .transpose()?;
            // COLLIERY-T-0267: the comparison with the values of the row is
            // in `change`, so REST and this tool do the same.
            let change = crate::api::org::repositories::change(
                conn,
                &slug,
                user,
                &params.repository,
                kairos_client::types_repositories::UpdateRepositoryRequest {
                    description: params.description,
                    default_branch: params.default_branch,
                    default_branch_source,
                    repo_url: params.repo_url,
                    code_index_build,
                    slug: params.slug,
                    team: params.team,
                    forge_filters: params.forge_filters.map(Into::into),
                    issue_import_interval: params.issue_import_interval,
                    ..Default::default()
                },
            )?;
            if change.changed.is_empty() {
                return Ok(format!(
                    "No change to repository {}: it has these values already.",
                    change.repository.slug
                ));
            }
            // The sequence of the names is that of the result text of
            // COLLIERY-T-0266.
            let changed: Vec<&str> = [
                "description",
                "default_branch",
                "default_branch_source",
                "repo_url",
                "code_index_build",
                "slug",
                "team",
                "forge_filters",
                "issue_import_interval",
            ]
            .into_iter()
            .filter(|name| change.changed.contains(name))
            .collect();
            let mut text = format!(
                "Updated repository {}: {}.",
                change.repository.slug,
                changed.join(", ")
            );
            // KAIROS-T-0398: say what the switch did to the queue.
            if change.repository.code_index_build
                == kairos_client::types_repositories::CodeIndexBuild::Off
                && change.changed.contains(&"code_index_build")
            {
                text.push_str(&format!(
                    " The code index builder is off: {} waiting build job(s) cancelled.",
                    change.cancelled_builds
                ));
            }
            Ok(text)
        })
        .await
    }

    #[tool(
        description = "Ask for a full build of the code index of a repository (the head of its default branch), when the index is missing, old or wrong. It also resets a tripped code index breaker. `repository` is a slug or UUID; the answer names the run. Refused: CODE_INDEX_BUILD_RUNNING (a run is active), CODE_INDEX_BUILD_OFF (set `code_index_build` on first), CODE_INDEX_BUILDER_OFF (no builder on this deployment). Needs membership of the owner team, or the org admin role."
    )]
    pub async fn rebuild_code_index(
        &self,
        Parameters(params): Parameters<RebuildCodeIndexParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let instance = match crate::api::org::code_indexes::rebuild_instance(&self.state) {
            Ok(instance) => instance,
            Err(e) => return Ok(tool_error(e)),
        };
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        let reference = params.repository;
        let max_tries = self.state.config.jobs.max_tries;
        let requested = self
            .state
            .blocking
            .run(&tenant.slug, move |conn| {
                crate::api::org::code_indexes::request_rebuild(
                    conn, &slug, user, &reference, instance, max_tries,
                )
            })
            .await;
        match requested {
            Ok((repo, run)) => {
                self.state.jobs.wake();
                Ok(tool_text(format!(
                    "Kairos builds the code index of the repository {} again: run {}, started {}. \
                     Read the runs with GET /api/repositories/{}/code-indexes/builds or \
                     `kairos repos builds {}`.",
                    repo.slug,
                    run.id,
                    run.started_at.to_rfc3339(),
                    repo.slug,
                    repo.slug
                )))
            }
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Move an item to another column on its board (`to_column` is a column name or UUID). An invalid move fails with the allowed target columns enumerated — pick one and retry. Needs `transition_items` on the board. The creator of the item gets no right to move it. A task that you move to Active gets your claim: it shows that you work on it. The claim ends when the task leaves Active. A move to Active by a service account gives no claim. An agent's move of intake (a support-lane task in the entry column) is refused with AGENT_TRIAGE_OFF while the board lets a person decide: write the proposal with `append_note`."
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
                        if item.item_type == ItemType::Document {
                            format!(
                                "The document {} is not on a board, so it cannot move between \
                                 columns. Use set_lifecycle to move it between draft, review, \
                                 published and archived.",
                                item.short_code
                            )
                        } else {
                            format!(
                                "The {} {} is not on a board, so it cannot move between columns.",
                                item.item_type, item.short_code
                            )
                        },
                    ));
                }
            };
            // NOT an edit (COLLIERY-T-0228): creation grants no right here.
            // The creator of an item needs this capability as all others do,
            // because a team controls its own plan (COLLIERY-T-0218).
            require_capability_explained(
                conn,
                &slug,
                Some(board_id),
                user,
                "transition_items",
                &item,
            )?;
            let columns = board_columns(conn, board_id)?;
            let to_column_id = resolve_column(&columns, &params.to_column)?;
            let mut triaged = None;
            if item.item_type == ItemType::Task {
                // KAIROS-T-0428: an agent's triage of intake.
                triaged = crate::agent_triage::check(
                    conn,
                    &auth,
                    item.id,
                    crate::agent_triage::TriageMove::Column(to_column_id),
                )?;
                crate::claims::note(conn, item.id, user)?;
            }

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
            crate::agent_triage::record(conn, &auth, item.id, triaged)?;
            // KAIROS-T-0371: a move into a column over its WIP limit.
            // KAIROS-T-0565: the open blockers of a task.
            if item.item_type == ItemType::Task {
                crate::claims::note_task_move(conn, item.id, to_column_id, &item.short_code)?;
            } else {
                crate::claims::note_wip(conn, to_column_id, &item.short_code)?;
            }

            let name_of = |id: Uuid| {
                columns
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            };
            let mut out = format!(
                "Transitioned {}: {} -> {}.",
                item.short_code,
                name_of(from_column_id),
                name_of(to_column_id)
            );
            // KAIROS-T-0359: a move into Active gives the claim.
            if item.item_type == ItemType::Task {
                let claim = crate::claims::claim_text(conn, item.id, Some(to_column_id))?;
                if !claim.is_empty() {
                    out.push('\n');
                    out.push_str(claim.trim_end());
                }
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Take a free TASK in Active (no claim): the claim names you, so the board shows who works it. Or become the owner of a STRATEGY or INITIATIVE that has none: the one person who answers for it. A task: you need `transition_items` on its board; a task that a different person holds is refused with CLAIMED and the holder's name (ask for `hand_off_item`). A strategy or initiative: you must be a person and a member of its board (a grant), or an org admin; an item with an owner is refused with the owner's name. The owner gives no extra right. Moving a task to Active gives you the claim too."
    )]
    pub async fn claim_item(
        &self,
        Parameters(params): Parameters<ClaimItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if kairos_db::item_owners::is_subject(item.item_type) {
                crate::owners::claim(
                    conn,
                    &slug,
                    user,
                    (item.id, item.item_type),
                    &item.short_code,
                )?;
                return Ok(format!(
                    "Claimed {}.\n{}",
                    item.short_code,
                    crate::owners::owner_text(conn, item.id, item.item_type)?.trim_end()
                ));
            }
            // KAIROS-T-0528: a task in Active with no claim.
            let item = task_for_claim(item)?;
            let board_id = item.board_id.unwrap_or_default();
            crate::claims::require_claim_take(conn, &slug, board_id, user)?;
            kairos_db::task_claims::claim(conn, item.id, user)
                .map_err(crate::claims::map_claim_error)?;
            Ok(format!(
                "Claimed {}.\n{}",
                item.short_code,
                crate::claims::claim_text(conn, item.id, item.column_id)?.trim_end()
            ))
        })
        .await
    }

    #[tool(
        description = "Give the claim of a TASK in Active, or the owner of a STRATEGY or INITIATIVE, to another person (`to`: user id, email, user name or `me`; not a service account). A task stays in Active; you can hand off your own claim, or any claim with `transition_items` on its board; the person must hold `transition_items` on that board too (team, grant or org admin), else VALIDATION names the board. A strategy or initiative: the owner hands it off, or an org admin sets it; the new owner must be a member of its board (else NOT_BOARD_MEMBER). The person gets a notification."
    )]
    pub async fn hand_off_item(
        &self,
        Parameters(params): Parameters<HandOffItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if kairos_db::item_owners::is_subject(item.item_type) {
                crate::owners::hand_off(
                    conn,
                    &slug,
                    user,
                    (item.id, item.item_type),
                    &item.short_code,
                    &params.to,
                )?;
                return Ok(format!(
                    "Handed off {}.\n{}",
                    item.short_code,
                    crate::owners::owner_text(conn, item.id, item.item_type)?.trim_end()
                ));
            }
            let item = task_for_claim(item)?;
            let board_id = item.board_id.unwrap_or_default();
            crate::claims::require_claim_change(conn, &slug, item.id, board_id, user)?;
            let to = crate::claims::resolve_person(conn, &slug, &params.to, user)?;
            crate::claims::require_claim_target(
                conn,
                &slug,
                board_id,
                to,
                &params.to,
                &item.short_code,
            )?;
            kairos_db::task_claims::hand_off(conn, item.id, to, user)
                .map_err(crate::claims::map_claim_error)?;
            Ok(format!(
                "Handed off {}.\n{}",
                item.short_code,
                crate::claims::claim_text(conn, item.id, item.column_id)?.trim_end()
            ))
        })
        .await
    }

    #[tool(
        description = "End the claim on a TASK in Active (it stays in Active, free for anyone: `claim_item` takes it), or remove the owner of a STRATEGY or INITIATIVE. A task: your own claim, or any with `transition_items` on its board. A strategy or initiative: the owner, or an org admin."
    )]
    pub async fn release_item(
        &self,
        Parameters(params): Parameters<ReleaseItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            if kairos_db::item_owners::is_subject(item.item_type) {
                let ended = crate::owners::release(
                    conn,
                    &slug,
                    user,
                    (item.id, item.item_type),
                    &item.short_code,
                )?;
                let name = kairos_db::task_claims::display_names(conn, &[ended.user_id])
                    .map_err(ApiError::internal)?
                    .remove(&ended.user_id)
                    .unwrap_or_else(|| ended.user_id.to_string());
                return Ok(format!(
                    "Released {}: {name} is not its owner now. It has no owner, and a member of \
                     its board can claim it.",
                    item.short_code
                ));
            }
            let item = task_for_claim(item)?;
            let board_id = item.board_id.unwrap_or_default();
            crate::claims::require_claim_change(conn, &slug, item.id, board_id, user)?;
            let ended = kairos_db::task_claims::release(conn, item.id, user)
                .map_err(crate::claims::map_claim_error)?;
            let name = kairos_db::task_claims::display_names(conn, &[ended.user_id])
                .map_err(ApiError::internal)?
                .remove(&ended.user_id)
                .unwrap_or_else(|| ended.user_id.to_string());
            Ok(format!(
                "Released {}: {name} has no claim on it now. The task stays in Active, and it \
                 has no claim.",
                item.short_code
            ))
        })
        .await
    }

    #[tool(
        description = "Create an edge between two items by short code: parent, supports, informs, supersedes or blocks (type rules and cycles are checked). `blocks`: the target waits for the source; add it when a task must wait, none between parallel tasks. You need to be able to edit one of the two: you created it, or you hold `manage_<type>` on its board. A `blocks` edge needs the target: if you can edit only the source, the call makes a proposal, and the answer names it and who can confirm it. `impacts` goes from a document or an ADR (`source`) to a repository (`target`: slug or UUID): it says what the item is about and gives no right. A task links to a repository with `set_repository`. An item has one parent: `set_parent` replaces it. `external` links any item (`source`) to a URI (`target`: http, https or mailto) with an optional `label`: an RFC, a design page or an issue that Kairos does not hold; `get_item` and `related_work` show it."
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
            // KAIROS-T-0508: the target of `external` is a URI.
            if params.relationship == kairos_db::external_refs::RELATIONSHIP {
                let source = require_live_typed(conn, &params.source, "source")?;
                let made = crate::api::meta::external::add(
                    conn,
                    tenant_ctx.role,
                    user,
                    source.typed(),
                    &params.target,
                    params.label.as_deref(),
                )?;
                return Ok(format!(
                    "Linked {} -[external]-> {}.{}",
                    source.code,
                    external_ref_text(&made),
                    renamed_lines(&[&source])
                ));
            }
            if params.label.is_some() {
                return Err(ApiError::validation(format!(
                    "The parameter label applies to the relationship external only, not to {:?}.",
                    params.relationship
                )));
            }
            // COLLIERY-T-0269: the target of `impacts` is a repository.
            if params.relationship == kairos_db::impacts::RELATIONSHIP {
                let source = require_live_typed(conn, &params.source, "source")?;
                let linked = crate::api::meta::impacts::add(
                    conn,
                    tenant_ctx.role,
                    user,
                    source.typed(),
                    &params.target,
                    crate::api::meta::impacts::Surface::Mcp,
                )?;
                return Ok(format!(
                    "Linked {} -[impacts]-> repository {}.{}",
                    source.code,
                    linked.slug,
                    renamed_lines(&[&source])
                ));
            }
            let relationship = parse_relationship(&params.relationship)?;
            let source = require_live_typed(conn, &params.source, "source")?;
            let target = require_live_typed(conn, &params.target, "target")?;
            // COLLIERY-T-0228, KAIROS-T-0569: the link rule and the write,
            // by the ONE function that REST `POST /api/relationships` calls.
            match crate::api::link_or_propose(
                conn,
                tenant_ctx.role,
                user,
                relationship,
                (source.typed(), target.typed()),
                map_link_error,
            )? {
                crate::api::LinkOutcome::Linked(_) => Ok(format!(
                    "Linked {} -[{relationship}]-> {}.{}",
                    source.code,
                    target.code,
                    renamed_lines(&[&source, &target])
                )),
                crate::api::LinkOutcome::Proposed { proposal, boards } => Ok(format!(
                    "{}{}",
                    crate::api::link_proposal_text(
                        &source.code,
                        &target.code,
                        relationship.as_str(),
                        &proposal,
                        &boards,
                    ),
                    renamed_lines(&[&source, &target])
                )),
            }
        })
        .await
    }

    #[tool(
        description = "Remove a relationship edge between two items (by short code and relationship type). The rule of link_items applies: you can edit one of the two items. To remove a `supports` edge of a document, you must be able to edit the document. The owner board of the document does not change, so a document can lose its last `supports` edge. For the relationship `impacts`, `target` is the slug or the UUID of the repository, and you must be able to edit the document or the ADR. For `external`, `target` is the URI of the reference."
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
            // KAIROS-T-0508: the target of `external` is a URI.
            if params.relationship == kairos_db::external_refs::RELATIONSHIP {
                let source = require_live_typed(conn, &params.source, "source")?;
                let removed = crate::api::meta::external::remove(
                    conn,
                    tenant_ctx.role,
                    user,
                    source.typed(),
                    &params.target,
                )?;
                return Ok(format!(
                    "Unlinked {} -[external]-> {}.{}",
                    source.code,
                    external_ref_text(&removed),
                    renamed_lines(&[&source])
                ));
            }
            // COLLIERY-T-0269: the target of `impacts` is a repository.
            if params.relationship == kairos_db::impacts::RELATIONSHIP {
                let source = require_live_typed(conn, &params.source, "source")?;
                let removed = crate::api::meta::impacts::remove(
                    conn,
                    tenant_ctx.role,
                    user,
                    source.typed(),
                    &params.target,
                    crate::api::meta::impacts::Surface::Mcp,
                )?;
                return Ok(format!(
                    "Unlinked {} -[impacts]-> repository {}.{}",
                    source.code,
                    removed.slug,
                    renamed_lines(&[&source])
                ));
            }
            let relationship = parse_relationship(&params.relationship)?;
            let source = require_live_typed(conn, &params.source, "source")?;
            let target = require_live_typed(conn, &params.target, "target")?;
            // COLLIERY-T-0235: the rule of the remove and the delete, in
            // one transaction, by the ONE function that REST
            // `DELETE /api/relationships/{id}` calls.
            crate::api::remove_edge(
                conn,
                tenant_ctx.role,
                user,
                relationship,
                source.typed(),
                target.typed(),
            )?;
            Ok(format!(
                "Unlinked {} -[{relationship}]-> {}.{}",
                source.code,
                target.code,
                renamed_lines(&[&source, &target])
            ))
        })
        .await
    }

    #[tool(
        description = "Set, update, or clear (null) metadata values on an item, keyed by metadata-definition slug. Values validate against the definition (enum membership, YYYY-MM-DD dates). Returns the item's resulting metadata set. You can set metadata on an item that you created, or with `manage_<type>` on its board."
    )]
    pub async fn set_metadata(
        &self,
        Parameters(params): Parameters<SetMetadataParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            require_item_edit(conn, role, user, item.id, item.item_type)?;

            // Phase 1 — resolve + validate every entry (no writes yet); a bad
            // entry rejects the whole call (A-0003). The SAME function the REST
            // handler calls (KAIROS-T-0096): this loop used to be a copy that
            // had drifted, missing the KAIROS-T-0078 entity-type scoping guard,
            // so MCP would stamp a documents-only definition onto a task.
            let ops = validated_metadata_ops(conn, item.item_type, &params.values)?;
            // KAIROS-T-0381: a bug that becomes P0 tells its team.
            let was_p0 =
                kairos_db::notifications::is_p0_bug(conn, item.id).map_err(ApiError::internal)?;

            // Phase 2 — apply atomically (the same function as the API handler).
            conn.transaction::<_, diesel::result::Error, _>(|conn| {
                apply_metadata_ops(conn, item.id, item.item_type.entity_type(), &ops, user)?;
                if !was_p0 {
                    kairos_db::notifications::on_p0_bug(conn, item.id, user)?;
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
        description = "Archive an item by short code (it is not deleted): hidden from boards and default search, read-only, and `restore_item` brings it back. Needs `confirm: true`: the archive cascades to the descendants that you can edit, and the answer lists what was archived and what stayed, and why. `preview: true` lists that cascade first and changes nothing. You can archive an item you created, or with `manage_<type>` on its board. An agent's archive of intake is refused with AGENT_TRIAGE_OFF while the board lets a person decide. `purge_task` (admin profile) deletes an archived task for good."
    )]
    pub async fn delete_item(
        &self,
        Parameters(params): Parameters<DeleteItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let confirm = params.confirm.unwrap_or(false);
            let preview = params.preview.unwrap_or(false);
            if confirm && preview {
                return Err(ApiError::validation(
                    "Send preview=true to see the cascade, or confirm=true to archive. Do not \
                     send both.",
                ));
            }
            if preview {
                let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
                return delete_preview_text(conn, &slug, role, user, &item);
            }
            if !confirm {
                return Err(ApiError::validation(
                    "delete_item must have confirm=true. The call archives the item and \
                     each descendant that a parent edge connects to it. A restore brings \
                     each one back. Send preview=true to see the cascade first.",
                ));
            }
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            // KAIROS-T-0537: an archive of intake is a triage (won't do).
            // The edit rule first, so a caller with no right gets its 403.
            let triaged = if item.item_type == ItemType::Task {
                crate::api::require_item_archive(conn, role, user, item.id, item.item_type)?;
                crate::agent_triage::check(
                    conn,
                    &auth,
                    item.id,
                    crate::agent_triage::TriageMove::Cancel,
                )?
            } else {
                None
            };
            // The same archive as REST `DELETE` (COLLIERY-T-0234): the edit
            // rule for the item, and then for each descendant.
            let outcome = crate::api::cascade::archive_item(
                conn,
                &slug,
                role,
                user,
                item.id,
                item.item_type,
            )?;
            crate::agent_triage::record(conn, &auth, item.id, triaged)?;
            let mut out = format!("Archived {}.\n", outcome.root_short_code);
            if outcome.cascaded_short_codes.is_empty() {
                out.push_str("Cascade: none.\n");
            } else {
                let count = outcome.cascaded_short_codes.len();
                out.push_str(&format!(
                    "Cascade archived {count} descendant{}: {}.\n",
                    if count == 1 { "" } else { "s" },
                    outcome.cascaded_short_codes.join(", ")
                ));
            }
            out.push_str(&crate::api::cascade::not_reached_lines(
                &outcome.not_reached,
                false,
            ));
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Put an archived item back on its board by short code. Only that item: archived descendants stay archived and are listed for you to restore. Refused with RESTORE_BLOCKED when its board, column, team or repository is gone (the answer names it). You can restore an item you created, or with `manage_<type>` on its board."
    )]
    pub async fn restore_item(
        &self,
        Parameters(params): Parameters<RestoreItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::IncludeArchived)?;
            if item.archived_at.is_none() {
                return Err(ApiError::validation(format!(
                    "{} is not archived, so the server cannot restore it.",
                    item.short_code
                )));
            }
            crate::api::require_item_archive(conn, role, user, item.id, item.item_type)?;
            match items::restore_item(conn, item.item_type, item.id, user)
                .map_err(map_item_error)?
            {
                Ok(outcome) => {
                    // KAIROS-T-0566: the WIP warning of a move.
                    if let Some(column_id) = outcome.column_id {
                        crate::claims::note_wip(conn, column_id, &outcome.short_code)?;
                    }
                    let mut out = format!(
                        "Restored {}. It is on its board again.\n",
                        outcome.short_code
                    );
                    if outcome.still_archived_descendants.is_empty() {
                        out.push_str("Nothing below it is archived.\n");
                    } else {
                        out.push_str(&format!(
                            "Archived below it ({}): {}\n\
                             To get one back, restore it by its short code.\n",
                            outcome.still_archived_descendants.len(),
                            outcome.still_archived_descendants.join(", ")
                        ));
                    }
                    Ok(out)
                }
                Err(blocked) => Err(ApiError::unprocessable(
                    "RESTORE_BLOCKED",
                    format!(
                        "The server cannot restore {}. The item needs {}. Move the item \
                         to a place that exists, or first restore the thing that the item \
                         needs.",
                        item.short_code,
                        blocked.missing.join(" and ")
                    ),
                )),
            }
        })
        .await
    }

    #[tool(
        description = "Cancel a TASK the team will not do, with a `reason`: it moves to the done column with the mark \"cancelled\", from any column (the transition rules do not apply), and its claim ends. `reopen_item` brings it back and removes the mark. Needs `transition_items` on its board. An empty reason, or a task already done (TASK_DONE), is refused. An agent's cancel of intake is refused with AGENT_TRIAGE_OFF while the board lets a person decide."
    )]
    pub async fn cancel_item(
        &self,
        Parameters(params): Parameters<CancelItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            let Some(board_id) = item.board_id.filter(|_| item.item_type == ItemType::Task) else {
                return Err(ApiError::validation(format!(
                    "{} is a {}. You can cancel only a task.",
                    item.short_code, item.item_type
                ))
                .with_details(serde_json::json!({ "field": "short_code" })));
            };
            // KAIROS-T-0456: a cancel is a triage of intake (won't do).
            let triaged = crate::agent_triage::check(
                conn,
                &auth,
                item.id,
                crate::agent_triage::TriageMove::Cancel,
            )?;
            let cancelled =
                crate::cancel::cancel(conn, &slug, item.id, board_id, &params.reason, user)?;
            crate::agent_triage::record(conn, &auth, item.id, triaged)?;
            Ok(format!(
                "Cancelled {}: {} -> {}.\nReason: {}\nTo undo the cancel: reopen_item.",
                cancelled.short_code,
                cancelled.from_column,
                cancelled.to_column,
                cancelled.mark.reason
            ))
        })
        .await
    }

    #[tool(
        description = "Reopen a done TASK (completed or cancelled): it moves to the first column of its board that is not done (Backlog on a default board). The transition rules do not apply; the board policy parent_gate does, for a column that holds claims. The reopen removes the cancel mark (the history keeps the reason) and sends item_transitioned. Needs `transition_items` on its board. A task that is not done (TASK_NOT_DONE) is refused. A done task does not `move_item` to another board: reopen it first."
    )]
    pub async fn reopen_item(
        &self,
        Parameters(params): Parameters<ReopenItemParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
            let Some(board_id) = item.board_id.filter(|_| item.item_type == ItemType::Task) else {
                return Err(ApiError::validation(format!(
                    "{} is a {}. You can reopen only a task. Move another item out of its \
                     done column with transition_item, where the board allows it.",
                    item.short_code, item.item_type
                ))
                .with_details(serde_json::json!({ "field": "short_code" })));
            };
            let reopened = crate::reopen::reopen(conn, &slug, item.id, board_id, user)?;
            Ok(crate::reopen::reopen_text(&reopened))
        })
        .await
    }

    #[tool(
        description = "Delete an archived TASK for good, with its history, metadata, edges, forge links, external references, forge intake row and claim. You cannot undo it. Archive the task first with `delete_item`: a live task is refused with TASK_LIVE. The activity log keeps the code and title. Needs `confirm: true` and `purge` on its board: team membership does not give it, and organization admins have it. An agent's purge of intake is refused with AGENT_TRIAGE_OFF while the board lets a person decide. Send the current code (a retired code is not followed)."
    )]
    pub async fn purge_task(
        &self,
        Parameters(params): Parameters<PurgeTaskParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            if !params.confirm {
                return Err(ApiError::validation(
                    "purge_task must have confirm=true. The call deletes the task for good, \
                     and you cannot undo it.",
                )
                .with_details(serde_json::json!({ "field": "confirm" })));
            }
            let purged = crate::api::tasks::purge(conn, &slug, &params.short_code, &auth)?;
            Ok(format!(
                "Deleted {} (\"{}\") for good. No restore can bring it back.",
                purged.short_code, purged.title
            ))
        })
        .await
    }

    #[tool(
        description = "Archive each task in the done columns of one delivery board (\"Archive completed\"), cancelled tasks too. Each task is archived as by `delete_item`, and `restore_item` brings back each one. One transaction: the call archives each task, or none. Requires confirm=true, and `manage_tasks` on the board. Optional `older_than_days` (1 to 36500): only the done tasks that entered their done column at least that many days ago. The response gives the count and the codes."
    )]
    pub async fn archive_completed(
        &self,
        Parameters(params): Parameters<ArchiveCompletedParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        let role = tenant.role;
        self.run_tool(&tenant, move |conn| {
            if !params.confirm {
                return Err(ApiError::validation(
                    "archive_completed must have confirm=true. The call archives each task \
                     in the done columns of the board. A restore brings each one back.",
                )
                .with_details(serde_json::json!({ "field": "confirm" })));
            }
            let older_than_days = crate::api::org::boards::older_than_days(params.older_than_days)?;
            let board = board_by_ref(conn, &params.board)?;
            let response = crate::api::org::boards::archive_completed_tasks(
                conn,
                &slug,
                role,
                &board,
                user,
                older_than_days,
            )?;
            Ok(if response.count == 0 {
                match older_than_days {
                    Some(days) => format!(
                        "The done columns of {} have no tasks that entered them {days} days \
                         ago or before. Kairos archived nothing.",
                        board.slug
                    ),
                    None => format!(
                        "The done columns of {} have no tasks. Kairos archived nothing.",
                        board.slug
                    ),
                }
            } else {
                format!(
                    "Archived {} completed task{} of {}: {}.\nTo get one back, restore it by its \
                     short code.",
                    response.count,
                    if response.count == 1 { "" } else { "s" },
                    board.slug,
                    response.short_codes.join(", ")
                )
            })
        })
        .await
    }

    // KAIROS-T-0337: the structure tools. Each calls the function of its
    // REST route, so the gate and the refusals are those of REST: an
    // organization admin, or a grant of `manage_structure` at the scope.
    #[tool(
        description = "Create a team and its delivery board ({slug}-delivery). `code_prefix`: a capital letter then 1 to 9 capitals or digits (OPS gives OPS-T-0001); it never changes. Optional: `team_type` (stream_aligned by default, platform, enabling, complicated_subsystem), `adr_board: true` to also make its ADR board ({slug}-adrs). The new board has no members: use `add_team_member` and `grant_capability`. Needs the org admin role or `manage_structure` on the organization."
    )]
    pub async fn create_team(
        &self,
        Parameters(params): Parameters<CreateTeamParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            atomically(conn, |conn| {
                let team = crate::api::org::teams::create(
                    conn,
                    &slug,
                    user,
                    kairos_client::types_org::CreateTeamRequest {
                        name: params.name.clone(),
                        slug: params.slug.clone(),
                        team_type: params.team_type.clone(),
                        code_prefix: params.code_prefix.clone(),
                    },
                )?;
                let mut out = format!(
                    "Created team {} ({}, {}). Delivery board: {}-delivery (prefix {}).",
                    team.slug, team.name, team.team_type, team.slug, params.code_prefix
                );
                if params.adr_board.unwrap_or(false) {
                    let adr = crate::api::org::boards::create(
                        conn,
                        &slug,
                        user,
                        kairos_client::types_org::CreateBoardRequest {
                            name: format!("{} ADRs", params.name),
                            slug: format!("{}-adrs", params.slug),
                            board_level: BoardLevel::Adr.to_string(),
                            team_id: Some(team.id.clone()),
                            code_prefix: params.code_prefix.clone(),
                        },
                    )?;
                    out.push_str(&format!(" ADR board: {}.", adr.board.slug));
                }
                out.push_str(if params.adr_board.unwrap_or(false) {
                    "\nThe boards have no members. Use add_team_member and grant_capability."
                } else {
                    "\nThe board has no members. Use add_team_member and grant_capability."
                });
                Ok(out)
            })
        })
        .await
    }

    #[tool(
        description = "Create a board: an ADR board (`board_level: adr`; with `team`, that team's ADR board, with its prefix), a strategy or initiative board, or the delivery board of a team that has none. `slug` matches ^[a-z][a-z0-9_-]{1,62}$; `code_prefix` is a capital letter then 1 to 9 capitals or digits, fixed. It gets the default columns and transitions of its level. Needs the org admin role or `manage_structure` on the organization."
    )]
    pub async fn create_board(
        &self,
        Parameters(params): Parameters<CreateBoardParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let team = params
                .team
                .as_deref()
                .map(|team| team_by_ref(conn, team))
                .transpose()?;
            let detail = crate::api::org::boards::create(
                conn,
                &slug,
                user,
                kairos_client::types_org::CreateBoardRequest {
                    name: params.name,
                    slug: params.slug,
                    board_level: params.board_level,
                    team_id: team.as_ref().map(|team| team.id.to_string()),
                    code_prefix: params.code_prefix,
                },
            )?;
            let columns: Vec<&str> = detail.columns.iter().map(|c| c.name.as_str()).collect();
            Ok(format!(
                "Created board {} ({}, {}, prefix {}{}). Columns: {}.",
                detail.board.slug,
                detail.board.name,
                detail.board.board_level,
                detail.board.code_prefix,
                team.map(|team| format!(", team {}", team.slug))
                    .unwrap_or_default(),
                columns.join(", ")
            ))
        })
        .await
    }

    #[tool(
        description = "Add a member of the organization to a team, as POST /api/teams/{id}/members does. A team member gets manage_tasks, manage_documents and transition_items on the boards of the team with no grant, and manage_adrs on the ADR board of the team. `team` is a slug or UUID. `user` is a user id, an email, a user name, or the name of a service account. Requires the organization admin role or `manage_structure` on the organization."
    )]
    pub async fn add_team_member(
        &self,
        Parameters(params): Parameters<AddTeamMemberParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        let routes = crate::api::org::structure_grants::SignInRoutes::of(&self.state.config);
        self.run_tool(&tenant, move |conn| {
            let team = team_by_ref(conn, &params.team)?;
            let target = crate::api::org::structure_grants::resolve_member_to_add(
                conn,
                &slug,
                &params.user,
                "user",
                routes,
            )?;
            let member =
                crate::api::org::teams::add_member_as(conn, &slug, user, team.id, target.id)?;
            Ok(format!(
                "Added {} <{}> to the team {}.",
                member.display_name, member.email, team.slug
            ))
        })
        .await
    }

    #[tool(
        description = "Grant a capability to a member (`user`: user id, email, user name or service account name). A board capability (manage_tasks, manage_documents, manage_adrs, manage_initiatives, manage_strategies, transition_items, configure_boards, administer_members, pages, purge, member, approve_proposals, or a glob such as manage_*) needs `board` and `administer_members` there, or `manage_structure`. `manage_structure` (only an org admin grants it): with no `board`, the structure of the organization; with `board`, that board's columns, transitions, members and grants."
    )]
    pub async fn grant_capability(
        &self,
        Parameters(params): Parameters<GrantCapabilityParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        let routes = crate::api::org::structure_grants::SignInRoutes::of(&self.state.config);
        self.run_tool(&tenant, move |conn| {
            if params.capability == kairos_core::abac::MANAGE_STRUCTURE {
                let grant = crate::api::org::structure_grants::grant(
                    conn,
                    &slug,
                    user,
                    &params.user,
                    params.board.as_deref(),
                    routes,
                )?;
                let scope = match &grant.board_slug {
                    Some(board) => format!("on the board {board}"),
                    None => "on the organization".to_string(),
                };
                return Ok(format!(
                    "Granted manage_structure {scope} to {} <{}> (grant {}).",
                    grant.display_name, grant.email, grant.id
                ));
            }
            let Some(board) = params.board.as_deref() else {
                return Err(ApiError::validation(format!(
                    "The capability {:?} is a board capability. Send `board`, the slug or the \
                     UUID of the board.",
                    params.capability
                ))
                .with_details(json!({ "field": "board" })));
            };
            let board = board_by_ref(conn, board)?;
            let target = crate::api::org::structure_grants::resolve_member_to_add(
                conn,
                &slug,
                &params.user,
                "user",
                routes,
            )?;
            let member = crate::api::org::boards::grant_as(
                conn,
                &slug,
                user,
                board.id,
                target.id,
                std::slice::from_ref(&params.capability),
            )?;
            Ok(format!(
                "Granted {} on the board {} to {} <{}>. The capabilities of the member on the \
                 board: {}.",
                params.capability,
                board.slug,
                member.display_name,
                member.email,
                member.capabilities.join(", ")
            ))
        })
        .await
    }

    // KAIROS-T-0443: team and board administration, as REST does. Each tool
    // calls the function of its REST route, so the gate is the same.
    #[tool(
        description = "Change a team, as PATCH /api/teams/{id} does: its `name`, `slug`, `team_type` (stream_aligned | platform | enabling | complicated_subsystem) or `site_enabled` (the published site of the team). Send one or more. A value that the team has already changes nothing. `team` is a slug or UUID. Requires the organization admin role or `manage_structure` on the organization."
    )]
    pub async fn update_team(
        &self,
        Parameters(params): Parameters<UpdateTeamParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let team = team_by_ref(conn, &params.team)?;
            let updated = crate::api::org::teams::update_as(
                conn,
                &slug,
                user,
                team.id,
                &kairos_client::types_org::UpdateTeamRequest {
                    name: params.name,
                    slug: params.slug,
                    team_type: params.team_type,
                    site_enabled: params.site_enabled,
                },
            )?;
            Ok(format!(
                "The team {} ({}): name {:?}, type {}, site {}.",
                updated.slug,
                updated.id,
                updated.name,
                updated.team_type,
                if updated.site_enabled { "on" } else { "off" }
            ))
        })
        .await
    }

    #[tool(
        description = "Take a member off a team, as DELETE /api/teams/{id}/members/{user_id} does. The person loses the rights that the team gives on its boards; grants on a board stay. `team` is a slug or UUID. `user` is a user id, an email, a user name, or the name of a service account. Requires the organization admin role or `manage_structure` on the organization."
    )]
    pub async fn remove_team_member(
        &self,
        Parameters(params): Parameters<RemoveTeamMemberParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let team = team_by_ref(conn, &params.team)?;
            let target = match Uuid::parse_str(params.user.trim()) {
                Ok(id) => id,
                Err(_) => {
                    crate::api::org::structure_grants::resolve_member(
                        conn,
                        &slug,
                        &params.user,
                        "user",
                    )?
                    .id
                }
            };
            crate::api::org::teams::remove_member_as(conn, &slug, user, team.id, target)?;
            Ok(format!("Took {} off the team {}.", params.user, team.slug))
        })
        .await
    }

    #[tool(
        description = "Set a person's capabilities on a board: `capabilities` is the full set and replaces theirs; an empty list revokes each one; a person with none gets the set. The capabilities are those of `grant_capability`, without manage_structure. `board` is a slug or UUID; `user` is a user id, email, user name or service account name. Needs `administer_members` on the board, or `manage_structure`."
    )]
    pub async fn set_board_capabilities(
        &self,
        Parameters(params): Parameters<SetBoardCapabilitiesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            use crate::api::org::boards as b;
            let board = board_by_ref(conn, &params.board)?;
            let target = crate::api::org::structure_grants::resolve_member(
                conn,
                &slug,
                &params.user,
                "user",
            )?;
            let had = kairos_db::schema::board_member_capabilities::table
                .filter(kairos_db::schema::board_member_capabilities::board_id.eq(board.id))
                .filter(kairos_db::schema::board_member_capabilities::user_id.eq(target.id))
                .count()
                .get_result::<i64>(conn)
                .map_err(ApiError::internal)?;
            if params.capabilities.is_empty() {
                if had == 0 {
                    return Ok(format!(
                        "{} has no capability on the board {}: nothing changed.",
                        params.user, board.slug
                    ));
                }
                let revoked = b::revoke_all_as(conn, &slug, user, board.id, target.id)?;
                return Ok(format!(
                    "Revoked each capability of {} on the board {}: {}.",
                    params.user,
                    board.slug,
                    revoked.revoked_capabilities.join(", ")
                ));
            }
            let member = if had == 0 {
                b::grant_as(conn, &slug, user, board.id, target.id, &params.capabilities)?
            } else {
                b::replace_as(conn, &slug, user, board.id, target.id, &params.capabilities)?
            };
            Ok(format!(
                "{} has on the board {}: {}.",
                params.user,
                board.slug,
                member.capabilities.join(", ")
            ))
        })
        .await
    }

    #[tool(
        description = "Add a column to a board, as POST /api/boards/{id}/columns does. `position` is from 0 (the first); the default is the last. The new column has no transition: add them with set_transition. `board` is a slug or UUID. Requires `configure_boards` on the board."
    )]
    pub async fn add_column(
        &self,
        Parameters(params): Parameters<AddColumnParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let board = board_by_ref(conn, &params.board)?;
            let position = match params.position {
                Some(position) => position,
                None => kairos_db::schema::board_columns::table
                    .filter(kairos_db::schema::board_columns::board_id.eq(board.id))
                    .filter(kairos_db::schema::board_columns::deleted_at.is_null())
                    .count()
                    .get_result::<i64>(conn)
                    .map_err(ApiError::internal)? as i32,
            };
            let column = crate::api::org::boards::add_column_as(
                conn,
                &slug,
                user,
                board.id,
                &params.name,
                position,
            )?;
            Ok(format!(
                "Added the column {} ({}) to the board {} at position {}.",
                column.name, column.id, board.slug, column.position
            ))
        })
        .await
    }

    #[tool(
        description = "Change a column of a board, as PATCH /api/boards/{id}/columns/{col_id} does: `name`, `position` (from 0), `is_done` (a done column), `claims` (a task in it has a claim), `keeps_claims` (a task keeps its claim in it, as in Blocked), `wip_limit` (1 or more), or `no_wip_limit: true` to remove the limit. Send one or more. `board` is a slug or UUID; `column` is a name or UUID. Requires `configure_boards` on the board."
    )]
    pub async fn update_column(
        &self,
        Parameters(params): Parameters<UpdateColumnParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            if params.no_wip_limit && params.wip_limit.is_some() {
                return Err(
                    ApiError::validation("Send wip_limit or no_wip_limit, not both.")
                        .with_details(json!({ "field": "no_wip_limit" })),
                );
            }
            let board = board_by_ref(conn, &params.board)?;
            let column_id = crate::api::org::dispatch::column_of(conn, &board, &params.column)?;
            let wip_limit = if params.no_wip_limit {
                Some(None)
            } else {
                params.wip_limit.map(Some)
            };
            let column = crate::api::org::boards::update_column_as(
                conn,
                &slug,
                user,
                board.id,
                column_id,
                &kairos_client::types_org::UpdateColumnRequest {
                    name: params.name,
                    position: params.position,
                    is_done: params.is_done,
                    claims: params.claims,
                    keeps_claims: params.keeps_claims,
                    wip_limit,
                },
            )?;
            Ok(format!(
                "The column {} of the board {}: position {}, done {}, claims {}, WIP limit {}.",
                column.name,
                board.slug,
                column.position,
                column.is_done,
                column.claims,
                column
                    .wip_limit
                    .map(|limit| limit.to_string())
                    .unwrap_or_else(|| "none".into())
            ))
        })
        .await
    }

    #[tool(
        description = "Remove a column of a board, as DELETE /api/boards/{id}/columns/{col_id} does. The column must hold no live item (COLUMN_NOT_EMPTY). Its transitions stop. `board` is a slug or UUID; `column` is a name or UUID. Requires `configure_boards` on the board."
    )]
    pub async fn remove_column(
        &self,
        Parameters(params): Parameters<RemoveColumnParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let board = board_by_ref(conn, &params.board)?;
            let column_id = crate::api::org::dispatch::column_of(conn, &board, &params.column)?;
            crate::api::org::boards::remove_column_as(conn, &slug, user, board.id, column_id)?;
            Ok(format!(
                "Removed the column {} of the board {}.",
                params.column, board.slug
            ))
        })
        .await
    }

    #[tool(
        description = "Allow or stop a move between two columns of a board, as POST and DELETE /api/boards/{id}/transitions do. `allowed: true` adds the transition `from` -> `to`; `allowed: false` removes it. `board` is a slug or UUID; `from` and `to` are column names or UUIDs. Requires `configure_boards` on the board."
    )]
    pub async fn set_transition(
        &self,
        Parameters(params): Parameters<SetTransitionParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            use crate::api::org::boards as b;
            let board = board_by_ref(conn, &params.board)?;
            let from = crate::api::org::dispatch::column_of(conn, &board, &params.from)
                .map_err(|e| e.with_details(json!({ "field": "from" })))?;
            let to = crate::api::org::dispatch::column_of(conn, &board, &params.to)
                .map_err(|e| e.with_details(json!({ "field": "to" })))?;
            if params.allowed {
                b::add_transition_as(conn, &slug, user, board.id, from, to)?;
                Ok(format!(
                    "The board {} allows the move {} -> {}.",
                    board.slug, params.from, params.to
                ))
            } else {
                b::remove_transition_as(conn, &slug, user, board.id, from, to)?;
                Ok(format!(
                    "The board {} does not allow the move {} -> {} now.",
                    board.slug, params.from, params.to
                ))
            }
        })
        .await
    }

    #[tool(
        description = "Remove a repository from the directory, as DELETE /api/repositories/{slug} does. Refused while a live task links to it or a forge connection uses it (409). `repository` is a slug or UUID. Requires the organization admin role or `manage_structure` on the organization."
    )]
    pub async fn delete_repository(
        &self,
        Parameters(params): Parameters<DeleteRepositoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let slug = tenant.slug.clone();
        self.run_tool(&tenant, move |conn| {
            let removed =
                crate::api::org::repositories::delete_as(conn, &slug, user, &params.repository)?;
            Ok(format!(
                "Removed the repository {} ({}) from the directory.",
                removed.slug, removed.id
            ))
        })
        .await
    }

    #[tool(
        description = "Make or change a metadata definition, as POST and PATCH /api/metadata-definitions do. With `definition` (slug or UUID), the tool changes it: `name`, `slug`, `enum_options` (the scale, in order) and `entity_types` (each replaces the value). With no `definition`, it makes one: `name`, `slug` and `field_type` (text | number | date | enum | boolean) are required. The type does not change later. Organization admins only."
    )]
    pub async fn save_metadata_definition(
        &self,
        Parameters(params): Parameters<SaveMetadataDefinitionParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let gate = crate::api::meta::require_org_admin(&tenant);
        let user = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            gate?;
            use crate::api::meta::definitions as d;
            use kairos_client::types_meta as dto;
            let saved = match params.definition.as_deref() {
                Some(reference) => {
                    if params.field_type.is_some() {
                        return Err(ApiError::validation(
                            "The type of a definition does not change. Omit field_type.",
                        )
                        .with_details(json!({ "field": "field_type" })));
                    }
                    let id = meta_id(conn, "metadata_definitions", reference, "definition")?;
                    d::update_definition_in(
                        conn,
                        user,
                        id,
                        &dto::UpdateMetadataDefinitionRequest {
                            name: params.name,
                            slug: params.slug,
                            enum_options: params.enum_options,
                            entity_types: params.entity_types,
                        },
                    )?
                }
                None => {
                    let required = |value: Option<String>, name: &str| {
                        value.ok_or_else(|| {
                            ApiError::validation(format!(
                                "A new definition needs {name}. To change a definition, send \
                                 definition."
                            ))
                            .with_details(json!({ "field": name }))
                        })
                    };
                    d::create_definition_in(
                        conn,
                        &dto::CreateMetadataDefinitionRequest {
                            name: required(params.name, "name")?,
                            slug: required(params.slug, "slug")?,
                            field_type: required(params.field_type, "field_type")?,
                            enum_options: params.enum_options.unwrap_or_default(),
                            entity_types: params.entity_types.unwrap_or_default(),
                        },
                    )?
                }
            };
            Ok(format!(
                "The metadata definition {} ({}): {}, type {}.",
                saved.slug, saved.id, saved.name, saved.field_type
            ))
        })
        .await
    }

    #[tool(
        description = "Delete a metadata definition, as DELETE /api/metadata-definitions/{id} does. Refused while an item has a value or a template has the field (DEFINITION_IN_USE names them). `reference` is a slug or UUID. Organization admins only."
    )]
    pub async fn delete_metadata_definition(
        &self,
        Parameters(params): Parameters<DeleteMetaParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        let gate = crate::api::meta::require_org_admin(&tenant);
        self.run_tool(&tenant, move |conn| {
            gate?;
            let id = meta_id(conn, "metadata_definitions", &params.reference, "reference")?;
            crate::api::meta::definitions::delete_definition_in(conn, id)?;
            Ok(format!(
                "Deleted the metadata definition {}.",
                params.reference
            ))
        })
        .await
    }

    #[tool(
        description = "Make or change a template, as POST and PATCH /api/templates do. With `template` (slug or UUID), the tool changes it: `name`, `slug`, `content` and `metadata` (each replaces the value). With no `template`, it makes one: `name` and `slug` are required. `metadata` is a list of fields: `definition_slug`, optional `default_value`, `required`. Organization admins only."
    )]
    pub async fn save_template(
        &self,
        Parameters(params): Parameters<SaveTemplateParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let gate = crate::api::meta::require_org_admin(&tenant);
        let user = auth.user_id;
        self.run_tool(&tenant, move |conn| {
            gate?;
            use crate::api::meta::templates as t;
            use kairos_client::types_meta as dto;
            let metadata = params.metadata.map(|fields| {
                fields
                    .into_iter()
                    .map(|field| dto::TemplateMetadataEntry {
                        definition_slug: field.definition_slug,
                        default_value: field.default_value,
                        required: field.required,
                    })
                    .collect::<Vec<_>>()
            });
            let saved = match params.template.as_deref() {
                Some(reference) => {
                    let id = meta_id(conn, "templates", reference, "template")?;
                    t::update_template_in(
                        conn,
                        user,
                        id,
                        &dto::UpdateTemplateRequest {
                            name: params.name,
                            slug: params.slug,
                            content: params.content,
                            metadata,
                        },
                    )?
                }
                None => {
                    let required = |value: Option<String>, name: &str| {
                        value.ok_or_else(|| {
                            ApiError::validation(format!(
                                "A new template needs {name}. To change a template, send \
                                 template."
                            ))
                            .with_details(json!({ "field": name }))
                        })
                    };
                    t::create_template_in(
                        conn,
                        &dto::CreateTemplateRequest {
                            name: required(params.name, "name")?,
                            slug: required(params.slug, "slug")?,
                            content: params.content.unwrap_or_default(),
                            metadata: metadata.unwrap_or_default(),
                        },
                    )?
                }
            };
            Ok(format!(
                "The template {} ({}): {}.",
                saved.slug, saved.id, saved.name
            ))
        })
        .await
    }

    #[tool(
        description = "Delete a template, as DELETE /api/templates/{id} does. Items made from it keep their content and their metadata. `reference` is a slug or UUID. Organization admins only."
    )]
    pub async fn delete_template(
        &self,
        Parameters(params): Parameters<DeleteMetaParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        let gate = crate::api::meta::require_org_admin(&tenant);
        self.run_tool(&tenant, move |conn| {
            gate?;
            let id = meta_id(conn, "templates", &params.reference, "reference")?;
            crate::api::meta::templates::delete_template_in(conn, id)?;
            Ok(format!("Deleted the template {}.", params.reference))
        })
        .await
    }

    // KAIROS-T-0434: the team page tools. Each calls the function of its
    // REST route, so the gate (a team member or an organization admin) and
    // the refusals are those of REST.
    #[tool(
        description = "Read the team page of a team: its tree of folders and pages, with the charter first, and the documents and ADRs that the boards of the team own, each in its folder (one that nobody placed is at the end of the root). With `page` (a path of slugs such as \"documentation/runbooks\", or a UUID), read one page or folder in full: its content and its version. `team` is a slug or UUID. Every member of the organization can read."
    )]
    pub async fn team_pages(
        &self,
        Parameters(params): Parameters<TeamPagesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, tenant) = Self::caller(&context)?;
        self.run_tool(&tenant, move |conn| {
            let team = team_by_ref(conn, &params.team)?;
            let pages = crate::api::org::team_pages::list_as(conn, team.id)?;
            match params.page {
                None => {
                    let items = crate::api::org::team_pages::tree_items_as(conn, team.id)?;
                    Ok(render_page_tree(&team.slug, &pages, &items))
                }
                Some(reference) => {
                    // KAIROS-T-0629: the list has no content; read the page.
                    let summary = page_by_ref(&pages, &reference)?;
                    let page = crate::api::org::team_pages::page_as(
                        conn,
                        team.id,
                        parse_page_id(summary)?,
                    )?;
                    Ok(format!(
                        "# {} — {}\n- kind: {}{}\n- path: {}\n- id: {}\n- version: {}\n\n{}",
                        team.slug,
                        page.title,
                        page.kind,
                        if page.is_protected {
                            " (the charter)"
                        } else {
                            ""
                        },
                        page_path(&pages, summary),
                        page.id,
                        page.version,
                        page.content
                    ))
                }
            }
        })
        .await
    }

    #[tool(
        description = "Create a page or a folder in the team page of a team, as POST /api/teams/{id}/pages does. `kind` is page or folder: a folder holds folders and pages, a page holds nothing. `parent` is the folder (a path of slugs or a UUID); omit it for the root. `slug` comes from the title when you omit it. `content` is the markdown of a page. Requires membership of the team, or the organization admin role."
    )]
    pub async fn create_team_page(
        &self,
        Parameters(params): Parameters<CreateTeamPageParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let gate = tenant.clone();
        self.run_tool(&tenant, move |conn| {
            let team = team_by_ref(conn, &params.team)?;
            let pages = crate::api::org::team_pages::list_as(conn, team.id)?;
            let parent_id = match &params.parent {
                Some(reference) => Some(page_by_ref(&pages, reference)?.id.clone()),
                None => None,
            };
            let position = params.position.unwrap_or_else(|| {
                pages
                    .iter()
                    .filter(|p| p.parent_id == parent_id)
                    .map(|p| p.position + 1)
                    .max()
                    .unwrap_or(0)
            });
            let slug = match params.slug {
                Some(slug) => slug,
                None => slug_from_title(&params.title),
            };
            let created = crate::api::org::team_pages::create_as(
                conn,
                &gate,
                user,
                team.id,
                &kairos_client::types_team_pages::CreateTeamPageRequest {
                    parent_id,
                    kind: params.kind,
                    slug,
                    title: params.title,
                    content: params.content.unwrap_or_default(),
                    position,
                },
            )?;
            let mut all = pages;
            let summary = kairos_client::types_team_pages::TeamPageSummary::from(&created);
            all.push(summary.clone());
            Ok(format!(
                "Created the {} {} ({}) in the team page of {}: path {}, version {}.",
                created.kind,
                created.title,
                created.id,
                team.slug,
                page_path(&all, &summary),
                created.version
            ))
        })
        .await
    }

    #[tool(
        description = "Edit the content of a page of a team, as PATCH /api/teams/{id}/pages/{page_id} does with content and version. `page` is a path of slugs (\"charter\" is the charter) or a UUID. `content` replaces the whole page. `version` is the version that you read with team_pages: a newer version refuses the edit with CONFLICT and the current page. `title` changes the title too. Requires membership of the team, or the organization admin role."
    )]
    pub async fn edit_team_page(
        &self,
        Parameters(params): Parameters<EditTeamPageParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let gate = tenant.clone();
        self.run_tool(&tenant, move |conn| {
            let team = team_by_ref(conn, &params.team)?;
            let pages = crate::api::org::team_pages::list_as(conn, team.id)?;
            let page_id = parse_page_id(page_by_ref(&pages, &params.page)?)?;
            let updated = crate::api::org::team_pages::update_as(
                conn,
                &gate,
                user,
                team.id,
                page_id,
                &kairos_client::types_team_pages::UpdateTeamPageRequest {
                    title: params.title,
                    content: Some(params.content),
                    version: Some(params.version),
                    ..Default::default()
                },
            )?;
            Ok(format!(
                "Edited {} ({}) in the team page of {}: version {}.",
                updated.title, updated.id, team.slug, updated.version
            ))
        })
        .await
    }

    #[tool(
        description = "Move, reorder or rename a page or a folder of a team, as PATCH /api/teams/{id}/pages/{page_id} does with slug, parent_id, move_to_root and position. `page` is a path of slugs or a UUID, or the short code of a document or an ADR of the team, which places that item in the tree (its board keeps it). `parent` is the new folder (a path of slugs or a UUID); `to_root: true` moves it to the root. `slug` renames a page or a folder. `position` reorders it among its siblings. The charter cannot be moved or renamed. Requires membership of the team, or the organization admin role."
    )]
    pub async fn move_team_page(
        &self,
        Parameters(params): Parameters<MoveTeamPageParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let gate = tenant.clone();
        self.run_tool(&tenant, move |conn| {
            if params.parent.is_some() && params.to_root == Some(true) {
                return Err(ApiError::validation(
                    "The call has parent and to_root. Send one of them.",
                ));
            }
            let team = team_by_ref(conn, &params.team)?;
            let pages = crate::api::org::team_pages::list_as(conn, team.id)?;
            let parent_id = match &params.parent {
                Some(reference) => Some(page_by_ref(&pages, reference)?.id.clone()),
                None => None,
            };
            // KAIROS-T-0437: a document or an ADR of the team, by short code,
            // is placed in the tree; the board keeps it.
            let items = crate::api::org::team_pages::tree_items_as(conn, team.id)?;
            if let Some(item) = items
                .iter()
                .find(|i| i.short_code.eq_ignore_ascii_case(&params.page))
            {
                if params.slug.is_some() {
                    return Err(ApiError::validation(
                        "A document or an ADR has no slug in the tree. Send parent, to_root or position.",
                    ));
                }
                let position = params.position.unwrap_or_else(|| {
                    pages
                        .iter()
                        .filter(|p| p.parent_id == parent_id)
                        .map(|p| p.position + 1)
                        .chain(
                            items
                                .iter()
                                .filter(|i| i.parent_id == parent_id)
                                .filter_map(|i| i.position.map(|p| p + 1)),
                        )
                        .max()
                        .unwrap_or(0)
                });
                let placed = crate::api::org::team_pages::place_item_as(
                    conn,
                    &gate,
                    user,
                    team.id,
                    &item.short_code,
                    &kairos_client::types_team_pages::PlaceTeamItemRequest {
                        parent_id: parent_id.clone(),
                        position,
                    },
                )?;
                let folder = parent_id
                    .as_ref()
                    .and_then(|id| pages.iter().find(|p| &p.id == id))
                    .map(|p| page_path(&pages, p))
                    .unwrap_or_else(|| "the root".to_string());
                return Ok(format!(
                    "Placed {} ({}) in the team page of {}: in {folder}, position {}.",
                    placed.short_code,
                    placed.title,
                    team.slug,
                    placed.position.unwrap_or(position)
                ));
            }
            let page_id = parse_page_id(page_by_ref(&pages, &params.page)?)?;
            let updated = crate::api::org::team_pages::update_as(
                conn,
                &gate,
                user,
                team.id,
                page_id,
                &kairos_client::types_team_pages::UpdateTeamPageRequest {
                    slug: params.slug,
                    parent_id,
                    move_to_root: params.to_root.unwrap_or(false),
                    position: params.position,
                    ..Default::default()
                },
            )?;
            let all = crate::api::org::team_pages::list_as(conn, team.id)?;
            Ok(format!(
                "Moved {} ({}) in the team page of {}: path {}, position {}.",
                updated.title,
                updated.id,
                team.slug,
                page_path(
                    &all,
                    &kairos_client::types_team_pages::TeamPageSummary::from(&updated)
                ),
                updated.position
            ))
        })
        .await
    }

    #[tool(
        description = "Archive a page or a folder of a team, as DELETE /api/teams/{id}/pages/{page_id} does. A folder is archived with everything in it. `page` is a path of slugs or a UUID. The charter is refused. A person restores archived nodes in the Archived list of the Pages view of the team (/teams/<slug>/pages/) or with POST /api/teams/{id}/pages/{page_id}/restore. Requires membership of the team, or the organization admin role."
    )]
    pub async fn delete_team_page(
        &self,
        Parameters(params): Parameters<DeleteTeamPageParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let (auth, tenant) = Self::caller(&context)?;
        let user = auth.user_id;
        let gate = tenant.clone();
        self.run_tool(&tenant, move |conn| {
            let team = team_by_ref(conn, &params.team)?;
            let pages = crate::api::org::team_pages::list_as(conn, team.id)?;
            let page = page_by_ref(&pages, &params.page)?;
            let path = page_path(&pages, page);
            let page_id = parse_page_id(page)?;
            let archived =
                crate::api::org::team_pages::delete_as(conn, &gate, user, team.id, page_id)?;
            Ok(format!(
                "Archived the {} {} ({}) of the team page of {}: path {path}.",
                archived.kind, archived.title, archived.id, team.slug
            ))
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// Team page helpers (KAIROS-T-0434)
// ---------------------------------------------------------------------------

/// The path of slugs of a page, from the root ("documentation/runbooks").
fn page_path(
    pages: &[kairos_client::types_team_pages::TeamPageSummary],
    page: &kairos_client::types_team_pages::TeamPageSummary,
) -> String {
    let mut slugs = vec![page.slug.clone()];
    let mut parent = page.parent_id.clone();
    // The depth bound guards against a malformed cycle.
    for _ in 0..64 {
        let Some(id) = parent else { break };
        let Some(up) = pages.iter().find(|p| p.id == id) else {
            break;
        };
        slugs.push(up.slug.clone());
        parent = up.parent_id.clone();
    }
    slugs.reverse();
    slugs.join("/")
}

/// A page of the tree by its UUID or its path of slugs from the root.
fn page_by_ref<'a>(
    pages: &'a [kairos_client::types_team_pages::TeamPageSummary],
    reference: &str,
) -> Result<&'a kairos_client::types_team_pages::TeamPageSummary, ApiError> {
    if let Some(page) = pages.iter().find(|p| p.id == reference) {
        return Ok(page);
    }
    let mut parent: Option<String> = None;
    let mut found = None;
    for slug in reference.trim_matches('/').split('/') {
        let next = pages
            .iter()
            .find(|p| p.parent_id == parent && p.slug == slug);
        let Some(next) = next else {
            found = None;
            break;
        };
        parent = Some(next.id.clone());
        found = Some(next);
    }
    found.ok_or_else(|| {
        ApiError::validation(format!(
            "The team page has no page {reference:?}. Send a path of slugs from the root, \
             or the id of a page. The path of the charter is \"charter\". team_pages gives \
             the tree."
        ))
    })
}

fn parse_page_id(
    page: &kairos_client::types_team_pages::TeamPageSummary,
) -> Result<Uuid, ApiError> {
    Uuid::parse_str(&page.id).map_err(ApiError::internal)
}

/// The tree, one line for each node, indented by depth, siblings in
/// position order, the charter first. The documents and ADRs of the team
/// (KAIROS-T-0437) are nodes too; one that nobody placed is at the end of
/// the root.
fn render_page_tree(
    team: &str,
    pages: &[kairos_client::types_team_pages::TeamPageSummary],
    items: &[kairos_client::types_team_pages::TeamTreeItem],
) -> String {
    use kairos_client::types_team_pages::{TeamPageSummary as TeamPage, TeamTreeItem};
    enum Node<'a> {
        Page(&'a TeamPage),
        Item(&'a TeamTreeItem),
    }
    fn walk(
        pages: &[TeamPage],
        items: &[TeamTreeItem],
        parent: Option<&String>,
        depth: usize,
        out: &mut String,
    ) {
        let mut level: Vec<(bool, i32, String, Node)> = pages
            .iter()
            .filter(|p| p.parent_id.as_ref() == parent)
            .map(|p| (!p.is_protected, p.position, p.title.clone(), Node::Page(p)))
            .chain(
                items
                    .iter()
                    .filter(|i| i.parent_id.as_ref() == parent)
                    .map(|i| {
                        (
                            true,
                            i.position.unwrap_or(i32::MAX),
                            i.short_code.clone(),
                            Node::Item(i),
                        )
                    }),
            )
            .collect();
        level.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        for (_, _, _, node) in level {
            match node {
                Node::Page(page) => {
                    let marker = if page.is_protected {
                        " [charter]"
                    } else if page.kind == "folder" {
                        " [folder]"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "{}- {} — {}{marker} (v{}, {})\n",
                        "  ".repeat(depth),
                        page.slug,
                        page.title,
                        page.version,
                        page.id
                    ));
                    if page.kind == "folder" && depth < 64 {
                        walk(pages, items, Some(&page.id), depth + 1, out);
                    }
                }
                Node::Item(item) => out.push_str(&format!(
                    "{}- {} — {} [{}, {}]\n",
                    "  ".repeat(depth),
                    item.short_code,
                    item.title,
                    item.entity_type,
                    item.status
                )),
            }
        }
    }
    let mut out = format!(
        "# Team page of {team} ({} pages and folders, {} documents and ADRs)\n",
        pages.len(),
        items.len()
    );
    walk(pages, items, None, 0, &mut out);
    out
}

/// A slug from a title: lower case, each run of other characters one
/// hyphen ("How-to Guides" gives "how-to-guides").
/// `text` cut to `max` characters, with `…` when it was longer
/// (KAIROS-T-0504: the first line of a note in `get_item`).
fn clip_line(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

fn slug_from_title(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

// ---------------------------------------------------------------------------
// The content writes (KAIROS-T-0635: functions of their own, so that a test
// can count their statements)
// ---------------------------------------------------------------------------

/// The answer text of `update_item`.
pub fn update_item_text(
    conn: &mut PgConnection,
    role: OrgRole,
    user: Uuid,
    params: &UpdateItemParams,
) -> Result<String, ApiError> {
    // KAIROS-T-0635: the rights need no content.
    let item = load_item_key(conn, &params.short_code)?;
    require_item_edit(conn, role, user, item.id, item.item_type)?;
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
        Err(e) => Err(map_update_error(&item.short_code, e)),
    }
}

/// The answer text of `edit_item`.
pub fn edit_item_text(
    conn: &mut PgConnection,
    role: OrgRole,
    user: Uuid,
    params: &EditItemParams,
) -> Result<String, ApiError> {
    if params.search.is_empty() {
        return Err(ApiError::validation(
            "The argument search is empty. Send the text to find.",
        ));
    }
    // One retry on a version race (S-0006 edit_item semantics):
    // the read-modify-write below re-reads on the second attempt.
    for attempt in 0..2 {
        let item = load_item(conn, &params.short_code, Liveness::LiveOnly)?;
        require_item_edit(conn, role, user, item.id, item.item_type)?;
        let occurrences = item.content.matches(&params.search).count();
        if occurrences == 0 {
            return Err(ApiError::validation(format!(
                "The text of search is not in {} (version {}).",
                item.short_code, item.version
            )));
        }
        if occurrences > 1 && !params.replace_all {
            return Err(ApiError::validation(format!(
                "The text of search is in {} more than one time. The number of \
                     times is {occurrences}. Send replace_all=true, or send a longer \
                     text that is there one time.",
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
        // KAIROS-T-0635: the content before is the content read.
        match items::edit_item_content(conn, item.item_type, item.id, update, &item.content, user) {
            Ok(new_version) => {
                return Ok(format!(
                    "Edited {} to version {new_version} ({occurrences} replacement{}).",
                    item.short_code,
                    if occurrences == 1 { "" } else { "s" }
                ));
            }
            Err(items::ItemError::VersionConflict { .. }) if attempt == 0 => continue,
            Err(e) => return Err(map_update_error(&item.short_code, e)),
        }
    }
    unreachable!("edit_item loop returns within two attempts");
}

/// The answer text of `append_note`.
pub fn append_note_text(
    conn: &mut PgConnection,
    role: OrgRole,
    user: Uuid,
    params: &AppendNoteParams,
) -> Result<String, ApiError> {
    // KAIROS-T-0635: the rights need no content.
    let item = load_item_key(conn, &params.short_code)?;
    require_item_edit(conn, role, user, item.id, item.item_type)?;
    let appended =
        kairos_db::item_notes::append_note(conn, item.item_type, item.id, &params.text, user)
            .map_err(crate::api::map_note_error)?;
    Ok(format!(
        "Appended note {} to {}. The content stays at version {}.",
        appended.note_id, item.short_code, appended.version
    ))
}

// ---------------------------------------------------------------------------
// Shared item projection + lookups (sync, on the blocking connection)
// ---------------------------------------------------------------------------

/// The answer of `delete_item {preview: true}` (KAIROS-T-0558): what an
/// archive of `item` by the caller would take and leave, with the plan of
/// the archive ([`kairos_db::items::preview_cascade_as`]), and whether the
/// caller may archive the item. A read: it changes nothing.
fn delete_preview_text(
    conn: &mut PgConnection,
    slug: &str,
    role: kairos_db::models::OrgRole,
    user: Uuid,
    item: &ItemView,
) -> Result<String, ApiError> {
    let principal = kairos_db::items::Principal {
        org_slug: slug,
        user_id: user,
    };
    let preview = kairos_db::items::preview_cascade_as(conn, principal, item.item_type, item.id)
        .map_err(crate::api::map_item_error)?;
    let refusal = crate::api::archive_refusal(conn, role, user, item.id, item.item_type)?;
    Ok(preview_text(&preview, refusal.as_ref()))
}

/// The text of a preview of an archive (KAIROS-T-0558). Pure.
fn preview_text(preview: &kairos_db::items::CascadePreview, refusal: Option<&ApiError>) -> String {
    let code = &preview.root_short_code;
    let mut out = format!("Preview of an archive of {code}. Nothing is archived.\n");
    if let Some(refusal) = refusal {
        out.push_str(&format!(
            "You cannot archive {code}: {}: {}\n",
            refusal.code, refusal.message
        ));
    }
    if preview.cascaded_short_codes.is_empty() {
        out.push_str("Cascade: none.\n");
    } else {
        let count = preview.cascaded_short_codes.len();
        out.push_str(&format!(
            "The cascade would archive {count} descendant{}: {}.\n",
            if count == 1 { "" } else { "s" },
            preview.cascaded_short_codes.join(", ")
        ));
    }
    out.push_str(&crate::api::cascade::not_reached_lines(
        &preview.not_reached,
        true,
    ));
    if refusal.is_none() {
        out.push_str(&format!(
            "To archive, call delete_item with short_code {code} and confirm=true.\n"
        ));
    }
    out
}

/// The answer text of `get_item` for `short_code` (KAIROS-T-0626: a
/// function of its own, so that a test can count its statements).
///
/// The statements: [`load_item`] (one), [`item_facts`] (one),
/// [`relationship_lines`] (two), the teams of a strategy or an initiative
/// (one), the impacts of a document or an ADR (one), and the forge links
/// of each item that is not a document (one).
pub fn get_item_text(conn: &mut PgConnection, short_code: &str) -> Result<String, ApiError> {
    let item = load_item(conn, short_code, Liveness::IncludeArchived)?;
    let facts = item_facts(conn, &item)?;

    let mut out = format!("# {} — {}\n", item.short_code, item.title);
    // COLLIERY-T-3100: the code that was asked for is retired. Say
    // so first, so that the agent uses the current code from now on.
    // KAIROS-T-0626: only a retired code says so; a code in lower case
    // is the current code (KAIROS-T-0560).
    if item.retired.is_some() {
        out.push_str(&format!(
            "\n> **RETIRED CODE** The code {} is retired. The current code of \
             this item is {}. Use the current code.\n",
            short_code, item.short_code
        ));
    }
    // Before anything else: an agent that cannot tell retired work
    // from live work will try to act on it and be refused by every
    // write path, with no idea why (KAIROS-A-0020).
    if let Some(archived_at) = item.archived_at {
        out.push_str(&format!(
            "\n> **ARCHIVED** {}. This item is in the archive. You can read \
             it. It is not on a board, and Kairos refuses each write to \
             it. To change it, restore it first.\n\n",
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
        out.push_str(&format!(" · repository: {}", facts.repository_label()));
    }
    if let Some(lifecycle) = item.lifecycle {
        out.push_str(&format!(" · lifecycle: {lifecycle}"));
    }
    out.push('\n');
    if let (Some(_), Some(_)) = (item.board_id, item.column_id) {
        // The column by its id, removed or not: an archived card may sit
        // in a column that has since been removed, and the column it was
        // put away in is exactly what this line is for (KAIROS-T-0161).
        let column = facts.column_name.as_deref().unwrap_or_default();
        out.push_str(&format!(
            "- board: {} / column: {column}",
            facts.board_slug()?
        ));
        // KAIROS-T-0554: where it can move to from here, for a live
        // item on a live board.
        if item.archived_at.is_none() {
            let next = &facts.next_columns;
            if next.is_empty() {
                out.push_str(" · next: none (no transition starts here)");
            } else {
                out.push_str(&format!(" · next: {}", next.join(", ")));
            }
        }
        out.push('\n');
    }
    // KAIROS-T-0359: who has the task in Active.
    if item.item_type == ItemType::Task && item.archived_at.is_none() {
        out.push_str(&crate::claims::claim_line(
            facts.claim(),
            facts.column_claims,
        ));
    }
    // KAIROS-T-0362: the cancel mark and its reason.
    if item.item_type == ItemType::Task {
        if let (Some(at), Some(by), Some(reason)) = (
            facts.cancelled_at,
            &facts.cancelled_by_name,
            &facts.cancel_reason,
        ) {
            out.push_str(&crate::cancel::cancel_line(at, by, reason));
        }
        // KAIROS-T-0506: the live state of the task.
        if let Some((state, name)) = facts.state(item.id) {
            out.push_str(&crate::task_state::state_line_of(&state, name, Utc::now()));
        }
    }
    // KAIROS-T-0411: the owner of a strategy or an initiative, in
    // each column and in the archive.
    if kairos_db::item_owners::is_subject(item.item_type) {
        out.push_str(&crate::owners::owner_line(facts.owner()));
    }
    // COLLIERY-T-0269: the owner of a document. It is a board and
    // not a position, so the line has no column.
    if item.item_type == ItemType::Document && item.board_id.is_some() {
        out.push_str(&format!("- owner board: {}\n", facts.board_slug()?));
    }
    // KAIROS-T-0561: the creator, because the edit rule names the creator.
    out.push_str(&facts.creator_line(item.created_at));
    out.push_str(&format!("- version: {}\n", item.version));
    out.push_str(&format!(
        "- updated: {}\n",
        item.updated_at.format("%Y-%m-%dT%H:%M:%SZ")
    ));
    // COLLIERY-T-3100: the codes that the item had before.
    if !facts.retired_codes.is_empty() {
        out.push_str(&format!(
            "- retired codes: {}\n",
            facts.retired_codes.join(", ")
        ));
    }
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
    if let Some(name) = &facts.template_name {
        out.push_str(&format!("- template: {name}\n"));
    }

    // Metadata values, ordered by definition slug.
    if !facts.metadata_slugs.is_empty() {
        out.push_str("\n## Metadata\n");
        for (slug, value) in facts.metadata_slugs.iter().zip(&facts.metadata_values) {
            out.push_str(&format!("- {slug}: {value}\n"));
        }
    }

    // Relationships (both directions, agent-oriented labels). The
    // `is_done` flag of the item's column, removed columns too: an item
    // with no column is not done (COLLIERY-T-0214).
    let relationships = relationship_lines(conn, item.id, facts.column_done)?;
    // COLLIERY-T-0269: the repositories that the item impacts are
    // in the same section as its edges.
    let impacts = impacts_line(conn, &item)?;
    // KAIROS-T-0321: the teams of an initiative or a strategy.
    let teams = teams_line(conn, &item)?;
    // KAIROS-T-0508: the external references of the item, oldest first.
    let external: String = facts
        .external_uris
        .iter()
        .zip(&facts.external_labels)
        .map(|(uri, label)| format!("- external: {}\n", external_text(uri, label.as_deref())))
        .collect();
    if !relationships.is_empty() || !impacts.is_empty() || !teams.is_empty() || !external.is_empty()
    {
        out.push_str("\n## Relationships\n");
        out.push_str(&teams);
        out.push_str(&relationships);
        out.push_str(&impacts);
        out.push_str(&external);
    }

    // KAIROS-T-0558: the edge proposals of the item, so that an agent
    // learns the outcome of `propose_edge`.
    let proposals: Vec<ProposalRow> =
        serde_json::from_str(&facts.proposals).map_err(ApiError::internal)?;
    out.push_str(&proposals_section(
        &item.short_code,
        facts.proposals_pending,
        &proposals,
    ));

    // KAIROS-T-0504: the notes of the item, each one with its id, so
    // that an agent cites `note n-3 on ACME-T-0012`. KAIROS-T-0630: the
    // notes are rows; the text of each one follows the content.
    let notes =
        kairos_db::item_notes::parse_notes_json(&facts.notes_json).map_err(ApiError::internal)?;
    if !notes.is_empty() {
        out.push_str("\n## Notes\n");
        for note in &notes {
            out.push_str(&format!(
                "- {} ({}, {})",
                note.note_id(),
                note.date(),
                note.author
            ));
            if !note.first_line().is_empty() {
                out.push_str(&format!(": {}", clip_line(note.first_line(), 120)));
            }
            out.push('\n');
        }
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
    // KAIROS-T-0630: the notes after the content, each one under its
    // `## Note n-3 (date, name)` heading, as before the notes were rows.
    out.push_str(&kairos_db::item_notes::content_with_notes(
        &item.content,
        &notes,
    ));
    out.push('\n');
    Ok(out)
}

/// What `get_item` shows besides the row of the item and its edges, in one
/// statement (KAIROS-T-0626): the board and the column (removed columns
/// too, KAIROS-T-0161), the moves from the column (live targets only,
/// KAIROS-T-0554), the repository of a task, the template of a document,
/// the retired codes (COLLIERY-T-3100), the metadata, the claim
/// (KAIROS-T-0359), the cancel mark (KAIROS-T-0362), the state
/// (KAIROS-T-0506), the owner (KAIROS-T-0411), the external references
/// (KAIROS-T-0508) and the creator (KAIROS-T-0561). A name that `public.users` does not have is the id, as
/// `task_claims::display_names` gives it.
const ITEM_FACTS_SQL: &str = "\
    SELECT b.slug AS board_slug, \
           c.name AS column_name, \
           COALESCE(c.is_done, false) AS column_done, \
           COALESCE(c.claims, false) AS column_claims, \
           ARRAY(SELECT n.name FROM board_transitions bt \
                   JOIN board_columns n ON n.id = bt.to_column_id \
                  WHERE bt.from_column_id = $3 AND n.deleted_at IS NULL \
                  ORDER BY n.position) AS next_columns, \
           rp.slug AS repository_slug, rt.slug AS repository_team, \
           tp.name AS template_name, \
           ARRAY(SELECT rc.code FROM retired_codes rc WHERE rc.item_id = $1 \
                  ORDER BY rc.retired_at, rc.code) AS retired_codes, \
           ARRAY(SELECT md.slug FROM item_metadata im \
                   JOIN metadata_definitions md ON md.id = im.metadata_definition_id \
                  WHERE im.item_id = $1 ORDER BY md.slug, im.id) AS metadata_slugs, \
           ARRAY(SELECT im.value FROM item_metadata im \
                   JOIN metadata_definitions md ON md.id = im.metadata_definition_id \
                  WHERE im.item_id = $1 ORDER BY md.slug, im.id) AS metadata_values, \
           COALESCE(cu.display_name, cl.user_id::text) AS claim_holder, \
           (cl.agent_key_id IS NOT NULL) AS claim_agent, \
           cl.claimed_at, \
           k.cancelled_at, k.reason AS cancel_reason, \
           COALESCE(ku.display_name, k.cancelled_by::text) AS cancelled_by_name, \
           st.branch AS state_branch, st.commit AS state_commit, \
           st.pushed AS state_pushed, st.pr_url AS state_pr_url, \
           st.next_step AS state_next_step, st.updated_at AS state_updated_at, \
           st.updated_by AS state_updated_by, \
           COALESCE(su.display_name, st.updated_by::text) AS state_updated_by_name, \
           COALESCE(ou.display_name, io.user_id::text) AS owner_name, \
           io.claimed_at AS owner_since, \
           COALESCE(cb.display_name, $6::text) AS creator_name, \
           cb.user_name AS creator_user_name, \
           ARRAY(SELECT x.uri FROM item_external_refs x WHERE x.item_id = $1 \
                  ORDER BY x.created_at, x.uri, x.id) AS external_uris, \
           ARRAY(SELECT x.label FROM item_external_refs x WHERE x.item_id = $1 \
                  ORDER BY x.created_at, x.uri, x.id) AS external_labels, \
           {notes} AS notes_json, \
           (SELECT count(*) FROM edge_proposals ep WHERE ep.state = 'pending' \
               AND (ep.source_id = $1 OR ep.target_id = $1)) AS proposals_pending, \
           COALESCE((SELECT json_agg(json_build_object( \
                   'id', p.id, 'source', p.source, 'target', p.target, \
                   'relationship', p.relationship, 'state', p.state, 'why', p.why, \
                   'proposer', p.proposer, 'proposed_on', p.proposed_on, \
                   'decider', p.decider, 'decided_on', p.decided_on) \
                   ORDER BY p.pick, p.n) \
              FROM ((SELECT 0 AS pick, row_number() OVER ( \
                            ORDER BY ep.score DESC, ep.created_at, ep.id) AS n, \
                            ep.id, sd.short_code AS source, td.short_code AS target, \
                            ep.relationship, ep.state, ep.why, \
                            COALESCE(pu.display_name, ep.proposed_by::text) AS proposer, \
                            to_char(ep.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') \
                                AS proposed_on, \
                            NULL::text AS decider, NULL::text AS decided_on \
                       FROM edge_proposals ep \
                       LEFT JOIN entity_directory sd ON sd.id = ep.source_id \
                       LEFT JOIN entity_directory td ON td.id = ep.target_id \
                       LEFT JOIN public.users pu ON pu.id = ep.proposed_by \
                      WHERE ep.state = 'pending' \
                        AND (ep.source_id = $1 OR ep.target_id = $1) \
                      ORDER BY n LIMIT 20) \
                    UNION ALL \
                    (SELECT 1, row_number() OVER ( \
                            ORDER BY ep.decided_at DESC NULLS LAST, ep.id), \
                            ep.id, sd.short_code, td.short_code, \
                            ep.relationship, ep.state, ep.why, \
                            COALESCE(pu.display_name, ep.proposed_by::text), \
                            to_char(ep.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD'), \
                            COALESCE(du.display_name, ep.decided_by::text), \
                            to_char(ep.decided_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') \
                       FROM edge_proposals ep \
                       LEFT JOIN entity_directory sd ON sd.id = ep.source_id \
                       LEFT JOIN entity_directory td ON td.id = ep.target_id \
                       LEFT JOIN public.users pu ON pu.id = ep.proposed_by \
                       LEFT JOIN public.users du ON du.id = ep.decided_by \
                      WHERE ep.state <> 'pending' \
                        AND (ep.source_id = $1 OR ep.target_id = $1) \
                      ORDER BY 2 LIMIT 5)) AS p), '[]')::text AS proposals \
      FROM (SELECT 1) AS one \
      LEFT JOIN boards b ON b.id = $2 \
      LEFT JOIN board_columns c ON c.id = $3 \
      LEFT JOIN repositories rp ON rp.id = $4 AND rp.deleted_at IS NULL \
      LEFT JOIN teams rt ON rt.id = rp.team_id \
      LEFT JOIN templates tp ON tp.id = $5 \
      LEFT JOIN task_claims cl ON cl.task_id = $1 \
      LEFT JOIN public.users cu ON cu.id = cl.user_id \
      LEFT JOIN task_cancellations k ON k.task_id = $1 \
      LEFT JOIN public.users ku ON ku.id = k.cancelled_by \
      LEFT JOIN task_state st ON st.task_id = $1 \
      LEFT JOIN public.users su ON su.id = st.updated_by \
      LEFT JOIN item_owners io ON io.item_id = $1 \
      LEFT JOIN public.users ou ON ou.id = io.user_id \
      LEFT JOIN public.users cb ON cb.id = $6";

/// The row of [`ITEM_FACTS_SQL`].
#[derive(QueryableByName)]
struct ItemFacts {
    #[diesel(sql_type = SqlNullable<SqlText>)]
    board_slug: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    column_name: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    column_done: bool,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    column_claims: bool,
    #[diesel(sql_type = diesel::sql_types::Array<SqlText>)]
    next_columns: Vec<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    repository_slug: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    repository_team: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    template_name: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Array<SqlText>)]
    retired_codes: Vec<String>,
    #[diesel(sql_type = diesel::sql_types::Array<SqlText>)]
    metadata_slugs: Vec<String>,
    #[diesel(sql_type = diesel::sql_types::Array<SqlText>)]
    metadata_values: Vec<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    claim_holder: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    claim_agent: bool,
    #[diesel(sql_type = SqlNullable<SqlTimestamptz>)]
    claimed_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = SqlNullable<SqlTimestamptz>)]
    cancelled_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    cancel_reason: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    cancelled_by_name: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    state_branch: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    state_commit: Option<String>,
    #[diesel(sql_type = SqlNullable<diesel::sql_types::Bool>)]
    state_pushed: Option<bool>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    state_pr_url: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    state_next_step: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlTimestamptz>)]
    state_updated_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = SqlNullable<SqlUuid>)]
    state_updated_by: Option<Uuid>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    state_updated_by_name: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    owner_name: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlTimestamptz>)]
    owner_since: Option<DateTime<Utc>>,
    /// The display name of the creator, or the id (KAIROS-T-0561).
    #[diesel(sql_type = SqlText)]
    creator_name: String,
    /// The user name of the creator, when `public.users` has the row.
    #[diesel(sql_type = SqlNullable<SqlText>)]
    creator_user_name: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Array<SqlText>)]
    external_uris: Vec<String>,
    #[diesel(sql_type = diesel::sql_types::Array<SqlNullable<SqlText>>)]
    external_labels: Vec<Option<String>>,
    /// The notes ([`kairos_db::item_notes::NOTES_JSON_OF_ITEM`],
    /// KAIROS-T-0630).
    #[diesel(sql_type = SqlText)]
    notes_json: String,
    /// The count of the pending proposals of the item (KAIROS-T-0558).
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    proposals_pending: i64,
    /// The proposals of the item as JSON: at most 20 pending ones, then the
    /// 5 newest decided ones (KAIROS-T-0558). See [`ProposalRow`].
    #[diesel(sql_type = SqlText)]
    proposals: String,
}

/// One edge proposal of an item, as `get_item` lists it (KAIROS-T-0558).
#[derive(Debug, Deserialize)]
struct ProposalRow {
    id: Uuid,
    source: Option<String>,
    target: Option<String>,
    relationship: String,
    state: String,
    why: String,
    proposer: String,
    proposed_on: String,
    decider: Option<String>,
    decided_on: Option<String>,
}

/// The `## Proposals` section of `get_item` (KAIROS-T-0558): each pending
/// edge proposal of the item, and the newest decided ones, with the state.
/// Empty when the item has none. Pure.
fn proposals_section(short_code: &str, pending: i64, rows: &[ProposalRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Proposals\n");
    for row in rows {
        let edge = format!(
            "{} -[{}]-> {}",
            row.source.as_deref().unwrap_or("(gone)"),
            row.relationship,
            row.target.as_deref().unwrap_or("(gone)")
        );
        out.push_str(&format!(
            "- {}: {edge} ({}), proposed by {} on {}",
            row.state, row.id, row.proposer, row.proposed_on
        ));
        if let (Some(by), Some(on)) = (&row.decider, &row.decided_on) {
            out.push_str(&format!(", {} by {by} on {on}", row.state));
        }
        out.push_str(&format!(". Why: {}\n", clip_line(&row.why, 120)));
    }
    let listed = rows.iter().filter(|row| row.state == "pending").count() as i64;
    if pending > listed {
        out.push_str(&format!(
            "- {} more pending proposals. GET /api/items/{short_code}/proposals lists each one.\n",
            pending - listed
        ));
    }
    out
}

impl ItemFacts {
    /// `slug (owner: team)` for the repository of a task, or `(none)`
    /// (as [`repo_label`] says it).
    fn repository_label(&self) -> String {
        match (&self.repository_slug, &self.repository_team) {
            (Some(slug), Some(team)) => format!("{slug} (owner: {team})"),
            _ => "(none)".to_string(),
        }
    }

    /// The slug of the board of the item: it must be there, the item holds
    /// a FK to it.
    fn board_slug(&self) -> Result<&str, ApiError> {
        self.board_slug
            .as_deref()
            .ok_or_else(|| ApiError::internal("the board of the item is not there"))
    }

    /// The claim as (holder, made with an agent key, since).
    fn claim(&self) -> Option<(&str, bool, DateTime<Utc>)> {
        Some((
            self.claim_holder.as_deref()?,
            self.claim_agent,
            self.claimed_at?,
        ))
    }

    /// `- created by: Robin (robin) on 2026-10-16T22:04:24Z`.
    fn creator_line(&self, created_at: DateTime<Utc>) -> String {
        let user_name = self
            .creator_user_name
            .as_deref()
            .map(|name| format!(" ({name})"))
            .unwrap_or_default();
        format!(
            "- created by: {}{user_name} on {}\n",
            self.creator_name,
            created_at.format("%Y-%m-%dT%H:%M:%SZ")
        )
    }

    /// The owner as (name, since).
    fn owner(&self) -> Option<(&str, DateTime<Utc>)> {
        Some((self.owner_name.as_deref()?, self.owner_since?))
    }

    /// The state of the task and the name of who wrote it.
    fn state(&self, task_id: Uuid) -> Option<(kairos_db::models::TaskState, &str)> {
        Some((
            kairos_db::models::TaskState {
                task_id,
                branch: self.state_branch.clone(),
                commit: self.state_commit.clone(),
                pushed: self.state_pushed?,
                pr_url: self.state_pr_url.clone(),
                next_step: self.state_next_step.clone(),
                updated_at: self.state_updated_at?,
                updated_by: self.state_updated_by?,
            },
            self.state_updated_by_name.as_deref()?,
        ))
    }
}

/// [`ITEM_FACTS_SQL`] for `item`.
fn item_facts(conn: &mut PgConnection, item: &ItemView) -> Result<ItemFacts, ApiError> {
    sql_query(ITEM_FACTS_SQL.replace("{notes}", kairos_db::item_notes::NOTES_JSON_OF_ITEM))
        .bind::<SqlUuid, _>(item.id)
        .bind::<SqlNullable<SqlUuid>, _>(item.board_id)
        .bind::<SqlNullable<SqlUuid>, _>(item.column_id)
        .bind::<SqlNullable<SqlUuid>, _>(item.repository_id)
        .bind::<SqlNullable<SqlUuid>, _>(item.template_id)
        .bind::<SqlUuid, _>(item.created_by)
        .get_result(conn)
        .map_err(ApiError::internal)
}

/// A uniform projection of any live item, whatever its table.
struct ItemView {
    id: Uuid,
    item_type: ItemType,
    /// The current code of the item.
    short_code: String,
    /// The retired code that found the item (COLLIERY-T-3100), or `None`.
    retired: Option<String>,
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
    /// Who created the item. A refusal uses it to know whether the caller
    /// is the person who filed a request (COLLIERY-T-0218).
    created_by: Uuid,
    /// When the item was created; `get_item` shows it (KAIROS-T-0561).
    created_at: DateTime<Utc>,
}

/// Resolve a short code and load its [`ItemView`]; 404 `NOT_FOUND`
/// otherwise (mirrors the REST 404 contract).
///
/// One statement (KAIROS-T-0626): [`LOAD_ITEM_SQL`] finds the code in
/// `entity_directory`, or else the item of the retired code, and reads the
/// row of the item's table in the same query. The code is upper-cased
/// first (KAIROS-T-0560).
///
/// `liveness` is decided here, on the row that was found: a read
/// ([`Liveness::IncludeArchived`]) follows a retired code to the item and
/// serves archived work; a write ([`Liveness::LiveOnly`]) is refused for a
/// retired code, and the refusal names the current code (COLLIERY-T-3100),
/// and for an archived item (KAIROS-T-0560).
fn load_item(
    conn: &mut PgConnection,
    short_code: &str,
    liveness: Liveness,
) -> Result<ItemView, ApiError> {
    let upper = short_code.to_ascii_uppercase();
    let row: Option<LoadedItem> = sql_query(LOAD_ITEM_SQL)
        .bind::<SqlText, _>(&upper)
        .get_result(conn)
        .optional()
        .map_err(ApiError::internal)?;
    let Some(row) = row else {
        return Err(no_item(short_code, liveness));
    };
    if liveness == Liveness::LiveOnly {
        refuse_write(
            short_code,
            row.retired.is_some(),
            row.deleted_at.is_some(),
            &row.short_code,
        )?;
    }
    let item_type = item_type_of(&row.entity_type)?;
    Ok(ItemView {
        id: row.id,
        item_type,
        short_code: row.short_code,
        retired: row.retired,
        title: row.title,
        content: row.content,
        version: row.version,
        board_id: row.board_id,
        column_id: row.column_id,
        task_type: row.task_type,
        work_class: row.work_class,
        repository_id: row.repository_id,
        lifecycle: row.lifecycle,
        complexity: row.complexity,
        hypothesis: row.hypothesis,
        bucket_type: row.bucket_type,
        template_id: row.template_id,
        decision_maker: row.decision_maker,
        decision_date: row.decision_date,
        updated_at: row.updated_at,
        archived_at: row.deleted_at,
        created_by: row.created_by,
        created_at: row.created_at,
    })
}

/// The refusal of [`load_item`] for a code that finds no item.
fn no_item(short_code: &str, liveness: Liveness) -> ApiError {
    match liveness {
        Liveness::LiveOnly => {
            ApiError::not_found(format!("No live item has the short code {short_code:?}."))
        }
        Liveness::IncludeArchived => {
            ApiError::not_found(format!("No item has the short code {short_code:?}."))
        }
    }
}

/// The refusals of a write: a retired code names the current code
/// (COLLIERY-T-3100), and an archived item says so (KAIROS-T-0560).
fn refuse_write(
    short_code: &str,
    retired: bool,
    archived: bool,
    current_code: &str,
) -> Result<(), ApiError> {
    if retired {
        return Err(crate::api::retired_code_refused(short_code, current_code));
    }
    if archived {
        return Err(crate::api::archived_write_refused(
            &short_code.to_ascii_uppercase(),
        ));
    }
    Ok(())
}

/// The [`ItemType`] of an `entity_type` of `entity_directory`.
fn item_type_of(entity_type: &str) -> Result<ItemType, ApiError> {
    ItemType::ALL
        .iter()
        .copied()
        .find(|t| t.entity_type() == entity_type)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "entity_directory returned unknown entity_type {entity_type:?}"
            ))
        })
}

/// The id, the type and the code of a live item, for a write that needs
/// no content (KAIROS-T-0635).
struct ItemKey {
    id: Uuid,
    item_type: ItemType,
    short_code: String,
}

/// [`load_item`] for a write that needs no content (KAIROS-T-0635): one
/// statement on `entity_directory`, which has no content, and the same
/// refusals as [`load_item`] with [`Liveness::LiveOnly`].
fn load_item_key(conn: &mut PgConnection, short_code: &str) -> Result<ItemKey, ApiError> {
    let row: Option<LoadedKey> = sql_query(LOAD_ITEM_KEY_SQL)
        .bind::<SqlText, _>(short_code.to_ascii_uppercase())
        .get_result(conn)
        .optional()
        .map_err(ApiError::internal)?;
    let Some(row) = row else {
        return Err(no_item(short_code, Liveness::LiveOnly));
    };
    refuse_write(
        short_code,
        row.retired.is_some(),
        row.deleted_at.is_some(),
        &row.short_code,
    )?;
    Ok(ItemKey {
        id: row.id,
        item_type: item_type_of(&row.entity_type)?,
        short_code: row.short_code,
    })
}

/// The statement of [`load_item_key`]: the `hit` of [`LOAD_ITEM_SQL`],
/// with the code and the archive time from `entity_directory`.
const LOAD_ITEM_KEY_SQL: &str = "\
    SELECT d.id, d.entity_type, d.short_code, d.deleted_at, NULL::text AS retired, 0 AS pick \
      FROM entity_directory d WHERE d.short_code = $1 \
    UNION ALL \
    SELECT d.id, d.entity_type, d.short_code, d.deleted_at, r.code, 1 \
      FROM retired_codes r JOIN entity_directory d ON d.id = r.item_id \
     WHERE r.code = $1 \
    ORDER BY pick LIMIT 1";

/// The row of [`LOAD_ITEM_KEY_SQL`].
#[derive(QueryableByName)]
struct LoadedKey {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlText)]
    entity_type: String,
    #[diesel(sql_type = SqlText)]
    short_code: String,
    #[diesel(sql_type = SqlNullable<diesel::sql_types::Timestamptz>)]
    deleted_at: Option<DateTime<Utc>>,
    /// The retired code that found the item, or `None`.
    #[diesel(sql_type = SqlNullable<SqlText>)]
    retired: Option<String>,
}

/// The statement of [`load_item`] (KAIROS-T-0626). `hit` is the code in
/// `entity_directory`, or else the item of the retired code `$1` (a code
/// is held or retired, never the two). Then the row of the item's own
/// table: each LEFT JOIN matches for one type only. A document has an
/// owner board and no column (COLLIERY-T-0269), so no reader takes it for
/// a card.
const LOAD_ITEM_SQL: &str = "\
    WITH hit AS ( \
        (SELECT d.id, d.entity_type, NULL::text AS retired, 0 AS pick \
           FROM entity_directory d WHERE d.short_code = $1 \
         UNION ALL \
         SELECT d.id, d.entity_type, r.code, 1 \
           FROM retired_codes r JOIN entity_directory d ON d.id = r.item_id \
          WHERE r.code = $1) \
        ORDER BY pick LIMIT 1) \
    SELECT h.entity_type, h.retired, h.id, \
           COALESCE(s.short_code, i.short_code, t.short_code, o.short_code, a.short_code) \
               AS short_code, \
           COALESCE(s.title, i.title, t.title, o.title, a.title) AS title, \
           COALESCE(s.content, i.content, t.content, o.content, a.content) AS content, \
           COALESCE(s.version, i.version, t.version, o.version, a.version) AS version, \
           COALESCE(s.board_id, i.board_id, t.board_id, o.board_id, a.board_id) AS board_id, \
           COALESCE(s.column_id, i.column_id, t.column_id, a.column_id) AS column_id, \
           t.task_type, t.work_class, t.repository_id, o.lifecycle, i.complexity, \
           s.hypothesis, i.bucket_type, o.template_id, a.decision_maker, a.decision_date, \
           COALESCE(s.updated_at, i.updated_at, t.updated_at, o.updated_at, a.updated_at) \
               AS updated_at, \
           COALESCE(s.deleted_at, i.deleted_at, t.deleted_at, o.deleted_at, a.deleted_at) \
               AS deleted_at, \
           COALESCE(s.created_by, i.created_by, t.created_by, o.created_by, a.created_by) \
               AS created_by, \
           COALESCE(s.created_at, i.created_at, t.created_at, o.created_at, a.created_at) \
               AS created_at \
      FROM hit h \
      LEFT JOIN strategies s ON h.entity_type = 'strategy' AND s.id = h.id \
      LEFT JOIN initiatives i ON h.entity_type = 'initiative' AND i.id = h.id \
      LEFT JOIN tasks t ON h.entity_type = 'task' AND t.id = h.id \
      LEFT JOIN documents o ON h.entity_type = 'document' AND o.id = h.id \
      LEFT JOIN adrs a ON h.entity_type = 'adr' AND a.id = h.id";

/// The row of [`LOAD_ITEM_SQL`].
#[derive(QueryableByName)]
struct LoadedItem {
    #[diesel(sql_type = SqlText)]
    entity_type: String,
    /// The retired code that found the item, or `None`.
    #[diesel(sql_type = SqlNullable<SqlText>)]
    retired: Option<String>,
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlText)]
    short_code: String,
    #[diesel(sql_type = SqlText)]
    title: String,
    #[diesel(sql_type = SqlText)]
    content: String,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    version: i32,
    #[diesel(sql_type = SqlNullable<SqlUuid>)]
    board_id: Option<Uuid>,
    #[diesel(sql_type = SqlNullable<SqlUuid>)]
    column_id: Option<Uuid>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    task_type: Option<TaskType>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    work_class: Option<WorkClass>,
    #[diesel(sql_type = SqlNullable<SqlUuid>)]
    repository_id: Option<Uuid>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    lifecycle: Option<DocumentLifecycle>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    complexity: Option<Complexity>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    hypothesis: Option<String>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    bucket_type: Option<BucketType>,
    #[diesel(sql_type = SqlNullable<SqlUuid>)]
    template_id: Option<Uuid>,
    #[diesel(sql_type = SqlNullable<SqlText>)]
    decision_maker: Option<String>,
    #[diesel(sql_type = SqlNullable<diesel::sql_types::Date>)]
    decision_date: Option<NaiveDate>,
    #[diesel(sql_type = SqlTimestamptz)]
    updated_at: DateTime<Utc>,
    #[diesel(sql_type = SqlNullable<SqlTimestamptz>)]
    deleted_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = SqlUuid)]
    created_by: Uuid,
    #[diesel(sql_type = SqlTimestamptz)]
    created_at: DateTime<Utc>,
}

/// `require_capability` for a MOVE (`transition_item`, `move_item`), with
/// one explanation added. When the refused caller CREATED this item as a
/// request and it is still in the entry column, the refusal explains the
/// request rule instead of the bare capability name (KAIROS-T-0123, UAT
/// finding #5). The HTTP API keeps its generic envelope; this is
/// agent-facing text.
///
/// COLLIERY-T-0218 (COLLIERY-A-0023): the explanation is given ONLY when
/// all of these are true:
///
/// - `board_id` is the board the item is on (a move asks about the target
///   board as well, and the item is not there),
/// - the item is in the entry column of that board,
/// - the caller created the item,
/// - the caller holds `file_backlog` there, so the board is a delivery
///   board.
///
/// Until then the only test was `check_file_backlog`, which takes no column
/// and no author. Every member holds it on every delivery board, so EVERY
/// refused write by a member was told that the item "sits in the Backlog",
/// whatever column the card was in and whoever had created it. That was a
/// false statement about the board, given to an agent that acts on what it
/// reads. In every other case the plain refusal, which names the missing
/// capability, is what the caller gets.
///
/// COLLIERY-T-0228: the text said "To move, edit or delete it, you need
/// ...". The creator of an item can now edit it, link it and archive it
/// (the edit rule, [`require_item_edit`]), so no edit comes here, and the
/// creator is refused a move only. The text says that. This function is
/// NOT the edit rule and must not become it: creation grants no movement.
/// `details.held` stays `file_backlog`: that capability is how the request
/// was created.
fn require_capability_explained(
    conn: &mut PgConnection,
    slug: &str,
    board_id: Option<Uuid>,
    user: Uuid,
    capability: &str,
    item: &ItemView,
) -> Result<(), ApiError> {
    let err = match require_capability(conn, slug, board_id, user, capability) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    let Some(board_id) = board_id else {
        return Err(err);
    };
    if err.code != "FORBIDDEN" || item.board_id != Some(board_id) || item.created_by != user {
        return Err(err);
    }
    let in_entry_column = item.column_id.is_some()
        && boards::entry_column(conn, board_id).map_err(ApiError::internal)? == item.column_id;
    if !in_entry_column || !abac::check_file_backlog(conn, slug, board_id, user).unwrap_or(false) {
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
        .map(|team| format!("the team {team}"))
        .unwrap_or_else(|| "the team that owns the board".to_string());
    let short_code = &item.short_code;
    Err(ApiError::forbidden(format!(
        "{short_code} is a request in the entry column of the board of {whose}. \
         That team moves it. You created it (file_backlog), so you can edit it, link it and archive it. \
         To move it, you need {capability:?} on that board."
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
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "No live board has the slug or the id {reference:?}."
            ))
        })
}

/// Resolve a team by UUID or slug; 422 otherwise (a filter value).
/// The id of a metadata definition or a template (KAIROS-T-0443), by slug
/// or UUID. `table` is `metadata_definitions` or `templates`.
fn meta_id(
    conn: &mut PgConnection,
    table: &str,
    reference: &str,
    argument: &str,
) -> Result<Uuid, ApiError> {
    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        id: Uuid,
    }
    let reference = reference.trim();
    let sql = format!("SELECT id FROM {table} WHERE id::text = $1 OR slug = $1 LIMIT 1");
    let row: Option<Row> = diesel::sql_query(sql)
        .bind::<diesel::sql_types::Text, _>(reference)
        .get_result(conn)
        .optional()
        .map_err(ApiError::internal)?;
    row.map(|row| row.id).ok_or_else(|| {
        ApiError::not_found(format!(
            "No {} has the slug or the id {reference:?}.",
            match table {
                "templates" => "template",
                _ => "metadata definition",
            }
        ))
        .with_details(json!({ "field": argument }))
    })
}

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
        .ok_or_else(|| {
            ApiError::validation(format!(
                "The team {reference:?} is not in the organization. Send the id or the \
                 slug of a team of the organization."
            ))
        })
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
        "This board has no column {reference:?}. The columns are: {}.",
        names.join(", ")
    )))
}

/// One compact row of a board listing.
struct BoardItemRow {
    /// The item's id: the key of the blocks rollup (COLLIERY-T-0214).
    id: Uuid,
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
    /// The last change (KAIROS-T-0368: `board_items` with `details`).
    updated_at: DateTime<Utc>,
}

/// What each family's board listing selects. The trailing
/// `Option<DateTime<Utc>>` is `deleted_at`, selected in BOTH modes rather
/// than only the widened one: the live mode must still be able to prove
/// it served nothing archived, and a marker that is only fetched when it
/// might be set is a marker nobody checks (KAIROS-T-0159).
type BoardStrategySelect = (
    Uuid,
    Uuid,
    String,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);
type BoardInitiativeSelect = (
    Uuid,
    Uuid,
    String,
    String,
    bool,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);
type BoardTaskSelect = (
    Uuid,
    Uuid,
    String,
    String,
    TaskType,
    WorkClass,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);
type BoardAdrSelect = (
    Uuid,
    Option<Uuid>,
    String,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

/// Every item placed on a board (strategies, initiatives, tasks, and
/// on-board ADRs — documents have no placement), unified for listing.
/// `repository` (KAIROS-T-0107) narrows the TASKS only.
///
/// `liveness` is a parameter rather than a constant so that the one caller
/// that wants the audit view — MCP `board_items` with `include_deleted` —
/// can have it. The per-column counts of `my_boards` do not come from this
/// function (COLLIERY-T-0265): `kairos_db::board_items::live_column_counts`
/// counts live work in the database (ADR-20 rule 5).
///
/// `only` narrows the rows to the items with these ids
/// (COLLIERY-T-0261): the items of one page of the board.
fn board_item_rows(
    conn: &mut PgConnection,
    board_id: Uuid,
    repository: Option<Uuid>,
    liveness: Liveness,
    only: Option<&[Uuid]>,
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
    if let Some(ids) = only {
        strategy_query = strategy_query.filter(strategies::id.eq_any(ids));
    }
    let strategies: Vec<BoardStrategySelect> = strategy_query
        .order(strategies::short_code.asc())
        .select((
            strategies::id,
            strategies::column_id,
            strategies::short_code,
            strategies::title,
            strategies::deleted_at,
            strategies::updated_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(strategies.into_iter().map(
        |(id, column_id, short_code, title, deleted_at, updated_at)| BoardItemRow {
            id,
            column_id,
            short_code,
            title,
            repository_id: None,
            kind: "strategy".to_string(),
            archived: deleted_at.is_some(),
            updated_at,
        },
    ));

    let mut initiative_query = initiatives::table
        .filter(initiatives::board_id.eq(board_id))
        .into_boxed();
    if live_only {
        initiative_query = initiative_query.filter(initiatives::deleted_at.is_null());
    }
    if let Some(ids) = only {
        initiative_query = initiative_query.filter(initiatives::id.eq_any(ids));
    }
    let initiatives: Vec<BoardInitiativeSelect> = initiative_query
        .order(initiatives::short_code.asc())
        .select((
            initiatives::id,
            initiatives::column_id,
            initiatives::short_code,
            initiatives::title,
            initiatives::is_bucket,
            initiatives::deleted_at,
            initiatives::updated_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(initiatives.into_iter().map(
        |(id, column_id, short_code, title, is_bucket, deleted_at, updated_at)| BoardItemRow {
            id,
            column_id,
            short_code,
            title,
            repository_id: None,
            kind: if is_bucket { "bucket" } else { "initiative" }.to_string(),
            archived: deleted_at.is_some(),
            updated_at,
        },
    ));

    let mut task_query = tasks::table
        .filter(tasks::board_id.eq(board_id))
        .into_boxed();
    if live_only {
        task_query = task_query.filter(tasks::deleted_at.is_null());
    }
    if let Some(ids) = only {
        task_query = task_query.filter(tasks::id.eq_any(ids));
    }
    if let Some(repository) = repository {
        task_query = task_query.filter(tasks::repository_id.eq(repository));
    }
    let tasks: Vec<BoardTaskSelect> = task_query
        .order(tasks::short_code.asc())
        .select((
            tasks::id,
            tasks::column_id,
            tasks::short_code,
            tasks::title,
            tasks::task_type,
            tasks::work_class,
            tasks::repository_id,
            tasks::deleted_at,
            tasks::updated_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(tasks.into_iter().map(
        |(
            id,
            column_id,
            short_code,
            title,
            task_type,
            work_class,
            repository_id,
            deleted_at,
            updated_at,
        )| {
            BoardItemRow {
                id,
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
                updated_at,
            }
        },
    ));

    let mut adr_query = adrs::table.filter(adrs::board_id.eq(board_id)).into_boxed();
    if live_only {
        adr_query = adr_query.filter(adrs::deleted_at.is_null());
    }
    if let Some(ids) = only {
        adr_query = adr_query.filter(adrs::id.eq_any(ids));
    }
    let adrs: Vec<BoardAdrSelect> = adr_query
        .order(adrs::short_code.asc())
        .select((
            adrs::id,
            adrs::column_id,
            adrs::short_code,
            adrs::title,
            adrs::deleted_at,
            adrs::updated_at,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.extend(adrs.into_iter().filter_map(
        |(id, column_id, short_code, title, deleted_at, updated_at)| {
            column_id.map(|column_id| BoardItemRow {
                id,
                column_id,
                short_code,
                title,
                repository_id: None,
                kind: "adr".to_string(),
                archived: deleted_at.is_some(),
                updated_at,
            })
        },
    ));

    Ok(rows)
}

/// The details of one `board_items` line (KAIROS-T-0368):
/// ` {updated 2026-10-08; priority: high; size: m}`.
fn card_details(updated_at: DateTime<Utc>, metadata: Option<&Vec<(String, String)>>) -> String {
    let mut parts = vec![format!("updated {}", updated_at.format("%Y-%m-%d"))];
    for (slug, value) in metadata.into_iter().flatten() {
        parts.push(format!("{slug}: {value}"));
    }
    format!(" {{{}}}", parts.join("; "))
}

/// The filter of a rule, for a sentence: `"and is a bug "`, or empty. Pure.
/// When a rule fires (KAIROS-T-0427). Pure.
fn rule_when_text(rule: &kairos_client::types_dispatch::DispatchRule) -> String {
    match (rule.trigger.as_str(), rule.column.as_deref()) {
        ("unblocked", Some(column)) => format!("a task in {column} that is unblocked"),
        ("unblocked", None) => "a task that is unblocked".to_string(),
        (_, column) => format!("a task that enters {}", column.unwrap_or("?")),
    }
}

fn rule_filter_text(rule: &kairos_client::types_dispatch::DispatchRule) -> String {
    let mut parts = Vec::new();
    if let Some(t) = &rule.task_type {
        parts.push(format!("has the type {t}"));
    }
    if let (Some(slug), Some(value)) = (&rule.metadata_slug, &rule.metadata_value) {
        parts.push(format!("has {slug}={value}"));
    }
    if let Some(repo) = &rule.repository {
        parts.push(format!("has the repository {repo}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("and {} ", parts.join(" and "))
    }
}

/// The text of `list_org_read_tokens` (KAIROS-T-0431). Pure. It has no
/// token: the status has none.
fn org_read_tokens_text(tokens: &[kairos_client::types_code_index::OrgReadToken]) -> String {
    if tokens.is_empty() {
        return "Read tokens of the organization: none. The builder uses the token of each \
                repository, or no token."
            .to_string();
    }
    let mut out = String::from("Read tokens of the organization, by host:\n");
    for token in tokens {
        let by = token.set_by_name.as_deref().unwrap_or(&token.set_by);
        out.push_str(&format!(
            "- {}: set by {by} at {}\n",
            token.host, token.set_at
        ));
    }
    out
}

/// The text of `list_dispatch_rules` (KAIROS-T-0385). Pure.
fn dispatch_text(
    board: &str,
    rules: &[kairos_client::types_dispatch::DispatchRule],
    targets: &[kairos_client::types_dispatch::WebhookTarget],
    log: &[kairos_client::types_dispatch::WebhookDelivery],
) -> String {
    let mut out = format!("Dispatch rules of {board} ({}):\n", rules.len());
    if rules.is_empty() {
        out.push_str("  (none)\n");
    }
    for rule in rules {
        out.push_str(&format!(
            "- {}: {} {}-> {} ({})\n",
            rule.id,
            rule_when_text(rule),
            rule_filter_text(rule),
            rule.target,
            rule.target_id
        ));
    }
    out.push_str(&format!("Webhook targets ({}):\n", targets.len()));
    if targets.is_empty() {
        out.push_str("  (none)\n");
    }
    for target in targets {
        let state = if target.enabled {
            format!("on, {} failed tries in a row", target.consecutive_failures)
        } else {
            format!(
                "OFF: {}",
                target.disabled_reason.as_deref().unwrap_or("turned off")
            )
        };
        out.push_str(&format!(
            "- {} {} <{}>: {state}\n",
            target.id, target.name, target.url
        ));
    }
    out.push_str(&format!("Newest deliveries ({}):\n", log.len()));
    if log.is_empty() {
        out.push_str("  (none)\n");
    }
    for d in log {
        out.push_str(&format!(
            "- {} {} {} {}: {}, {} {}{}{}\n",
            d.created_at,
            d.id,
            d.event,
            d.short_code.as_deref().unwrap_or("-"),
            d.status,
            d.tries,
            if d.tries == 1 { "try" } else { "tries" },
            d.response_code
                .map(|c| format!(", code {c}"))
                .unwrap_or_default(),
            d.last_error
                .as_deref()
                .map(|e| format!(", {e}"))
                .unwrap_or_default()
        ));
    }
    out
}

/// The text of `board_flow` (KAIROS-T-0372). Pure.
fn flow_text(flow: &kairos_client::types_org::BoardFlow) -> String {
    let days = |value: Option<f64>| value.map_or_else(|| "-".to_string(), |v| format!("{v}"));
    let mut out = format!(
        "# Flow of {} ({} to {})\n\nStart columns: {}. Done columns: {}.\nDone: {}. Cancelled: \
         {} (counted apart). Done with no start: {}.\n",
        flow.board_slug,
        flow.from,
        flow.to,
        if flow.start_columns.is_empty() {
            "none (no cycle time)".to_string()
        } else {
            flow.start_columns.join(", ")
        },
        if flow.done_columns.is_empty() {
            "none".to_string()
        } else {
            flow.done_columns.join(", ")
        },
        flow.done_count,
        flow.cancelled_count,
        flow.done_without_start,
    );
    let cycle = &flow.cycle_time;
    out.push_str(&format!(
        "\n## Cycle time (days)\ncount {}, mean {}, median {}, 85th percentile {}\n",
        cycle.count,
        days(cycle.mean_days),
        days(cycle.median_days),
        days(cycle.p85_days)
    ));
    for item in &cycle.items {
        out.push_str(&format!(
            "- {} {} ({} days, done {})\n",
            item.short_code,
            item.title,
            item.days,
            item.done_at.get(..10).unwrap_or(&item.done_at)
        ));
    }
    out.push_str("\n## Throughput per week\n");
    for week in &flow.throughput {
        out.push_str(&format!(
            "- week of {}: done {}, cancelled {}\n",
            week.week_start, week.done, week.cancelled
        ));
    }
    out.push_str("\n## Work in progress (oldest first)\n");
    if flow.ageing.is_empty() {
        out.push_str("No item is in progress.\n");
    }
    for item in &flow.ageing {
        out.push_str(&format!(
            "- {} {} [{}] {} days\n",
            item.short_code, item.title, item.column, item.age_days
        ));
    }
    let cfd = &flow.cumulative_flow;
    out.push_str(&format!(
        "\n## Cumulative flow (last 14 days)\ndate | {}\n",
        cfd.columns.join(" | ")
    ));
    let skip = cfd.days.len().saturating_sub(14);
    for day in cfd.days.iter().skip(skip) {
        out.push_str(&format!(
            "{} | {}\n",
            day.date,
            day.counts
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    out
}

/// The WIP limit of a column in the text of `board_items` (KAIROS-T-0371):
/// ` [WIP 2/3]`, ` [WIP 4/3, over the limit]`, or nothing for a column with
/// no limit. `count` is the live count of the column, on each page. Pure.
fn wip_marker(count: i64, wip_limit: Option<i32>) -> String {
    match wip_limit {
        None => String::new(),
        Some(limit) if kairos_db::board_items::over_wip_limit(count, Some(limit)) => {
            format!(" [WIP {count}/{limit}, over the limit]")
        }
        Some(limit) => format!(" [WIP {count}/{limit}]"),
    }
}

/// The open-dependency tags of one `board_items` line: `" [blocked by
/// N]"` and `" [blocks N]"`, each only when N is above zero
/// (COLLIERY-T-0214). The words are the ones the board card shows, so the
/// agent and the person read the same thing. A card with no open `blocks`
/// edge gets no tag: that includes every card in a terminal column, and
/// every card whose blockers are all done or archived.
fn blocks_marker(counts: graph::BlocksCounts) -> String {
    let mut out = String::new();
    if counts.blocked_by > 0 {
        out.push_str(&format!(" [blocked by {}]", counts.blocked_by));
    }
    if counts.blocks > 0 {
        out.push_str(&format!(" [blocks {}]", counts.blocks));
    }
    out
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

/// The relationship of `link_items` or of `unlink_items`. The refusal
/// names `impacts` with the five relationships of an edge between two
/// items (COLLIERY-T-0269): the caller that handles `impacts` does it
/// before this function.
fn parse_relationship(value: &str) -> Result<RelationshipType, ApiError> {
    value.parse::<RelationshipType>().map_err(|_| {
        let mut allowed: Vec<String> = RelationshipType::ALL
            .iter()
            .map(|relationship| relationship.to_string())
            .collect();
        allowed.push(kairos_db::impacts::RELATIONSHIP.to_string());
        allowed.push(kairos_db::external_refs::RELATIONSHIP.to_string());
        ApiError::validation(format!(
            "The value {value:?} is not a value of relationship. The values are: {}.",
            allowed.join(", ")
        ))
    })
}

/// One line of a document or an ADR in a section of `get_repository`
/// (COLLIERY-T-0269, KAIROS-T-0599): the short code, the title, the kind,
/// the lifecycle or the column, and `[archived]`.
fn impacting_item_line(item: &kairos_client::types_repositories::ImpactingItem) -> String {
    let kind = match (item.entity_type.as_str(), item.document_type.as_deref()) {
        ("document", Some(document_type)) => format!("document ({document_type})"),
        (entity_type, _) => entity_type.to_string(),
    };
    let state = match (item.lifecycle.as_deref(), item.column.as_deref()) {
        (Some(lifecycle), _) => format!(" · lifecycle: {lifecycle}"),
        (None, Some(column)) => format!(" · column: {column}"),
        (None, None) => String::new(),
    };
    format!(
        "- {} — {} · {kind}{state}{}\n",
        item.short_code,
        item.title,
        if item.archived_at.is_some() {
            " [archived]"
        } else {
            ""
        }
    )
}

/// The line `- impacts: ...` of a document or of an ADR for `get_item`
/// (COLLIERY-T-0269), or an empty string. A link to an archived
/// repository is in the line, marked.
/// The teams of an initiative or a strategy (KAIROS-T-0321): one line,
/// also when the item has no team, so that an agent does not guess.
fn teams_line(conn: &mut PgConnection, item: &ItemView) -> Result<String, ApiError> {
    if !kairos_db::item_teams::is_subject(item.item_type) {
        return Ok(String::new());
    }
    let teams = kairos_db::item_teams::teams_of(conn, item.id).map_err(ApiError::internal)?;
    if teams.is_empty() {
        return Ok("- teams: none (no task on a team board, and no team set by hand)\n".into());
    }
    let entries: Vec<String> = teams
        .into_iter()
        .map(|team| {
            let how = match (team.from_tasks, team.set_by_hand) {
                (true, true) => "from tasks, set by hand",
                (true, false) => "from tasks",
                _ => "set by hand",
            };
            format!("{} ({how})", team.slug)
        })
        .collect();
    Ok(format!("- teams: {}\n", entries.join("; ")))
}

fn impacts_line(conn: &mut PgConnection, item: &ItemView) -> Result<String, ApiError> {
    if !kairos_db::impacts::is_subject(item.item_type) {
        return Ok(String::new());
    }
    let links = kairos_db::impacts::repositories_of(conn, item.id).map_err(ApiError::internal)?;
    if links.is_empty() {
        return Ok(String::new());
    }
    let entries: Vec<String> = links
        .into_iter()
        .map(|link| {
            let archived = if link.archived_at.is_some() {
                " [archived]"
            } else {
                ""
            };
            format!(
                "repository {} ({} {}){archived}",
                link.slug, link.forge, link.repo_full_name
            )
        })
        .collect();
    Ok(format!("- impacts: {}\n", entries.join("; ")))
}

/// One external reference as the tools print it (KAIROS-T-0508):
/// `label <uri>`, or `<uri>` when the reference has no label.
fn external_ref_text(reference: &kairos_db::external_refs::ExternalRef) -> String {
    external_text(&reference.uri, reference.label.as_deref())
}

/// [`external_ref_text`] from the URI and the label.
fn external_text(uri: &str, label: Option<&str>) -> String {
    match label.filter(|label| !label.is_empty()) {
        Some(label) => format!("{label} <{uri}>"),
        None => format!("<{uri}>"),
    }
}

/// The external references of the item of `related_work` and of each
/// proposal (KAIROS-T-0508), as `(short_code, references)`, for the items
/// that have one. The item asked about comes first.
fn external_refs_of_related(
    conn: &mut PgConnection,
    found: &crate::embedding::RelatedWork,
) -> Result<Vec<(String, Vec<kairos_db::external_refs::ExternalRef>)>, ApiError> {
    let codes = std::iter::once(found.short_code.as_str())
        .chain(found.proposals.iter().map(|p| p.short_code.as_str()));
    let mut ids: Vec<(String, Uuid)> = Vec::new();
    for code in codes {
        if let Some((id, _)) = resolve_short_code(conn, code, Liveness::IncludeArchived)? {
            ids.push((code.to_string(), id));
        }
    }
    let only_ids: Vec<Uuid> = ids.iter().map(|(_, id)| *id).collect();
    let mut by_item =
        kairos_db::external_refs::refs_of_items(conn, &only_ids).map_err(ApiError::internal)?;
    Ok(ids
        .into_iter()
        .filter_map(|(code, id)| by_item.remove(&id).map(|refs| (code, refs)))
        .collect())
}

/// The move of a DOCUMENT to a different owner board (COLLIERY-T-0269):
/// `move_item` for a document. The rule and the write are those of
/// `PATCH /api/documents/{short_code}/board`
/// ([`crate::api::documents::change_board`]).
fn move_document(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    item: &ItemView,
    to_board: &str,
    rename: bool,
) -> Result<String, ApiError> {
    let change = crate::api::documents::change_board(conn, slug, user, item.id, to_board, rename)?;
    if !change.changed {
        return Ok(format!(
            "No change to {}: its owner board is {} already.",
            item.short_code, change.to.slug
        ));
    }
    let mut out = format!(
        "Moved {}: owner board {} -> {}.",
        item.short_code, change.from.slug, change.to.slug
    );
    if let Some(rename) = &change.rename {
        out.push_str(&rename_lines(rename));
    }
    Ok(out)
}

/// The renames of an item for `get_history` (COLLIERY-T-3101): the
/// `rename` rows of the activity log, oldest first, with the old code, the
/// new code, the time and who did it. Empty when the item has none.
fn rename_history(conn: &mut PgConnection, item_id: Uuid) -> Result<String, ApiError> {
    use kairos_db::schema::{activity_log, users};
    let rows: Vec<(String, DateTime<Utc>, Uuid, Option<Uuid>)> = activity_log::table
        .filter(activity_log::entity_id.eq(item_id))
        .filter(activity_log::action.eq(kairos_db::models::enums::ActivityAction::Rename))
        .order(activity_log::occurred_at.asc())
        .select((
            activity_log::details,
            activity_log::occurred_at,
            activity_log::actor_id,
            activity_log::agent_key_id,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    if rows.is_empty() {
        return Ok(String::new());
    }
    let actor_ids: Vec<Uuid> = rows.iter().map(|(_, _, actor, _)| *actor).collect();
    let names: HashMap<Uuid, String> = users::table
        .filter(users::id.eq_any(actor_ids))
        .select((users::id, users::display_name))
        .load::<(Uuid, String)>(conn)
        .map_err(ApiError::internal)?
        .into_iter()
        .collect();
    let mut out = "\n## Renames\n".to_string();
    for (details, at, actor, agent_key) in rows {
        let codes: &str = details.strip_prefix("code:").unwrap_or(&details);
        let codes = codes.replace("->", " -> ");
        out.push_str(&format!(
            "- {codes} — {} by {}\n",
            at.format("%Y-%m-%dT%H:%M:%SZ"),
            actor_text(
                names.get(&actor).map_or("unknown", String::as_str),
                agent_key
            )
        ));
    }
    Ok(out)
}

/// The activity of an item for `get_history` (KAIROS-T-0557): at most
/// `limit` rows, newest first, each with what changed, the time and who
/// did it ([`kairos_db::item_activity`]). A last line says how many older
/// rows the list does not show.
fn activity_section(
    conn: &mut PgConnection,
    item_id: Uuid,
    short_code: &str,
    limit: i64,
) -> Result<String, ApiError> {
    let page = kairos_db::item_activity::of_item(conn, item_id, short_code, limit, 0)
        .map_err(ApiError::internal)?;
    let mut out = "\n## Activity\n".to_string();
    if page.rows.is_empty() {
        out.push_str("No activity is recorded for this item.\n");
        return Ok(out);
    }
    for row in &page.rows {
        out.push_str(&format!(
            "- {} — {} by {}\n",
            row.text,
            row.occurred_at.format("%Y-%m-%dT%H:%M:%SZ"),
            actor_text(
                row.actor_name.as_deref().unwrap_or("unknown"),
                row.agent_key_id
            )
        ));
    }
    let shown = i64::try_from(page.rows.len()).unwrap_or(i64::MAX);
    let older = page.total.saturating_sub(shown);
    match older {
        ..=0 => {}
        1 => out.push_str("- 1 older row is not shown. Send a larger `limit` to see it.\n"),
        _ => out.push_str(&format!(
            "- {older} older rows are not shown. Send a larger `limit` to see them.\n"
        )),
    }
    Ok(out)
}

/// The live TASK of `claim_item`, `hand_off_item` and `release_item`, or a
/// refusal for an item that is not a task (KAIROS-T-0359). A strategy and an initiative
/// have an owner instead (KAIROS-T-0411), and the tools take that path
/// before.
fn task_for_claim(item: ItemView) -> Result<ItemView, ApiError> {
    if item.item_type != ItemType::Task {
        return Err(ApiError::validation(format!(
            "The {} {} is not a task, a strategy or an initiative. Only a task has a claim, and \
             only a strategy or an initiative has an owner.",
            item.item_type, item.short_code
        ))
        .with_details(json!({ "field": "short_code" })));
    }
    Ok(item)
}

/// The name of an actor in a history or activity text: "Alice (agent)"
/// when an agent made the change with the agent key of the person
/// (KAIROS-T-0359), else the name.
/// The `## Agent key scope` section of `whoami` (KAIROS-T-0536).
fn key_scope_section(
    conn: &mut PgConnection,
    scope: Option<&kairos_core::key_scope::KeyScope>,
) -> Result<String, ApiError> {
    let mut out = String::from("\n## Agent key scope\n");
    let Some(scope) = scope else {
        out.push_str(
            "This request uses an agent key with no scope: the key can do each thing that \
             you can do.\n",
        );
        return Ok(out);
    };
    if scope.capabilities.is_empty() {
        out.push_str("- capabilities: none. The key is read-only.\n");
    } else {
        let names: Vec<&str> = scope.capabilities.iter().map(String::as_str).collect();
        out.push_str(&format!("- capabilities: {}\n", names.join(", ")));
    }
    match &scope.boards {
        None => out.push_str("- boards: each board\n"),
        Some(ids) => {
            use kairos_db::schema::boards;
            let ids: Vec<Uuid> = ids.iter().copied().collect();
            let slugs: Vec<(Uuid, String)> = boards::table
                .filter(boards::id.eq_any(&ids))
                .select((boards::id, boards::slug))
                .load(conn)
                .map_err(ApiError::internal)?;
            let names: Vec<String> = ids
                .iter()
                .map(|id| {
                    slugs
                        .iter()
                        .find(|(b, _)| b == id)
                        .map(|(_, slug)| slug.clone())
                        .unwrap_or_else(|| id.to_string())
                })
                .collect();
            out.push_str(&format!("- boards: {}\n", names.join(", ")));
        }
    }
    out.push_str(
        "The key can do an action only when you can do it and the scope has it. A refusal \
         outside the scope is KEY_SCOPE.\n",
    );
    Ok(out)
}

fn actor_text(name: &str, agent_key: Option<Uuid>) -> String {
    match agent_key {
        Some(_) => format!("{name} (agent)"),
        None => name.to_string(),
    }
}

/// The lines of a `move_item` answer for a rename (COLLIERY-T-3101).
fn rename_lines(rename: &kairos_db::code_rename::CodeRename) -> String {
    let mut out = format!(
        "\nRenamed {} -> {}. The code {} is retired: a read with it finds the item.",
        rename.old_code, rename.new_code, rename.old_code
    );
    if rename.references.is_empty() {
        out.push_str(" No text of an item named the old code.");
    } else {
        out.push_str(&format!(
            " The references changed in {} item(s): {}.",
            rename.references.len(),
            rename.references.join(", ")
        ));
    }
    out
}

/// A live item by short code WITH its type (the edge-permission check needs
/// it); 422 `VALIDATION` naming the field otherwise, mirroring REST. A
/// retired code follows to its item (KAIROS-T-0560): see
/// [`crate::api::resolve_live_ref`].
fn require_live_typed(
    conn: &mut PgConnection,
    short_code: &str,
    field: &str,
) -> Result<crate::api::LiveRef, ApiError> {
    crate::api::resolve_live_ref(conn, short_code, field)
}

/// One line for each reference that came in with a retired code, e.g.
/// `\nACME-T-0100 is now ACME-T-0200.` (KAIROS-T-0560). Empty otherwise.
fn renamed_lines(refs: &[&crate::api::LiveRef]) -> String {
    refs.iter()
        .filter_map(|r| r.renamed())
        .map(|line| format!("\n{line}"))
        .collect()
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
///
/// One statement (KAIROS-T-0626): a recursive CTE that takes, at each
/// step, the incoming `parent` edge whose source is in `entity_directory`,
/// to a depth of 10. An item has one parent since KAIROS-T-0572 (a unique
/// index), so each step has one edge; `ORDER BY ... LIMIT 1` stays as
/// defense-in-depth.
fn parent_chain(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<ChainRow>, ApiError> {
    sql_query(
        "WITH RECURSIVE chain (depth, id, short_code, title, deleted_at) AS ( \
             SELECT 0, $1::uuid, NULL::text, NULL::text, NULL::timestamptz \
           UNION ALL \
             SELECT chain.depth + 1, p.id, p.short_code, p.title, p.deleted_at \
               FROM chain \
               CROSS JOIN LATERAL ( \
                   SELECT d.id, d.short_code, d.title, d.deleted_at \
                     FROM item_relationships r \
                     JOIN entity_directory d ON d.id = r.source_id \
                    WHERE r.target_id = chain.id AND r.relationship = 'parent' \
                    ORDER BY r.created_at ASC LIMIT 1) p \
              WHERE chain.depth < 10) \
         SELECT short_code, title, deleted_at FROM chain \
          WHERE depth > 0 ORDER BY depth",
    )
    .bind::<SqlUuid, _>(item_id)
    .load(conn)
    .map_err(ApiError::internal)
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
///
/// A `blocks` neighbour in a terminal column carries a `[done]` tag
/// (COLLIERY-T-0214). Done work does not block and is not blocked, so that
/// edge is resolved: the entry stays, as history, and the tag is what
/// stops an agent from waiting on work that is finished. The tag is put on
/// `blocks` edges only, because that is where it changes a decision; the
/// word is the one the board uses for the column ("done").
///
/// `item_done` says that the item ITSELF sits in a terminal column. Then
/// every one of its `blocks` edges is resolved, whatever the other end is
/// doing, and the two labels say so. The `[done]` tag cannot carry that:
/// it is a statement about the neighbour, and an open blocker of a
/// completed item is not done.
fn relationship_lines(
    conn: &mut PgConnection,
    item_id: Uuid,
    item_done: bool,
) -> Result<String, ApiError> {
    let relationships = graph::relationships_for(conn, item_id).map_err(ApiError::internal)?;

    let mut groups: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut push =
        |label: &'static str, entry: String| groups.entry(label).or_default().push(entry);

    /// One neighbour line, tagged when the neighbour is archived, and,
    /// on a `blocks` edge, when the neighbour is done or cancelled.
    /// KAIROS-T-0567: a cancelled blocker is unresolved, so it is
    /// `[cancelled]`, never `[done]`.
    fn line(neighbor: &kairos_db::graph::Neighbor) -> String {
        let done = if neighbor.relationship != RelationshipType::Blocks {
            ""
        } else if neighbor.cancelled {
            " [cancelled]"
        } else if neighbor.done {
            " [done]"
        } else {
            ""
        };
        let archived = if neighbor.archived_at.is_some() {
            " [archived]"
        } else {
            ""
        };
        format!(
            "{} — {}{done}{archived}",
            neighbor.short_code, neighbor.title
        )
    }
    let (blocks_label, blocked_by_label) = if item_done {
        (
            "blocks (resolved: this item is done)",
            "blocked by (resolved: this item is done)",
        )
    } else {
        ("blocks", "blocked by")
    };

    for neighbor in &relationships.outgoing {
        let entry = line(neighbor);
        match neighbor.relationship {
            RelationshipType::Parent => push("children", entry),
            RelationshipType::Supports => push("supporting docs", entry),
            RelationshipType::Informs => push("informs", entry),
            RelationshipType::Supersedes => push("supersedes", entry),
            RelationshipType::Blocks => push(blocks_label, entry),
        }
    }
    for neighbor in &relationships.incoming {
        let entry = line(neighbor);
        match neighbor.relationship {
            RelationshipType::Parent => {} // rendered as the parent chain below
            RelationshipType::Supports => push("supports", entry),
            RelationshipType::Informs => push("informed by", entry),
            RelationshipType::Supersedes => push("superseded by", entry),
            RelationshipType::Blocks => push(blocked_by_label, entry),
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
/// everything else takes the standard REST mapping.
fn map_update_error(short_code: &str, e: items::ItemError) -> ApiError {
    match e {
        items::ItemError::VersionConflict {
            expected_version,
            current_version,
            current_title,
            current_content,
            ..
        } => ApiError::conflict(format!(
            "The call has the version {expected_version} of {short_code}, and the current \
             version is {current_version}. Make the edit on the current version."
        ))
        .with_details(json!({
            "current": {
                "version": current_version,
                "title": current_title,
                "content": current_content,
            }
        })),
        other => map_item_error(other),
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
        // KAIROS-T-0572: an item has one parent.
        e @ GraphError::ParentExists { .. } => {
            crate::api::meta::relationships::parent_exists(e, "set_parent")
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

/// The capabilities that membership of the owning team implies on a board
/// of `level` (KAIROS-T-0072; the ADR board of a team adds `manage_adrs`,
/// COLLIERY-T-3108; a strategy or initiative board adds its `manage_<type>`,
/// KAIROS-T-0440). The vocabulary is `kairos_core::abac`.
/// The policies of a board for `my_boards` and `set_board_policy`
/// (KAIROS-T-0641), or `None` when its level has none.
fn policy_text(board: &Board) -> Option<String> {
    match board.board_level {
        BoardLevel::Delivery => Some(format!(
            "parent_gate {} (a task moves to a claims column only while its initiative is \
             active); blockers_gate {} (on: a task with an open blocker cannot move to a claims \
             or done column; off: the move gets a warning)",
            if board.parent_gate { "on" } else { "off" },
            if board.blockers_gate { "on" } else { "off" }
        )),
        BoardLevel::Initiative => Some(format!(
            "owner_gate {} (an initiative moves to Active only with an owner)",
            if board.owner_gate { "on" } else { "off" }
        )),
        _ => None,
    }
}

/// One line of `list_members` (KAIROS-T-0556): a member and, as the scope
/// gives it, the organization role or the capabilities on a board.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MemberLine {
    user_name: String,
    display_name: String,
    email: String,
    kind: String,
    detail: Option<String>,
}

impl MemberLine {
    fn of(user: &kairos_db::models::User, detail: Option<String>) -> Self {
        Self {
            user_name: user.user_name.clone(),
            display_name: user.display_name.clone(),
            email: user.email.clone(),
            kind: user.kind.clone(),
            detail,
        }
    }
}

/// The word of `list_members` for a `users.kind`.
fn member_kind(kind: &str) -> &'static str {
    use kairos_db::models::public::{USER_KIND_SERVICE_ACCOUNT, USER_KIND_SYSTEM};
    match kind {
        USER_KIND_SERVICE_ACCOUNT => "service account",
        USER_KIND_SYSTEM => "system actor",
        _ => "person",
    }
}

/// The `users` rows of `ids`, by id.
fn users_by_id(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, kairos_db::models::User>, ApiError> {
    use kairos_db::schema::users;
    let rows: Vec<kairos_db::models::User> = users::table
        .filter(users::id.eq_any(ids))
        .select(kairos_db::models::User::as_select())
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(rows.into_iter().map(|user| (user.id, user)).collect())
}

/// The members of the organization, with their roles.
fn org_member_lines(conn: &mut PgConnection, org_id: Uuid) -> Result<Vec<MemberLine>, ApiError> {
    use kairos_db::schema::{organization_members, users};
    let rows: Vec<(kairos_db::models::User, OrgRole)> = organization_members::table
        .inner_join(users::table)
        .filter(organization_members::organization_id.eq(org_id))
        .select((
            kairos_db::models::User::as_select(),
            organization_members::role,
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(rows
        .iter()
        .map(|(user, role)| MemberLine::of(user, Some(format!("role: {role}"))))
        .collect())
}

/// The members of a team. `team_members` is in the tenant schema and
/// `users` in `public`, so these are two queries.
fn team_member_lines(conn: &mut PgConnection, team_id: Uuid) -> Result<Vec<MemberLine>, ApiError> {
    use kairos_db::schema::team_members;
    let ids: Vec<Uuid> = team_members::table
        .filter(team_members::team_id.eq(team_id))
        .select(team_members::user_id)
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(users_by_id(conn, &ids)?
        .values()
        .map(|user| MemberLine::of(user, None))
        .collect())
}

/// The members of a board and their capabilities: the grants, the
/// capabilities that board membership gives (KAIROS-T-0410), and those
/// that the team of the board gives (KAIROS-T-0072). `whoami` says them
/// the same way.
fn board_member_lines(conn: &mut PgConnection, board: &Board) -> Result<Vec<MemberLine>, ApiError> {
    use kairos_db::schema::{board_member_capabilities as caps, team_members, teams};
    let grants: Vec<(Uuid, String)> = caps::table
        .filter(caps::board_id.eq(board.id))
        .order(caps::capability.asc())
        .select((caps::user_id, caps::capability))
        .load(conn)
        .map_err(ApiError::internal)?;
    let mut granted: BTreeMap<Uuid, Vec<String>> = BTreeMap::new();
    for (user_id, capability) in grants {
        granted.entry(user_id).or_default().push(capability);
    }
    let (team_slug, in_team): (Option<String>, Vec<Uuid>) = match board.team_id {
        Some(team_id) => (
            teams::table
                .filter(teams::id.eq(team_id))
                .filter(teams::deleted_at.is_null())
                .select(teams::slug)
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?,
            team_members::table
                .filter(team_members::team_id.eq(team_id))
                .select(team_members::user_id)
                .load(conn)
                .map_err(ApiError::internal)?,
        ),
        None => (None, Vec::new()),
    };
    // A deleted team gives no capability.
    let in_team: Vec<Uuid> = if team_slug.is_some() {
        in_team
    } else {
        Vec::new()
    };
    let mut ids: Vec<Uuid> = granted.keys().copied().collect();
    ids.extend(in_team.iter().filter(|id| !granted.contains_key(*id)));
    let users = users_by_id(conn, &ids)?;
    let team = team_capabilities(board.board_level);
    let mut lines = Vec::new();
    for id in ids {
        let Some(user) = users.get(&id) else { continue };
        let mut parts = Vec::new();
        if let Some(capabilities) = granted.get(&id) {
            parts.push(format!("{} (grant)", capabilities.join(", ")));
            // KAIROS-T-0541: only the `member` grant (or `*`) of this person
            // makes a member of a strategy or initiative board.
            let member = member_capabilities(board.board_level, capabilities);
            if !member.is_empty() {
                parts.push(format!("{} (board member, no grant)", member.join(", ")));
            }
        }
        if let Some(slug) = team_slug.as_deref()
            && in_team.contains(&id)
        {
            parts.push(format!("{} (team {slug}, no grant)", team.join(", ")));
        }
        lines.push(MemberLine::of(
            user,
            Some(format!("capabilities: {}", parts.join("; "))),
        ));
    }
    Ok(lines)
}

/// The text of `list_members`: the members in the order of their user
/// names, one page of them. Pure.
fn render_members(heading: &str, mut members: Vec<MemberLine>, limit: i64, offset: i64) -> String {
    members.sort_by(|a, b| a.user_name.cmp(&b.user_name));
    let total = members.len() as i64;
    let page: Vec<&MemberLine> = members
        .iter()
        .skip(offset.max(0) as usize)
        .take(limit.max(0) as usize)
        .collect();
    let shown = page.len() as i64;
    let mut out = heading.to_string();
    if offset > 0 || offset + shown < total {
        out.push_str(&format!(
            "Members: {total}. Shown: {shown} (limit {limit}, offset {offset})."
        ));
        if offset + shown < total {
            out.push_str(&format!(
                " Call again with offset {} for the next part.",
                offset + shown
            ));
        }
        out.push('\n');
    }
    if total == 0 {
        out.push_str("(none)\n");
    }
    for member in page {
        out.push_str(&format!(
            "- {} — {} <{}> · {}",
            member.user_name,
            member.display_name,
            member.email,
            member_kind(&member.kind)
        ));
        if let Some(detail) = &member.detail {
            out.push_str(&format!(" · {detail}"));
        }
        out.push('\n');
    }
    out
}

fn team_capabilities(level: BoardLevel) -> &'static [&'static str] {
    kairos_core::abac::team_capabilities(level.as_str())
}

/// The capabilities that membership of a board of `level` gives
/// (KAIROS-T-0410): on a strategy or initiative board, a member edits and
/// moves each item. Since KAIROS-T-0541 only the `member` grant (or `*`)
/// in `grants` makes a member. The vocabulary is `kairos_core::abac`.
fn member_capabilities<S: AsRef<str>>(level: BoardLevel, grants: &[S]) -> &'static [&'static str] {
    kairos_core::abac::member_capabilities_of_grants(level.as_str(), grants)
}

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
            "The organization has no live {level} board. Make the board, or send `board`."
        ))),
        _ => {
            let slugs: Vec<&str> = candidates.iter().map(|b| b.slug.as_str()).collect();
            Err(ApiError::validation(format!(
                "The organization has more than one {level} board. Send `board` as one of: {}.",
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
            "No template has the id, the slug or the name {reference:?}."
        ))),
        _ => Err(ApiError::validation(format!(
            "More than one template has the name {reference:?}. Send the id or the slug \
             of the template."
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
            "{field} applies to {applies_to}. It does not apply to an item of the type \
             {item_type}."
        ))),
        None => Ok(()),
    }
}

/// The create_item body: resolve the target board (or parent, for
/// documents), enforce `manage_<type>` (A-0006), create through the T-0012
/// service, then write the `parent`/`supports` edge when `parent` is given.
///
/// COLLIERY-T-0227: the caller runs this in ONE transaction
/// ([`atomically`]). The item and the edge are two writes, and an error from
/// the second must remove the first. Each check that can refuse the request
/// still runs before the first write, because a refusal is a better answer
/// than a rollback; the transaction is for the failures that no check can
/// know (the parent is archived between the check and the insert, the
/// database refuses the edge).
fn create_item_impl(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    params: &CreateItemParams,
) -> Result<String, ApiError> {
    let slug = tenant.slug.as_str();
    let role = tenant.role;
    let item_type = match params.item_type.as_str() {
        "strategy" => ItemType::Strategy,
        "initiative" => ItemType::Initiative,
        "task" => ItemType::Task,
        "document" => ItemType::Document,
        "adr" => ItemType::Adr,
        other => {
            return Err(ApiError::validation(format!(
                "{other:?} is not an item type. The item types are: strategy, initiative, \
                 task, document, adr."
            )));
        }
    };
    if item_type != ItemType::Document {
        reject_field("template", params.template.as_ref(), item_type, "documents")?;
    }
    if item_type != ItemType::Task {
        reject_field("task_type", params.task_type.as_ref(), item_type, "tasks")?;
        // KAIROS-T-0553: refused like the others, not read and dropped.
        reject_field("work_class", params.work_class.as_ref(), item_type, "tasks")?;
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
    if item_type != ItemType::Initiative {
        reject_field(
            "bucket_type",
            params.bucket_type.as_ref(),
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
        reject_field(
            "decision_date",
            params.decision_date.as_ref(),
            item_type,
            "ADRs",
        )?;
    }
    let content = params.content.as_deref().unwrap_or("");

    // Documents: an owner is REQUIRED, which is `board`, or `parent`, or
    // the two (COLLIERY-T-0269). The rule, the gate and the refusals are
    // those of POST /api/documents: the two call one function.
    if item_type == ItemType::Document {
        // Until COLLIERY-T-0269 the tool did not read `repository` for a
        // document, and it did not say so.
        if params.repository.is_some() {
            return Err(ApiError::validation(
                "The argument repository applies to tasks. It does not apply to an item \
                 of the type document. To say that the document impacts a repository, \
                 create the document. Then call link_items with the relationship \
                 impacts.",
            ));
        }
        let template_id = params
            .template
            .as_deref()
            .map(|t| resolve_template(conn, t))
            .transpose()?;
        let created = crate::api::documents::create(
            conn,
            slug,
            role,
            user,
            crate::api::documents::NewDocument {
                title: &params.title,
                content: params.content.as_deref(),
                template_id,
                board: params.board.as_deref(),
                parent: params.parent.as_deref(),
            },
            crate::api::documents::CreateWording {
                request: "call",
                details_key: "field",
                board: "`board`",
                parent: "`parent`",
            },
        )?;
        let mut out = format!(
            "Created document {}: {} (version 1)",
            created.document.short_code, created.document.title
        );
        out.push_str(&format!(", owner board {}", created.board.slug));
        if let Some(parent_code) = &created.parent {
            out.push_str(&format!(", supports {parent_code}"));
        }
        out.push('.');
        if let Some(renamed) = &created.parent_renamed {
            out.push_str(&format!("\n{renamed}"));
        }
        return Ok(out);
    }

    if item_type != ItemType::Task {
        reject_field("repository", params.repository.as_ref(), item_type, "tasks")?;
    }

    // Board items: resolve the board (explicit slug/UUID or the single
    // board of the matching level), then manage_<type> on it. Tasks go
    // through the SAME routing + capability helpers as POST /api/tasks
    // (KAIROS-T-0104/T-0105), and a caller who does not manage the board
    // may still send a request: entry column, support lane, repository or
    // none (COLLIERY-T-0218).
    //
    // COLLIERY-T-0217 (COLLIERY-A-0023): the board is the named one, or the
    // single delivery board of the tenant. The repository is carried along
    // as the link and has no say. Until then a repository with no `board`
    // chose its owner's delivery board, and a `board` beside it had to be
    // that one. MCP has no `team` argument, so the team arm of `route_task`
    // is never reached from here.
    let (board, route, task_access) = if item_type == ItemType::Task {
        let explicit = params
            .board
            .as_deref()
            .map(|reference| board_by_ref(conn, reference))
            .transpose()?;
        let board = match explicit {
            Some(board) => board,
            None => default_board_for(conn, level_of(item_type))?,
        };
        // COLLIERY-T-0216: through the shared routing helper rather than a
        // hand-built route, so the defaulted board gives the task its team
        // exactly as a named board does. Built by hand, the defaulted arm
        // left every such task with no team at all.
        let route = crate::api::tasks::resolve_routing(
            conn,
            Some(board.id),
            None,
            params.repository.as_deref(),
        )?;
        // COLLIERY-T-0218: the work class the caller asked for is parsed
        // here, before the capability check, because the check needs it: a
        // caller who does not manage the board is refused `planned`. Until
        // then it was parsed below, beside the create, where a refusal was
        // already too late to give.
        let task_type = params
            .task_type
            .as_deref()
            .map(|v| parse_enum(v, "task_type", TaskType::ALL))
            .transpose()?
            .unwrap_or(TaskType::Task);
        let requested_work_class = params
            .work_class
            .as_deref()
            .map(|v| parse_enum(v, "work_class", WorkClass::ALL))
            .transpose()?;
        let access = crate::api::tasks::require_task_create_capability(
            conn,
            slug,
            user,
            &route,
            requested_work_class,
            params.from_team.as_deref(),
        )?;
        // The SAME function as POST /api/tasks chooses the stored work
        // class, so the two cannot disagree about what a request is.
        let work_class = access.work_class(requested_work_class, task_type);
        (board, Some(route), Some((task_type, work_class)))
    } else {
        // KAIROS-T-0426: `from_team` says who sends a task.
        if params.from_team.is_some() {
            return Err(ApiError::validation(format!(
                "The argument from_team is for a task. A {item_type} has no sending team."
            ))
            .with_details(json!({ "field": "from_team" })));
        }
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
        (board, None, None)
    };

    // COLLIERY-T-0227: the type rules (`check_link`) run here, BEFORE the
    // first write. The two types are known before the item exists, so an
    // edge that cannot exist is a `VALIDATION`, whoever asks for it, and
    // nothing is written.
    //
    // An ADR takes the `supports` edge, as a document does. No ADR can have
    // a `parent` edge.
    //
    // COLLIERY-T-0228: WHO may write the edge is the link rule, and it is
    // asked below, after the create, when the two ends exist. Until then
    // this place held two stand-ins for it: `manage_documents` on the board
    // of the parent for an ADR, and a copy of the rule for `parent` that
    // took a board in place of the item that was not there yet. The caller
    // is the creator of the new item, so the link rule lets the caller
    // link it. What is left as the gate of the create is the create gate
    // above: `manage_<type>` on the board, or a request.
    let parent = params
        .parent
        .as_deref()
        .map(|parent_code| {
            let parent = require_live_typed(conn, parent_code, "parent")?;
            let parent_type = parent.item_type;
            let relationship = if item_type == ItemType::Adr {
                RelationshipType::Supports
            } else {
                RelationshipType::Parent
            };
            kairos_core::graph::check_link(
                graph::core_relationship(relationship),
                parent_type,
                item_type,
            )
            .map_err(|e| ApiError::validation(e.to_string()))?;
            Ok::<_, ApiError>((parent, relationship))
        })
        .transpose()?;

    // KAIROS-T-0458: the answer names the lane of a task. `from_team` is
    // not kept, so this is the one place the caller learns that the task
    // is a request (entry column, support lane).
    let mut lane: Option<WorkClass> = None;
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
            // Same parse as POST /api/initiatives, so the three surfaces agree
            // on the vocabulary and on the error text (KAIROS-T-0178).
            let bucket_type = params
                .bucket_type
                .as_deref()
                .map(|v| parse_enum(v, "bucket_type", BucketType::ALL))
                .transpose()?;
            let created = items::create_initiative(
                conn,
                items::CreateInitiative {
                    board_id: board.id,
                    column_id: None,
                    title: &params.title,
                    content,
                    complexity,
                    bucket_type,
                },
                user,
            )
            .map_err(map_item_error)?;
            (created.short_code, created.title, created.id)
        }
        ItemType::Task => {
            // Decided above, with the capability check (COLLIERY-T-0218).
            let (task_type, work_class) = task_access.expect("tasks always resolve a work class");
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
                    repository_id: route.repository_id,
                },
                user,
            )
            .map_err(map_item_error)?;
            // KAIROS-T-0566: the WIP warning of the entry column.
            crate::claims::note_wip(conn, created.column_id, &created.short_code)?;
            lane = Some(created.work_class);
            (created.short_code, created.title, created.id)
        }
        ItemType::Adr => {
            // Same parse and same message as POST /api/adrs.
            let decision_date = params
                .decision_date
                .as_deref()
                .map(|value| {
                    value.parse::<chrono::NaiveDate>().map_err(|_| {
                        ApiError::validation(format!(
                            "The value {value:?} of decision_date is not a date. Send the \
                             date as YYYY-MM-DD."
                        ))
                    })
                })
                .transpose()?;
            let created = items::create_adr(
                conn,
                items::CreateAdr {
                    board_id: Some(board.id),
                    column_id: None,
                    title: &params.title,
                    content,
                    decision_maker: params.decision_maker.as_deref(),
                    decision_date,
                },
                user,
            )
            .map_err(map_item_error)?;
            (created.short_code, created.title, created.id)
        }
        ItemType::Document => unreachable!("handled above"),
    };

    let mut out = format!(
        "Created {item_type} {created_code}: {created_title} (version 1) on board {}.",
        board.slug
    );
    if let Some(lane) = lane {
        out.push_str(&format!(" Lane: {lane}."));
    }
    if let Some((parent, relationship)) = parent {
        // The link rule (COLLIERY-T-0228), by the ONE function that
        // `link_items` calls. The item and the edge are in one transaction
        // (COLLIERY-T-0227), so a refusal here leaves no item.
        require_edge_write(
            conn,
            role,
            user,
            relationship.as_str(),
            parent.typed(),
            (created_id, item_type),
        )?;
        graph::link_items(conn, parent.id, created_id, relationship, user)
            .map_err(map_graph_error)?;
        // KAIROS-T-0641: a create into a claims column is a move into it.
        // The entry column can have the flag; the parent edge exists now.
        if item_type == ItemType::Task {
            use kairos_db::schema::tasks;
            let column: Uuid = tasks::table
                .filter(tasks::id.eq(created_id))
                .select(tasks::column_id)
                .first(conn)
                .map_err(ApiError::internal)?;
            kairos_db::board_policy::check_active_parent(conn, created_id, column)
                .map_err(map_board_error)?;
        }
        out.push_str(&format!(
            "\nparent: {} ({relationship} edge created).{}",
            parent.code,
            renamed_lines(&[&parent])
        ));
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
    Uuid::parse_str(value).map_err(|_| {
        field_invalid(
            field,
            format!("The value {value:?} of {field} is not a UUID."),
        )
    })
}

fn timestamp_field(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| {
            field_invalid(
                field,
                format!(
                    "The value {value:?} of {field} is not a timestamp. Send an RFC 3339 \
                     timestamp."
                ),
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
            format!("The value {value:?} is not a value of {field}. The values are: {allowed}."),
        )
    })
}

/// Convert the tool input into the typed `kairos_core::search` request and
/// validate it — exactly the conversions the REST endpoint applies.
/// Render related-work proposals for an agent (KAIROS-T-0191).
///
/// The wording carries rule 6 as much as the documentation does. Every line says
/// "possible" or "prior art"; nothing says "blocks" or "duplicates". An agent
/// told flatly that something blocks it will do wrong work confidently, and at
/// the measured precision — roughly half at the top of the distribution — that
/// is the likelier outcome of a confident verb.
///
/// `external` is the external references of the item and of the proposals
/// (KAIROS-T-0508): the documents outside Kairos that the search cannot
/// read, so an agent does not take "no related work" as "nothing exists".
fn render_related_work(
    found: &crate::embedding::RelatedWork,
    external: &[(String, Vec<kairos_db::external_refs::ExternalRef>)],
) -> String {
    let mut out = format!("# Possibly related to {}\n\n", found.short_code);
    if found.proposals.is_empty() {
        if found.sources.degraded() {
            // KAIROS-T-0562: no item clears the text bar. Say that, not five
            // weak matches.
            out.push_str("None found. No item matches enough of the rare words of the title. ");
        } else {
            out.push_str("The search found no related work. ");
        }
        out.push_str(found.sources.note());
        out.push_str("\n\nThis is one search. It does not prove that no related work exists.\n");
        out.push_str(&render_external_refs(external));
        return out;
    }
    out.push_str(found.sources.note());
    if let Some(caveat) = kairos_core::retrieval::graph_caveat(&found.proposals) {
        out.push(' ');
        out.push_str(caveat);
    }
    out.push_str(
        "\n\nThese are PROPOSALS, not findings. Examine a proposal before you use it.\n\n",
    );
    for p in &found.proposals {
        out.push_str(&format!(
            "- **{}** — {} ({})\n  {}\n",
            p.short_code, p.title, p.claim, p.why
        ));
    }
    out.push_str(&render_external_refs(external));
    out
}

/// The section `## External references` of `related_work`
/// (KAIROS-T-0508), or an empty string when no item has one.
fn render_external_refs(
    external: &[(String, Vec<kairos_db::external_refs::ExternalRef>)],
) -> String {
    if external.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n## External references\n\nDocuments outside Kairos that these items name. \
         The search does not read them.\n\n",
    );
    for (code, refs) in external {
        for reference in refs {
            out.push_str(&format!("- {code}: {}\n", external_ref_text(reference)));
        }
    }
    out
}

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
                updated_after: filter
                    .updated_after
                    .as_deref()
                    .map(|v| timestamp_field(v, "filter.updated_after"))
                    .transpose()?,
                updated_before: filter
                    .updated_before
                    .as_deref()
                    .map(|v| timestamp_field(v, "filter.updated_before"))
                    .transpose()?,
                metadata_missing: filter.metadata_missing.clone(),
                // Resolved inside the tool, with a conn (KAIROS-T-0488).
                claimed_by: None,
                owned_by: None,
                created_by: None,
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
        SearchError::UnknownMetadataDefinition(_) => ApiError::validation(e.to_string()),
        SearchError::TraverseRootNotFound { reference } => ApiError::not_found(format!(
            "The item {reference:?} of traverse.from does not exist."
        )),
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
fn render_search_results(
    results: &SearchResults,
    facts: &HashMap<Uuid, search::TaskLineFacts>,
) -> String {
    let shown = results.strategies.len()
        + results.initiatives.len()
        + results.tasks.len()
        + results.documents.len()
        + results.adrs.len();
    let mut out = format!(
        "{} found, {} shown (limit {}, offset {}).{}\n",
        results.total,
        shown,
        results.limit,
        results.offset,
        more_text(results.total, results.offset, shown as i64)
            .map(|text| format!(" {text}"))
            .unwrap_or_default()
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
                task_line_tail(facts.get(&row.id)),
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

/// The part of a search line of a task after its type (KAIROS-T-0561):
/// ` kairos/Active, claimed by Robin, blocked by 1 [repo:kairos]`. Each
/// part is there when it applies. Pure.
fn task_line_tail(facts: Option<&search::TaskLineFacts>) -> String {
    let Some(facts) = facts else {
        return String::new();
    };
    let mut out = String::new();
    if let Some(board) = &facts.board_slug {
        out.push_str(&format!(
            " {board}/{}",
            facts.column_name.as_deref().unwrap_or("?")
        ));
    }
    if let Some(holder) = &facts.claim_holder {
        let agent = if facts.claim_agent { " (agent)" } else { "" };
        out.push_str(&format!(", claimed by {holder}{agent}"));
    }
    if facts.blocked_by > 0 {
        out.push_str(&format!(", blocked by {}", facts.blocked_by));
    }
    if let Some(slug) = &facts.repository_slug {
        out.push_str(&format!(" [repo:{slug}]"));
    }
    out
}

/// `N more; offset M.` when a list of `total` rows, cut at `offset` with
/// `shown` rows, has more rows after it (KAIROS-T-0561). Each list that
/// cuts says it in these words. Pure.
pub(crate) fn more_text(total: i64, offset: i64, shown: i64) -> Option<String> {
    let next = offset + shown;
    (shown > 0 && next < total).then(|| format!("{} more; offset {next}.", total - next))
}

/// The rows of "In flight" and of "Not linked to an item" that
/// `get_repository` shows at most (KAIROS-T-0561).
const REPOSITORY_LIST_LIMIT: i64 = 50;

/// The text of an empty page of a list: `empty` when the list has no rows,
/// else that the offset is past the end. Pure.
fn empty_page_text(empty: &str, total: i64, offset: i64) -> String {
    if total > 0 && offset > 0 {
        format!("(none at offset {offset}: the list has {total})\n")
    } else {
        format!("{empty}\n")
    }
}

/// The last line of a list that cuts: `- N more; offset M.` (see
/// [`more_text`]), or empty. Pure.
pub(crate) fn more_line(total: i64, offset: i64, shown: i64) -> String {
    more_text(total, offset, shown)
        .map(|text| format!("- {text}\n"))
        .unwrap_or_default()
}

/// One line of a notification (KAIROS-T-0382): the id, the time, the kind,
/// the item and the text; `(read)` for a read one.
pub(super) fn notification_line(n: &kairos_db::notifications::Notification) -> String {
    format!(
        "- {} · {} · {} · {}: {}{}\n",
        n.id,
        n.created_at.format("%Y-%m-%d %H:%MZ"),
        n.kind,
        n.short_code.as_deref().unwrap_or("(item gone)"),
        n.text,
        if n.read_at.is_some() { " (read)" } else { "" }
    )
}

/// One notification in the ride-along block (KAIROS-T-0549): the time, the
/// kind, the item and the text, with no id. The ids are 36 characters each
/// and the block repeats on every answer until the notifications are read;
/// `get_events` gives the ids.
fn ride_along_line(n: &kairos_db::notifications::Notification) -> String {
    format!(
        "- {} · {} · {}: {}\n",
        n.created_at.format("%Y-%m-%d %H:%MZ"),
        n.kind,
        n.short_code.as_deref().unwrap_or("(item gone)"),
        n.text,
    )
}

/// The text of `get_events`: the unread lines, then the read ones, then
/// what to send next. The newest unread id is the `up_to` of
/// `mark_events_read`; the oldest unread one is the `before` of the next
/// page when there are more. `page` is true for a page after the first.
fn events_text(
    items: &[kairos_db::notifications::Notification],
    unread: i64,
    page: bool,
) -> String {
    let unread_items: Vec<_> = items.iter().filter(|n| n.read_at.is_none()).collect();
    if items.is_empty() {
        return if page {
            format!("# Your notifications ({unread} unread)\nNo older unread ones.\n")
        } else {
            "# Your notifications\n(none)\n".to_string()
        };
    }
    let mut out = format!("# Your notifications ({unread} unread)\n");
    let mut read_heading = false;
    for n in items {
        if n.read_at.is_some() && !read_heading {
            out.push_str("\n## Read\n");
            read_heading = true;
        }
        out.push_str(&notification_line(n));
    }
    if unread_items.len() as i64 == crate::api::notifications::LIST_UNREAD
        && let Some(oldest) = unread_items.last()
    {
        out.push_str(&format!(
            "\nMore unread ones: call get_events with `before`: \"{}\".\n",
            oldest.id
        ));
    }
    if !page && let Some(newest) = unread_items.first() {
        out.push_str(&format!(
            "\nMark them read with mark_events_read: `ids`, or `all: true` with `up_to`: \"{}\".\n",
            newest.id
        ));
    }
    out
}

/// How many unread notifications come at the end of an answer.
pub(super) const RIDE_ALONG: i64 = 5;

/// The block at the end of an answer to a person with unread notifications
/// (KAIROS-T-0382): the [`RIDE_ALONG`] newest, then "More" when there are
/// more. `None` when the person has none (a service account never has
/// one). One query, with no count (KAIROS-T-0612): the heading says
/// `5+ unread` when there are more than [`RIDE_ALONG`].
pub(super) fn ride_along(
    conn: &mut PgConnection,
    user: Uuid,
    agent_key: Option<Uuid>,
) -> Result<Option<String>, ApiError> {
    let reader = crate::api::notifications::reader_for(conn, user, agent_key)?;
    let unread =
        kairos_db::notifications::unread(conn, &reader, RIDE_ALONG).map_err(ApiError::internal)?;
    Ok(ride_along_text(&unread))
}

/// The text of [`ride_along`].
fn ride_along_text(unread: &kairos_db::notifications::Unread) -> Option<String> {
    if unread.newest.is_empty() {
        return None;
    }
    let count = if unread.more {
        format!("{}+", unread.newest.len())
    } else {
        unread.newest.len().to_string()
    };
    let mut out = format!("## Your notifications ({count} unread)\n");
    for n in &unread.newest {
        out.push_str(&ride_along_line(n));
    }
    if unread.more {
        out.push_str("More: call get_events\n");
    }
    out.push_str(
        "get_events gives each with its id; mark them read with mark_events_read when you \
         have acted on them.\n",
    );
    Some(out)
}

#[cfg(test)]
mod ride_along_tests {
    //! KAIROS-T-0612: the heading of the ride-along has no count past
    //! [`RIDE_ALONG`].

    use super::*;
    use kairos_db::notifications::{Notification, Unread};

    fn notification(n: u32) -> Notification {
        Notification {
            id: Uuid::new_v4(),
            kind: "mention".into(),
            item_id: None,
            short_code: Some(format!("ACME-T-{n:04}")),
            entity_type: None,
            title: None,
            text: format!("Robin mentioned you ({n})"),
            actor_id: None,
            created_at: chrono::Utc::now(),
            read_at: None,
        }
    }

    #[test]
    fn none_without_unread() {
        assert_eq!(ride_along_text(&Unread::default()), None);
    }

    #[test]
    fn the_count_up_to_five() {
        let unread = Unread {
            newest: (1..=3).map(notification).collect(),
            more: false,
        };
        let text = ride_along_text(&unread).expect("a block");
        assert!(
            text.starts_with("## Your notifications (3 unread)\n"),
            "{text}"
        );
        assert_eq!(text.matches("\n- ").count(), 3, "{text}");
        assert!(!text.contains("More:"), "{text}");
    }

    #[test]
    fn five_plus_past_five() {
        let unread = Unread {
            newest: (1..=5).map(notification).collect(),
            more: true,
        };
        let text = ride_along_text(&unread).expect("a block");
        assert!(
            text.starts_with("## Your notifications (5+ unread)\n"),
            "{text}"
        );
        assert!(text.contains("More: call get_events\n"), "{text}");
    }
}

#[cfg(test)]
mod create_item_parity_tests {
    //! KAIROS-T-0178: three surfaces create work items — the CLI, REST, and this
    //! MCP tool — and they drifted apart silently.
    //!
    //! `bucket_type` was the one that mattered: an initiative created over MCP
    //! could never be a bucket, because `create_item` hardcoded `None`. An agent
    //! reading the tool schema saw no such field and had no way to know whether
    //! that was a deliberate restriction or an omission. `get_item` reported
    //! `bucket` on read, which made it worse: the concept was visible and
    //! unreachable.
    //!
    //! This test diffs the MCP parameter names against the REST create requests,
    //! which are the widest surface. Deliberate differences live in
    //! `DELIBERATELY_ABSENT` with a reason each, so the next divergence is either
    //! a decision someone wrote down or a failing test.

    use std::collections::BTreeSet;

    /// Field names declared by a `pub struct X { .. }` block in some source.
    fn struct_fields(source: &str, name: &str) -> BTreeSet<String> {
        let header = format!("pub struct {name} {{");
        let start = source
            .find(&header)
            .unwrap_or_else(|| panic!("{name} not found"))
            + header.len();
        let body = &source[start..];
        let end = body
            .find("\n}")
            .unwrap_or_else(|| panic!("{name} unterminated"));
        body[..end]
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let rest = line.strip_prefix("pub ")?;
                let (field, _) = rest.split_once(':')?;
                (!field.contains(' ')).then(|| field.to_string())
            })
            .collect()
    }

    fn read(rel: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// REST fields `create_item` deliberately does not take, each with the reason
    /// it does not. Adding an entry here is a decision; leaving one out is a bug.
    const DELIBERATELY_ABSENT: &[(&str, &str)] = &[
        (
            "column_id",
            "Every item is created in its board's entry column, and transition_item \
             is the only way work moves. Accepting a column would let an agent \
             place an item past states the board's transition graph exists to \
             enforce — something a human using the GUI cannot do.",
        ),
        (
            "board_id",
            "MCP takes `board` instead, a slug OR uuid, and defaults it when the \
             tenant has exactly one board of the right level. Strictly wider.",
        ),
        ("template_id", "MCP takes `template`, by id OR name."),
        (
            "repository_id",
            "MCP takes `repository`, by slug OR uuid. It is the link of the task \
             and does not choose the board.",
        ),
        (
            "team_id",
            "The board decides the team of a task, and MCP takes `board`. REST \
             uses `team_id` to name the delivery board of a team; MCP defaults \
             the board when the tenant has exactly one.",
        ),
        (
            "parent_short_code",
            "MCP takes `parent`, which is the same thing under a shorter name — a \
             short code, the identifier an agent actually has, since short codes \
             are what every other tool returns.",
        ),
    ];

    #[test]
    fn create_item_covers_every_rest_create_field() {
        let mcp = struct_fields(&read("src/mcp/tools.rs"), "CreateItemParams");
        let dtos = read("../kairos-client/src/types.rs");

        let mut rest = BTreeSet::new();
        for request in [
            "CreateStrategyRequest",
            "CreateInitiativeRequest",
            "CreateTaskRequest",
            "CreateDocumentRequest",
            "CreateAdrRequest",
        ] {
            rest.extend(struct_fields(&dtos, request));
        }

        let excused: BTreeSet<&str> = DELIBERATELY_ABSENT.iter().map(|(f, _)| *f).collect();
        let missing: Vec<&String> = rest
            .iter()
            .filter(|f| !mcp.contains(*f) && !excused.contains(f.as_str()))
            .collect();

        assert!(
            missing.is_empty(),
            "REST can set these on create and the MCP tool cannot: {missing:?}\n\
             Kairos treats agents as a first-class audience, so a field an agent \
             cannot set is a capability an agent does not have. Either add it to \
             CreateItemParams, or add it to DELIBERATELY_ABSENT with the reason \
             and say so in the tool description — an agent reading the schema \
             cannot ask why a field is missing."
        );
    }

    #[test]
    fn the_two_fields_an_agent_could_not_reach_are_reachable() {
        // Named, because these are the ones that were actually broken:
        // bucket_type made buckets unreachable over MCP entirely.
        let mcp = struct_fields(&read("src/mcp/tools.rs"), "CreateItemParams");
        for field in ["bucket_type", "decision_date"] {
            assert!(mcp.contains(field), "create_item lost {field}");
        }
    }

    #[test]
    fn the_absence_of_column_is_documented_not_merely_true() {
        // A silent absence is the defect this ticket was filed about, so the
        // reason has to be where an agent will read it: the tool description.
        let source = read("src/mcp/tools.rs");
        let start = source
            .find("description = \"Create a work item")
            .expect("create_item tool description");
        let description = &source[start..start + 1400];
        assert!(
            description.contains("no `column` argument"),
            "the create_item description must state that `column` is deliberately \
             excluded, and why"
        );
        assert!(
            description.contains("bucket_type"),
            "the create_item description must mention bucket_type"
        );
    }
}

/// The rules of a board on one line (KAIROS-T-0554): each live column in
/// its order with its flags, then the columns a card can move to from it.
/// `Backlog [entry] → Todo | Active [claims, WIP 3] → Completed, Blocked |
/// Completed [done]`. The entry column is the first by position, the rule
/// of `kairos_db::boards::entry_column`. Pure.
fn moves_line(
    columns: &[kairos_db::board_items::ColumnCardCount],
    transitions: &[(Uuid, Uuid)],
) -> String {
    let mut ordered: Vec<&kairos_db::board_items::ColumnCardCount> = columns.iter().collect();
    ordered.sort_by_key(|c| c.position);
    let entry = ordered.first().map(|c| c.column_id);
    let mut parts = Vec::with_capacity(ordered.len());
    for column in &ordered {
        let mut flags = Vec::new();
        if entry == Some(column.column_id) {
            flags.push("entry".to_string());
        }
        if column.is_done {
            flags.push("done".to_string());
        }
        if column.claims {
            flags.push("claims".to_string());
        }
        if let Some(limit) = column.wip_limit {
            flags.push(format!("WIP {limit}"));
        }
        let mut part = column.name.clone();
        if !flags.is_empty() {
            part.push_str(&format!(" [{}]", flags.join(", ")));
        }
        let targets: Vec<&str> = ordered
            .iter()
            .filter(|c| {
                transitions
                    .iter()
                    .any(|(from, to)| *from == column.column_id && *to == c.column_id)
            })
            .map(|c| c.name.as_str())
            .collect();
        if !targets.is_empty() {
            part.push_str(&format!(" → {}", targets.join(", ")));
        }
        parts.push(part);
    }
    parts.join(" | ")
}

#[cfg(test)]
mod moves_line_tests {
    use super::moves_line;
    use kairos_db::board_items::ColumnCardCount;
    use uuid::Uuid;

    fn column(
        board: Uuid,
        name: &str,
        position: i32,
        is_done: bool,
        claims: bool,
        wip_limit: Option<i32>,
    ) -> ColumnCardCount {
        ColumnCardCount {
            board_id: board,
            column_id: Uuid::new_v4(),
            name: name.to_string(),
            count: 0,
            position,
            is_done,
            claims,
            wip_limit,
        }
    }

    #[test]
    fn the_line_names_the_flags_and_the_targets_in_board_order() {
        let board = Uuid::new_v4();
        let columns = vec![
            column(board, "Active", 2, false, true, Some(3)),
            column(board, "Backlog", 0, false, false, None),
            column(board, "Todo", 1, false, false, None),
            column(board, "Completed", 3, true, false, None),
        ];
        let id = |name: &str| columns.iter().find(|c| c.name == name).unwrap().column_id;
        let transitions = vec![
            (id("Backlog"), id("Todo")),
            (id("Todo"), id("Active")),
            (id("Active"), id("Completed")),
            (id("Active"), id("Todo")),
        ];
        assert_eq!(
            moves_line(&columns, &transitions),
            "Backlog [entry] → Todo | Todo → Active | Active [claims, WIP 3] → Todo, Completed | Completed [done]"
        );
        assert_eq!(moves_line(&[], &[]), "");
    }
}

#[cfg(test)]
mod render_members_tests {
    //! KAIROS-T-0556: the text of `list_members`.
    use super::{MemberLine, render_members};

    fn member(user_name: &str, kind: &str, detail: Option<&str>) -> MemberLine {
        MemberLine {
            user_name: user_name.into(),
            display_name: format!("{user_name} Doe"),
            email: format!("{user_name}@example.test"),
            kind: kind.into(),
            detail: detail.map(str::to_string),
        }
    }

    #[test]
    fn the_members_are_in_the_order_of_the_user_names_with_the_kind() {
        let text = render_members(
            "# Members\n",
            vec![
                member("sam", "service_account", Some("role: member")),
                member("jamie", "human", Some("role: admin")),
                member("forge", "system", None),
            ],
            50,
            0,
        );
        assert_eq!(
            text,
            "# Members\n\
             - forge — forge Doe <forge@example.test> · system actor\n\
             - jamie — jamie Doe <jamie@example.test> · person · role: admin\n\
             - sam — sam Doe <sam@example.test> · service account · role: member\n"
        );
    }

    #[test]
    fn a_part_gives_the_count_and_the_next_offset() {
        let members = || {
            vec![
                member("robin", "human", None),
                member("sam", "human", None),
                member("jamie", "human", None),
            ]
        };
        let first = render_members("# M\n", members(), 2, 0);
        assert!(
            first.contains(
                "Members: 3. Shown: 2 (limit 2, offset 0). Call again with offset 2 for the next part."
            ),
            "{first}"
        );
        assert!(
            first.contains("- jamie") && first.contains("- robin"),
            "{first}"
        );
        assert!(!first.contains("- sam"), "{first}");
        let last = render_members("# M\n", members(), 2, 2);
        assert!(
            last.contains("Members: 3. Shown: 1 (limit 2, offset 2).\n"),
            "{last}"
        );
        assert!(last.contains("- sam"), "{last}");
        assert_eq!(render_members("# M\n", Vec::new(), 50, 0), "# M\n(none)\n");
    }
}

#[cfg(test)]
mod related_work_render_tests {
    //! KAIROS-T-0562: the text of `related_work`. A text-only answer says
    //! "Text only" one time, at the top, and "None found" when no item is over
    //! the bar. The text of an answer with vectors does not change.
    use super::render_related_work;
    use crate::embedding::RelatedWork;
    use kairos_core::retrieval::{Candidate, GraphRelation, RetrievalConfig, Sources, propose};

    fn candidate(code: &str, lexical: usize, vector: Option<usize>, text: f32) -> Candidate {
        Candidate {
            short_code: code.into(),
            title: format!("Title of {code}"),
            entity_type: "task".into(),
            lexical_rank: Some(lexical),
            vector_rank: vector,
            graph: GraphRelation::default(),
            finished: false,
            heading: None,
            text_score: Some(text),
        }
    }

    fn answer(sources: Sources, candidates: &[Candidate]) -> RelatedWork {
        RelatedWork {
            short_code: "ACME-T-0001".into(),
            proposals: propose(candidates, sources, &RetrievalConfig::default()),
            sources,
        }
    }

    const TEXT_ONLY: Sources = Sources {
        lexical: true,
        vector: false,
    };
    const BOTH: Sources = Sources {
        lexical: true,
        vector: true,
    };

    #[test]
    fn a_text_only_answer_says_text_only_once_at_the_top() {
        let candidates: Vec<Candidate> = (0..6)
            .map(|i| candidate(&format!("ACME-T-{:04}", i + 2), i, None, 0.9))
            .collect();
        let text = render_related_work(&answer(TEXT_ONLY, &candidates), &[]);
        assert_eq!(text.matches("Text only").count(), 1, "{text}");
        let note = text.find("Text only").expect("the note");
        let first = text.find("- **").expect("a proposal");
        assert!(
            note < first,
            "the warning comes before the proposals: {text}"
        );
        assert_eq!(text.matches("- **").count(), 3, "at most 3: {text}");
    }

    #[test]
    fn a_text_only_answer_under_the_bar_is_none_found() {
        let weak = [candidate("ACME-T-0002", 0, None, 0.1)];
        let text = render_related_work(&answer(TEXT_ONLY, &weak), &[]);
        assert!(
            text.starts_with("# Possibly related to ACME-T-0001\n\nNone found."),
            "{text}"
        );
        assert_eq!(text.matches("Text only").count(), 1, "{text}");
        assert!(!text.contains("- **"), "{text}");
    }

    /// The text of an answer with vectors, word for word as before
    /// KAIROS-T-0562. A low text score changes nothing here.
    #[test]
    fn an_answer_with_vectors_reads_as_before() {
        let found = answer(BOTH, &[candidate("ACME-T-0002", 0, Some(0), 0.01)]);
        assert_eq!(
            render_related_work(&found, &[]),
            "# Possibly related to ACME-T-0001\n\n\
             Ranked across text and meaning. \"Nothing joins them\" means no direct edge and \
             no shared parent — not that a path was searched for and missed. The graph is \
             sparse, so absence is weak evidence.\n\n\
             These are PROPOSALS, not findings. Examine a proposal before you use it.\n\n\
             - **ACME-T-0002** — Title of ACME-T-0002 (possible dependency)\n  \
             Matches both the words and the meaning, and nothing in the graph joins them.\n"
        );
        let none = answer(BOTH, &[]);
        assert_eq!(
            render_related_work(&none, &[]),
            "# Possibly related to ACME-T-0001\n\n\
             The search found no related work. Ranked across text and meaning.\n\n\
             This is one search. It does not prove that no related work exists.\n"
        );
    }
}

#[cfg(test)]
mod t0558_tests {
    //! KAIROS-T-0558: the preview of `delete_item` and the proposals of
    //! `get_item`.
    use super::{ProposalRow, preview_text, proposals_section};
    use crate::error::ApiError;
    use kairos_db::items::{CascadePreview, NotReached, NotReachedReason};
    use uuid::Uuid;

    #[test]
    fn a_preview_lists_the_cascade_and_archives_nothing() {
        let preview = CascadePreview {
            root_short_code: "ACME-I-0001".into(),
            cascaded_short_codes: vec!["ACME-T-0001".into(), "ACME-T-0002".into()],
            not_reached: vec![NotReached {
                short_code: "WEB-T-0003".into(),
                reason: NotReachedReason::Below {
                    short_code: "ACME-T-0002".into(),
                },
            }],
        };
        assert_eq!(
            preview_text(&preview, None),
            "Preview of an archive of ACME-I-0001. Nothing is archived.\n\
             The cascade would archive 2 descendants: ACME-T-0001, ACME-T-0002.\n\
             The archive would not reach 1 item. They would stay live and keep their parent.\n\
             - WEB-T-0003: It is below ACME-T-0002.\n\
             To archive, call delete_item with short_code ACME-I-0001 and confirm=true.\n"
        );
        let alone = CascadePreview {
            root_short_code: "ACME-T-0009".into(),
            cascaded_short_codes: Vec::new(),
            not_reached: Vec::new(),
        };
        let refusal = ApiError::forbidden("No right.");
        let text = preview_text(&alone, Some(&refusal));
        assert_eq!(
            text,
            "Preview of an archive of ACME-T-0009. Nothing is archived.\n\
             You cannot archive ACME-T-0009: FORBIDDEN: No right.\n\
             Cascade: none.\n"
        );
    }

    fn row(state: &str, decider: Option<&str>) -> ProposalRow {
        ProposalRow {
            id: Uuid::nil(),
            source: Some("ACME-T-0001".into()),
            target: Some("ACME-T-0002".into()),
            relationship: "blocks".into(),
            state: state.into(),
            why: "The first writes the table.".into(),
            proposer: "Robin".into(),
            proposed_on: "2026-10-17".into(),
            decider: decider.map(str::to_string),
            decided_on: decider.map(|_| "2026-10-18".to_string()),
        }
    }

    #[test]
    fn the_proposals_section_gives_each_state() {
        assert_eq!(proposals_section("ACME-T-0001", 0, &[]), "");
        assert_eq!(
            proposals_section(
                "ACME-T-0001",
                3,
                &[row("pending", None), row("rejected", Some("Sam"))]
            ),
            "\n## Proposals\n\
             - pending: ACME-T-0001 -[blocks]-> ACME-T-0002 \
             (00000000-0000-0000-0000-000000000000), proposed by Robin on 2026-10-17. \
             Why: The first writes the table.\n\
             - rejected: ACME-T-0001 -[blocks]-> ACME-T-0002 \
             (00000000-0000-0000-0000-000000000000), proposed by Robin on 2026-10-17, \
             rejected by Sam on 2026-10-18. Why: The first writes the table.\n\
             - 2 more pending proposals. GET /api/items/ACME-T-0001/proposals lists each one.\n"
        );
    }
}

#[cfg(test)]
mod t0561_tests {
    //! KAIROS-T-0561: the search line of a task and the part of a list.

    use super::{empty_page_text, more_line, more_text, task_line_tail};
    use uuid::Uuid;

    /// KAIROS-T-0561: the facts of a task line, as the search query gives
    /// them.
    fn line_facts(
        claim: Option<(&str, bool)>,
        blocked_by: i64,
        repository: Option<&str>,
    ) -> kairos_db::search::TaskLineFacts {
        kairos_db::search::TaskLineFacts {
            id: Uuid::nil(),
            board_slug: Some("kairos".into()),
            column_name: Some("Active".into()),
            claim_holder: claim.map(|(name, _)| name.to_string()),
            claim_agent: claim.is_some_and(|(_, agent)| agent),
            blocked_by,
            repository_slug: repository.map(str::to_string),
        }
    }

    #[test]
    fn a_task_line_gives_the_board_the_column_the_claim_and_the_blockers() {
        let facts = line_facts(Some(("Robin", true)), 1, Some("kairos"));
        assert_eq!(
            task_line_tail(Some(&facts)),
            " kairos/Active, claimed by Robin (agent), blocked by 1 [repo:kairos]"
        );
        let facts = line_facts(Some(("Robin", false)), 0, None);
        assert_eq!(
            task_line_tail(Some(&facts)),
            " kairos/Active, claimed by Robin"
        );
        let facts = line_facts(None, 2, None);
        assert_eq!(task_line_tail(Some(&facts)), " kairos/Active, blocked by 2");
        assert_eq!(task_line_tail(None), "");
    }

    #[test]
    fn a_cut_list_says_how_many_more_and_the_next_offset() {
        assert_eq!(more_text(12, 0, 10).as_deref(), Some("2 more; offset 10."));
        assert_eq!(more_text(12, 10, 2), None);
        assert_eq!(more_text(0, 0, 0), None);
        assert_eq!(more_text(5, 9, 0), None, "a page past the end");
        assert_eq!(more_line(51, 0, 50), "- 1 more; offset 50.\n");
        assert_eq!(more_line(50, 0, 50), "");
        assert_eq!(empty_page_text("(none)", 0, 0), "(none)\n");
        assert_eq!(empty_page_text("(none)", 0, 50), "(none)\n");
        assert_eq!(
            empty_page_text("(none)", 51, 100),
            "(none at offset 100: the list has 51)\n"
        );
    }
}
