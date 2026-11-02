//! Admin data layer (KAIROS-T-0043): endpoint wrappers + mirror DTOs for
//! the T-0019 org families (boards/columns/transitions/board members,
//! teams, delivery streams, org members) and the T-0020 meta families
//! (templates, metadata definitions).
//!
//! Follows the conventions data-layer rules: every call goes through the
//! `crate::api` `*_json` helpers (envelope-aware error mapping, 401 hook);
//! mirrors are partial on purpose and each carries a `mirror of:` line so
//! drift stays greppable. Mutation responses the views never read are
//! decoded as `serde_json::Value` — refetch is the source of truth.

use aurora_dark::tokens::ApiError;
use serde::Deserialize;
use serde_json::{Value, json};

pub use crate::api::Refusal;
use crate::api::{
    delete_json, get_json, patch_json, patch_json_refusal, post_empty, post_json,
    post_json_refusal, put_json_refusal,
};
use crate::auth::Auth;

// ---------------------------------------------------------------------------
// Boards + configuration (columns, transitions)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::Board` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub board_level: String,
    pub team_id: Option<String>,
    /// The short-code prefix of the board (COLLIERY-T-3099).
    #[serde(default)]
    pub code_prefix: String,
    /// The board policy `parent_gate` of a delivery board (KAIROS-T-0641);
    /// `None` on each other level.
    #[serde(default)]
    pub parent_gate: Option<bool>,
    /// The board policy `owner_gate` of an initiative board (KAIROS-T-0643);
    /// `None` on each other level.
    #[serde(default)]
    pub owner_gate: Option<bool>,
    /// The board policy `blockers_gate` of a delivery board (KAIROS-T-0565);
    /// `None` on each other level.
    #[serde(default)]
    pub blockers_gate: Option<bool>,
}

/// mirror of: `kairos_client::types_org::BoardColumn` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumn {
    pub id: String,
    pub name: String,
    pub position: i32,
    /// Occupants count as completed for children-progress rollups
    /// (KAIROS-T-0080).
    #[serde(default)]
    pub is_done: bool,
    /// The WIP limit of the column (KAIROS-T-0371); `None`: no limit.
    #[serde(default)]
    pub wip_limit: Option<i32>,
    /// A move into the column gives the mover the claim of a free task
    /// (KAIROS-T-0359).
    #[serde(default)]
    pub claims: bool,
    /// A task keeps its claim in the column, and gets none (KAIROS-T-0529).
    #[serde(default)]
    pub keeps_claims: bool,
}

/// A flag of a column that the admin page sets with a switch
/// (KAIROS-T-0587). `field` is the name in `UpdateColumnRequest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnFlag {
    Claims,
    KeepsClaims,
}

impl ColumnFlag {
    /// The field of the flag in the body of the column update.
    pub fn field(self) -> &'static str {
        match self {
            ColumnFlag::Claims => "claims",
            ColumnFlag::KeepsClaims => "keeps_claims",
        }
    }

    /// The value of the flag on `column`.
    pub fn of(self, column: &BoardColumn) -> bool {
        match self {
            ColumnFlag::Claims => column.claims,
            ColumnFlag::KeepsClaims => column.keeps_claims,
        }
    }
}

/// The body that sets ONE flag of a column, and nothing else. Pure,
/// host-tested.
pub fn column_flag_body(flag: ColumnFlag, on: bool) -> Value {
    let mut body = serde_json::Map::new();
    body.insert(flag.field().to_string(), Value::Bool(on));
    Value::Object(body)
}

/// `PATCH /api/boards/{id}/columns/{col_id}` with one flag
/// (KAIROS-T-0587): `claims` or `keeps_claims`.
pub async fn set_column_flag(
    auth: Auth,
    board_id: &str,
    column_id: &str,
    flag: ColumnFlag,
    on: bool,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/boards/{board_id}/columns/{column_id}"),
        &column_flag_body(flag, on),
    )
    .await
}

/// A policy of a board (KAIROS-A-0040, KAIROS-A-0042, KAIROS-A-0053).
/// A delivery board has two policies, an initiative board one, each other
/// level none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardPolicy {
    /// Delivery boards: a task moves into a claims column only while its
    /// initiative is active.
    Parent,
    /// Initiative boards: an initiative moves to Active only with an owner.
    Owner,
    /// Delivery boards: a task with an open blocker does not move into a
    /// claims column or a done column (KAIROS-T-0565).
    Blockers,
}

impl BoardPolicy {
    /// The policies of a board of `level`, in the order of the page; empty
    /// for a level with no policy. Pure, host-tested.
    pub fn for_level(level: &str) -> &'static [BoardPolicy] {
        match level {
            "delivery" => &[BoardPolicy::Parent, BoardPolicy::Blockers],
            "initiative" => &[BoardPolicy::Owner],
            _ => &[],
        }
    }

    /// The value of the policy on a new board: `blockers_gate` is off
    /// (KAIROS-A-0053), the other two are on.
    pub fn default_on(self) -> bool {
        !matches!(self, BoardPolicy::Blockers)
    }

    /// The field of the policy in the board and in `UpdateBoardRequest`.
    pub fn field(self) -> &'static str {
        match self {
            BoardPolicy::Parent => "parent_gate",
            BoardPolicy::Owner => "owner_gate",
            BoardPolicy::Blockers => "blockers_gate",
        }
    }

    /// The value on `board`. The server sends it for the level of the
    /// policy; a board with no value reads as the default of the policy.
    pub fn of(self, board: &Board) -> bool {
        match self {
            BoardPolicy::Parent => board.parent_gate,
            BoardPolicy::Owner => board.owner_gate,
            BoardPolicy::Blockers => board.blockers_gate,
        }
        .unwrap_or(self.default_on())
    }
}

/// The body that sets ONE policy of a board. Pure, host-tested.
pub fn board_policy_body(policy: BoardPolicy, on: bool) -> Value {
    let mut body = serde_json::Map::new();
    body.insert(policy.field().to_string(), Value::Bool(on));
    Value::Object(body)
}

/// `PATCH /api/boards/{id}` with one policy (KAIROS-T-0587). The server
/// refuses a policy of a different level with a 422.
pub async fn set_board_policy(
    auth: Auth,
    board_id: &str,
    policy: BoardPolicy,
    on: bool,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/boards/{board_id}"),
        &board_policy_body(policy, on),
    )
    .await
}

/// mirror of: `kairos_client::types_org::BoardTransition` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardTransition {
    pub id: String,
    pub from_column_id: String,
    pub to_column_id: String,
}

/// mirror of: `kairos_client::types_org::BoardDetail` (partial; the board
/// fields arrive flattened at the top level, exactly like the wire shape).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardDetail {
    #[serde(flatten)]
    pub board: Board,
    pub columns: Vec<BoardColumn>,
    pub transitions: Vec<BoardTransition>,
}

/// `GET /api/boards`: each live board, page after page (COLLIERY-T-0257).
/// The page of the boards counts the delivery boards of a team, so it
/// reads the full list.
pub async fn list_boards(auth: Auth) -> Result<Vec<Board>, ApiError> {
    crate::api::get_all(auth, "/api/boards").await
}

/// `POST /api/boards` — seeded with the level's default columns/transitions.
///
/// A refusal keeps the field that the server names, for the form
/// (COLLIERY-T-0265).
pub async fn create_board(
    auth: Auth,
    name: &str,
    slug: &str,
    board_level: &str,
    team_id: Option<&str>,
    code_prefix: &str,
) -> Result<Board, Refusal> {
    post_json_refusal(
        auth,
        "/api/boards",
        &json!({
            "name": name,
            "slug": slug,
            "board_level": board_level,
            "team_id": team_id,
            "code_prefix": code_prefix,
        }),
    )
    .await
}

/// `DELETE /api/boards/{id}` (422 `BOARD_NOT_EMPTY` while items reference it).
pub async fn delete_board(auth: Auth, board_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/boards/{board_id}")).await
}

/// `GET /api/boards/{id}` — board + columns + transitions.
pub async fn board_detail(auth: Auth, board_id: &str) -> Result<BoardDetail, ApiError> {
    get_json(auth, &format!("/api/boards/{board_id}")).await
}

/// `POST /api/boards/{id}/columns` (422 `DUPLICATE_COLUMN_NAME` /
/// `DUPLICATE_COLUMN_POSITION`).
pub async fn add_column(
    auth: Auth,
    board_id: &str,
    name: &str,
    position: i32,
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/boards/{board_id}/columns"),
        &json!({ "name": name, "position": position }),
    )
    .await
}

/// `PATCH /api/boards/{id}/columns/{col_id}` — rename, move, and/or set
/// the done flag (KAIROS-T-0080).
pub async fn update_column(
    auth: Auth,
    board_id: &str,
    column_id: &str,
    name: Option<&str>,
    position: Option<i32>,
    is_done: Option<bool>,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/boards/{board_id}/columns/{column_id}"),
        &json!({ "name": name, "position": position, "is_done": is_done }),
    )
    .await
}

/// `PATCH /api/boards/{id}/columns/{col_id}` with `wip_limit` only
/// (KAIROS-T-0371): a number above 0, or `None` to remove the limit.
pub async fn set_column_wip_limit(
    auth: Auth,
    board_id: &str,
    column_id: &str,
    wip_limit: Option<i32>,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/boards/{board_id}/columns/{column_id}"),
        &json!({ "wip_limit": wip_limit }),
    )
    .await
}

/// `DELETE /api/boards/{id}/columns/{col_id}` (422 `COLUMN_NOT_EMPTY` while
/// items sit in the column).
pub async fn remove_column(auth: Auth, board_id: &str, column_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/boards/{board_id}/columns/{column_id}")).await
}

/// `POST /api/boards/{id}/transitions` (422 `DUPLICATE_TRANSITION` /
/// `VALIDATION` for self-transitions).
pub async fn add_transition(
    auth: Auth,
    board_id: &str,
    from_column_id: &str,
    to_column_id: &str,
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/boards/{board_id}/transitions"),
        &json!({ "from_column_id": from_column_id, "to_column_id": to_column_id }),
    )
    .await
}

/// `DELETE /api/boards/{id}/transitions/{transition_id}`.
pub async fn remove_transition(
    auth: Auth,
    board_id: &str,
    transition_id: &str,
) -> Result<Value, ApiError> {
    delete_json(
        auth,
        &format!("/api/boards/{board_id}/transitions/{transition_id}"),
    )
    .await
}

// ---------------------------------------------------------------------------
// Board members + capability grants (KAIROS-A-0006)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::BoardMember`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardMember {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    /// The user name (KAIROS-T-0665). An older server does not send it.
    #[serde(default)]
    pub user_name: String,
    pub capabilities: Vec<String>,
}

/// `GET /api/boards/{id}/members`.
pub async fn board_members(auth: Auth, board_id: &str) -> Result<Vec<BoardMember>, ApiError> {
    get_json(auth, &format!("/api/boards/{board_id}/members")).await
}

/// `POST /api/boards/{id}/members` — first grant(s) for a user.
pub async fn add_board_member(
    auth: Auth,
    board_id: &str,
    user_id: &str,
    capabilities: &[String],
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/boards/{board_id}/members"),
        &json!({ "user_id": user_id, "capabilities": capabilities }),
    )
    .await
}

/// `PATCH /api/boards/{id}/members/{user_id}` — replaces the full set.
pub async fn replace_capabilities(
    auth: Auth,
    board_id: &str,
    user_id: &str,
    capabilities: &[String],
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/boards/{board_id}/members/{user_id}"),
        &json!({ "capabilities": capabilities }),
    )
    .await
}

/// `DELETE /api/boards/{id}/members/{user_id}` — full revocation.
pub async fn remove_board_member(
    auth: Auth,
    board_id: &str,
    user_id: &str,
) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/boards/{board_id}/members/{user_id}")).await
}

// ---------------------------------------------------------------------------
// Teams (reads live in the shared team data layer — KAIROS-T-0067 hoisted
// them for the user-facing team pages; writes stay admin-only, here)
// ---------------------------------------------------------------------------

pub use crate::pages::teams::api::{Team, list_teams, team_members};

/// `POST /api/teams` — also creates the team's delivery board
/// (`{slug}-delivery`); the returned `delivery_board_id` names it.
///
/// A refusal keeps the field that the server names, for the form
/// (COLLIERY-T-0265).
pub async fn create_team(
    auth: Auth,
    name: &str,
    slug: &str,
    team_type: &str,
    code_prefix: &str,
) -> Result<Team, Refusal> {
    post_json_refusal(
        auth,
        "/api/teams",
        &json!({
            "name": name,
            "slug": slug,
            "team_type": team_type,
            "code_prefix": code_prefix,
        }),
    )
    .await
}

/// `PATCH /api/teams/{id}`. A refusal keeps the field that the server
/// names, for the form (COLLIERY-T-0265).
pub async fn update_team(
    auth: Auth,
    team_id: &str,
    name: &str,
    slug: &str,
    team_type: &str,
    site_enabled: bool,
) -> Result<Value, Refusal> {
    patch_json_refusal(
        auth,
        &format!("/api/teams/{team_id}"),
        &json!({
            "name": name,
            "slug": slug,
            "team_type": team_type,
            "site_enabled": site_enabled,
        }),
    )
    .await
}

/// `DELETE /api/teams/{id}`.
pub async fn delete_team(auth: Auth, team_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/teams/{team_id}")).await
}

/// `POST /api/teams/{id}/members`.
pub async fn add_team_member(auth: Auth, team_id: &str, user_id: &str) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/teams/{team_id}/members"),
        &json!({ "user_id": user_id }),
    )
    .await
}

/// `DELETE /api/teams/{id}/members/{user_id}`.
pub async fn remove_team_member(
    auth: Auth,
    team_id: &str,
    user_id: &str,
) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/teams/{team_id}/members/{user_id}")).await
}

// ---------------------------------------------------------------------------
// Delivery streams (reads shared with the team pages, same hoist)
// ---------------------------------------------------------------------------

pub use crate::pages::teams::api::{DeliveryStream, list_streams, stream_teams};

/// `POST /api/delivery-streams`. A refusal keeps the field that the server
/// names, for the form (COLLIERY-T-0265).
pub async fn create_stream(
    auth: Auth,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> Result<Value, Refusal> {
    post_json_refusal(
        auth,
        "/api/delivery-streams",
        &json!({ "name": name, "slug": slug, "description": description }),
    )
    .await
}

/// `PATCH /api/delivery-streams/{id}`. A refusal keeps the field that the
/// server names, for the form (COLLIERY-T-0265).
pub async fn update_stream(
    auth: Auth,
    stream_id: &str,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> Result<Value, Refusal> {
    patch_json_refusal(
        auth,
        &format!("/api/delivery-streams/{stream_id}"),
        &json!({ "name": name, "slug": slug, "description": description }),
    )
    .await
}

/// `DELETE /api/delivery-streams/{id}`.
pub async fn delete_stream(auth: Auth, stream_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/delivery-streams/{stream_id}")).await
}

/// `POST /api/delivery-streams/{id}/teams`.
pub async fn add_stream_team(
    auth: Auth,
    stream_id: &str,
    team_id: &str,
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/delivery-streams/{stream_id}/teams"),
        &json!({ "team_id": team_id }),
    )
    .await
}

/// `DELETE /api/delivery-streams/{id}/teams/{team_id}`.
pub async fn remove_stream_team(
    auth: Auth,
    stream_id: &str,
    team_id: &str,
) -> Result<Value, ApiError> {
    delete_json(
        auth,
        &format!("/api/delivery-streams/{stream_id}/teams/{team_id}"),
    )
    .await
}

// ---------------------------------------------------------------------------
// The grants of one person on many boards (KAIROS-T-0413)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::UserBoardGrants`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct UserBoardGrants {
    pub board_id: String,
    pub board_slug: String,
    pub board_name: String,
    pub board_level: String,
    /// The capabilities that apply at the level of the board.
    pub applicable: Vec<String>,
    /// The stored grants of the person on the board (globs too).
    pub capabilities: Vec<String>,
}

/// mirror of: `kairos_client::types_org::UserTeam`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct UserTeam {
    pub team_id: String,
    pub slug: String,
    pub name: String,
}

/// mirror of: `kairos_client::types_org::UserGrants`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct UserGrants {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub boards: Vec<UserBoardGrants>,
    /// `None` when the viewer cannot change team members.
    pub teams: Option<Vec<UserTeam>>,
}

/// mirror of: `kairos_client::types_org::BoardGrantChange`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BoardGrantChange {
    pub board_id: String,
    pub grant: Vec<String>,
    pub revoke: Vec<String>,
}

/// `GET /api/members/{user_id}/grants`: the grants of one person on each
/// board that the viewer administers, and the teams of the person.
pub async fn user_grants(auth: Auth, user_id: &str) -> Result<UserGrants, ApiError> {
    get_json(auth, &format!("/api/members/{user_id}/grants")).await
}

/// `PATCH /api/members/{user_id}/grants`: the changed cells, in one
/// transaction.
pub async fn change_user_grants(
    auth: Auth,
    user_id: &str,
    changes: &[BoardGrantChange],
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/members/{user_id}/grants"),
        &json!({ "changes": changes }),
    )
    .await
}

// ---------------------------------------------------------------------------
// Organization members
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::OrgMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct OrgMember {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
}

/// `GET /api/members`: each member, page after page (COLLIERY-T-0258).
pub async fn list_org_members(auth: Auth) -> Result<Vec<OrgMember>, ApiError> {
    crate::api::get_all(auth, "/api/members").await
}

/// `POST /api/members` — resolve by email (users are JIT-provisioned at
/// first login; unknown email → 404 with the "log in once first" guidance).
pub async fn add_org_member(auth: Auth, email: &str, role: &str) -> Result<Value, ApiError> {
    post_json(
        auth,
        "/api/members",
        &json!({ "email": email, "role": role }),
    )
    .await
}

/// `PATCH /api/members/{user_id}` — role change (422 `LAST_ADMIN` when
/// demoting the only admin).
pub async fn set_org_member_role(auth: Auth, user_id: &str, role: &str) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/members/{user_id}"),
        &json!({ "role": role }),
    )
    .await
}

/// `DELETE /api/members/{user_id}` (422 `LAST_ADMIN` for the only admin).
pub async fn remove_org_member(auth: Auth, user_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/members/{user_id}")).await
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_meta::Template` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub is_system_default: bool,
}

/// mirror of: `kairos_client::types_meta::TemplateMetadataField` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateMetadataField {
    pub slug: String,
    pub default_value: Option<String>,
    pub required: bool,
}

/// mirror of: `kairos_client::types_meta::TemplateDetail` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateDetail {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub content: String,
    pub is_system_default: bool,
    pub metadata: Vec<TemplateMetadataField>,
}

/// One template ↔ definition association in a template write (the
/// `metadata` entry of `CreateTemplateRequest`/`UpdateTemplateRequest`).
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateMetadataEntry {
    pub definition_slug: String,
    pub default_value: Option<String>,
    pub required: bool,
}

fn metadata_entries_json(entries: &[TemplateMetadataEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| {
            json!({
                "definition_slug": entry.definition_slug,
                "default_value": entry.default_value,
                "required": entry.required,
            })
        })
        .collect()
}

/// `GET /api/templates`: each template, page after page (COLLIERY-T-0258).
pub async fn list_templates(auth: Auth) -> Result<Vec<Template>, ApiError> {
    crate::api::get_all(auth, "/api/templates").await
}

/// `GET /api/templates/{id}` — template + its metadata associations.
pub async fn template_detail(auth: Auth, template_id: &str) -> Result<TemplateDetail, ApiError> {
    get_json(auth, &format!("/api/templates/{template_id}")).await
}

/// `POST /api/templates`.
pub async fn create_template(
    auth: Auth,
    name: &str,
    slug: &str,
    content: &str,
    metadata: &[TemplateMetadataEntry],
) -> Result<Value, ApiError> {
    post_json(
        auth,
        "/api/templates",
        &json!({
            "name": name,
            "slug": slug,
            "content": content,
            "metadata": metadata_entries_json(metadata),
        }),
    )
    .await
}

/// `PATCH /api/templates/{id}` — `metadata` replaces the association list.
pub async fn update_template(
    auth: Auth,
    template_id: &str,
    name: &str,
    slug: &str,
    content: &str,
    metadata: &[TemplateMetadataEntry],
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/templates/{template_id}"),
        &json!({
            "name": name,
            "slug": slug,
            "content": content,
            "metadata": metadata_entries_json(metadata),
        }),
    )
    .await
}

/// `DELETE /api/templates/{id}` (hard delete).
pub async fn delete_template(auth: Auth, template_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/templates/{template_id}")).await
}

// ---------------------------------------------------------------------------
// Metadata definitions
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_meta::MetadataDefinition` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MetadataDefinition {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub field_type: String,
    pub is_system_default: bool,
    pub enum_options: Vec<String>,
}

/// `GET /api/metadata-definitions`: each definition, page after page
/// (COLLIERY-T-0258).
pub async fn list_definitions(auth: Auth) -> Result<Vec<MetadataDefinition>, ApiError> {
    crate::api::get_all(auth, "/api/metadata-definitions").await
}

/// `POST /api/metadata-definitions` — `enum_options` required non-empty for
/// `enum`, forbidden otherwise (server-validated, 422 `VALIDATION`).
pub async fn create_definition(
    auth: Auth,
    name: &str,
    slug: &str,
    field_type: &str,
    enum_options: &[String],
) -> Result<Value, ApiError> {
    post_json(
        auth,
        "/api/metadata-definitions",
        &json!({
            "name": name,
            "slug": slug,
            "field_type": field_type,
            "enum_options": enum_options,
        }),
    )
    .await
}

/// `PATCH /api/metadata-definitions/{id}` — `enum_options` replaces the
/// full option list (enum definitions only).
pub async fn update_definition(
    auth: Auth,
    definition_id: &str,
    name: &str,
    slug: &str,
    enum_options: Option<&[String]>,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/metadata-definitions/{definition_id}"),
        &json!({ "name": name, "slug": slug, "enum_options": enum_options }),
    )
    .await
}

/// `DELETE /api/metadata-definitions/{id}` (hard delete).
pub async fn delete_definition(auth: Auth, definition_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/metadata-definitions/{definition_id}")).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_import_says_its_state() {
        let mut import = super::IssueImport {
            state: "waiting".into(),
            waiting_until: Some("2026-10-15T04:00:00+00:00".into()),
            pages: 3,
            result: super::IssueImportResult {
                message: "Kairos imported 297 issue(s).".into(),
            },
            error: None,
        };
        assert!(import.open());
        assert!(import.summary().contains("rate limit"));
        assert!(import.summary().contains("2026-10-15T04:00:00"));
        import.state = "done".into();
        assert!(!import.open());
        assert!(import.summary().contains("done. Kairos imported 297"));
        import.state = "failed".into();
        import.error = Some("GitHub refused the read token.".into());
        assert!(import.summary().contains("GitHub refused"));
    }

    use super::*;

    /// `BoardDetail` decodes the wire shape: board fields flattened at the
    /// top level next to `columns`/`transitions` (field-name lock).
    #[test]
    fn board_detail_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "0b7c9a44-9c7e-4f4e-8a3f-2f5b1a9d0e11",
            "name": "Platform Delivery",
            "slug": "platform-delivery",
            "board_level": "delivery",
            "team_id": "0d1e2f3a-4b5c-6d7e-8f90-a1b2c3d4e5f6",
            "created_at": "2026-07-14T00:00:00Z",
            "updated_at": "2026-07-14T00:00:00Z",
            "columns": [
                {"id": "c1", "board_id": "b", "name": "Backlog", "position": 0,
                 "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"}
            ],
            "transitions": [
                {"id": "t1", "board_id": "b", "from_column_id": "c1", "to_column_id": "c2"}
            ]
        });
        let detail: BoardDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(detail.board.slug, "platform-delivery");
        assert_eq!(detail.columns[0].name, "Backlog");
        assert_eq!(detail.transitions[0].to_column_id, "c2");
    }

    /// `BoardMember` decodes the grants list (capability editor input).
    #[test]
    fn board_member_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "user_id": "u1",
            "email": "bob@kairos.test",
            "display_name": "bob",
            "capabilities": ["manage_tasks", "transition_items"]
        });
        let member: BoardMember = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(member.capabilities.len(), 2);
    }

    // (the Team mirror's field-name lock moved to `pages::teams::api` with
    // the type — KAIROS-T-0067 hoist)

    /// `MetadataDefinition` decodes enum options in order.
    #[test]
    fn definition_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "d1", "name": "Priority", "slug": "priority",
            "field_type": "enum", "is_system_default": true,
            "enum_options": ["low", "medium", "high"],
            "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"
        });
        let definition: MetadataDefinition = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(definition.enum_options, vec!["low", "medium", "high"]);
    }

    /// `TemplateDetail` decodes the metadata association rows.
    #[test]
    fn template_detail_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "tp1", "name": "PRD", "slug": "prd",
            "content": "# PRD\n", "is_system_default": false,
            "metadata": [
                {"definition_id": "d1", "slug": "priority", "name": "Priority",
                 "field_type": "enum", "enum_options": ["low", "high"],
                 "default_value": "low", "required": true}
            ],
            "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"
        });
        let detail: TemplateDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(detail.metadata[0].slug, "priority");
        assert!(detail.metadata[0].required);
    }
}

// ---------------------------------------------------------------------------
// Repositories + forge connections (KAIROS-T-0109, A-0019)
// ---------------------------------------------------------------------------

pub use crate::pages::repositories::api::{Repository, list_repositories};

/// mirror of: `kairos_client::types_forge::CreatedForgeConnection` (partial —
/// the fields shown once at connect time).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CreatedForgeConnection {
    pub id: String,
    pub webhook_url: String,
    pub webhook_secret: String,
}

/// mirror of: `kairos_client::types_repositories::RepositoryDetail`
/// (partial — the connection id).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RepositoryDetail {
    #[serde(default)]
    pub connection_id: Option<String>,
}

/// `POST /api/repositories`. A refusal keeps the field that the server
/// names, for the form (COLLIERY-T-0267).
#[allow(clippy::too_many_arguments)]
pub async fn create_repository(
    auth: Auth,
    slug: Option<&str>,
    forge: &str,
    repo_full_name: &str,
    repo_url: &str,
    team: &str,
    default_branch: Option<&str>,
    description: Option<&str>,
) -> Result<Repository, Refusal> {
    post_json_refusal(
        auth,
        "/api/repositories",
        &json!({
            "slug": slug,
            "forge": forge,
            "repo_full_name": repo_full_name,
            "repo_url": repo_url,
            "team": team,
            "default_branch": default_branch,
            "description": description,
        }),
    )
    .await
}

/// The fields of a `PATCH /api/repositories/{slug}`. `None` sends no field.
#[derive(Debug, Clone, Copy, Default)]
pub struct RepositoryEdit<'a> {
    pub slug: Option<&'a str>,
    pub repo_url: Option<&'a str>,
    pub default_branch: Option<&'a str>,
    /// `forge` clears the branch set by hand (KAIROS-T-0430).
    pub default_branch_source: Option<&'a str>,
    pub team: Option<&'a str>,
    pub description: Option<&'a str>,
    /// `on` or `off` (KAIROS-T-0318).
    pub code_index_build: Option<&'a str>,
    /// `embedded` or `hosted` (KAIROS-T-0340).
    pub code_index_summaries: Option<&'a str>,
    /// The site switch (KAIROS-T-0408).
    pub site_enabled: Option<bool>,
    /// The forge filters (KAIROS-T-0479): field → (include, exclude).
    pub forge_filters: Option<&'a [(&'static str, String, String)]>,
    /// `off`, or `15m`, `1h`, `1d` (KAIROS-T-0670).
    pub issue_import_interval: Option<&'a str>,
}

/// `PATCH /api/repositories/{slug}` — every field optional; `team` re-homes.
/// A refusal keeps the field that the server names, for the form
/// (COLLIERY-T-0267).
pub async fn update_repository(
    auth: Auth,
    reference: &str,
    edit: RepositoryEdit<'_>,
) -> Result<Repository, Refusal> {
    let mut body = serde_json::Map::new();
    for (key, value) in [
        ("slug", edit.slug),
        ("repo_url", edit.repo_url),
        ("default_branch", edit.default_branch),
        ("default_branch_source", edit.default_branch_source),
        ("team", edit.team),
        ("description", edit.description),
        ("code_index_build", edit.code_index_build),
        ("code_index_summaries", edit.code_index_summaries),
        ("issue_import_interval", edit.issue_import_interval),
    ] {
        if let Some(value) = value {
            body.insert(key.to_string(), Value::String(value.to_string()));
        }
    }
    if let Some(on) = edit.site_enabled {
        body.insert("site_enabled".to_string(), Value::Bool(on));
    }
    if let Some(filters) = edit.forge_filters {
        let mut map = serde_json::Map::new();
        for (field, include, exclude) in filters {
            map.insert(
                field.to_string(),
                json!({ "include": include, "exclude": exclude }),
            );
        }
        body.insert("forge_filters".to_string(), Value::Object(map));
    }
    patch_json_refusal(
        auth,
        &format!("/api/repositories/{reference}"),
        &Value::Object(body),
    )
    .await
}

/// mirror of: `kairos_client::types_repositories::IssueImportResult`
/// (partial — the sentence for the page, KAIROS-T-0483).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct IssueImportResult {
    pub message: String,
}

/// mirror of: `kairos_client::types_repositories::IssueImport` (partial,
/// KAIROS-T-0487): an import of the open issues, a job of the server.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct IssueImport {
    pub state: String,
    pub waiting_until: Option<String>,
    pub pages: usize,
    pub result: IssueImportResult,
    pub error: Option<String>,
}

impl IssueImport {
    /// True while the job waits or runs.
    pub fn open(&self) -> bool {
        matches!(self.state.as_str(), "queued" | "running" | "waiting")
    }

    /// The line of the page. Pure, host-tested.
    pub fn summary(&self) -> String {
        match self.state.as_str() {
            "queued" => "Import of open issues: queued.".to_string(),
            "running" => format!(
                "Import of open issues: running, {} page(s) read. {}",
                self.pages, self.result.message
            ),
            "waiting" => format!(
                "Import of open issues: waiting for the rate limit of the forge until {}. {}",
                self.waiting_until.as_deref().unwrap_or("later"),
                self.result.message
            ),
            "failed" => format!(
                "Import of open issues: failed. {} {}",
                self.error.as_deref().unwrap_or(""),
                self.result.message
            ),
            _ => format!("Import of open issues: done. {}", self.result.message),
        }
    }
}

/// `POST /api/repositories/{slug}/import-issues` — start the import of the
/// open issues (org admin, KAIROS-T-0483); the answer is the job
/// (KAIROS-T-0487).
pub async fn import_issues(auth: Auth, reference: &str) -> Result<IssueImport, ApiError> {
    crate::api::post_empty(
        auth,
        &format!("/api/repositories/{reference}/import-issues"),
    )
    .await
}

/// `GET /api/repositories/{slug}/import-issues` — the last import of the
/// open issues (KAIROS-T-0487); `None` when no import ran (404).
pub async fn issue_import(auth: Auth, reference: &str) -> Result<Option<IssueImport>, ApiError> {
    match get_json(
        auth,
        &format!("/api/repositories/{reference}/import-issues"),
    )
    .await
    {
        Ok(import) => Ok(Some(import)),
        Err(ApiError::Http { status: 404, .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `DELETE /api/repositories/{slug}` (409 while referenced).
pub async fn delete_repository(auth: Auth, reference: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/repositories/{reference}")).await
}

/// `GET /api/repositories/{slug}` — for the connection id.
pub async fn repository_detail(auth: Auth, reference: &str) -> Result<RepositoryDetail, ApiError> {
    get_json(auth, &format!("/api/repositories/{reference}")).await
}

/// `POST /api/forge-connections` — the secret is shown ONCE.
pub async fn connect_webhook(
    auth: Auth,
    repository: &str,
) -> Result<CreatedForgeConnection, ApiError> {
    post_json(
        auth,
        "/api/forge-connections",
        &json!({ "repository": repository }),
    )
    .await
}

pub use crate::pages::repositories::api::RepositoryCredential;

/// `PUT /api/repositories/{slug}/credential` — set or replace the read
/// token (COLLIERY-T-3105). The response is the status, with no token. A
/// refusal keeps the field that the server names (`token`).
pub async fn set_repository_credential(
    auth: Auth,
    reference: &str,
    token: &str,
) -> Result<RepositoryCredential, Refusal> {
    put_json_refusal(
        auth,
        &format!("/api/repositories/{reference}/credential"),
        &json!({ "token": token }),
    )
    .await
}

/// `DELETE /api/repositories/{slug}/credential` — remove the read token.
pub async fn remove_repository_credential(
    auth: Auth,
    reference: &str,
) -> Result<RepositoryCredential, ApiError> {
    delete_json(auth, &format!("/api/repositories/{reference}/credential")).await
}

/// `PUT /api/repositories/{slug}/write-token` — set or replace the write
/// token of the GitHub write-back (KAIROS-T-0386). A 422 names the field.
pub async fn set_repository_write_token(
    auth: Auth,
    reference: &str,
    token: &str,
) -> Result<Value, Refusal> {
    put_json_refusal(
        auth,
        &format!("/api/repositories/{reference}/write-token"),
        &json!({ "token": token }),
    )
    .await
}

/// `DELETE /api/repositories/{slug}/write-token` — the write-back is off.
pub async fn remove_repository_write_token(auth: Auth, reference: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/repositories/{reference}/write-token")).await
}

/// `POST /api/repositories/{slug}/credential/check` — test the read token
/// with `git ls-remote` on the server.
pub async fn check_repository_credential(
    auth: Auth,
    reference: &str,
) -> Result<RepositoryCredential, ApiError> {
    post_empty(
        auth,
        &format!("/api/repositories/{reference}/credential/check"),
    )
    .await
}

/// `DELETE /api/forge-connections/{id}` — disconnect (the repo stays).
pub async fn disconnect_webhook(auth: Auth, connection_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/forge-connections/{connection_id}")).await
}

/// `POST /api/forge-connections/{id}/rotate` (KAIROS-T-0587): a new
/// connection takes the place of the old one. The webhook URL and the
/// secret change, and the response shows them once.
pub async fn rotate_webhook(
    auth: Auth,
    connection_id: &str,
) -> Result<CreatedForgeConnection, ApiError> {
    post_empty(
        auth,
        &format!("/api/forge-connections/{connection_id}/rotate"),
    )
    .await
}

#[cfg(test)]
mod flag_and_policy_tests {
    use super::*;

    #[test]
    fn a_column_flag_body_sets_one_flag_only() {
        assert_eq!(
            column_flag_body(ColumnFlag::Claims, true),
            json!({ "claims": true })
        );
        assert_eq!(
            column_flag_body(ColumnFlag::KeepsClaims, false),
            json!({ "keeps_claims": false })
        );
    }

    #[test]
    fn the_column_mirror_reads_both_claim_flags() {
        let wire = json!({
            "id": "c", "name": "Blocked", "position": 4, "is_done": false,
            "wip_limit": null, "claims": false, "keeps_claims": true
        });
        let column: BoardColumn = serde_json::from_value(wire).expect("parses");
        assert!(!ColumnFlag::Claims.of(&column));
        assert!(ColumnFlag::KeepsClaims.of(&column));
        // An older server sends neither flag: both read as off.
        let old: BoardColumn =
            serde_json::from_value(json!({ "id": "c", "name": "Todo", "position": 1 }))
                .expect("parses");
        assert!(!old.claims && !old.keeps_claims);
    }

    #[test]
    fn each_level_has_its_policies_or_none() {
        assert_eq!(
            BoardPolicy::for_level("delivery"),
            &[BoardPolicy::Parent, BoardPolicy::Blockers]
        );
        assert_eq!(BoardPolicy::for_level("initiative"), &[BoardPolicy::Owner]);
        assert!(BoardPolicy::for_level("strategy").is_empty());
        assert!(BoardPolicy::for_level("adr").is_empty());
    }

    #[test]
    fn a_policy_body_sets_one_policy_only() {
        assert_eq!(
            board_policy_body(BoardPolicy::Parent, false),
            json!({ "parent_gate": false })
        );
        assert_eq!(
            board_policy_body(BoardPolicy::Owner, true),
            json!({ "owner_gate": true })
        );
        assert_eq!(
            board_policy_body(BoardPolicy::Blockers, true),
            json!({ "blockers_gate": true })
        );
    }

    #[test]
    fn the_board_mirror_reads_the_policy_of_its_level() {
        let wire = json!({
            "id": "b", "name": "Delivery", "slug": "delivery", "board_level": "delivery",
            "team_id": "t", "code_prefix": "DEL", "parent_gate": false, "owner_gate": null
        });
        let board: Board = serde_json::from_value(wire).expect("parses");
        assert!(!BoardPolicy::Parent.of(&board));
        // No value reads as on, the default of the server.
        assert!(BoardPolicy::Owner.of(&board));
        // An older server sends no blockers_gate: it reads as off, its default.
        assert!(!BoardPolicy::Blockers.of(&board));

        let wire = json!({
            "id": "b", "name": "Delivery", "slug": "delivery", "board_level": "delivery",
            "team_id": "t", "code_prefix": "DEL", "parent_gate": true, "owner_gate": null,
            "blockers_gate": true
        });
        let board: Board = serde_json::from_value(wire).expect("parses");
        assert!(BoardPolicy::Blockers.of(&board));
        assert!(BoardPolicy::Parent.of(&board));
    }
}
