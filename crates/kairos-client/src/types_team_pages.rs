//! Team-page and announcement DTOs (KAIROS-T-0083, design in
//! KAIROS-I-0007): the per-team folder tree of markdown pages and the
//! one-way announcements feed. Writes require team membership or org
//! admin; reads are open tenant-wide.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One node of a team's page tree, as returned by
/// `GET /api/teams/{id}/pages` (a FLAT list — clients nest by
/// `parent_id`, ordered by position then title).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamPage {
    /// Page id (UUID).
    pub id: String,
    /// Owning team (UUID).
    pub team_id: String,
    /// Parent folder (UUID); `null` for roots.
    pub parent_id: Option<String>,
    /// `folder|page`.
    pub kind: String,
    /// URL segment, unique among siblings.
    pub slug: String,
    pub title: String,
    /// Markdown content (empty for folders).
    pub content: String,
    /// Display order among siblings.
    pub position: i32,
    /// The Charter: content-editable, never renamed/moved/deleted.
    pub is_protected: bool,
    /// Optimistic-concurrency version (content edits only).
    pub version: i32,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// Body of `POST /api/teams/{id}/pages`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateTeamPageRequest {
    /// Parent FOLDER (UUID); omit for a root node.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// `folder|page`.
    pub kind: String,
    /// URL segment, unique among siblings.
    pub slug: String,
    pub title: String,
    /// Markdown content (pages only); defaults to empty.
    #[serde(default)]
    pub content: String,
    /// Display order among siblings; defaults to 0.
    #[serde(default)]
    pub position: i32,
}

/// Body of `PATCH /api/teams/{id}/pages/{page_id}` — EITHER a
/// version-checked content edit (`content` present, `version` required)
/// OR a structural rename/move (`slug`/`parent_id`/`position`); mixing
/// both in one call is a 422.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateTeamPageRequest {
    /// New title (content edits only).
    #[serde(default)]
    pub title: Option<String>,
    /// New markdown content; requires `version`.
    #[serde(default)]
    pub content: Option<String>,
    /// The version this edit is based on (KAIROS-A-0004 pattern; 409 with
    /// `details.current` when stale).
    #[serde(default)]
    pub version: Option<i32>,
    /// New sibling slug (rename).
    #[serde(default)]
    pub slug: Option<String>,
    /// New parent folder id (move). JSON `null` cannot distinguish
    /// "unchanged" from "move to root" — set `move_to_root` for the
    /// latter.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Move the node to the tree root (`parent_id` ignored when set).
    #[serde(default)]
    pub move_to_root: bool,
    /// New display position.
    #[serde(default)]
    pub position: Option<i32>,
}

/// One team announcement (append-only; no edit anywhere).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamAnnouncement {
    /// Announcement id (UUID).
    pub id: String,
    /// Owning team (UUID).
    pub team_id: String,
    /// Markdown body.
    pub body: String,
    /// Pinned announcements sort first.
    pub pinned: bool,
    /// Author user id (UUID).
    pub created_by: String,
    /// RFC 3339.
    pub created_at: String,
}

/// One row of `GET /api/teams/{id}/work-documents` (KAIROS-T-0084): a
/// live document attached to the team's WORK — its `supports` parent is a
/// task of the team or an item on the team's delivery board. Documents
/// under org-level items deliberately do not appear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamWorkDocument {
    /// Document short code (links to `/items/{code}`).
    pub short_code: String,
    pub title: String,
    /// `draft|published|adopted|superseded` (KAIROS-T-0080).
    pub lifecycle: String,
    /// The supports-parent item, for attribution.
    pub parent_short_code: String,
    pub parent_title: String,
    /// `strategy|initiative|task`.
    pub parent_type: String,
}

/// Body of `POST /api/teams/{id}/announcements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateTeamAnnouncementRequest {
    /// Markdown body.
    pub body: String,
    /// Pin it to the top of the feed.
    #[serde(default)]
    pub pinned: bool,
}
