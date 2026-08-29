//! Team-page content models (KAIROS-T-0082, design in KAIROS-I-0007):
//! the per-team folder tree of markdown pages, its version history, and
//! one-way announcements. A DEDICATED construct — deliberately not the
//! documents table: team pages authorize by team membership with
//! tenant-wide reads, documents by their parent item's board.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::TeamPageKind;
use crate::schema::{team_announcements, team_page_history, team_pages};

/// One node of a team's page tree (`team_pages`): a folder or a markdown
/// page. `is_protected` marks the Charter — content-editable, never
/// renamed/moved/deleted.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = team_pages)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TeamPage {
    pub id: Uuid,
    pub team_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: TeamPageKind,
    pub slug: String,
    pub title: String,
    pub content: String,
    pub position: i32,
    pub is_protected: bool,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`TeamPage`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = team_pages)]
pub struct NewTeamPage {
    pub team_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: TeamPageKind,
    pub slug: String,
    pub title: String,
    pub content: String,
    pub position: i32,
    pub is_protected: bool,
    pub created_by: Uuid,
    pub updated_by: Uuid,
}

/// Partial update for [`TeamPage`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = team_pages)]
pub struct TeamPageChangeset {
    pub parent_id: Option<Option<Uuid>>,
    pub slug: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub position: Option<i32>,
    pub version: Option<i32>,
    pub updated_by: Option<Uuid>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// One version snapshot of a team page (`team_page_history`, append-only;
/// complete from birth — v1 baseline written at creation, the A-0004
/// posture).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = team_page_history)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TeamPageHistory {
    pub id: Uuid,
    pub page_id: Uuid,
    pub version: i32,
    pub title: String,
    pub content: String,
    pub edited_by: Uuid,
    pub edited_at: DateTime<Utc>,
}

/// Insert for [`TeamPageHistory`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = team_page_history)]
pub struct NewTeamPageHistory {
    pub page_id: Uuid,
    pub version: i32,
    pub title: String,
    pub content: String,
    pub edited_by: Uuid,
}

/// A one-way team announcement (`team_announcements`, append-only by
/// convention: no updated_at, no edit path — the KAIROS-I-0007 "no
/// comments" decision).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = team_announcements)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TeamAnnouncement {
    pub id: Uuid,
    pub team_id: Uuid,
    pub body: String,
    pub pinned: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Insert for [`TeamAnnouncement`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = team_announcements)]
pub struct NewTeamAnnouncement {
    pub team_id: Uuid,
    pub body: String,
    pub pinned: bool,
    pub created_by: Uuid,
}
