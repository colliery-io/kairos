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
    /// A strategy or initiative board has one too (KAIROS-T-0440): the one
    /// in the create, or its own team. The ADR board of the organization has
    /// none.
    pub team_id: Option<String>,
    /// The short-code prefix of the board (COLLIERY-T-3099): an item on
    /// the board gets a code `{code_prefix}-{type letter}-{number}`. It is
    /// set when the board is created and never changes. The default is for
    /// the client only: a response of an older server has no prefix.
    #[serde(default)]
    #[schema(required = true)]
    pub code_prefix: String,
    /// The board policy `parent_gate` of a delivery board (KAIROS-T-0641):
    /// when true, a task moves into a claims column (Active) only while its
    /// parent initiative is active (in Active or Monitoring, or a bucket).
    /// A move under an initiative that is not active is a 422
    /// `PARENT_NOT_ACTIVE`. True by default. `null` on each other level.
    #[serde(default)]
    #[schema(required = true)]
    pub parent_gate: Option<bool>,
    /// The board policy `owner_gate` of an initiative board (KAIROS-T-0643):
    /// when true, an initiative moves into its Active column only with an
    /// owner, and the owner of an initiative in Active or Monitoring cannot
    /// release it (a hand-off can change it). A refused move or release is a
    /// 422 `OWNER_REQUIRED`. True by default. `null` on each other level.
    #[serde(default)]
    #[schema(required = true)]
    pub owner_gate: Option<bool>,
    /// The board policy `blockers_gate` of a delivery board (KAIROS-T-0565):
    /// when true, a task with an open blocker does not move into a claims
    /// column or a done column. The refusal is a 422 `BLOCKED`. When false,
    /// the move goes on with a `Kairos-Warning`. False by default. `null` on
    /// each other level.
    #[serde(default)]
    #[schema(required = true)]
    pub blockers_gate: Option<bool>,
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
    /// A task that a person moves into this column gets a claim for the
    /// person (KAIROS-T-0359). The Active column of a delivery board has
    /// it. The default is for the client only: a response of an older
    /// server has no `claims`, and it reads as `false`.
    #[serde(default)]
    #[schema(required = true)]
    pub claims: bool,
    /// A task with a claim keeps it in this column, and a task with no
    /// claim gets none here (KAIROS-T-0529). The Blocked column of a
    /// delivery board has it. The default is for the client only: a
    /// response of an older server has no `keeps_claims`.
    #[serde(default)]
    #[schema(required = true)]
    pub keeps_claims: bool,
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
    /// The most live items that the column should hold (KAIROS-T-0371);
    /// `null`: no limit. Kairos does not refuse a move into a column over its
    /// limit, and the answer has the header `Kairos-Warning`. The server sends
    /// it in each response; the default is for the client only (an older
    /// server).
    #[serde(default)]
    #[schema(required = true)]
    pub wip_limit: Option<i32>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
    /// The board policy `parent_gate` (KAIROS-T-0641), for a delivery board
    /// only: true turns it on, false turns it off. On each other level the
    /// field is a 422 `VALIDATION`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_gate: Option<bool>,
    /// The board policy `owner_gate` (KAIROS-T-0643), for an initiative
    /// board only: true turns it on, false turns it off. On each other level
    /// the field is a 422 `VALIDATION`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_gate: Option<bool>,
    /// The board policy `blockers_gate` (KAIROS-T-0565), for a delivery
    /// board only: true turns it on, false turns it off. On each other level
    /// the field is a 422 `VALIDATION`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blockers_gate: Option<bool>,
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
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
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
    /// The live items in the column, on each page (KAIROS-T-0371): the
    /// count that the WIP limit is for. Not changed by the filters or the
    /// page. The default is for the client only (an older server).
    #[serde(default)]
    #[schema(required = true)]
    pub item_count: i64,
    /// `item_count` is above `column.wip_limit`.
    #[serde(default)]
    #[schema(required = true)]
    pub over_wip_limit: bool,
    /// The cards of the column (KAIROS-T-0602): no `content` unless the
    /// query has `fields=full`.
    pub strategies: Vec<crate::types_cards::StrategyCard>,
    pub initiatives: Vec<crate::types_cards::InitiativeCard>,
    pub tasks: Vec<crate::types_cards::TaskCard>,
    pub adrs: Vec<crate::types_cards::AdrCard>,
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
    /// Narrow the STRATEGIES and the INITIATIVES to those of this team
    /// (slug or UUID), from tasks or set by hand (KAIROS-T-0321). Tasks
    /// and ADRs are unaffected. Unknown team → 422. Not with `no_team`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// Narrow the STRATEGIES and the INITIATIVES to those with no team
    /// (KAIROS-T-0321). Tasks and ADRs are unaffected. Not with `team`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_team: bool,
    /// Add `item_metadata` to the response: the metadata values of each
    /// item of the page (KAIROS-T-0368). Default false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub with_metadata: bool,
    /// `card` (the default): the cards have no `content`, `created_by` or
    /// `updated_by`. `full`: the cards have them, as before cards
    /// (KAIROS-T-0602). An other value is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[param(inline)]
    pub fields: Option<crate::types_cards::CardFields>,
    /// Only the card with this short code (KAIROS-T-0615). The page then
    /// has one card, or no card when the board has no such card. A live
    /// board reads one card with it after an event of that item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Only the last N cards of each done column, by the time that each
    /// card entered the column (KAIROS-T-0649). The other columns keep
    /// each card. `item_count` stays the count of the full column. At
    /// least 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_limit: Option<i64>,
}

/// One team of an initiative or a strategy (KAIROS-T-0321). The item gets
/// the team from its tasks, or a person sets it by hand, or both. A task
/// gives the team of its board. A strategy reads two levels down. A team
/// gives no right on the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemTeam {
    /// The slug of the team.
    pub slug: String,
    /// The name of the team.
    pub name: String,
    /// The item gets the team from its tasks.
    pub from_tasks: bool,
    /// The team is set on the item by hand.
    pub set_by_hand: bool,
}

/// Response of `GET /api/{entity_type}/{short_code}/teams` (KAIROS-T-0321).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemTeamsResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The teams of the item, by slug. Empty when the item has no team.
    pub teams: Vec<ItemTeam>,
}

/// Body of `POST /api/{entity_type}/{short_code}/teams` (KAIROS-T-0321):
/// the team to set on the item by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetItemTeamRequest {
    /// A live team of the organization, by slug or UUID.
    pub team: String,
}

/// Response of `DELETE /api/{entity_type}/{short_code}/teams/{team}`
/// (KAIROS-T-0321).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ClearedItemTeamResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The slug of the team that is not set on the item by hand now.
    pub team: String,
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
    /// The teams of each strategy and initiative of the page, keyed by
    /// short code (KAIROS-T-0321), by slug — one query for the board. An
    /// item with no team has no entry.
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required. It is an empty map when no item has a team. The default
    /// is for the client only.
    #[serde(default)]
    #[schema(required = true)]
    pub item_teams: std::collections::BTreeMap<String, Vec<ItemTeam>>,
    /// With `with_metadata=true` only (KAIROS-T-0368): the metadata values
    /// of each item of the page, keyed by short code and then by the slug
    /// of the definition. An item with no value has no entry. Absent when
    /// the query did not ask for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_metadata:
        Option<std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>>,
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
        self.item_teams.extend(page.item_teams);
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
/// children sitting in `is_done` columns. An archived child counts by the
/// column that it was archived from (KAIROS-T-0571).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProgressCounts {
    /// Children in a done column, less the cancelled tasks
    /// (KAIROS-T-0567).
    pub done: i64,
    pub total: i64,
    /// Cancelled tasks among the children (KAIROS-T-0567). The default is
    /// for the client only: a response of an older server has no
    /// `cancelled`.
    #[serde(default)]
    #[schema(required = true)]
    pub cancelled: i64,
    /// Children that are not done and not cancelled.
    #[serde(default)]
    #[schema(required = true)]
    pub open: i64,
    /// Children archived from a column that is not done (KAIROS-T-0571):
    /// not open work, and not in `total`. The default is for the client
    /// only.
    #[serde(default)]
    #[schema(required = true)]
    pub removed: i64,
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
    /// A task that a person moves into this column gets a claim for the
    /// person (KAIROS-T-0359). When the flag goes off, the claims of the
    /// tasks in the column end.
    #[serde(default)]
    pub claims: Option<bool>,
    /// A task with a claim keeps it in this column, and a task with no
    /// claim gets none here (KAIROS-T-0529). When the flag goes off, the
    /// claims of the tasks in the column end, unless the column has
    /// `claims` too.
    #[serde(default)]
    pub keeps_claims: Option<bool>,
    /// The WIP limit of the column (KAIROS-T-0371): a number above 0, or
    /// `null` to remove the limit. Leave it out to keep the limit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    #[schema(value_type = Option<i32>)]
    pub wip_limit: Option<Option<i32>>,
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

/// The grants of one person on the boards that the caller administers, and
/// the teams of the person (KAIROS-T-0413). The answer of `GET` and `PATCH
/// /api/members/{user_id}/grants`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UserGrants {
    /// `public.users.id` (UUID).
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    /// One row for each live board that the caller administers. The order
    /// is by level (strategy, initiative, delivery, ADR), then by name.
    pub boards: Vec<UserBoardGrants>,
    /// The teams of the person, by name. `null` when the caller cannot
    /// change team members.
    pub teams: Option<Vec<UserTeam>>,
}

/// The grants of one person on one board (a row of [`UserGrants`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UserBoardGrants {
    /// The board (UUID).
    pub board_id: String,
    pub board_slug: String,
    pub board_name: String,
    /// `strategy|initiative|delivery|adr`.
    pub board_level: String,
    /// The capabilities that apply at the level of the board. A change
    /// can grant only these on the board.
    pub applicable: Vec<String>,
    /// The stored grants of the person on the board (sorted). Globs too.
    pub capabilities: Vec<String>,
}

/// A team of a person (an element of [`UserGrants::teams`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UserTeam {
    /// The team (UUID).
    pub team_id: String,
    pub slug: String,
    pub name: String,
}

/// Body of `PATCH /api/members/{user_id}/grants` (KAIROS-T-0413): the
/// changes to the grants of one person on one or more boards. Kairos
/// applies all of them in one transaction, or none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeUserGrantsRequest {
    /// One entry for each board (at least one; a board once only).
    pub changes: Vec<BoardGrantChange>,
}

/// The changes on one board (an element of
/// [`ChangeUserGrantsRequest::changes`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardGrantChange {
    /// The board (UUID).
    pub board_id: String,
    /// The capabilities to grant. Each must apply at the level of the
    /// board, and the person must not have it.
    #[serde(default)]
    pub grant: Vec<String>,
    /// The stored grants to revoke (a glob too). The person must have each.
    #[serde(default)]
    pub revoke: Vec<String>,
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
    /// The docs site of the team is on (KAIROS-T-0404).
    #[serde(default = "site_on")]
    pub site_enabled: bool,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

fn site_on() -> bool {
    true
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
    /// The docs site of the team (KAIROS-T-0404): on by default. Off, its
    /// rendered pages are not served.
    #[serde(default)]
    pub site_enabled: Option<bool>,
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
// The structure capability (KAIROS-T-0337)
// ---------------------------------------------------------------------------

/// One grant of `manage_structure` (`GET /api/structure-grants` element).
/// `scope` is `organization` or `board`. A grant on the organization covers
/// teams, team members, boards and repositories. A grant on a board covers
/// the configuration, the members and the grants of that board only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StructureGrant {
    /// The id of the grant (UUID). `DELETE /api/structure-grants/{id}`
    /// revokes it.
    pub id: String,
    /// `public.users.id` of the principal (UUID).
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    /// `human` | `service_account`.
    pub kind: String,
    /// `organization` | `board`.
    pub scope: String,
    /// The board of a board grant (UUID), or null.
    pub board_id: Option<String>,
    /// The slug of the board of a board grant, or null.
    pub board_slug: Option<String>,
    /// `public.users.id` of the organization admin who granted it.
    pub granted_by: String,
    /// RFC 3339.
    pub granted_at: String,
}

/// Body of `POST /api/structure-grants`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStructureGrantRequest {
    /// The principal: a user id (UUID), an email, or a user name of a member
    /// of the organization. A service account is given by its id.
    pub user: String,
    /// The board (slug or UUID) of a board grant. Omit it for a grant at the
    /// organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
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
    /// The caller's grants of `manage_structure` (KAIROS-T-0337): at the
    /// organization, or on one board. Empty when the caller has none.
    #[serde(default)]
    pub structure: Vec<WhoamiStructureGrant>,
    /// The agent key of the request (KAIROS-T-0536). `None` when the
    /// request does not use an agent key, and from an older server.
    #[serde(default)]
    pub agent_key: Option<WhoamiAgentKey>,
}

/// The agent key of [`WhoamiResponse::agent_key`] (KAIROS-T-0536).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiAgentKey {
    /// `api_keys.id` (UUID).
    pub id: String,
    /// The scope of the key. `None`: no scope, so no limit (a key made
    /// before KAIROS-T-0536).
    #[serde(default)]
    pub scope: Option<crate::types_service_accounts::KeyScope>,
}

/// One grant of [`WhoamiResponse::structure`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WhoamiStructureGrant {
    /// `organization` | `board`.
    pub scope: String,
    /// The board of a board grant (UUID), or null.
    pub board_id: Option<String>,
    /// The slug of the board of a board grant, or null.
    pub board_slug: Option<String>,
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
    /// The capabilities that membership of this board gives with no grant
    /// (KAIROS-T-0410): `manage_strategies` or `manage_initiatives`, and
    /// `transition_items`, on a strategy or initiative board. Empty on other
    /// boards, and absent from an older server.
    #[serde(default)]
    pub implied: Vec<String>,
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

// ---------------------------------------------------------------------------
// Flow metrics (KAIROS-T-0372)
// ---------------------------------------------------------------------------

/// Query of `GET /api/boards/{id}/flow` (KAIROS-T-0372). Dates are
/// `YYYY-MM-DD` (UTC), and the range has the two days. The defaults: `to`
/// is today, `from` is 89 days before `to` (90 days). A range has 366 days
/// at most.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct BoardFlowQuery {
    /// The first day of the range, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The last day of the range, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// The flow metrics of a board for a range (KAIROS-T-0372), from the
/// history of the moves (`activity_log`). An archived item counts until
/// its archive (KAIROS-T-0571), and a cancelled task apart from the done
/// items.
///
/// The start of an item is its first entry into a start column. A start
/// column has `claims` (Active). With no such column, the column named
/// Active is the start. An item is done in an `is_done` column; its done
/// time is its last entry into one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BoardFlow {
    /// The board (UUID).
    pub board_id: String,
    pub board_slug: String,
    /// The first day of the range, `YYYY-MM-DD`.
    pub from: String,
    /// The last day of the range, `YYYY-MM-DD`.
    pub to: String,
    /// The start columns, by name. Empty: the board has no start, and no
    /// cycle time.
    pub start_columns: Vec<String>,
    /// The done columns, by name.
    pub done_columns: Vec<String>,
    /// The done items of the range (not cancelled).
    pub done_count: i64,
    /// The cancelled tasks of the range, by the time of the cancel.
    pub cancelled_count: i64,
    pub cycle_time: FlowCycleTime,
    /// The done items of the range that never entered a start column (no
    /// cycle time).
    pub done_without_start: i64,
    /// One row for each week of the range (weeks start on Monday, UTC).
    pub throughput: Vec<FlowWeek>,
    /// The work in progress: each item that is not done and has a start,
    /// oldest first.
    pub ageing: Vec<FlowAgeingItem>,
    pub cumulative_flow: CumulativeFlow,
}

/// The cycle time of the done items of the range (KAIROS-T-0372): from the
/// start to the done time, in days.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FlowCycleTime {
    /// The number of items with a cycle time.
    pub count: i64,
    pub mean_days: Option<f64>,
    pub median_days: Option<f64>,
    /// The 85th percentile (nearest rank).
    pub p85_days: Option<f64>,
    /// Each item, in the order of its done time.
    pub items: Vec<FlowCycleTimeItem>,
}

/// One done item and its cycle time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FlowCycleTimeItem {
    pub short_code: String,
    pub title: String,
    /// RFC 3339.
    pub started_at: String,
    /// RFC 3339.
    pub done_at: String,
    pub days: f64,
}

/// The throughput of one week.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FlowWeek {
    /// The Monday of the week, `YYYY-MM-DD`.
    pub week_start: String,
    /// The items that went to a done column in the week (not cancelled).
    pub done: i64,
    /// The tasks cancelled in the week.
    pub cancelled: i64,
}

/// One item in progress, and its age.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FlowAgeingItem {
    pub short_code: String,
    pub title: String,
    /// The column of the item now.
    pub column: String,
    /// The start of the item (RFC 3339).
    pub started_at: String,
    /// The time from the start to now, in days.
    pub age_days: f64,
}

/// The number of live items in each column at the end of each day of the
/// range (UTC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CumulativeFlow {
    /// The columns: those of the board in position order, then an old name
    /// of a column that the history has (a column that has a new name now),
    /// in name order. An old name is in the list only when it has an item
    /// on a day of the range (KAIROS-T-0627).
    pub columns: Vec<String>,
    /// One row for each day; `counts` has the order of `columns`.
    pub days: Vec<CumulativeFlowDay>,
}

/// One day of the cumulative flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CumulativeFlowDay {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub counts: Vec<i64>,
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
            claims: false,
            keeps_claims: false,
            removed_at: None,
            wip_limit: None,
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

// ---------------------------------------------------------------------------
// Retention (KAIROS-T-0631)
// ---------------------------------------------------------------------------

/// `GET /api/org/dictionary` (KAIROS-T-0734, KAIROS-A-0079): the dictionary
/// of the organization. An organization has one dictionary, on the board
/// `strategy`; there are no team dictionaries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OrganizationDictionary {
    /// The short code of the dictionary document; `null` when the
    /// organization has no dictionary.
    pub short_code: Option<String>,
    /// Each term, sorted by the word (case ignored).
    pub terms: Vec<DictionaryTerm>,
    /// The bad rows of the dictionary. A bad row gives no term.
    pub problems: Vec<DictionaryProblem>,
}

/// One term of [`OrganizationDictionary`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DictionaryTerm {
    /// The term: one word or more, as the dictionary writes it.
    pub word: String,
    pub part_of_speech: String,
    pub meaning: String,
}

/// One bad row of the dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DictionaryProblem {
    /// The 1-based line of the row in the document.
    pub line: u32,
    pub message: String,
}

/// `GET /api/org/retention`: the retention settings of the deployment and
/// the last retention pass. Never the path of the archive target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RetentionStatus {
    /// `archive`, `discard` or `off` (`KAIROS_RETENTION_MODE`).
    pub mode: String,
    /// The kind of `KAIROS_ARCHIVE_TARGET`: `filesystem` or `s3`. Absent
    /// when no target is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_target: Option<String>,
    /// What a pass does: `prunes`, `off`, `no_target` (it removes no row)
    /// or `target_not_implemented` (each pass fails).
    pub status: String,
    /// The text for an operator when the pass removes nothing or fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub history_hot_days: u32,
    pub history_keep_latest: u32,
    pub activity_retention_days: u32,
    /// The record of the last pass that ended: `skipped: no target`, `off`
    /// or the counts. Absent before the first pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_result: Option<String>,
    /// When the last pass ended, RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
}

// ---------------------------------------------------------------------------
// The job queue (KAIROS-T-0576)
// ---------------------------------------------------------------------------

/// The jobs of one kind in each state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct JobKindCounts {
    /// The kind of job: `hosted_build`, `webhook_delivery`, `periodic`, and
    /// the others.
    pub kind: String,
    /// The jobs that wait for a server.
    pub queued: i64,
    /// The jobs that a server runs now.
    pub leased: i64,
    /// The jobs that ended well and are kept for a time.
    pub done: i64,
    /// The jobs that failed and are kept for a time.
    pub failed: i64,
}

/// A job that failed, with its error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct JobFailure {
    /// The id of the job (a UUID).
    pub id: String,
    pub kind: String,
    /// The slug of the organization; absent for a job of the deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub tries: i32,
    pub max_tries: i32,
    /// The reason of the failure, cut to
    /// [`JOB_ERROR_LIMIT`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When the job failed, RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

/// The number of failures in a [`JobQueueStatus`], at most.
pub const JOB_FAILURE_LIMIT: usize = 20;

/// The characters of the error of a [`JobFailure`], at most.
pub const JOB_ERROR_LIMIT: usize = 500;

/// `GET /api/admin/jobs` (KAIROS-T-0576): the job queue of the
/// organization and of the deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct JobQueueStatus {
    /// The jobs of each kind in each state, by kind. A finished job stays
    /// for 7 days.
    pub kinds: Vec<JobKindCounts>,
    /// The failed jobs, newest first, at most [`JOB_FAILURE_LIMIT`].
    pub failures: Vec<JobFailure>,
}
