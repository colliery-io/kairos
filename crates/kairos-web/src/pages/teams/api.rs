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

/// mirror of: `kairos_client::types::ListEnvelope<T>` (partial — these
/// views read `items` only).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ListEnvelope<T> {
    pub items: Vec<T>,
}

/// Big-enough page for org-scale lists (server clamps to its own max).
const PAGE: &str = "limit=200";

/// mirror of: `kairos_client::types_org::Team` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub team_type: String,
    pub delivery_board_id: Option<String>,
}

/// mirror of: `kairos_client::types_org::TeamMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TeamMember {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
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

/// `GET /api/teams`.
pub async fn list_teams(auth: Auth) -> Result<Vec<Team>, ApiError> {
    let envelope: ListEnvelope<Team> = get_json(auth, &format!("/api/teams?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `GET /api/teams/{id}/members`.
pub async fn team_members(auth: Auth, team_id: &str) -> Result<Vec<TeamMember>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/members")).await
}

/// `GET /api/delivery-streams`.
pub async fn list_streams(auth: Auth) -> Result<Vec<DeliveryStream>, ApiError> {
    let envelope: ListEnvelope<DeliveryStream> =
        get_json(auth, &format!("/api/delivery-streams?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `GET /api/delivery-streams/{id}/teams`.
pub async fn stream_teams(auth: Auth, stream_id: &str) -> Result<Vec<Team>, ApiError> {
    get_json(auth, &format!("/api/delivery-streams/{stream_id}/teams")).await
}

/// `GET /api/boards` — id/name/slug refs (delivery-board link resolution).
pub async fn list_board_refs(auth: Auth) -> Result<Vec<BoardRef>, ApiError> {
    let envelope: ListEnvelope<BoardRef> = get_json(auth, &format!("/api/boards?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `GET /api/teams/by-slug/{slug}` (KAIROS-T-0083) — kills the
/// fetch-all directory scan the detail page used before KAIROS-T-0085.
pub async fn team_by_slug(auth: Auth, slug: &str) -> Result<Team, ApiError> {
    get_json(auth, &format!("/api/teams/by-slug/{slug}")).await
}

/// mirror of: `kairos_client::types_team_pages::TeamPage` (partial — the
/// landing page needs the tree shape + charter content; `updated_at`
/// stays server-side).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TeamPageNode {
    pub id: String,
    pub parent_id: Option<String>,
    /// `folder|page`.
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub content: String,
    pub position: i32,
    pub is_protected: bool,
    pub version: i32,
}

/// `GET /api/teams/{id}/pages` — the flat page tree (nest by parent_id).
pub async fn team_pages(auth: Auth, team_id: &str) -> Result<Vec<TeamPageNode>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/pages")).await
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
}

/// `POST /api/teams/{id}/pages` (team member or org admin).
pub async fn create_page(
    auth: Auth,
    team_id: &str,
    parent_id: Option<&str>,
    kind: &str,
    slug: &str,
    title: &str,
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
        },
    )
    .await
}

/// `PATCH /api/teams/{id}/pages/{page_id}` — move under `new_parent`
/// (`None` = to the tree root, via `move_to_root`).
pub async fn move_page(
    auth: Auth,
    team_id: &str,
    page_id: &str,
    new_parent: Option<&str>,
) -> Result<TeamPageNode, ApiError> {
    crate::api::patch_json(
        auth,
        &format!("/api/teams/{team_id}/pages/{page_id}"),
        &StructureBody {
            slug: None,
            parent_id: new_parent,
            move_to_root: new_parent.is_none(),
        },
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

/// `GET /api/teams/{id}/announcements` (pinned first, newest first).
pub async fn team_announcements(auth: Auth, team_id: &str) -> Result<Vec<Announcement>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/announcements")).await
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

/// mirror of: `kairos_client::types_team_pages::TeamWorkDocument`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkDocument {
    pub short_code: String,
    pub title: String,
    pub lifecycle: String,
    pub parent_short_code: String,
    pub parent_title: String,
    pub parent_type: String,
}

/// `GET /api/teams/{id}/work-documents` (KAIROS-T-0084) — live documents
/// supporting the team's tasks or items on its delivery board.
pub async fn team_work_documents(auth: Auth, team_id: &str) -> Result<Vec<WorkDocument>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/work-documents")).await
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

    /// `WorkDocument` decodes the derived work-documents row.
    #[test]
    fn work_document_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "short_code": "ACME-D-0002",
            "title": "Rollout runbook",
            "lifecycle": "published",
            "parent_short_code": "ACME-T-0009",
            "parent_title": "Ship the rollout",
            "parent_type": "task"
        });
        let doc: WorkDocument = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(doc.lifecycle, "published");
        assert_eq!(doc.parent_short_code, "ACME-T-0009");
    }
}
