//! Shared wire types for the KAIROS-T-0019 organizational + admin endpoint
//! families (KAIROS-S-0005): boards (+columns/transitions/items/members),
//! teams (+members), delivery streams (+teams), organization membership,
//! and deployment-admin tenant provisioning.
//!
//! Same dependency discipline as [`crate::types`]: serde/utoipa only, so
//! ids travel as canonical UUID strings and timestamps as RFC 3339 strings.
//! The server parses inbound id strings and rejects malformed values with
//! 422 `VALIDATION`.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::types::{Adr, Initiative, Strategy, Task};

// ---------------------------------------------------------------------------
// Boards
// ---------------------------------------------------------------------------

/// A board (`/api/boards` list element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Board {
    /// Board id (UUID).
    pub id: String,
    pub name: String,
    pub slug: String,
    /// `strategy|initiative|delivery|adr`.
    pub board_level: String,
    /// Owning team (UUID). A delivery board has one (COLLIERY-T-0230). An
    /// ADR board can have one: the ADR board of the team (COLLIERY-T-3102).
    /// A board of the organization has none: its team is the list of its
    /// members.
    pub team_id: Option<String>,
    /// The short-code prefix of the board (COLLIERY-T-3099): an item on
    /// the board gets a code `{code_prefix}-{type letter}-{number}`. It is
    /// set when the board is created and never changes. The default is for
    /// the client only: a response of an older server has no prefix.
    #[serde(default)]
    #[schema(required = true)]
    pub code_prefix: String,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// A board column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BoardColumn {
    /// Column id (UUID).
    pub id: String,
    /// Owning board (UUID).
    pub board_id: String,
    pub name: String,
    /// Display ordering, 0-indexed.
    pub position: i32,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
    /// Occupants count as completed for children-progress rollups
    /// (KAIROS-T-0080).
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required (COLLIERY-T-0254). The default is for the client only: a
    /// response of an older server has no `is_done`, and it reads as
    /// `false`.
    #[serde(default)]
    #[schema(required = true)]
    pub is_done: bool,
    /// When this column was REMOVED from its board (RFC 3339), `null`
    /// while it is part of the board (KAIROS-T-0161, KAIROS-T-0164).
    ///
    /// `GET /api/boards/{id}` omits removed columns entirely unless it is
    /// asked for them (`?include_removed_columns=true`), so this is `null`
    /// on every default read. It is a timestamp only for a caller that
    /// wants the column an ARCHIVED card was put away in — the audit fact
    /// the soft delete exists to preserve.
    ///
    /// Named `removed_at`, not `archived_at`: the entity DTOs' `archived_at`
    /// is the KAIROS-A-0020 work-item state, and a column is not work.
    ///
    /// The server sends it in each response, `null` for a live column, so
    /// the schema shows it as required and nullable (COLLIERY-T-0256, the
    /// pattern of COLLIERY-T-0254). The default is for the client only: a
    /// response of an older server has no `removed_at`, and it reads as
    /// `None`.
    #[serde(default)]
    #[schema(required = true)]
    pub removed_at: Option<String>,
}

/// An allowed column-to-column transition edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BoardTransition {
    /// Transition id (UUID) — used by `DELETE /api/boards/{id}/transitions/{transition_id}`.
    pub id: String,
    /// Owning board (UUID).
    pub board_id: String,
    /// Source column (UUID).
    pub from_column_id: String,
    /// Target column (UUID).
    pub to_column_id: String,
}

/// Board detail: the board plus its full configuration
/// (`GET /api/boards/{id}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BoardDetail {
    #[serde(flatten)]
    pub board: Board,
    /// Columns in position order.
    pub columns: Vec<BoardColumn>,
    /// Allowed transition edges.
    pub transitions: Vec<BoardTransition>,
}

/// Body of `POST /api/boards`: creates a board seeded with the system
/// default columns/transitions for its level (KAIROS-A-0002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateBoardRequest {
    pub name: String,
    pub slug: String,
    /// `strategy|initiative|delivery|adr`.
    pub board_level: String,
    /// Owning team (UUID). Required for a delivery board
    /// (COLLIERY-T-0230). Optional for an ADR board: the ADR board of the
    /// team, with the prefix of the team (COLLIERY-T-3102). Leave it out
    /// for a strategy or initiative board.
    #[serde(default)]
    pub team_id: Option<String>,
    /// The short-code prefix of the board (COLLIERY-T-3099). Required. It
    /// must match `^[A-Z][A-Z0-9]{1,9}$`, and no live board of the same
    /// level can have it: each pair (prefix, type) is unique, and the
    /// level gives the types of a board. It never changes. A bad prefix is
    /// a 422 `VALIDATION` with `details.field` = `code_prefix`. A prefix
    /// that a live board of the same level has is a 409 `CONFLICT` that
    /// names that board.
    pub code_prefix: String,
}

/// Body of `PATCH /api/boards/{id}` (board settings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateBoardRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    /// The team of the board (UUID, or null for a board of the
    /// organization). The team of a board does not change
    /// (COLLIERY-T-0243): a value that is not the team of the board is
    /// refused with 422 `BOARD_TEAM_IS_FIXED`. Leave it out, or send the
    /// value that the board has.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    #[schema(value_type = Option<String>)]
    pub team_id: Option<Option<String>>,
}

/// Body of `PUT /api/boards/{id}/code-sequences/{item_type}`
/// (COLLIERY-T-3104): the number of the next code of the type on the
/// board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCodeSequenceRequest {
    /// The next create of the type on the board gets this number. It must
    /// be above the last number of the sequence, and above the number of
    /// each code of the prefix and type that exists or is retired.
    pub next_number: i64,
}

/// The sequence of a prefix and a type (COLLIERY-T-3104).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CodeSequence {
    /// The short-code prefix of the board.
    pub code_prefix: String,
    /// `strategy|initiative|task|document|adr`.
    pub item_type: String,
    /// The last number that the sequence gave.
    pub last_number: i64,
    /// The code that the next create of the type gets.
    pub next_code: String,
}

/// A field that is present, with its value or its null. With
/// `#[serde(default)]`, a field that is absent is `None` and a null is
/// `Some(None)`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// One column's items in the `GET /api/boards/{id}/items` view: every live
/// workflow item of every entity type currently in this column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BoardColumnItems {
    pub column: BoardColumn,
    pub strategies: Vec<Strategy>,
    pub initiatives: Vec<Initiative>,
    pub tasks: Vec<Task>,
    pub adrs: Vec<Adr>,
}

/// The default of `limit` of `GET /api/boards/{id}/items`
/// (COLLIERY-T-0261).
pub const BOARD_ITEMS_DEFAULT_LIMIT: i64 = 200;
/// The maximum of `limit` of `GET /api/boards/{id}/items`
/// (COLLIERY-T-0261).
pub const BOARD_ITEMS_MAX_LIMIT: i64 = 1000;

/// Query of `GET /api/boards/{id}/items` (KAIROS-T-0104,
/// COLLIERY-T-0261). Each filter applies before the page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct BoardItemsQuery {
    /// Narrow the TASKS to those bound to this repository (slug or UUID).
    /// Other entity types are unaffected. Unknown repository → 422.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Include archived (put-away) cards, each marked with `archived_at`
    /// (KAIROS-A-0020 rule 2). Default false — a board is a live board
    /// unless the reader says otherwise (rule 3).
    #[serde(default)]
    pub include_deleted: bool,
    /// The number of items on a page (default 200, maximum 1000). The
    /// server changes a larger value to 1000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// The number of items to skip (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
}

/// Response of `GET /api/boards/{id}/items`: one page of the items on the
/// board, grouped by column (columns in position order).
///
/// The order of the items is: the position of the column, then the
/// type, then the short code (COLLIERY-T-0261). The order of the types
/// is: strategy, initiative, task, ADR. Each column is in each page, and
/// a column can have no item on a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BoardItemsResponse {
    pub board: Board,
    pub columns: Vec<BoardColumnItems>,
    /// The number of items on the board after the filters, on all pages
    /// (COLLIERY-T-0261). The default is for the client only: a response
    /// of an older server has no `total`, and it has each item.
    #[serde(default)]
    #[schema(required = true)]
    pub total: i64,
    /// The `limit` that the server applied.
    #[serde(default)]
    #[schema(required = true)]
    pub limit: i64,
    /// The `offset` that the server applied.
    #[serde(default)]
    #[schema(required = true)]
    pub offset: i64,
    /// `(done, total)` direct-children counts keyed by the PARENT item's
    /// short code, for every item on this board that has children
    /// (KAIROS-T-0080) — computed in one grouped query, never per item.
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required (COLLIERY-T-0254). It is an empty map when no item has
    /// children. The default is for the client only.
    #[serde(default)]
    #[schema(required = true)]
    pub children_progress: std::collections::BTreeMap<String, ProgressCounts>,
    /// Blocked-by/blocks counts keyed by short code, for every item on
    /// this board with at least one open `blocks` edge (KAIROS-T-0091) —
    /// one grouped query; soft-deleted neighbors never count. An edge is
    /// open only while neither end sits in a terminal column
    /// (COLLIERY-T-0214): done work does not block and is not blocked, so
    /// a card in a terminal column has no entry.
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required (COLLIERY-T-0254). It is an empty map when no item has an
    /// open `blocks` edge. The default is for the client only.
    #[serde(default)]
    #[schema(required = true)]
    pub blocks_summary: std::collections::BTreeMap<String, BlocksCounts>,
}

impl BoardItemsResponse {
    /// The number of items of the response, in all columns.
    pub fn item_count(&self) -> usize {
        self.columns
            .iter()
            .map(|c| c.strategies.len() + c.initiatives.len() + c.tasks.len() + c.adrs.len())
            .sum()
    }

    /// Add the next page to this response (COLLIERY-T-0261): the items go
    /// to the end of their columns, and the two maps get the entries of
    /// the page. `total` becomes that of the newer page. A column that
    /// this response does not have goes to the end of the columns.
    pub fn add_page(&mut self, page: BoardItemsResponse) {
        for mut group in page.columns {
            match self
                .columns
                .iter_mut()
                .find(|mine| mine.column.id == group.column.id)
            {
                Some(mine) => {
                    mine.strategies.append(&mut group.strategies);
                    mine.initiatives.append(&mut group.initiatives);
                    mine.tasks.append(&mut group.tasks);
                    mine.adrs.append(&mut group.adrs);
                }
                None => self.columns.push(group),
            }
        }
        self.children_progress.extend(page.children_progress);
        self.blocks_summary.extend(page.blocks_summary);
        self.total = page.total;
    }

    /// The note of a response that is a part of the board
    /// ([`incomplete_board_note`]), or `None` when the response has each
    /// item of the board.
    pub fn incomplete_note(&self, next: &str) -> Option<String> {
        let shown = i64::try_from(self.item_count()).unwrap_or(i64::MAX);
        incomplete_board_note(
            &format!("The board has {} items.", self.total),
            self.total,
            shown,
            self.limit,
            self.offset,
            next,
        )
    }
}

/// The sentences that tell a reader that a result is a part of the items
/// of a board, and how to get the next part (COLLIERY-T-0261). `None` when
/// the result has each item. `subject` is the sentence that gives the
/// total. `next` names the parameter of the reader: `call the tool with
/// offset` for MCP, `use --offset` for the CLI. Pure.
///
/// "The board has 340 items. This result shows 200 (limit 200, offset 0).
/// To read the next part, call the tool with offset 200."
pub fn incomplete_board_note(
    subject: &str,
    total: i64,
    shown: i64,
    limit: i64,
    offset: i64,
    next: &str,
) -> Option<String> {
    if shown >= total {
        return None;
    }
    let mut note = format!("{subject} This result shows {shown} (limit {limit}, offset {offset}).");
    let next_offset = offset.saturating_add(shown);
    if shown > 0 && next_offset < total {
        note.push_str(&format!(" To read the next part, {next} {next_offset}."));
    } else if offset > 0 {
        note.push_str(&format!(" To read the first part, {next} 0."));
    }
    Some(note)
}

/// Dependency counts behind a board card's blocked-by/blocks badges
/// (KAIROS-T-0091).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BlocksCounts {
    /// Open incoming `blocks` edges (things blocking this item): the
    /// blocker is live, and neither end is in a terminal column
    /// (COLLIERY-T-0214).
    pub blocked_by: i64,
    /// Open outgoing `blocks` edges (things this item blocks): the blocked
    /// item is live, and neither end is in a terminal column
    /// (COLLIERY-T-0214).
    pub blocks: i64,
}

/// A `(done, total)` children rollup (KAIROS-T-0080). `done` counts the
/// children sitting in `is_done` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProgressCounts {
    pub done: i64,
    pub total: i64,
    /// False when no board hosting the children has a done-flagged
    /// column — clients show composition only, never a done fraction.
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required (COLLIERY-T-0254). The default is for the client only: a
    /// response of an older server has no `has_done`, and it reads as
    /// `false`.
    #[serde(default)]
    #[schema(required = true)]
    pub has_done: bool,
}

/// Body of `POST /api/boards/{id}/columns`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateColumnRequest {
    pub name: String,
    /// 0-indexed position; must not collide with an existing column
    /// (422 `DUPLICATE_COLUMN_POSITION`).
    pub position: i32,
}

/// Body of `PATCH /api/boards/{id}/columns/{col_id}` — rename, move,
/// and/or set the done flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateColumnRequest {
    #[serde(default)]
    pub name: Option<String>,
    /// New 0-indexed position; the other columns shift around it.
    #[serde(default)]
    pub position: Option<i32>,
    /// Mark occupants as completed for children-progress rollups
    /// (KAIROS-T-0080). An explicit admin choice — the dead-end heuristic
    /// only ever suggests.
    #[serde(default)]
    pub is_done: Option<bool>,
}

/// Body of `POST /api/boards/{id}/transitions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTransitionRequest {
    /// Source column (UUID).
    pub from_column_id: String,
    /// Target column (UUID).
    pub to_column_id: String,
}

// ---------------------------------------------------------------------------
// Board authorization (KAIROS-A-0006 capability grants)
// ---------------------------------------------------------------------------

/// A board member and their capability grants
/// (`GET /api/boards/{id}/members` element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BoardMember {
    /// `public.users.id` (UUID).
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    /// The user's capability grants on this board (sorted).
    pub capabilities: Vec<String>,
}

/// Body of `POST /api/boards/{id}/members`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddBoardMemberRequest {
    /// `public.users.id` (UUID).
    pub user_id: String,
    /// Capabilities to grant (KAIROS-A-0006 vocabulary or glob; at least
    /// one).
    pub capabilities: Vec<String>,
}

/// Body of `PATCH /api/boards/{id}/members/{user_id}` — replaces the user's
/// full capability set (revokes what is absent, grants what is new).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceCapabilitiesRequest {
    /// The complete new capability set (at least one; use DELETE to revoke
    /// board membership entirely).
    pub capabilities: Vec<String>,
}

/// Response of `DELETE /api/boards/{id}/members/{user_id}` — full
/// revocation of a user's board membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RemoveBoardMemberResponse {
    /// The user whose grants were revoked (UUID).
    pub user_id: String,
    /// The capabilities that were revoked (sorted).
    pub revoked_capabilities: Vec<String>,
}

/// Generic delete acknowledgement for organizational resources (boards,
/// columns, transitions, teams, streams, membership rows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OrgDeleteResponse {
    /// Id of the deleted resource (UUID).
    pub id: String,
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// Teams
// ---------------------------------------------------------------------------

/// A team (`/api/teams`). Every team owns a delivery board
/// (KAIROS-A-0002/T-0010: created with the team from the system delivery
/// defaults).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Team {
    /// Team id (UUID).
    pub id: String,
    pub name: String,
    pub slug: String,
    /// `stream_aligned|platform|enabling|complicated_subsystem`.
    pub team_type: String,
    /// The team's delivery board (UUID); created with the team.
    pub delivery_board_id: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// Body of `POST /api/teams`. Creating a team also creates its delivery
/// board (slug `{slug}-delivery`) from the system defaults, in the same
/// transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTeamRequest {
    pub name: String,
    pub slug: String,
    /// `stream_aligned|platform|enabling|complicated_subsystem`; defaults
    /// to `stream_aligned`.
    #[serde(default)]
    pub team_type: Option<String>,
    /// The short-code prefix of the delivery board of the team
    /// (COLLIERY-T-3099). Required. The rules are those of
    /// `code_prefix` in `POST /api/boards`.
    pub code_prefix: String,
}

/// Body of `PATCH /api/teams/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTeamRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub team_type: Option<String>,
}

/// A team member (`GET /api/teams/{id}/members` element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamMember {
    /// `public.users.id` (UUID).
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    /// RFC 3339.
    pub joined_at: String,
}

/// Body of `POST /api/teams/{id}/members`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddTeamMemberRequest {
    /// `public.users.id` (UUID).
    pub user_id: String,
}

// ---------------------------------------------------------------------------
// Delivery streams
// ---------------------------------------------------------------------------

/// A delivery stream (`/api/delivery-streams`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeliveryStream {
    /// Stream id (UUID).
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// Body of `POST /api/delivery-streams`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStreamRequest {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Body of `PATCH /api/delivery-streams/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateStreamRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Body of `POST /api/delivery-streams/{id}/teams`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddStreamTeamRequest {
    /// Team id (UUID).
    pub team_id: String,
}

// ---------------------------------------------------------------------------
// Organization membership (/api/members)
// ---------------------------------------------------------------------------

/// An organization member (`GET /api/members` element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OrgMember {
    /// `public.users.id` (UUID).
    pub user_id: String,
    /// OIDC `sub`.
    pub external_id: String,
    pub email: String,
    pub display_name: String,
    /// `admin|member`.
    pub role: String,
    /// RFC 3339.
    pub joined_at: String,
}

/// Body of `POST /api/members`. The user is resolved by email from
/// `public.users` — users are JIT-provisioned at first login, so an unknown
/// email means the person must log in once first (404 with that guidance).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddOrgMemberRequest {
    pub email: String,
    /// `admin|member`; defaults to `member`.
    #[serde(default)]
    pub role: Option<String>,
}

/// Body of `PATCH /api/members/{user_id}` — role change. Demoting the last
/// admin is rejected (422 `LAST_ADMIN`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateOrgMemberRequest {
    /// `admin|member`.
    pub role: String,
}

/// Response of `DELETE /api/members/{user_id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RemoveOrgMemberResponse {
    /// The removed member's user id (UUID).
    pub user_id: String,
    pub removed: bool,
}

// ---------------------------------------------------------------------------
// Whoami (/api/whoami — the KAIROS-T-0017 identity probe)
// ---------------------------------------------------------------------------

/// Response of `GET /api/whoami`: everything the auth → tenant stack
/// resolved for the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiResponse {
    /// The authenticated user (JIT-provisioned `public.users` row).
    pub user: WhoamiUser,
    /// The resolved tenant and the caller's role in it.
    pub organization: WhoamiOrganization,
    /// Teams the caller belongs to within this tenant.
    pub teams: Vec<WhoamiTeam>,
    /// Boards on which the caller holds explicit capability grants
    /// (KAIROS-A-0006), grouped by board. Empty for a plain member with no
    /// grants; org admins hold implicit full access via `organization.role
    /// == "admin"` and usually appear here with no explicit grants.
    #[serde(default)]
    pub capabilities: Vec<WhoamiBoardCapabilities>,
    /// COMPUTED capabilities every tenant member holds without a grant
    /// (KAIROS-T-0105): currently `file_backlog` — send a request to any
    /// team: a task in the entry column of its delivery board, in the
    /// support lane (COLLIERY-T-0218, COLLIERY-A-0023).
    #[serde(default)]
    pub implicit: Vec<String>,
    /// Repositories owned by the caller's teams (KAIROS-T-0107, A-0019).
    #[serde(default)]
    pub repositories: Vec<WhoamiRepository>,
}

/// One repository of [`WhoamiResponse::repositories`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiRepository {
    /// Repository id (UUID).
    pub id: String,
    pub slug: String,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo`.
    pub repo_full_name: String,
    /// The owning team's slug.
    pub team_slug: String,
}

/// One board on which the caller holds explicit capability grants
/// (KAIROS-A-0006 `board_member_capabilities`), with the grant list — an
/// element of [`WhoamiResponse::capabilities`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiBoardCapabilities {
    /// `boards.id` (UUID).
    pub board_id: String,
    /// `boards.slug`.
    pub board_slug: String,
    /// The capability strings granted on this board (may include globs like
    /// `*`, `manage_*`, `configure_*`), sorted.
    pub grants: Vec<String>,
}

/// The `user` object of [`WhoamiResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiUser {
    /// `public.users.id` (UUID).
    pub id: String,
    /// OIDC `sub`.
    pub external_id: String,
    pub email: String,
    pub display_name: String,
}

/// The `organization` object of [`WhoamiResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiOrganization {
    /// `public.organizations.id` (UUID).
    pub id: String,
    pub slug: String,
    /// `admin|member`.
    pub role: String,
}

/// One team of [`WhoamiResponse::teams`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiTeam {
    /// Team id (UUID).
    pub id: String,
    pub slug: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Deployment-admin tenant provisioning (/api/admin/tenants)
// ---------------------------------------------------------------------------

/// Body of `POST /api/admin/tenants` (deployment-admin only; see
/// `KAIROS_DEPLOYMENT_ADMINS`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTenantRequest {
    /// Organization slug (`^[a-z][a-z0-9_-]{1,62}$`).
    pub slug: String,
    /// Organization display name.
    pub name: String,
    /// OIDC `sub` of the user to seed as the tenant's first org admin;
    /// defaults to the caller. The user must have authenticated once
    /// already (422 otherwise).
    #[serde(default)]
    pub initial_admin_external_id: Option<String>,
}

/// The org-admin membership created with a new tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TenantInitialAdmin {
    /// `public.users.id` (UUID).
    pub user_id: String,
    /// OIDC `sub`.
    pub external_id: String,
    pub email: String,
    /// Always `admin`.
    pub role: String,
}

/// Response of `POST /api/admin/tenants` — the T-0008 provisioning report
/// plus the seeded initial admin membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TenantCreatedResponse {
    pub slug: String,
    /// The created schema (`org_{slug}`).
    pub schema: String,
    /// Tenant migration versions applied.
    pub migrations_applied: Vec<String>,
    /// Slugs of the default boards created.
    pub boards_created: Vec<String>,
    pub templates_copied: i64,
    pub metadata_definitions_copied: i64,
    /// The organization admin seeded at creation.
    pub initial_admin: TenantInitialAdmin,
}

/// A provisioned tenant (`GET /api/admin/tenants` element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TenantSummary {
    pub slug: String,
    pub name: String,
    /// Whether the `org_{slug}` schema actually exists (drift check).
    pub schema_exists: bool,
}

/// Response of `DELETE /api/admin/tenants/{slug}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TenantDeletedResponse {
    pub slug: String,
    pub dropped: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use utoipa::PartialSchema;

    /// The names in the `required` list of a schema.
    fn required_of(schema: impl serde::Serialize) -> Vec<String> {
        let schema = serde_json::to_value(schema).expect("serializes");
        schema["required"]
            .as_array()
            .expect("the schema has a `required` list")
            .iter()
            .filter_map(|name| name.as_str().map(str::to_string))
            .collect()
    }

    /// COLLIERY-T-0254, the schema half: the server sends `is_done` in
    /// each response, so the schema lists it as required.
    #[test]
    fn board_column_schema_requires_is_done() {
        let required = required_of(BoardColumn::schema());
        assert!(
            required.iter().any(|name| name == "is_done"),
            "`is_done` must be required: {required:?}"
        );
    }

    /// COLLIERY-T-0256, the schema half: the server sends `removed_at` in
    /// each response, `null` for a live column. So the schema lists it as
    /// required, and its type permits `null`.
    #[test]
    fn board_column_schema_requires_removed_at_and_permits_null() {
        let required = required_of(BoardColumn::schema());
        assert!(
            required.iter().any(|name| name == "removed_at"),
            "`removed_at` must be required: {required:?}"
        );
        let schema = serde_json::to_value(BoardColumn::schema()).expect("serializes");
        assert_eq!(
            schema["properties"]["removed_at"]["type"],
            json!(["string", "null"]),
            "{schema}"
        );
        // What the server sends for a live column.
        let column = BoardColumn {
            id: "c".to_string(),
            board_id: "b".to_string(),
            name: "Todo".to_string(),
            position: 0,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            is_done: false,
            removed_at: None,
        };
        let sent = serde_json::to_value(&column).expect("serializes");
        assert!(
            sent.as_object()
                .expect("an object")
                .contains_key("removed_at")
        );
        assert_eq!(sent["removed_at"], json!(null));
    }

    /// COLLIERY-T-0256, the read half: a response of an older server has
    /// no `removed_at`. The client reads it, and the column is live.
    #[test]
    fn board_column_without_removed_at_deserializes() {
        let column: BoardColumn = serde_json::from_value(json!({
            "id": "c",
            "board_id": "b",
            "name": "Todo",
            "position": 0,
            "is_done": true,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }))
        .expect("a column with no `removed_at` deserializes");
        assert_eq!(column.removed_at, None);
    }

    /// COLLIERY-T-0254, the read half: a response of an older server has
    /// no `is_done`. The client reads it, and the column is not terminal.
    #[test]
    fn board_column_without_is_done_deserializes() {
        let column: BoardColumn = serde_json::from_value(json!({
            "id": "c",
            "board_id": "b",
            "name": "Todo",
            "position": 0,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }))
        .expect("a column with no `is_done` deserializes");
        assert!(!column.is_done);
        assert_eq!(column.removed_at, None);
    }

    /// COLLIERY-T-0254, the schema half: the server sends the two maps in
    /// each response, so the schema lists them as required.
    #[test]
    fn board_items_schema_requires_the_two_maps() {
        let required = required_of(BoardItemsResponse::schema());
        for field in ["children_progress", "blocks_summary"] {
            assert!(
                required.iter().any(|name| name == field),
                "`{field}` must be required: {required:?}"
            );
        }
    }

    /// COLLIERY-T-0254, the read half: a response of an older server has
    /// neither map. The client reads it, and the two maps are empty.
    #[test]
    fn board_items_without_the_two_maps_deserializes() {
        let response: BoardItemsResponse = serde_json::from_value(json!({
            "board": {
                "id": "b",
                "name": "Platform",
                "slug": "platform-delivery",
                "board_level": "delivery",
                "team_id": null,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            },
            "columns": []
        }))
        .expect("a response with neither map deserializes");
        assert!(response.children_progress.is_empty());
        assert!(response.blocks_summary.is_empty());
        // COLLIERY-T-0261: and it has no `total`, `limit` or `offset`.
        assert_eq!((response.total, response.limit, response.offset), (0, 0, 0));
    }

    /// A response with one column and the tasks of the given short codes.
    fn page_of(codes: &[&str], total: i64, limit: i64, offset: i64) -> BoardItemsResponse {
        let tasks: Vec<serde_json::Value> = codes
            .iter()
            .map(|code| {
                json!({
                    "id": code, "short_code": code, "title": code, "content": "",
                    "board_id": "b", "column_id": "c", "task_type": "task",
                    "work_class": "planned", "team_id": null, "version": 1,
                    "created_by": "u", "updated_by": "u",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                })
            })
            .collect();
        let progress: serde_json::Map<String, serde_json::Value> = codes
            .iter()
            .map(|code| {
                (
                    code.to_string(),
                    json!({"done": 0, "total": 1, "has_done": true}),
                )
            })
            .collect();
        serde_json::from_value(json!({
            "board": {
                "id": "b", "name": "Platform", "slug": "platform-delivery",
                "board_level": "delivery", "team_id": null,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            },
            "columns": [{
                "column": {
                    "id": "c", "board_id": "b", "name": "Todo", "position": 0,
                    "is_done": false,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                },
                "strategies": [], "initiatives": [], "tasks": tasks, "adrs": []
            }],
            "total": total, "limit": limit, "offset": offset,
            "children_progress": progress,
            "blocks_summary": {}
        }))
        .expect("a page deserializes")
    }

    /// COLLIERY-T-0261: the pages of a board go together as one response.
    #[test]
    fn board_items_pages_go_together() {
        let mut all = page_of(&["A-T-0001", "A-T-0002"], 3, 2, 0);
        assert_eq!(all.item_count(), 2);
        all.add_page(page_of(&["A-T-0003"], 3, 2, 2));
        assert_eq!(all.item_count(), 3);
        assert_eq!(all.columns.len(), 1, "the column is there one time");
        let codes: Vec<&str> = all.columns[0]
            .tasks
            .iter()
            .map(|task| task.short_code.as_str())
            .collect();
        assert_eq!(codes, ["A-T-0001", "A-T-0002", "A-T-0003"]);
        assert_eq!(all.children_progress.len(), 3, "the maps go together");
        assert_eq!(all.total, 3);
    }

    /// COLLIERY-T-0261: a response that is a part of the board says so,
    /// and it says how to get the next part.
    #[test]
    fn board_items_note_gives_the_next_part() {
        let complete = page_of(&["A-T-0001"], 1, 200, 0);
        assert_eq!(complete.incomplete_note("use --offset"), None);
        assert_eq!(
            incomplete_board_note(
                "The board has 340 items.",
                340,
                200,
                200,
                0,
                "call the tool with offset"
            )
            .as_deref(),
            Some(
                "The board has 340 items. This result shows 200 (limit 200, offset 0). \
                 To read the next part, call the tool with offset 200."
            )
        );
        // The last part: no next part.
        assert_eq!(
            page_of(&["A-T-0003"], 3, 2, 2)
                .incomplete_note("use --offset")
                .as_deref(),
            Some(
                "The board has 3 items. This result shows 1 (limit 2, offset 2). \
                 To read the first part, use --offset 0."
            )
        );
        // An offset after the last item.
        assert_eq!(
            page_of(&[], 3, 2, 10)
                .incomplete_note("use --offset")
                .as_deref(),
            Some(
                "The board has 3 items. This result shows 0 (limit 2, offset 10). \
                 To read the first part, use --offset 0."
            )
        );
    }

    /// COLLIERY-T-0261: the query has no parameter that the caller did
    /// not set, so the server applies its defaults.
    #[test]
    fn board_items_query_has_no_absent_parameter() {
        let query = serde_json::to_value(BoardItemsQuery::default()).expect("the query");
        assert_eq!(query, json!({"include_deleted": false}));
    }

    /// COLLIERY-T-0254, the schema half: the server sends `has_done` in
    /// each response, so the schema lists it as required.
    #[test]
    fn progress_counts_schema_requires_has_done() {
        let required = required_of(ProgressCounts::schema());
        assert!(
            required.iter().any(|name| name == "has_done"),
            "`has_done` must be required: {required:?}"
        );
    }

    /// COLLIERY-T-0254, the read half: a response of an older server has
    /// no `has_done`, and it reads as `false`.
    #[test]
    fn progress_counts_without_has_done_deserializes() {
        let counts: ProgressCounts = serde_json::from_value(json!({"done": 1, "total": 2}))
            .expect("counts with no `has_done` deserialize");
        assert!(!counts.has_done);
    }
}
