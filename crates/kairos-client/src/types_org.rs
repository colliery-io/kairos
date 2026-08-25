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
use utoipa::ToSchema;

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
    /// Owning team (UUID); set for delivery boards (KAIROS-A-0002).
    pub team_id: Option<String>,
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
    #[serde(default)]
    pub is_done: bool,
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
pub struct CreateBoardRequest {
    pub name: String,
    pub slug: String,
    /// `strategy|initiative|delivery|adr`.
    pub board_level: String,
    /// Owning team (UUID) for delivery boards.
    #[serde(default)]
    pub team_id: Option<String>,
}

/// Body of `PATCH /api/boards/{id}` (board settings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateBoardRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
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

/// Response of `GET /api/boards/{id}/items`: all items on the board,
/// grouped by column (columns in position order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BoardItemsResponse {
    pub board: Board,
    pub columns: Vec<BoardColumnItems>,
    /// `(done, total)` direct-children counts keyed by the PARENT item's
    /// short code, for every item on this board that has children
    /// (KAIROS-T-0080) — computed in one grouped query, never per item.
    #[serde(default)]
    pub children_progress: std::collections::BTreeMap<String, ProgressCounts>,
}

/// A `(done, total)` children rollup (KAIROS-T-0080). `done` counts the
/// children sitting in `is_done` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProgressCounts {
    pub done: i64,
    pub total: i64,
    /// False when no board hosting the children has a done-flagged
    /// column — clients show composition only, never a done fraction.
    #[serde(default)]
    pub has_done: bool,
}

/// Body of `POST /api/boards/{id}/columns`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateColumnRequest {
    pub name: String,
    /// 0-indexed position; must not collide with an existing column
    /// (422 `DUPLICATE_COLUMN_POSITION`).
    pub position: i32,
}

/// Body of `PATCH /api/boards/{id}/columns/{col_id}` — rename, move,
/// and/or set the done flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
pub struct CreateTeamRequest {
    pub name: String,
    pub slug: String,
    /// `stream_aligned|platform|enabling|complicated_subsystem`; defaults
    /// to `stream_aligned`.
    #[serde(default)]
    pub team_type: Option<String>,
}

/// Body of `PATCH /api/teams/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
pub struct CreateStreamRequest {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Body of `PATCH /api/delivery-streams/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
pub struct AddOrgMemberRequest {
    pub email: String,
    /// `admin|member`; defaults to `member`.
    #[serde(default)]
    pub role: Option<String>,
}

/// Body of `PATCH /api/members/{user_id}` — role change. Demoting the last
/// admin is rejected (422 `LAST_ADMIN`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
