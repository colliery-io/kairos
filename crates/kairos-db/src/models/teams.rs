//! Models for the tenant organizational tables (KAIROS-S-0004): teams,
//! team membership, and delivery streams.
//!
//! All tables here are tenant-schema tables: unqualified in `schema.rs`,
//! resolved through the connection's pinned `search_path`
//! (`org_{slug}, public`; see [`crate::pool`]).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::TeamType;
use crate::schema::{delivery_streams, team_delivery_streams, team_members, teams};

// ---------------------------------------------------------------------------
// teams
// ---------------------------------------------------------------------------

/// A delivery team (`teams`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = teams)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub team_type: TeamType,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`Team`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = teams)]
pub struct NewTeam {
    pub name: String,
    pub slug: String,
    pub team_type: TeamType,
}

/// Partial update for [`Team`]. `deleted_at` is double-`Option`:
/// `Some(Some(..))` soft-deletes, `Some(None)` restores.
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = teams)]
pub struct TeamChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub team_type: Option<TeamType>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// team_members
// ---------------------------------------------------------------------------

/// Team membership (`team_members`, composite PK). `user_id` references
/// `public.users` (cross-schema, enforced at the application level).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = team_members)]
#[diesel(primary_key(team_id, user_id))]
#[diesel(belongs_to(Team))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TeamMember {
    pub team_id: Uuid,
    pub user_id: Uuid,
    pub joined_at: DateTime<Utc>,
}

/// Insert for [`TeamMember`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = team_members)]
pub struct NewTeamMember {
    pub team_id: Uuid,
    pub user_id: Uuid,
}

/// Partial update for [`TeamMember`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = team_members)]
pub struct TeamMemberChangeset {
    pub joined_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// delivery_streams
// ---------------------------------------------------------------------------

/// A delivery stream (`delivery_streams`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = delivery_streams)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DeliveryStream {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`DeliveryStream`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = delivery_streams)]
pub struct NewDeliveryStream {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
}

/// Partial update for [`DeliveryStream`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = delivery_streams)]
pub struct DeliveryStreamChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<Option<String>>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// team_delivery_streams
// ---------------------------------------------------------------------------

/// Team-to-delivery-stream membership (`team_delivery_streams`, composite
/// PK). Pure join table: both columns are the key, so there is no changeset
/// struct — membership changes are insert/delete.
#[derive(
    Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations, Insertable,
)]
#[diesel(table_name = team_delivery_streams)]
#[diesel(primary_key(team_id, delivery_stream_id))]
#[diesel(belongs_to(Team))]
#[diesel(belongs_to(DeliveryStream))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TeamDeliveryStream {
    pub team_id: Uuid,
    pub delivery_stream_id: Uuid,
}
