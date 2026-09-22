//! Repository models (KAIROS-T-0103, decision KAIROS-A-0019).
//!
//! A repository is the unit a ticket is issued against and executed in.
//! Boards and delivery streams stay the planning unit; every repository
//! has EXACTLY ONE owning team, and that ownership is how a task filed
//! against a repo is routed (repo -> team -> the team's delivery board).
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
    pub updated_by: Option<Uuid>,
    pub updated_at: Option<DateTime<Utc>>,
}
