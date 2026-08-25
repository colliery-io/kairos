//! Shared API wire types for the KAIROS-T-0020 endpoint families:
//! relationships, item metadata, metadata definitions, templates, content
//! history, and the activity log (contracts per KAIROS-S-0005 / A-0003 /
//! A-0004 / A-0006).
//!
//! Same dependency discipline as [`crate::types`]: serde/utoipa only, so
//! ids travel as canonical UUID strings, timestamps as RFC 3339, and dates
//! as `YYYY-MM-DD`. The server (`kairos-server/src/api/convert_meta.rs`)
//! owns the model → DTO encoding.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

// ---------------------------------------------------------------------------
// Relationships (KAIROS-A-0001 graph, T-0013 service)
// ---------------------------------------------------------------------------

/// One hydrated neighbor of an item in the relationship graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RelatedItem {
    /// The edge's id (UUID) — pass to `DELETE /api/relationships/{id}`.
    pub relationship_id: String,
    /// The neighbor's entity id (UUID).
    pub id: String,
    /// The neighbor's short code.
    pub short_code: String,
    /// `strategy|initiative|task|document|adr`.
    pub entity_type: String,
    /// The neighbor's title.
    pub title: String,
}

/// All of an item's neighbors under ONE relationship type in one direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RelationshipGroup {
    /// `parent|supports|informs|supersedes|blocks`.
    pub relationship: String,
    /// Hydrated neighbors, in edge-creation order.
    pub items: Vec<RelatedItem>,
}

/// Response of `GET /api/{entity_type}/{short_code}/relationships`: both
/// directions, grouped by relationship type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemRelationshipsResponse {
    /// The item the relationships belong to.
    pub short_code: String,
    /// Edges where the item is the SOURCE (the neighbor is the target,
    /// e.g. children via `parent`, items it blocks via `blocks`).
    pub outgoing: Vec<RelationshipGroup>,
    /// Edges where the item is the TARGET (the neighbor is the source,
    /// e.g. its parent via `parent`, items blocking it via `blocks`).
    pub incoming: Vec<RelationshipGroup>,
}

/// Response of `GET /api/{entity_type}/{short_code}/children-progress`
/// (KAIROS-T-0080): the item's direct `parent`-edge children grouped by
/// their board column. `done` counts children in `is_done` columns; when
/// NO involved column is flagged, clients must show composition only,
/// never a done percentage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ChildrenProgressResponse {
    /// The parent item's short code.
    pub short_code: String,
    /// Direct live children (soft-deleted excluded; supports/informs
    /// material never counts).
    pub total: i64,
    /// Children sitting in `is_done` columns.
    pub done: i64,
    /// False when no board hosting the children has a done-flagged
    /// column — show composition only, never a done fraction.
    #[serde(default)]
    pub has_done_columns: bool,
    /// Per-column composition, board-then-position order.
    pub by_column: Vec<ChildColumnProgress>,
}

/// One column bucket of a children-progress rollup (KAIROS-T-0080).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ChildColumnProgress {
    /// Column id (UUID).
    pub column_id: String,
    pub column_name: String,
    /// The column's board (children may span boards).
    pub board_id: String,
    pub is_done: bool,
    pub count: i64,
}

/// Body of `POST /api/relationships` (org admin only, KAIROS-A-0006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateRelationshipRequest {
    /// Short code of the edge's source item (KAIROS-A-0001 orientation).
    pub source_short_code: String,
    /// Short code of the edge's target item.
    pub target_short_code: String,
    /// `parent|supports|informs|supersedes|blocks`.
    pub relationship: String,
}

/// A relationship edge, as returned by `POST /api/relationships`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Relationship {
    /// Edge id (UUID).
    pub id: String,
    /// Source entity id (UUID).
    pub source_id: String,
    /// Target entity id (UUID).
    pub target_id: String,
    /// `parent|supports|informs|supersedes|blocks`.
    pub relationship: String,
    /// RFC 3339.
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Item metadata (KAIROS-A-0003)
// ---------------------------------------------------------------------------

/// One typed metadata value on an item, hydrated with its definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MetadataValue {
    /// `metadata_definitions.id` (UUID).
    pub definition_id: String,
    /// The definition's slug (the PATCH key).
    pub slug: String,
    /// The definition's display name.
    pub name: String,
    /// `string|enum|date`.
    pub field_type: String,
    /// The stored value.
    pub value: String,
}

/// Response of `GET/PATCH /api/{entity_type}/{short_code}/metadata`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemMetadataResponse {
    /// The item the values belong to.
    pub short_code: String,
    /// All metadata values on the item, ordered by definition slug.
    pub values: Vec<MetadataValue>,
}

/// Body of `PATCH /api/{entity_type}/{short_code}/metadata`: definition
/// slug → value. Values are validated against the definition's type
/// (KAIROS-A-0003: enum membership, date parse, string passthrough) and
/// upserted; `null` clears the value. Unknown slugs are 422 `VALIDATION`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateMetadataRequest {
    /// Definition slug → new value (`null` removes the value).
    pub values: BTreeMap<String, Option<String>>,
}

// ---------------------------------------------------------------------------
// Metadata definitions (KAIROS-A-0003; org-admin writes per A-0006)
// ---------------------------------------------------------------------------

/// A metadata field definition, as returned by `/api/metadata-definitions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MetadataDefinition {
    /// Definition id (UUID).
    pub id: String,
    pub name: String,
    pub slug: String,
    /// `string|enum|date`.
    pub field_type: String,
    /// Seeded from the system defaults at tenant provisioning.
    pub is_system_default: bool,
    /// Allowed values in display order (empty unless `field_type = enum`).
    pub enum_options: Vec<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// Body of `POST /api/metadata-definitions` (org admin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateMetadataDefinitionRequest {
    pub name: String,
    pub slug: String,
    /// `string|enum|date`.
    pub field_type: String,
    /// Required non-empty for `enum`; forbidden otherwise.
    #[serde(default)]
    pub enum_options: Vec<String>,
}

/// Body of `PATCH /api/metadata-definitions/{id}` (org admin). Omitted
/// fields are unchanged; `enum_options` replaces the full option list
/// (enum definitions only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateMetadataDefinitionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    /// Replacement option list (enum definitions only, non-empty).
    #[serde(default)]
    pub enum_options: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Templates (KAIROS-A-0003; org-admin writes per A-0006)
// ---------------------------------------------------------------------------

/// A document template, as returned by `GET /api/templates`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Template {
    /// Template id (UUID).
    pub id: String,
    pub name: String,
    pub slug: String,
    /// Starter markdown content.
    pub content: String,
    /// Seeded from the system defaults at tenant provisioning.
    pub is_system_default: bool,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// One metadata field a template carries (`template_metadata` hydrated
/// with its definition).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TemplateMetadataField {
    /// `metadata_definitions.id` (UUID).
    pub definition_id: String,
    /// The definition's slug.
    pub slug: String,
    /// The definition's display name.
    pub name: String,
    /// `string|enum|date`.
    pub field_type: String,
    /// Allowed values in display order (empty unless `field_type = enum`).
    pub enum_options: Vec<String>,
    /// Stamped as an `item_metadata` row at create-from-template time.
    pub default_value: Option<String>,
    pub required: bool,
}

/// Response of `GET /api/templates/{id}`: the template plus its associated
/// metadata definitions with defaults/required flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TemplateDetail {
    /// Template id (UUID).
    pub id: String,
    pub name: String,
    pub slug: String,
    /// Starter markdown content.
    pub content: String,
    pub is_system_default: bool,
    /// The metadata fields this template carries, ordered by slug.
    pub metadata: Vec<TemplateMetadataField>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// One template ↔ metadata-definition association in a template write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TemplateMetadataEntry {
    /// Slug of an existing metadata definition.
    pub definition_slug: String,
    /// Optional default, validated against the definition's type.
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// Body of `POST /api/templates` (org admin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateTemplateRequest {
    pub name: String,
    pub slug: String,
    /// Starter markdown content; defaults to empty.
    #[serde(default)]
    pub content: String,
    /// Metadata fields the template carries.
    #[serde(default)]
    pub metadata: Vec<TemplateMetadataEntry>,
}

/// Body of `PATCH /api/templates/{id}` (org admin). Omitted fields are
/// unchanged; `metadata` replaces the full association list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateTemplateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    /// Replacement metadata association list.
    #[serde(default)]
    pub metadata: Option<Vec<TemplateMetadataEntry>>,
}

/// Response of the T-0020 hard-delete endpoints
/// (`DELETE /api/relationships/{id}`, `/api/metadata-definitions/{id}`,
/// `/api/templates/{id}`) — these resources have no soft-delete cascade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeletedResponse {
    /// The deleted row's id (UUID).
    pub id: String,
}

// ---------------------------------------------------------------------------
// Content history (KAIROS-A-0004)
// ---------------------------------------------------------------------------

/// One row of `GET /api/{entity_type}/{short_code}/history`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HistoryVersion {
    /// The content version this snapshot captured.
    pub version: i32,
    /// Editor user id (UUID).
    pub edited_by: String,
    /// RFC 3339.
    pub edited_at: String,
}

/// Response of `GET /api/{entity_type}/{short_code}/history?version=N`:
/// the full snapshot at that version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HistorySnapshot {
    pub version: i32,
    /// Title at this version.
    pub title: String,
    /// Content at this version.
    pub content: String,
    /// Editor user id (UUID).
    pub edited_by: String,
    /// RFC 3339.
    pub edited_at: String,
}

/// Query of `GET /api/{entity_type}/{short_code}/history`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    /// Return this version's full snapshot instead of the version list.
    #[serde(default)]
    pub version: Option<i32>,
    /// Page size for the version list (default 50, max 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Activity log (KAIROS-A-0004 / S-0005)
// ---------------------------------------------------------------------------

/// One `activity_log` row, as returned by `GET /api/activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActivityEntry {
    /// Row id (UUID).
    pub id: String,
    /// Who did it (user UUID).
    pub actor_id: String,
    /// `transition|create|delete|relationship_add|relationship_remove|capability_grant|capability_revoke|board_config`.
    pub action: String,
    /// The item acted on (UUID; null for relationship actions).
    pub entity_id: Option<String>,
    /// `strategy|initiative|task|document|adr` (null for relationship
    /// actions).
    pub entity_type: Option<String>,
    /// Structured context, e.g. `"column:Draft->Active"`.
    pub details: String,
    /// RFC 3339.
    pub occurred_at: String,
}

/// Query of `GET /api/activity` (S-0005: all filters combinable).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ActivityQuery {
    /// Filter: activity for a specific entity (UUID).
    #[serde(default)]
    pub entity_id: Option<String>,
    /// Filter: activity by a specific user (UUID).
    #[serde(default)]
    pub actor_id: Option<String>,
    /// Filter: activity by action type.
    #[serde(default)]
    pub action: Option<String>,
    /// Filter: activity at or after this RFC 3339 timestamp.
    #[serde(default)]
    pub since: Option<String>,
    /// Page size (default 50, max 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The metadata PATCH body round-trips: string values stay, `null`
    /// means "clear".
    #[test]
    fn update_metadata_request_round_trips() {
        let body: UpdateMetadataRequest = serde_json::from_value(serde_json::json!({
            "values": {"priority": "high", "due_date": null}
        }))
        .expect("deserializes");
        assert_eq!(body.values.get("priority"), Some(&Some("high".to_string())));
        assert_eq!(body.values.get("due_date"), Some(&None));
    }

    /// Grouped relationships serialize with the S-0005 field names.
    #[test]
    fn relationships_response_shape() {
        let response = ItemRelationshipsResponse {
            short_code: "ACME-T-0001".into(),
            outgoing: vec![RelationshipGroup {
                relationship: "blocks".into(),
                items: vec![RelatedItem {
                    relationship_id: "e".into(),
                    id: "i".into(),
                    short_code: "ACME-T-0002".into(),
                    entity_type: "task".into(),
                    title: "t".into(),
                }],
            }],
            incoming: vec![],
        };
        let value = serde_json::to_value(&response).expect("serializes");
        assert_eq!(value["outgoing"][0]["relationship"], "blocks");
        assert_eq!(
            value["outgoing"][0]["items"][0]["short_code"],
            "ACME-T-0002"
        );
        assert!(value["incoming"].as_array().expect("array").is_empty());
    }
}
