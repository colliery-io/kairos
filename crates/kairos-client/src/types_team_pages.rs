//! Team-page and announcement DTOs (KAIROS-T-0083, design in
//! KAIROS-I-0007): the per-team folder tree of markdown pages and the
//! one-way announcements feed. Writes require team membership or org
//! admin; reads are open tenant-wide.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// One node of a team's page tree with its content, as returned by
/// `GET /api/teams/{id}/pages/{page_id}` and by each write. The list of
/// the tree has [`TeamPageSummary`] rows, with no content (KAIROS-T-0629).
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

/// One node of a team's page tree without its content, as returned by
/// `GET /api/teams/{id}/pages` (a FLAT list — clients nest by
/// `parent_id`, ordered by position then title). Read the content with
/// `GET /api/teams/{id}/pages/{page_id}` (KAIROS-T-0629).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamPageSummary {
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

impl From<&TeamPage> for TeamPageSummary {
    fn from(page: &TeamPage) -> Self {
        TeamPageSummary {
            id: page.id.clone(),
            team_id: page.team_id.clone(),
            parent_id: page.parent_id.clone(),
            kind: page.kind.clone(),
            slug: page.slug.clone(),
            title: page.title.clone(),
            position: page.position,
            is_protected: page.is_protected,
            version: page.version,
            created_at: page.created_at.clone(),
            updated_at: page.updated_at.clone(),
        }
    }
}

/// Body of `POST /api/teams/{id}/pages`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
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

/// One row of `GET /api/teams/{id}/work-documents` (KAIROS-T-0084,
/// KAIROS-T-0393): a live document of the team. A board of the team owns
/// it (`owned`), or it supports a task of the team or an item on the
/// delivery board of the team (`parent_*`). Each row has one reason or
/// both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamWorkDocument {
    /// Document short code (links to `/items/{code}`).
    pub short_code: String,
    pub title: String,
    /// `draft|published|adopted|superseded` (KAIROS-T-0080).
    pub lifecycle: String,
    /// A board of the team is the owner board of the document.
    pub owned: bool,
    /// The supports-parent item of the team's work, for attribution;
    /// `null` when it supports no item of the team's work.
    pub parent_short_code: Option<String>,
    pub parent_title: Option<String>,
    /// `strategy|initiative|task`.
    pub parent_type: Option<String>,
}

/// `GET /api/teams/{id}/dictionary` (KAIROS-T-0734): the dictionary of the
/// team. The organization entries come first. A team entry wins for the
/// same word (case ignored, KAIROS-T-0733).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamDictionary {
    /// The short code of the organization dictionary; `null` when there
    /// is none.
    pub organization: Option<String>,
    /// The short code of the dictionary of the team; `null` when there is
    /// none.
    pub team: Option<String>,
    /// Each term, sorted by the word (case ignored).
    pub terms: Vec<DictionaryTerm>,
    /// The bad rows of the two dictionaries, the organization first. A bad
    /// row gives no term.
    pub problems: Vec<DictionaryProblem>,
}

/// One term of [`TeamDictionary`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DictionaryTerm {
    /// The term: one word or more, as the dictionary writes it.
    pub word: String,
    pub part_of_speech: String,
    pub meaning: String,
    /// `organization|team`: the dictionary that gives the term.
    pub layer: String,
    /// The short code of the dictionary that gives the term.
    pub document: String,
}

/// One bad row of a dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DictionaryProblem {
    /// The short code of the dictionary.
    pub document: String,
    /// The 1-based line of the row in the document.
    pub line: u32,
    pub message: String,
}

/// Body of `POST /api/teams/{id}/announcements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTeamAnnouncementRequest {
    /// Markdown body.
    pub body: String,
    /// Pin it to the top of the feed.
    #[serde(default)]
    pub pinned: bool,
}

/// One archived node of a team (KAIROS-T-0439): `GET
/// /api/teams/{id}/archived-pages`. Restore it with `POST
/// /api/teams/{id}/pages/{page_id}/restore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ArchivedTeamPage {
    /// The node as it was archived (its parent may be gone), without its
    /// content (KAIROS-T-0629).
    #[serde(flatten)]
    pub page: TeamPageSummary,
    /// RFC 3339. Nodes archived together (a folder and its contents) have
    /// the same value.
    pub archived_at: String,
}

/// One row of the version list of a page (KAIROS-T-0439, KAIROS-T-0629):
/// `GET /api/teams/{id}/pages/{page_id}/history`, newest first. It has no
/// content: `?version=N` gives one version in full ([`TeamPageVersion`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamPageVersionSummary {
    pub version: i32,
    /// The title at this version.
    pub title: String,
    /// The user id (UUID) of the person who saved it.
    pub edited_by: String,
    /// The display name of that person.
    pub edited_by_name: String,
    /// RFC 3339.
    pub edited_at: String,
    /// The agent key (UUID) of the request that saved it, when an agent saved
    /// it with the agent key of the person: show "Alice (agent)". Null
    /// otherwise.
    #[serde(default)]
    pub agent_key_id: Option<String>,
}

/// One saved version of a page in full (KAIROS-T-0439): `GET
/// /api/teams/{id}/pages/{page_id}/history?version=N`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamPageVersion {
    pub version: i32,
    pub title: String,
    pub content: String,
    /// The user id (UUID) of the person who saved it.
    pub edited_by: String,
    /// The display name of that person.
    pub edited_by_name: String,
    /// RFC 3339.
    pub edited_at: String,
    /// The agent key (UUID) of the request that saved it, when an agent saved
    /// it with the agent key of the person: show "Alice (agent)". Null
    /// otherwise.
    #[serde(default)]
    pub agent_key_id: Option<String>,
}

/// One document or ADR in the tree of a team (KAIROS-T-0437): `GET
/// /api/teams/{id}/tree-items`. Each document and ADR whose owner board is a
/// board of the team. The board owns it; this is where it sits in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamTreeItem {
    /// The item id (UUID).
    pub item_id: String,
    pub short_code: String,
    /// `document` or `adr`.
    pub entity_type: String,
    pub title: String,
    /// The lifecycle of a document, or the column of an ADR.
    pub status: String,
    /// Its folder (a team page id); null is the root.
    pub parent_id: Option<String>,
    /// Its position among its siblings; null when nobody placed it (it shows
    /// at the end of the root).
    pub position: Option<i32>,
}

/// Body of `PUT /api/teams/{id}/tree-items/{item}` (KAIROS-T-0437).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceTeamItemRequest {
    /// The folder (a team page id); null or absent puts it at the root.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Its position among its siblings.
    pub position: i32,
}

/// Query of `GET /api/teams/{id}/pages/{page_id}/history` (KAIROS-T-0629).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct TeamPageHistoryQuery {
    /// Return this version in full instead of the version list.
    #[serde(default)]
    pub version: Option<i32>,
    /// Page size for the version list (default 50, max 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}
