//! Repository models (KAIROS-T-0103, decision KAIROS-A-0019).
//!
//! A repository is the unit a ticket is issued against and executed in.
//! Boards and delivery streams stay the planning unit; every repository
//! has EXACTLY ONE owning team. Ownership does not choose the board of a
//! task: a board or a team does, and a task on any board may link to any
//! repository (COLLIERY-T-0217, COLLIERY-A-0023). Until then a task filed
//! against a repo was routed repo -> team -> the team's delivery board.
//! Webhook wiring is a separate row ([`super::forge::ForgeConnection`])
//! hanging off the repository; a repository may have none.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::Forge;
use super::teams::Team;
use crate::schema::repositories;

/// One repository (`repositories`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = repositories)]
#[diesel(belongs_to(Team))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Repository {
    pub id: Uuid,
    /// Tenant-unique, URL-safe handle — what agents and the CLI address.
    pub slug: String,
    pub forge: Forge,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    pub repo_url: String,
    pub default_branch: String,
    /// The one owning team (A-0019).
    pub team_id: Uuid,
    /// Short "how to work here" blurb agents read before starting.
    pub description: String,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// `on` or `off` (KAIROS-T-0318). With `off`, the code index builder
    /// does nothing for the repository. See [`CODE_INDEX_BUILD_VALUES`].
    pub code_index_build: String,
    /// `embedded`, `hosted` (KAIROS-T-0340) or `organization`
    /// (KAIROS-T-0358). With `hosted`, the summaries of the repository come
    /// from the provider of the tenant, and its code leaves the host. With
    /// `organization`, the default of the organization decides. See
    /// [`CODE_INDEX_SUMMARIES_VALUES`] and [`Repository::summaries_with`].
    pub code_index_summaries: String,
}

/// The values of [`Repository::code_index_build`]. The column has a CHECK
/// with the same values.
pub const CODE_INDEX_BUILD_VALUES: [&str; 2] = ["on", "off"];

/// The values of [`Repository::code_index_summaries`]. The column has a
/// CHECK with the same values.
pub const CODE_INDEX_SUMMARIES_VALUES: [&str; 3] = ["embedded", "hosted", "organization"];

impl Repository {
    /// Whether the code index builder works on this repository
    /// (KAIROS-T-0318).
    pub fn code_index_build_on(&self) -> bool {
        self.code_index_build != "off"
    }

    /// Where the summaries of this repository are made, `embedded` or
    /// `hosted`: its own value, or `default` (the `default_summaries` of the
    /// organization) when it follows the organization (KAIROS-T-0358).
    pub fn summaries_with<'a>(&'a self, default: &'a str) -> &'a str {
        match self.code_index_summaries.as_str() {
            "organization" => default,
            own => own,
        }
    }
}

/// Insert for [`Repository`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = repositories)]
pub struct NewRepository {
    pub slug: String,
    pub forge: Forge,
    pub repo_full_name: String,
    pub repo_url: String,
    pub default_branch: String,
    pub team_id: Uuid,
    pub description: String,
    pub created_by: Uuid,
    pub updated_by: Uuid,
}

/// Partial update for [`Repository`]. Repo identity (`forge`,
/// `repo_full_name`) is fixed for a row's lifetime; everything a human
/// would edit on the admin page is here.
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = repositories)]
pub struct RepositoryChangeset {
    pub slug: Option<String>,
    pub repo_url: Option<String>,
    pub default_branch: Option<String>,
    pub team_id: Option<Uuid>,
    pub description: Option<String>,
    /// `on` or `off` (KAIROS-T-0318).
    pub code_index_build: Option<String>,
    /// `embedded`, `hosted` or `organization` (KAIROS-T-0340, KAIROS-T-0358).
    pub code_index_summaries: Option<String>,
    pub updated_by: Option<Uuid>,
    pub updated_at: Option<DateTime<Utc>>,
}
