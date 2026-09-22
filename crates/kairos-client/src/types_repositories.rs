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
