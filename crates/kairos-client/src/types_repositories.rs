//! Repository DTOs (KAIROS-T-0104 / KAIROS-T-0106, decision KAIROS-A-0019):
//! where the code is. Every repository has exactly one owning team; a task
//! links to at most one repository, of any team (COLLIERY-A-0023).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The repository a task is bound to, embedded on [`super::types::Task`]
/// so cards and agents get the slug without a second read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryRef {
    /// Repository id (UUID).
    pub id: String,
    /// Tenant-unique slug — what agents and the CLI address.
    pub slug: String,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    /// Owning team (UUID).
    pub team_id: String,
}

/// Body of `PUT /api/tasks/{short_code}/repository` — set the repository
/// the task links to (slug or UUID), or clear it with `null` or an empty
/// string (COLLIERY-T-0231). It can be any
/// live repository, of any team. The board and the team of the task do not
/// change (COLLIERY-T-0217, COLLIERY-A-0023).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTaskRepositoryRequest {
    #[serde(default)]
    pub repository: Option<String>,
}

/// One `impacts` link of a document or of an ADR (COLLIERY-T-0269): a
/// repository that the item is about. The link gives no right on the item
/// and no right on the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Impact {
    /// Always `impacts`.
    pub relationship: String,
    /// Always `repository`. A later version can have other target kinds.
    pub target_kind: String,
    /// The repository.
    pub repository: ImpactedRepository,
    /// When the link was made, RFC 3339.
    pub created_at: String,
}

/// The repository of an [`Impact`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImpactedRepository {
    /// Repository id (UUID).
    pub id: String,
    /// The slug of the repository.
    pub slug: String,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    /// When the repository was archived, RFC 3339. Absent while the
    /// repository is live. The link stays when the repository is archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Body of `POST /api/{entity_type}/{short_code}/impacts`
/// (COLLIERY-T-0269): the repository that the item impacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateImpactRequest {
    /// A live repository of the organization, by slug or UUID. It can be
    /// the repository of each team.
    pub repository: String,
}

/// Response of `GET /api/{entity_type}/{short_code}/impacts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemImpactsResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The `impacts` links of the item, by the slug of the repository.
    pub impacts: Vec<Impact>,
}

/// Response of `DELETE /api/{entity_type}/{short_code}/impacts/{repository}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeletedImpactResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The slug of the repository that the item does not impact now.
    pub repository: String,
}

/// One document or ADR that impacts a repository (COLLIERY-T-0269), in
/// [`RepositoryDetail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImpactingItem {
    /// The short code of the item.
    pub short_code: String,
    pub title: String,
    /// `document` or `adr`.
    pub entity_type: String,
    /// The value of the metadata `document_type` (`vision`,
    /// `architecture`, ...). Null for an ADR, and for a document that has
    /// no document type.
    pub document_type: Option<String>,
    /// The editorial lifecycle of a document:
    /// `draft|review|published|archived`. Null for an ADR.
    pub lifecycle: Option<String>,
    /// The name of the column of an ADR. Null for a document, and for an
    /// ADR that is not on a board.
    pub column: Option<String>,
    /// When the item was archived, RFC 3339. Absent while the item is
    /// live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// Query of `GET /api/repositories/{slug}` (COLLIERY-T-0269).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::IntoParams,
)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct RepositoryDetailQuery {
    /// Include the archived documents and ADRs in `impacted_by`, each
    /// marked with `archived_at`. Default false.
    #[serde(default)]
    pub include_deleted: bool,
}

/// One repository, as returned by `/api/repositories` (KAIROS-T-0106).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Repository {
    /// Repository id (UUID).
    pub id: String,
    /// Tenant-unique slug (`^[a-z0-9][a-z0-9-]{1,62}$`).
    pub slug: String,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    /// Browser URL of the repository.
    pub repo_url: String,
    pub default_branch: String,
    /// Short "how to work here" blurb agents read before starting.
    pub description: String,
    /// The one owning team.
    pub team: RepositoryTeam,
    /// The delivery board of the owning team (UUID). It is a fact about
    /// the owner, not where tasks go: the board or the team of a task
    /// decides that (COLLIERY-T-0219, COLLIERY-A-0023). `None` only in a
    /// misconfigured tenant (team without a delivery board).
    pub delivery_board_id: Option<String>,
    /// Live tasks that link to this repository and are not in a done
    /// column, on all boards of all teams (COLLIERY-T-0219).
    pub open_tasks: i64,
    /// Whether a live webhook connection exists for it.
    pub has_webhook: bool,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// The owning team, embedded on [`Repository`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryTeam {
    /// Team id (UUID).
    pub id: String,
    pub slug: String,
    pub name: String,
}

/// `GET /api/repositories/{slug}`: the repository plus its webhook
/// connection (id only — secrets are never re-read) and in-flight links.
///
/// Until COLLIERY-T-0219 this also carried `stale_tasks`: linked tasks on a
/// board of a team that does not own the repository. COLLIERY-A-0023 makes
/// that normal work, so the field is removed. This is a wire change: a
/// client that reads the field finds it absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryDetail {
    #[serde(flatten)]
    pub repository: Repository,
    /// The live forge connection's id, if any.
    pub connection_id: Option<String>,
    /// In-flight (`open`, `draft`) branches and pull requests on this
    /// repository, newest first, each with the work item it belongs to.
    pub in_flight: Vec<crate::types_forge::TeamLink>,
    /// The documents and the ADRs that impact this repository
    /// (COLLIERY-T-0269): the documents first, by short code. Live items
    /// only, unless the request has `include_deleted=true`. Read them
    /// before you work in the repository.
    #[serde(default)]
    pub impacted_by: Vec<ImpactingItem>,
}

/// Body of `POST /api/repositories`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRepositoryRequest {
    /// Tenant-unique slug; defaults to one derived from `repo_full_name`
    /// (`acme/payments-api` → `acme-payments-api`).
    #[serde(default)]
    pub slug: Option<String>,
    /// `github|gitlab|other`.
    pub forge: String,
    /// The name on the forge — must match what the forge sends in webhook
    /// payloads. `github`: 2 parts (`owner/repo`). `gitlab`: 2 or more
    /// parts (`group/subgroup/project`). `other`: 1 or more parts. No
    /// space, no `.git` at the end, 255 characters at most.
    pub repo_full_name: String,
    /// Browser URL of the repository: an absolute `http` or `https` URL
    /// with a host, no space, and no user name or password (each member
    /// can read it). 2048 characters at most.
    pub repo_url: String,
    /// A branch name that git accepts, 255 characters at most. Defaults to
    /// `main`.
    #[serde(default)]
    pub default_branch: Option<String>,
    /// The owning team (UUID or slug). Exactly one (KAIROS-A-0019).
    pub team: String,
    /// Short "how to work here" blurb for agents.
    #[serde(default)]
    pub description: Option<String>,
}

/// Body of `PATCH /api/repositories/{slug}` — every field optional;
/// `team` re-homes the repository. The tasks that link to it do not
/// change: each stays on its board and keeps its link (COLLIERY-T-0219,
/// COLLIERY-A-0023). A field with the value of the repository changes
/// nothing. The server does not examine the form of that value
/// (COLLIERY-T-0267).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRepositoryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// The form is that of `repo_url` in `CreateRepositoryRequest`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    /// The form is that of `default_branch` in `CreateRepositoryRequest`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// New owning team (UUID or slug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
