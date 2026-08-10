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
use serde::Deserialize;

use crate::api::get_json;
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
}
