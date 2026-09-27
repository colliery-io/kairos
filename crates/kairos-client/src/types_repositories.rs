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
/// the task links to (slug or UUID), or clear it with `null`. It can be any
/// live repository, of any team. The board and the team of the task do not
/// change (COLLIERY-T-0217, COLLIERY-A-0023).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SetTaskRepositoryRequest {
    #[serde(default)]
    pub repository: Option<String>,
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
}

/// Body of `POST /api/repositories`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateRepositoryRequest {
    /// Tenant-unique slug; defaults to one derived from `repo_full_name`
    /// (`acme/payments-api` → `acme-payments-api`).
    #[serde(default)]
    pub slug: Option<String>,
    /// `github|gitlab|other`.
    pub forge: String,
    /// `owner/repo` — must match what the forge sends in webhook payloads.
    pub repo_full_name: String,
    /// Browser URL of the repository.
    pub repo_url: String,
    /// Defaults to `main`.
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
/// COLLIERY-A-0023).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateRepositoryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// New owning team (UUID or slug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
