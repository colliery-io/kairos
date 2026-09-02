//! Git-forge connection and link models (KAIROS-T-0097, design in
//! KAIROS-I-0009).
//!
//! [`ItemLink`] rows are DERIVED data mirrored from GitHub/GitLab — Kairos
//! is not the author — so unlike every item table there is no `version`,
//! no history, and no A-0004 optimistic concurrency. `forge_updated_at`
//! is the ordering authority instead: webhooks retry and arrive out of
//! order, and the guarded upsert uses it to keep a redelivered "opened"
//! from regressing a merged pull request.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::{Forge, LinkKind, LinkState};
use crate::schema::{forge_connections, item_links};

/// One repository wired up to this tenant (`forge_connections`). The
/// webhook secret is NOT stored: it is derived from the deployment
/// signing key and this row's id, so rotating means minting a new
/// connection.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = forge_connections)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ForgeConnection {
    pub id: Uuid,
    pub forge: Forge,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub repo_full_name: String,
    pub repo_url: String,
    /// Optional attribution so repo-level activity can roll up to a team
    /// even when a work item carries no `team_id` (KAIROS-T-0101).
    pub team_id: Option<Uuid>,
    pub created_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`ForgeConnection`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = forge_connections)]
pub struct NewForgeConnection {
    pub forge: Forge,
    pub repo_full_name: String,
    pub repo_url: String,
    pub team_id: Option<Uuid>,
    pub created_by: Uuid,
}

/// Partial update for [`ForgeConnection`] (team attribution is the only
/// editable field; repo identity is fixed for a connection's lifetime).
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = forge_connections)]
pub struct ForgeConnectionChangeset {
    pub team_id: Option<Option<Uuid>>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// One branch or pull/merge request associated with a work item
/// (`item_links`). `item_id` carries no FK — item ids span the five
/// entity tables in one shared UUID space (the `item_metadata`
/// convention).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = item_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ItemLink {
    pub id: Uuid,
    pub item_id: Uuid,
    pub connection_id: Uuid,
    pub kind: LinkKind,
    /// PR/MR number, or the branch ref.
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub state: LinkState,
    pub author: String,
    /// The forge's own last-update timestamp — the ordering guard.
    pub forge_updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`ItemLink`] (also the upsert payload).
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = item_links)]
pub struct NewItemLink {
    pub item_id: Uuid,
    pub connection_id: Uuid,
    pub kind: LinkKind,
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub state: LinkState,
    pub author: String,
    pub forge_updated_at: DateTime<Utc>,
}
