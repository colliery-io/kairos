//! Admin data layer (KAIROS-T-0043): endpoint wrappers + mirror DTOs for
//! the T-0019 org families (boards/columns/transitions/board members,
//! teams, delivery streams, org members) and the T-0020 meta families
//! (templates, metadata definitions).
//!
//! Follows the conventions data-layer rules: every call goes through the
//! `crate::api` `*_json` helpers (envelope-aware error mapping, 401 hook);
//! mirrors are partial on purpose and each carries a `mirror of:` line so
//! drift stays greppable. Mutation responses the views never read are
//! decoded as `serde_json::Value` — refetch is the source of truth.

use aurora_dark::tokens::ApiError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{delete_json, get_json, patch_json, post_json};
use crate::auth::Auth;

/// mirror of: `kairos_client::types::ListEnvelope<T>` (partial — the admin
/// views read `items` only; paging can land with a follow-up task).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ListEnvelope<T> {
    pub items: Vec<T>,
}

/// Big-enough page for admin lists (server clamps to its own max).
const PAGE: &str = "limit=200";

// ---------------------------------------------------------------------------
// Boards + configuration (columns, transitions)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::Board` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub board_level: String,
    pub team_id: Option<String>,
}

/// mirror of: `kairos_client::types_org::BoardColumn` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumn {
    pub id: String,
    pub name: String,
    pub position: i32,
}

/// mirror of: `kairos_client::types_org::BoardTransition` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardTransition {
    pub id: String,
    pub from_column_id: String,
    pub to_column_id: String,
}

/// mirror of: `kairos_client::types_org::BoardDetail` (partial; the board
/// fields arrive flattened at the top level, exactly like the wire shape).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardDetail {
    #[serde(flatten)]
    pub board: Board,
    pub columns: Vec<BoardColumn>,
    pub transitions: Vec<BoardTransition>,
}

/// `GET /api/boards` (first page; admin scale).
pub async fn list_boards(auth: Auth) -> Result<Vec<Board>, ApiError> {
    let envelope: ListEnvelope<Board> = get_json(auth, &format!("/api/boards?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `POST /api/boards` — seeded with the level's default columns/transitions.
pub async fn create_board(
    auth: Auth,
    name: &str,
    slug: &str,
    board_level: &str,
    team_id: Option<&str>,
) -> Result<Board, ApiError> {
    post_json(
        auth,
        "/api/boards",
        &json!({
            "name": name,
            "slug": slug,
            "board_level": board_level,
            "team_id": team_id,
        }),
    )
    .await
}

/// `DELETE /api/boards/{id}` (422 `BOARD_NOT_EMPTY` while items reference it).
pub async fn delete_board(auth: Auth, board_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/boards/{board_id}")).await
}

/// `GET /api/boards/{id}` — board + columns + transitions.
pub async fn board_detail(auth: Auth, board_id: &str) -> Result<BoardDetail, ApiError> {
    get_json(auth, &format!("/api/boards/{board_id}")).await
}

/// `POST /api/boards/{id}/columns` (422 `DUPLICATE_COLUMN_NAME` /
/// `DUPLICATE_COLUMN_POSITION`).
pub async fn add_column(
    auth: Auth,
    board_id: &str,
    name: &str,
    position: i32,
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/boards/{board_id}/columns"),
        &json!({ "name": name, "position": position }),
    )
    .await
}

/// `PATCH /api/boards/{id}/columns/{col_id}` — rename and/or move.
pub async fn update_column(
    auth: Auth,
    board_id: &str,
    column_id: &str,
    name: Option<&str>,
    position: Option<i32>,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/boards/{board_id}/columns/{column_id}"),
        &json!({ "name": name, "position": position }),
    )
    .await
}

/// `DELETE /api/boards/{id}/columns/{col_id}` (422 `COLUMN_NOT_EMPTY` while
/// items sit in the column).
pub async fn remove_column(auth: Auth, board_id: &str, column_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/boards/{board_id}/columns/{column_id}")).await
}

/// `POST /api/boards/{id}/transitions` (422 `DUPLICATE_TRANSITION` /
/// `VALIDATION` for self-transitions).
pub async fn add_transition(
    auth: Auth,
    board_id: &str,
    from_column_id: &str,
    to_column_id: &str,
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/boards/{board_id}/transitions"),
        &json!({ "from_column_id": from_column_id, "to_column_id": to_column_id }),
    )
    .await
}

/// `DELETE /api/boards/{id}/transitions/{transition_id}`.
pub async fn remove_transition(
    auth: Auth,
    board_id: &str,
    transition_id: &str,
) -> Result<Value, ApiError> {
    delete_json(
        auth,
        &format!("/api/boards/{board_id}/transitions/{transition_id}"),
    )
    .await
}

// ---------------------------------------------------------------------------
// Board members + capability grants (KAIROS-A-0006)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::BoardMember`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardMember {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub capabilities: Vec<String>,
}

/// `GET /api/boards/{id}/members`.
pub async fn board_members(auth: Auth, board_id: &str) -> Result<Vec<BoardMember>, ApiError> {
    get_json(auth, &format!("/api/boards/{board_id}/members")).await
}

/// `POST /api/boards/{id}/members` — first grant(s) for a user.
pub async fn add_board_member(
    auth: Auth,
    board_id: &str,
    user_id: &str,
    capabilities: &[String],
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/boards/{board_id}/members"),
        &json!({ "user_id": user_id, "capabilities": capabilities }),
    )
    .await
}

/// `PATCH /api/boards/{id}/members/{user_id}` — replaces the full set.
pub async fn replace_capabilities(
    auth: Auth,
    board_id: &str,
    user_id: &str,
    capabilities: &[String],
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/boards/{board_id}/members/{user_id}"),
        &json!({ "capabilities": capabilities }),
    )
    .await
}

/// `DELETE /api/boards/{id}/members/{user_id}` — full revocation.
pub async fn remove_board_member(
    auth: Auth,
    board_id: &str,
    user_id: &str,
) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/boards/{board_id}/members/{user_id}")).await
}

// ---------------------------------------------------------------------------
// Teams
// ---------------------------------------------------------------------------

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

/// `GET /api/teams`.
pub async fn list_teams(auth: Auth) -> Result<Vec<Team>, ApiError> {
    let envelope: ListEnvelope<Team> = get_json(auth, &format!("/api/teams?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `POST /api/teams` — also creates the team's delivery board
/// (`{slug}-delivery`); the returned `delivery_board_id` names it.
pub async fn create_team(
    auth: Auth,
    name: &str,
    slug: &str,
    team_type: &str,
) -> Result<Team, ApiError> {
    post_json(
        auth,
        "/api/teams",
        &json!({ "name": name, "slug": slug, "team_type": team_type }),
    )
    .await
}

/// `PATCH /api/teams/{id}`.
pub async fn update_team(
    auth: Auth,
    team_id: &str,
    name: &str,
    slug: &str,
    team_type: &str,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/teams/{team_id}"),
        &json!({ "name": name, "slug": slug, "team_type": team_type }),
    )
    .await
}

/// `DELETE /api/teams/{id}`.
pub async fn delete_team(auth: Auth, team_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/teams/{team_id}")).await
}

/// `GET /api/teams/{id}/members`.
pub async fn team_members(auth: Auth, team_id: &str) -> Result<Vec<TeamMember>, ApiError> {
    get_json(auth, &format!("/api/teams/{team_id}/members")).await
}

/// `POST /api/teams/{id}/members`.
pub async fn add_team_member(auth: Auth, team_id: &str, user_id: &str) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/teams/{team_id}/members"),
        &json!({ "user_id": user_id }),
    )
    .await
}

/// `DELETE /api/teams/{id}/members/{user_id}`.
pub async fn remove_team_member(
    auth: Auth,
    team_id: &str,
    user_id: &str,
) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/teams/{team_id}/members/{user_id}")).await
}

// ---------------------------------------------------------------------------
// Delivery streams
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::DeliveryStream` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DeliveryStream {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
}

/// `GET /api/delivery-streams`.
pub async fn list_streams(auth: Auth) -> Result<Vec<DeliveryStream>, ApiError> {
    let envelope: ListEnvelope<DeliveryStream> =
        get_json(auth, &format!("/api/delivery-streams?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `POST /api/delivery-streams`.
pub async fn create_stream(
    auth: Auth,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> Result<Value, ApiError> {
    post_json(
        auth,
        "/api/delivery-streams",
        &json!({ "name": name, "slug": slug, "description": description }),
    )
    .await
}

/// `PATCH /api/delivery-streams/{id}`.
pub async fn update_stream(
    auth: Auth,
    stream_id: &str,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/delivery-streams/{stream_id}"),
        &json!({ "name": name, "slug": slug, "description": description }),
    )
    .await
}

/// `DELETE /api/delivery-streams/{id}`.
pub async fn delete_stream(auth: Auth, stream_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/delivery-streams/{stream_id}")).await
}

/// `GET /api/delivery-streams/{id}/teams`.
pub async fn stream_teams(auth: Auth, stream_id: &str) -> Result<Vec<Team>, ApiError> {
    get_json(auth, &format!("/api/delivery-streams/{stream_id}/teams")).await
}

/// `POST /api/delivery-streams/{id}/teams`.
pub async fn add_stream_team(
    auth: Auth,
    stream_id: &str,
    team_id: &str,
) -> Result<Value, ApiError> {
    post_json(
        auth,
        &format!("/api/delivery-streams/{stream_id}/teams"),
        &json!({ "team_id": team_id }),
    )
    .await
}

/// `DELETE /api/delivery-streams/{id}/teams/{team_id}`.
pub async fn remove_stream_team(
    auth: Auth,
    stream_id: &str,
    team_id: &str,
) -> Result<Value, ApiError> {
    delete_json(
        auth,
        &format!("/api/delivery-streams/{stream_id}/teams/{team_id}"),
    )
    .await
}

// ---------------------------------------------------------------------------
// Organization members
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_org::OrgMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct OrgMember {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
}

/// `GET /api/members`.
pub async fn list_org_members(auth: Auth) -> Result<Vec<OrgMember>, ApiError> {
    let envelope: ListEnvelope<OrgMember> = get_json(auth, &format!("/api/members?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `POST /api/members` — resolve by email (users are JIT-provisioned at
/// first login; unknown email → 404 with the "log in once first" guidance).
pub async fn add_org_member(auth: Auth, email: &str, role: &str) -> Result<Value, ApiError> {
    post_json(
        auth,
        "/api/members",
        &json!({ "email": email, "role": role }),
    )
    .await
}

/// `PATCH /api/members/{user_id}` — role change (422 `LAST_ADMIN` when
/// demoting the only admin).
pub async fn set_org_member_role(auth: Auth, user_id: &str, role: &str) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/members/{user_id}"),
        &json!({ "role": role }),
    )
    .await
}

/// `DELETE /api/members/{user_id}` (422 `LAST_ADMIN` for the only admin).
pub async fn remove_org_member(auth: Auth, user_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/members/{user_id}")).await
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_meta::Template` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub is_system_default: bool,
}

/// mirror of: `kairos_client::types_meta::TemplateMetadataField` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateMetadataField {
    pub slug: String,
    pub default_value: Option<String>,
    pub required: bool,
}

/// mirror of: `kairos_client::types_meta::TemplateDetail` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TemplateDetail {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub content: String,
    pub is_system_default: bool,
    pub metadata: Vec<TemplateMetadataField>,
}

/// One template ↔ definition association in a template write (the
/// `metadata` entry of `CreateTemplateRequest`/`UpdateTemplateRequest`).
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateMetadataEntry {
    pub definition_slug: String,
    pub default_value: Option<String>,
    pub required: bool,
}

fn metadata_entries_json(entries: &[TemplateMetadataEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| {
            json!({
                "definition_slug": entry.definition_slug,
                "default_value": entry.default_value,
                "required": entry.required,
            })
        })
        .collect()
}

/// `GET /api/templates`.
pub async fn list_templates(auth: Auth) -> Result<Vec<Template>, ApiError> {
    let envelope: ListEnvelope<Template> =
        get_json(auth, &format!("/api/templates?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `GET /api/templates/{id}` — template + its metadata associations.
pub async fn template_detail(auth: Auth, template_id: &str) -> Result<TemplateDetail, ApiError> {
    get_json(auth, &format!("/api/templates/{template_id}")).await
}

/// `POST /api/templates`.
pub async fn create_template(
    auth: Auth,
    name: &str,
    slug: &str,
    content: &str,
    metadata: &[TemplateMetadataEntry],
) -> Result<Value, ApiError> {
    post_json(
        auth,
        "/api/templates",
        &json!({
            "name": name,
            "slug": slug,
            "content": content,
            "metadata": metadata_entries_json(metadata),
        }),
    )
    .await
}

/// `PATCH /api/templates/{id}` — `metadata` replaces the association list.
pub async fn update_template(
    auth: Auth,
    template_id: &str,
    name: &str,
    slug: &str,
    content: &str,
    metadata: &[TemplateMetadataEntry],
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/templates/{template_id}"),
        &json!({
            "name": name,
            "slug": slug,
            "content": content,
            "metadata": metadata_entries_json(metadata),
        }),
    )
    .await
}

/// `DELETE /api/templates/{id}` (hard delete).
pub async fn delete_template(auth: Auth, template_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/templates/{template_id}")).await
}

// ---------------------------------------------------------------------------
// Metadata definitions
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_meta::MetadataDefinition` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MetadataDefinition {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub field_type: String,
    pub is_system_default: bool,
    pub enum_options: Vec<String>,
}

/// `GET /api/metadata-definitions`.
pub async fn list_definitions(auth: Auth) -> Result<Vec<MetadataDefinition>, ApiError> {
    let envelope: ListEnvelope<MetadataDefinition> =
        get_json(auth, &format!("/api/metadata-definitions?{PAGE}")).await?;
    Ok(envelope.items)
}

/// `POST /api/metadata-definitions` — `enum_options` required non-empty for
/// `enum`, forbidden otherwise (server-validated, 422 `VALIDATION`).
pub async fn create_definition(
    auth: Auth,
    name: &str,
    slug: &str,
    field_type: &str,
    enum_options: &[String],
) -> Result<Value, ApiError> {
    post_json(
        auth,
        "/api/metadata-definitions",
        &json!({
            "name": name,
            "slug": slug,
            "field_type": field_type,
            "enum_options": enum_options,
        }),
    )
    .await
}

/// `PATCH /api/metadata-definitions/{id}` — `enum_options` replaces the
/// full option list (enum definitions only).
pub async fn update_definition(
    auth: Auth,
    definition_id: &str,
    name: &str,
    slug: &str,
    enum_options: Option<&[String]>,
) -> Result<Value, ApiError> {
    patch_json(
        auth,
        &format!("/api/metadata-definitions/{definition_id}"),
        &json!({ "name": name, "slug": slug, "enum_options": enum_options }),
    )
    .await
}

/// `DELETE /api/metadata-definitions/{id}` (hard delete).
pub async fn delete_definition(auth: Auth, definition_id: &str) -> Result<Value, ApiError> {
    delete_json(auth, &format!("/api/metadata-definitions/{definition_id}")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BoardDetail` decodes the wire shape: board fields flattened at the
    /// top level next to `columns`/`transitions` (field-name lock).
    #[test]
    fn board_detail_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "0b7c9a44-9c7e-4f4e-8a3f-2f5b1a9d0e11",
            "name": "Platform Delivery",
            "slug": "platform-delivery",
            "board_level": "delivery",
            "team_id": "0d1e2f3a-4b5c-6d7e-8f90-a1b2c3d4e5f6",
            "created_at": "2026-07-14T00:00:00Z",
            "updated_at": "2026-07-14T00:00:00Z",
            "columns": [
                {"id": "c1", "board_id": "b", "name": "Backlog", "position": 0,
                 "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"}
            ],
            "transitions": [
                {"id": "t1", "board_id": "b", "from_column_id": "c1", "to_column_id": "c2"}
            ]
        });
        let detail: BoardDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(detail.board.slug, "platform-delivery");
        assert_eq!(detail.columns[0].name, "Backlog");
        assert_eq!(detail.transitions[0].to_column_id, "c2");
    }

    /// `BoardMember` decodes the grants list (capability editor input).
    #[test]
    fn board_member_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "user_id": "u1",
            "email": "bob@kairos.test",
            "display_name": "bob",
            "capabilities": ["manage_tasks", "transition_items"]
        });
        let member: BoardMember = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(member.capabilities.len(), 2);
    }

    /// `Team` keeps `delivery_board_id` — the "creation surfaces its new
    /// delivery board" AC reads it.
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
    }

    /// `MetadataDefinition` decodes enum options in order.
    #[test]
    fn definition_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "d1", "name": "Priority", "slug": "priority",
            "field_type": "enum", "is_system_default": true,
            "enum_options": ["low", "medium", "high"],
            "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"
        });
        let definition: MetadataDefinition = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(definition.enum_options, vec!["low", "medium", "high"]);
    }

    /// `TemplateDetail` decodes the metadata association rows.
    #[test]
    fn template_detail_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "id": "tp1", "name": "PRD", "slug": "prd",
            "content": "# PRD\n", "is_system_default": false,
            "metadata": [
                {"definition_id": "d1", "slug": "priority", "name": "Priority",
                 "field_type": "enum", "enum_options": ["low", "high"],
                 "default_value": "low", "required": true}
            ],
            "created_at": "2026-07-14T00:00:00Z", "updated_at": "2026-07-14T00:00:00Z"
        });
        let detail: TemplateDetail = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(detail.metadata[0].slug, "priority");
        assert!(detail.metadata[0].required);
    }
}
