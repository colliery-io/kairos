//! Shared team data layer (KAIROS-T-0067, KAIROS-I-0006): the team/stream
//! read wrappers and mirror DTOs used by BOTH the user-facing team pages
//! (`/teams`, `/teams/:slug`) and the admin surfaces (`pages/admin/api.rs`
//! re-exports these — one copy of the fetch code, per the initiative's
//! "no duplicated fetch code" AC).
//!
//! Reads only: every endpoint here is member-readable (the MANAGE
//! capability gates writes alone, verified in
//! `kairos-server/src/api/org/teams.rs`). Team/stream WRITES stay in
//! `pages/admin/api.rs` — admin remains the only write surface.
//!
//! Conventions per docs/gui-conventions.md: calls go through the
//! `crate::api` `*_json` helpers; mirrors are partial on purpose and carry
//! a `mirror of:` line so drift stays greppable.

use aurora_dark::tokens::ApiError;
use serde::{Deserialize, Serialize};

use crate::api::{delete_json, get_json, post_json};
use crate::auth::Auth;

/// mirror of: `kairos_client::types_org::Team` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub team_type: String,
    pub delivery_board_id: Option<String>,
    /// The docs site of the team is on (KAIROS-T-0408). An older server does
    /// not send it: the site is on by default.
    #[serde(default = "site_on")]
    pub site_enabled: bool,
}

fn site_on() -> bool {
    true
}

/// mirror of: `kairos_client::types_org::TeamMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TeamMember {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    /// The user name (KAIROS-T-0665). An older server does not send it.
    #[serde(default)]
    pub user_name: String,
}

/// mirror of: `kairos_client::types_org::DeliveryStream` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DeliveryStream {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
}

/// mirror of: `kairos_client::types_org::Board` (partial — enough to
/// resolve a team's `delivery_board_id` to a name + slug for linking).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardRef {
    pub id: String,
    pub name: String,
    pub slug: String,
}

/// `GET /api/teams`: each live team, page after page (COLLIERY-T-0257).
pub async fn list_teams(auth: Auth) -> Result<Vec<Team>, ApiError> {
    crate::api::get_all(auth, "/api/teams").await
}

/// `GET /api/teams/{id}/members`.
pub async fn team_members(auth: Auth, team_id: &str) -> Result<Vec<TeamMember>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/members")).await
}

/// `GET /api/delivery-streams`: each stream, page after page
/// (COLLIERY-T-0258).
pub async fn list_streams(auth: Auth) -> Result<Vec<DeliveryStream>, ApiError> {
    crate::api::get_all(auth, "/api/delivery-streams").await
}

/// `GET /api/delivery-streams/{id}/teams`.
pub async fn stream_teams(auth: Auth, stream_id: &str) -> Result<Vec<Team>, ApiError> {
    get_json(auth, &format!("/api/delivery-streams/{stream_id}/teams")).await
}

/// `GET /api/boards` — id/name/slug refs (delivery-board link resolution).
/// Each board, page after page (COLLIERY-T-0258).
pub async fn list_board_refs(auth: Auth) -> Result<Vec<BoardRef>, ApiError> {
    crate::api::get_all(auth, "/api/boards").await
}

/// `GET /api/teams/by-slug/{slug}` (KAIROS-T-0083) — kills the
/// fetch-all directory scan the detail page used before KAIROS-T-0085.
pub async fn team_by_slug(auth: Auth, slug: &str) -> Result<Team, ApiError> {
    get_json(auth, &format!("/api/teams/by-slug/{slug}")).await
}

/// mirror of: `kairos_client::types_team_pages::TeamPageSummary` (partial;
/// `updated_at` stays server-side). One page (`TeamPage`) also has its
/// content: the landing page needs the content of the charter.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TeamPageNode {
    pub id: String,
    pub parent_id: Option<String>,
    /// `folder|page`.
    pub kind: String,
    pub slug: String,
    pub title: String,
    /// Empty in the rows of the list: the list has no content
    /// (KAIROS-T-0629). [`team_page`] reads one page with its content.
    #[serde(default)]
    pub content: String,
    pub position: i32,
    pub is_protected: bool,
    pub version: i32,
    /// KAIROS-T-0437: set when the node is a document or an ADR of the team
    /// (made by [`item_nodes`]), not a team page.
    #[serde(skip)]
    pub item: Option<ItemRef>,
}

/// What a tree node needs to know about a document or an ADR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRef {
    pub short_code: String,
    /// `document` or `adr`.
    pub entity_type: String,
    /// The lifecycle of a document, or the column of an ADR.
    pub status: String,
}

/// The position of an item that nobody placed: after every placed node.
pub const UNPLACED: i32 = 1_000_000;

/// mirror of: `kairos_client::types_team_pages::TeamTreeItem`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TreeItem {
    pub item_id: String,
    pub short_code: String,
    pub entity_type: String,
    pub title: String,
    pub status: String,
    pub parent_id: Option<String>,
    pub position: Option<i32>,
}

/// The documents and ADRs of a team as tree nodes (KAIROS-T-0437).
pub fn item_nodes(items: &[TreeItem]) -> Vec<TeamPageNode> {
    items
        .iter()
        .map(|item| TeamPageNode {
            id: item.item_id.clone(),
            parent_id: item.parent_id.clone(),
            kind: item.entity_type.clone(),
            slug: item.short_code.clone(),
            title: item.title.clone(),
            content: String::new(),
            position: item.position.unwrap_or(UNPLACED),
            is_protected: false,
            version: 0,
            item: Some(ItemRef {
                short_code: item.short_code.clone(),
                entity_type: item.entity_type.clone(),
                status: item.status.clone(),
            }),
        })
        .collect()
}

/// `GET /api/teams/{id}/tree-items` — the documents and ADRs of a team with
/// their places in its tree.
pub async fn tree_items(auth: Auth, team_id: &str) -> Result<Vec<TreeItem>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/tree-items")).await
}

/// `PUT /api/teams/{id}/tree-items/{code}` — put a document or an ADR of the
/// team in a folder (`None`: the root) at a position.
pub async fn place_item(
    auth: Auth,
    team_id: &str,
    short_code: &str,
    parent: Option<&str>,
    position: i32,
) -> Result<TreeItem, ApiError> {
    #[derive(Serialize)]
    struct Body<'a> {
        parent_id: Option<&'a str>,
        position: i32,
    }
    crate::api::put_json(
        auth,
        &format!("/api/teams/{team_id}/tree-items/{short_code}"),
        &Body {
            parent_id: parent,
            position,
        },
    )
    .await
}

impl TeamPageNode {
    /// The charter of the team: the protected page at the top of the tree
    /// (KAIROS-T-0436). Found by its flag, not by its slug.
    pub fn is_charter(&self) -> bool {
        self.parent_id.is_none() && self.is_protected && self.kind == "page"
    }
}

/// `GET /api/teams/{id}/pages` — the flat page tree (nest by parent_id),
/// without content (KAIROS-T-0629).
pub async fn team_pages(auth: Auth, team_id: &str) -> Result<Vec<TeamPageNode>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/pages")).await
}

/// `GET /api/teams/{id}/pages/{page_id}` — one page with its content.
pub async fn team_page(auth: Auth, team_id: &str, page_id: &str) -> Result<TeamPageNode, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/pages/{page_id}")).await
}

/// `PATCH /api/teams/{id}/pages/{page_id}` — the A-0004 versioned
/// content save, through the shared 409-parsing helper so the
/// generalized editor gets its merge dialog (KAIROS-T-0086).
pub async fn save_page_content(
    auth: Auth,
    team_id: &str,
    page_id: &str,
    title: &str,
    content: &str,
    version: i32,
) -> Result<i32, crate::pages::item::api::SaveError> {
    #[derive(Serialize)]
    struct Body<'a> {
        title: &'a str,
        content: &'a str,
        version: i32,
    }
    let node: TeamPageNode = crate::pages::item::api::patch_versioned(
        auth,
        &format!("/api/teams/{team_id}/pages/{page_id}"),
        &Body {
            title,
            content,
            version,
        },
    )
    .await?;
    Ok(node.version)
}

/// mirror of: `kairos_client::types_team_pages::CreateTeamPageRequest`.
#[derive(Serialize)]
struct CreatePageBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<&'a str>,
    kind: &'a str,
    slug: &'a str,
    title: &'a str,
    content: &'a str,
    position: i32,
}

/// `POST /api/teams/{id}/pages` (team member or org admin). `position` is
/// its place among its siblings (KAIROS-T-0438: after the last one).
pub async fn create_page(
    auth: Auth,
    team_id: &str,
    parent_id: Option<&str>,
    kind: &str,
    slug: &str,
    title: &str,
    position: i32,
) -> Result<TeamPageNode, ApiError> {
    post_json(
        auth,
        &format!("/api/teams/{team_id}/pages"),
        &CreatePageBody {
            parent_id,
            kind,
            slug,
            title,
            content: "",
            position,
        },
    )
    .await
}

/// mirror of: `kairos_client::types_team_pages::UpdateTeamPageRequest`
/// (structure half — content edits go through [`save_page_content`]).
#[derive(Serialize)]
struct StructureBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<&'a str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    move_to_root: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<i32>,
}

/// `PATCH /api/teams/{id}/pages/{page_id}` — rename (new sibling slug).
pub async fn rename_page(
    auth: Auth,
    team_id: &str,
    page_id: &str,
    slug: &str,
) -> Result<TeamPageNode, ApiError> {
    crate::api::patch_json(
        auth,
        &format!("/api/teams/{team_id}/pages/{page_id}"),
        &StructureBody {
            slug: Some(slug),
            parent_id: None,
            move_to_root: false,
            position: None,
        },
    )
    .await
}

/// `PATCH /api/teams/{id}/pages/{page_id}` — place a node (KAIROS-T-0438):
/// `parent` `Some(Some(id))` moves it into a folder, `Some(None)` to the
/// root, `None` keeps the parent; `position` sets its place among its
/// siblings.
pub async fn place_page(
    auth: Auth,
    team_id: &str,
    page_id: &str,
    parent: Option<Option<&str>>,
    position: i32,
) -> Result<TeamPageNode, ApiError> {
    crate::api::patch_json(
        auth,
        &format!("/api/teams/{team_id}/pages/{page_id}"),
        &StructureBody {
            slug: None,
            parent_id: parent.flatten(),
            move_to_root: parent == Some(None),
            position: Some(position),
        },
    )
    .await
}

/// mirror of: `kairos_client::types_team_pages::ArchivedTeamPage`
/// (partial, KAIROS-T-0439).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ArchivedPage {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub archived_at: String,
}

/// `GET /api/teams/{id}/archived-pages` — newest archive first.
pub async fn archived_pages(auth: Auth, team_id: &str) -> Result<Vec<ArchivedPage>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/archived-pages")).await
}

/// `POST /api/teams/{id}/pages/{page_id}/restore` — the node and what was
/// archived with it.
pub async fn restore_page(
    auth: Auth,
    team_id: &str,
    page_id: &str,
) -> Result<TeamPageNode, ApiError> {
    crate::api::post_empty(
        auth,
        &format!("/api/teams/{team_id}/pages/{page_id}/restore"),
    )
    .await
}

/// mirror of: `kairos_client::types_team_pages::TeamPageVersionSummary`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PageVersion {
    pub version: i32,
    pub title: String,
    pub edited_by_name: String,
    pub edited_at: String,
    #[serde(default)]
    pub agent_key_id: Option<String>,
}

/// mirror of: `kairos_client::types_team_pages::TeamPageVersion` (the
/// fields that the page shows).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PageSnapshot {
    pub version: i32,
    pub title: String,
    pub content: String,
}

/// The most versions that the history panel lists: the maximum of the
/// route.
const HISTORY_LIMIT: i64 = 200;

/// `GET /api/teams/{id}/pages/{page_id}/history` — the newest versions
/// first, without content (KAIROS-T-0629).
pub async fn page_history(
    auth: Auth,
    team_id: &str,
    page_id: &str,
) -> Result<Vec<PageVersion>, ApiError> {
    let page: crate::pages::activity::ListEnvelope<PageVersion> = get_json(
        auth,
        &format!("/api/teams/{team_id}/pages/{page_id}/history?limit={HISTORY_LIMIT}"),
    )
    .await?;
    Ok(page.items)
}

/// `GET /api/teams/{id}/pages/{page_id}/history?version=N` — one version
/// with its content.
pub async fn page_version(
    auth: Auth,
    team_id: &str,
    page_id: &str,
    version: i32,
) -> Result<PageSnapshot, ApiError> {
    get_json(
        auth,
        &format!("/api/teams/{team_id}/pages/{page_id}/history?version={version}"),
    )
    .await
}

/// `DELETE /api/teams/{id}/pages/{page_id}` (soft; folders must be
/// empty — the server 422 carries the live-children count).
pub async fn delete_page(auth: Auth, team_id: &str, page_id: &str) -> Result<(), ApiError> {
    let _: serde_json::Value =
        delete_json(auth, &format!("/api/teams/{team_id}/pages/{page_id}")).await?;
    Ok(())
}

/// mirror of: `kairos_client::types_team_pages::TeamAnnouncement`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Announcement {
    pub id: String,
    pub body: String,
    pub pinned: bool,
    pub created_by: String,
    pub created_at: String,
}

/// `GET /api/teams/{id}/announcements` (pinned first, newest first): the
/// first page, of the default size (KAIROS-T-0629).
pub async fn team_announcements(auth: Auth, team_id: &str) -> Result<Vec<Announcement>, ApiError> {
    let page: crate::pages::activity::ListEnvelope<Announcement> =
        get_json(auth, &format!("/api/teams/{team_id}/announcements")).await?;
    Ok(page.items)
}

/// mirror of: `kairos_client::types_team_pages::CreateTeamAnnouncementRequest`.
#[derive(Serialize)]
struct PostAnnouncementBody<'a> {
    body: &'a str,
    pinned: bool,
}

/// `POST /api/teams/{id}/announcements` (team member or org admin;
/// append-only — there is no edit call to mirror).
pub async fn post_announcement(
    auth: Auth,
    team_id: &str,
    body: &str,
    pinned: bool,
) -> Result<Announcement, ApiError> {
    post_json(
        auth,
        &format!("/api/teams/{team_id}/announcements"),
        &PostAnnouncementBody { body, pinned },
    )
    .await
}

/// `DELETE /api/teams/{id}/announcements/{announcement_id}` (author or
/// org admin).
pub async fn delete_announcement(
    auth: Auth,
    team_id: &str,
    announcement_id: &str,
) -> Result<(), ApiError> {
    let _: serde_json::Value = delete_json(
        auth,
        &format!("/api/teams/{team_id}/announcements/{announcement_id}"),
    )
    .await?;
    Ok(())
}

/// mirror of: `kairos_client::types_forge::TeamLink` (KAIROS-T-0101).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TeamLink {
    /// `branch|pull_request`.
    pub kind: String,
    /// PR/MR number, or the branch ref.
    pub external_id: String,
    pub title: String,
    /// Browser URL on the forge.
    pub url: String,
    /// `open|merged|closed|draft`.
    pub state: String,
    /// `github|gitlab`.
    pub forge: String,
    /// `owner/repo`.
    pub repo_full_name: String,
    /// The Kairos work item this link belongs to.
    pub item_short_code: String,
    pub item_title: String,
}

/// `GET /api/teams/{id}/links` — in-flight work (open + draft) across the
/// team's repos; server-ordered newest first.
pub async fn team_links(auth: Auth, team_id: &str) -> Result<Vec<TeamLink>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/links")).await
}

/// `GET /api/teams/{id}/unlinked-refs` — the open branches and pull
/// requests of the team's repositories that no item is linked to
/// (KAIROS-T-0480); server-ordered newest first.
pub async fn team_unlinked_refs(
    auth: Auth,
    team_id: &str,
) -> Result<Vec<crate::pages::repositories::api::ForgeRef>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/unlinked-refs")).await
}

/// The accent token for a team type pill (shared by directory, detail,
/// and the admin teams table).
pub fn team_type_color(team_type: &str) -> &'static str {
    use aurora_dark::tokens::token;
    match team_type {
        "stream_aligned" => token::TEAL,
        "platform" => token::ICE,
        "enabling" => token::VIOLET,
        "complicated_subsystem" => token::GOLD,
        _ => token::SKIP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Team` decodes the wire shape (field-name lock; same body the admin
    /// mirror locked before the hoist).
    #[test]
    fn team_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "t1", "name": "Platform", "slug": "platform",
            "team_type": "platform",
            "delivery_board_id": "b9",
            "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"
        });
        let team: Team = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(team.delivery_board_id.as_deref(), Some("b9"));
        assert_eq!(team.team_type, "platform");
    }

    /// `TeamMember` decodes the roster row.
    #[test]
    fn team_member_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "user_id": "u1",
            "email": "bob@kairos.test",
            "display_name": "bob",
            "joined_at": "2026-07-14T00:00:00Z"
        });
        let member: TeamMember = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(member.display_name, "bob");
    }

    /// `BoardRef` decodes a board list element (link resolution only).
    #[test]
    fn board_ref_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "b9", "name": "Platform Delivery", "slug": "platform-delivery",
            "board_level": "delivery", "team_id": "t1",
            "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"
        });
        let board: BoardRef = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(board.slug, "platform-delivery");
    }

    /// `TeamPageNode` decodes the server's TeamPage shape.
    #[test]
    fn team_page_node_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "p1",
            "team_id": "t1",
            "parent_id": null,
            "kind": "page",
            "slug": "charter",
            "title": "Team Charter",
            "content": "# Charter",
            "position": 0,
            "is_protected": true,
            "version": 1,
            "created_at": "2026-08-29T00:00:00Z",
            "updated_at": "2026-08-29T00:00:00Z"
        });
        let node: TeamPageNode = serde_json::from_value(body).expect("mirror decodes");
        assert!(node.is_protected);
        assert_eq!(node.parent_id, None);
        assert_eq!(node.kind, "page");
    }

    fn node(parent_id: Option<&str>, slug: &str, is_protected: bool) -> TeamPageNode {
        TeamPageNode {
            id: slug.into(),
            parent_id: parent_id.map(Into::into),
            kind: "page".into(),
            slug: slug.into(),
            title: slug.into(),
            content: String::new(),
            position: 0,
            is_protected,
            version: 1,
            item: None,
        }
    }

    /// KAIROS-T-0436: the charter is the protected root page, whatever its
    /// slug; a root page that a team calls `charter` is not the charter.
    #[test]
    fn the_charter_is_found_by_its_flag() {
        assert!(node(None, "charter", true).is_charter());
        assert!(node(None, "landing", true).is_charter());
        assert!(!node(None, "charter", false).is_charter());
        assert!(!node(Some("f1"), "charter", true).is_charter());
    }

    /// `Announcement` decodes the server's TeamAnnouncement shape.
    #[test]
    fn announcement_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "a1",
            "team_id": "t1",
            "body": "Deploy freeze",
            "pinned": true,
            "created_by": "u1",
            "created_at": "2026-08-29T00:00:00Z"
        });
        let a: Announcement = serde_json::from_value(body).expect("mirror decodes");
        assert!(a.pinned);
        assert_eq!(a.created_by, "u1");
    }

    /// `TeamLink` decodes the in-flight rollup row.
    #[test]
    fn team_link_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "kind": "pull_request",
            "external_id": "42",
            "title": "Password-less auth",
            "url": "https://github.com/acme/payments-api/pull/42",
            "state": "open",
            "author": "dylan",
            "forge": "github",
            "repo_full_name": "acme/payments-api",
            "item_short_code": "DEMO-T-0002",
            "item_title": "Password-less email auth",
            "forge_updated_at": "2026-09-01T10:00:00Z"
        });
        let link: TeamLink = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(link.item_short_code, "DEMO-T-0002");
        assert_eq!(link.state, "open");
    }
}
