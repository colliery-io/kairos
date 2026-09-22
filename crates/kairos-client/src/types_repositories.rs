//! Repository DTOs (KAIROS-T-0104 / KAIROS-T-0106, decision KAIROS-A-0019):
//! the unit a ticket is issued against and executed in. Every repository
//! has exactly one owning team; a task binds to at most one repository.

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

/// Body of `PUT /api/tasks/{short_code}/repository` — bind the task to a
/// repository (slug or UUID), or clear it with `null`. The repository
/// must be owned by the team whose delivery board the task sits on.
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
    /// The owning team's delivery board (UUID) — where tasks filed
    /// against this repository land. `None` only in a misconfigured
    /// tenant (team without a delivery board).
    pub delivery_board_id: Option<String>,
    /// Live tasks bound to this repository that are not in a done column.
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
/// `team` re-homes the repository (existing tasks are untouched; the
/// repo → team → board rule is re-checked on their next write).
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
