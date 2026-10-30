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
    /// When this neighbour was archived, RFC 3339; absent while it is
    /// live. Relationship lists are archived-INCLUSIVE (KAIROS-T-0158):
    /// an item's edges describe what it contains and depends on, and
    /// dropping an archived endpoint silently shrinks that answer. The
    /// row therefore comes back with this marker, and every renderer must
    /// show it — ADR-20: anything serving an archived row says so, or an
    /// auditor mistakes it for live work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// `true` when this neighbour sits in a terminal column (a column the
    /// board marks `is_done`); absent otherwise (COLLIERY-T-0214). Done
    /// work does not block and is not blocked, so a `blocks` edge to a
    /// done neighbour is history, not an open blocker: the board's
    /// `blocks_summary` does not count it. The edge still comes back
    /// here, because the list is the record, and every renderer of a
    /// `blocks` neighbour must show this marker. It follows the
    /// neighbour's current column; nothing is stored.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done: bool,
    /// `true` when the neighbour is a cancelled task (KAIROS-T-0567); absent
    /// otherwise. A cancelled task is in a done column, so `done` is `true`
    /// too. But a cancelled blocker is unresolved: it still blocks, and a
    /// client shows it as cancelled, not done.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
}

/// All of an item's neighbors under ONE relationship type in one direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RelationshipGroup {
    /// `parent|supports|informs|supersedes|amends|blocks`.
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
    /// Direct children that count: the live children, and the archived
    /// children in a done column (KAIROS-T-0571). Supports/informs
    /// material never counts.
    pub total: i64,
    /// Children sitting in `is_done` columns, less the cancelled tasks
    /// (KAIROS-T-0567). An archived child in a done column counts here
    /// (KAIROS-T-0571).
    pub done: i64,
    /// Cancelled tasks among the children (KAIROS-T-0567). A cancel is
    /// "won't do", not done. The default is for the client only: a
    /// response of an older server has no `cancelled`.
    #[serde(default)]
    #[schema(required = true)]
    pub cancelled: i64,
    /// Children that are not done and not cancelled: `total - done -
    /// cancelled`.
    #[serde(default)]
    #[schema(required = true)]
    pub open: i64,
    /// Children archived from a column that is not done (KAIROS-T-0571).
    /// They are not open work, and they are not in `total`. The default
    /// is for the client only: a response of an older server has no
    /// `removed`.
    #[serde(default)]
    #[schema(required = true)]
    pub removed: i64,
    /// False when no board hosting the children has a done-flagged
    /// column — show composition only, never a done fraction.
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required (COLLIERY-T-0254). The default is for the client only: a
    /// response of an older server has no `has_done_columns`, and it
    /// reads as `false`.
    #[serde(default)]
    #[schema(required = true)]
    pub has_done_columns: bool,
    /// Per-column composition of the children that count,
    /// board-then-position order.
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
    /// The cancelled tasks among `count` (KAIROS-T-0567).
    #[serde(default)]
    #[schema(required = true)]
    pub cancelled: i64,
}

/// Body of `POST /api/relationships`. The caller may edit the item at
/// either end (the link rule, COLLIERY-T-0228).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRelationshipRequest {
    /// Short code of the edge's source item (KAIROS-A-0001 orientation).
    pub source_short_code: String,
    /// Short code of the edge's target item.
    pub target_short_code: String,
    /// `parent|supports|informs|supersedes|amends|blocks`.
    pub relationship: String,
}

/// Body of `PUT /api/{entity_type}/{short_code}/parent` (KAIROS-T-0572):
/// set the parent of an initiative or a task, or remove it. An item has
/// one parent, and the new edge replaces the old one in one transaction.
///
/// `parent` is required: a short code, or `null` to remove the parent. A
/// body without it is a 422, so that an empty body never removes a parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParentRequest {
    /// The short code of the new parent: a strategy for an initiative, an
    /// initiative for a task. A retired code finds its item. `null`
    /// removes the parent.
    #[serde(deserialize_with = "present_or_null")]
    #[schema(required = true, nullable)]
    pub parent: Option<String>,
}

/// A field that must be in the body, with a value or `null`. Serde reads a
/// missing `Option` as `None`; with `deserialize_with` and no `default`,
/// a missing field is an error.
fn present_or_null<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

/// Response of `PUT /api/{entity_type}/{short_code}/parent` (KAIROS-T-0572).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SetParentResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The parent now, by short code; `null` when the item has none.
    pub parent: Option<String>,
    /// The parent before the write, by short code; `null` when it had none.
    pub previous_parent: Option<String>,
    /// `false` when the item had this parent already (or none, for
    /// `null`). Kairos then writes nothing.
    pub changed: bool,
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
    /// The current short code of the source. A retired code in the request
    /// follows to its item (KAIROS-T-0560), so this code can differ from
    /// the request.
    pub source_short_code: String,
    /// The current short code of the target (see `source_short_code`).
    pub target_short_code: String,
    /// `parent|supports|informs|supersedes|amends|blocks`.
    pub relationship: String,
    /// RFC 3339.
    pub created_at: String,
}

/// The answer of `POST /api/relationships` when the call made a proposal
/// and no edge (202, KAIROS-T-0569). A `blocks` edge needs the right to
/// edit the target. A caller who can edit the source only gets this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProposedRelationship {
    /// What the call did, and who can confirm the proposal.
    pub message: String,
    /// The proposal. Confirm it with `POST /api/proposals/{id}/confirm`.
    pub proposal: crate::types_search::EdgeProposalDto,
}

/// What `POST /api/relationships` did (KAIROS-T-0569): the edge (201), or
/// a proposal of it (202).
#[derive(Debug, Clone, PartialEq)]
pub enum RelationshipOutcome {
    /// 201: the edge exists.
    Linked(Relationship),
    /// 202: a proposal waits for a decision.
    Proposed(ProposedRelationship),
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
#[serde(deny_unknown_fields)]
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
    /// Entity types this definition applies to (KAIROS-T-0078):
    /// `strategy|initiative|task|document|adr`. EMPTY = applies to every
    /// type. Enforced on the write paths, not just rendering.
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required (COLLIERY-T-0254). The default is for the client only: a
    /// response of an older server has no `entity_types`, and it reads as
    /// an empty list.
    #[serde(default)]
    #[schema(required = true)]
    pub entity_types: Vec<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// Body of `POST /api/metadata-definitions` (org admin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMetadataDefinitionRequest {
    pub name: String,
    pub slug: String,
    /// `string|enum|date`.
    pub field_type: String,
    /// Required non-empty for `enum`; forbidden otherwise.
    #[serde(default)]
    pub enum_options: Vec<String>,
    /// Entity types the definition applies to (KAIROS-T-0078); empty =
    /// all types.
    #[serde(default)]
    pub entity_types: Vec<String>,
}

/// Body of `PATCH /api/metadata-definitions/{id}` (org admin). Omitted
/// fields are unchanged; `enum_options` replaces the full option list
/// (enum definitions only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMetadataDefinitionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    /// Replacement option list (enum definitions only, non-empty).
    #[serde(default)]
    pub enum_options: Option<Vec<String>>,
    /// Replacement scope list (KAIROS-T-0078); `Some([])` clears the
    /// scopes (definition applies to all types again).
    #[serde(default)]
    pub entity_types: Option<Vec<String>>,
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
    /// The typed sections of the template, in the order that the admin
    /// sent (KAIROS-T-0729).
    #[serde(default)]
    pub sections: Vec<TemplateSection>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

/// One template ↔ metadata-definition association in a template write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateMetadataEntry {
    /// Slug of an existing metadata definition. KAIROS-T-0696: the server
    /// also accepts the name `definition`, the name on MCP.
    #[serde(alias = "definition")]
    pub definition_slug: String,
    /// Optional default, validated against the definition's type.
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// A typed section of a template (KAIROS-T-0729). It has the text of a
/// heading in the content, and the type of check on the text under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateSection {
    /// The text of the heading, with no `#`. It matches a heading of each
    /// level. One line, not empty, one time in each template.
    pub heading: String,
    /// `free` (text that is not empty), `plain` (text that passes the
    /// plain-language check) or `ears` (each requirement fits one frame of
    /// EARS). The server refuses each other value.
    #[serde(rename = "type")]
    pub section_type: String,
}

/// Body of `POST /api/templates` (org admin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTemplateRequest {
    pub name: String,
    pub slug: String,
    /// Starter markdown content; defaults to empty.
    #[serde(default)]
    pub content: String,
    /// Metadata fields the template carries.
    #[serde(default)]
    pub metadata: Vec<TemplateMetadataEntry>,
    /// The typed sections of the template (KAIROS-T-0729). Default: none.
    /// Not sent when empty, so that a server from before the field takes
    /// the body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<TemplateSection>,
}

/// Body of `PATCH /api/templates/{id}` (org admin). Omitted fields are
/// unchanged; `metadata` replaces the full association list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
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
    /// Replacement list of the typed sections (KAIROS-T-0729); `[]`
    /// removes each one. Not sent when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<TemplateSection>>,
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
    /// The display name of the editor (KAIROS-T-0557). Null for an editor
    /// that Kairos does not know.
    #[serde(default)]
    pub edited_by_name: Option<String>,
    /// RFC 3339.
    pub edited_at: String,
    /// The agent key (UUID) of the request that made this change, when an
    /// agent made it with the agent key of a person (KAIROS-T-0359). The
    /// actor is still the person: show "Alice (agent)". Null for a change
    /// made without an agent key, and for each change made before Kairos
    /// recorded the key.
    #[serde(default)]
    pub agent_key_id: Option<String>,
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
    /// The agent key (UUID) of the request that made this change, when an
    /// agent made it with the agent key of a person (KAIROS-T-0359). The
    /// actor is still the person: show "Alice (agent)". Null for a change
    /// made without an agent key, and for each change made before Kairos
    /// recorded the key.
    #[serde(default)]
    pub agent_key_id: Option<String>,
}

/// Query of `GET /api/{entity_type}/{short_code}/history`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
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

/// Query of `GET /api/{entity_type}/{short_code}/activity` (KAIROS-T-0557).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct ItemActivityQuery {
    /// Page size (default 50, max 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

/// One row of the activity of one item, as returned by
/// `GET /api/{entity_type}/{short_code}/activity` (KAIROS-T-0557).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemActivityEntry {
    /// Row id (UUID).
    pub id: String,
    /// The action of the row, as in [`ActivityEntry::action`].
    pub action: String,
    /// The details of the row, as they are stored.
    pub details: String,
    /// The row as one line of text, with the names of the people, the
    /// boards, the columns and the repositories instead of their ids, for
    /// example `moved from Todo to Active` or `claim taken: Alice`.
    pub text: String,
    /// Who did it (user UUID).
    pub actor_id: String,
    /// The display name of the actor. Null for an actor that Kairos does
    /// not know.
    #[serde(default)]
    pub actor_name: Option<String>,
    /// The agent key (UUID) of the request, when an agent made the change
    /// with the agent key of a person (KAIROS-T-0359). Show
    /// "Alice (agent)".
    #[serde(default)]
    pub agent_key_id: Option<String>,
    /// RFC 3339.
    pub occurred_at: String,
}

// ---------------------------------------------------------------------------
// Activity log (KAIROS-A-0004 / S-0005)
// ---------------------------------------------------------------------------

/// The repository that an activity entry is about. It has enough data to
/// link the repository and the pull request or branch of a `forge_event`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActivityRepository {
    /// Tenant-unique handle.
    pub slug: String,
    /// `owner/repo` on GitHub, `group/subgroup/project` on GitLab.
    pub full_name: String,
    /// The web URL of the repository on the forge.
    pub url: String,
    /// `github`, `gitlab` or `other`.
    pub forge: String,
}

/// One `activity_log` row, as returned by `GET /api/activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActivityEntry {
    /// Row id (UUID).
    pub id: String,
    /// Who did it (user UUID).
    pub actor_id: String,
    /// `transition|create|delete|relationship_add|relationship_remove|capability_grant|capability_revoke|board_config|work_class|lifecycle|repository|board_move|restore|update|rename|claim|hand_off|release|cancel|purge`. `delete` is an archive. `cancel` is the cancel of a task, with the reason in `details` (KAIROS-T-0362). `purge` is the delete of a task for good: `details` names its code and its title.
    /// A change to a team, a delivery stream, a membership or a user has
    /// the action `update` (COLLIERY-T-0265). `claim`, `hand_off` and
    /// `release` are changes to the claim of a task (KAIROS-T-0359).
    pub action: String,
    /// The item acted on (UUID). For a relationship between two items it
    /// is the source of the edge, and `details` names the two
    /// (KAIROS-T-0638; null in older rows whose source is not known). For
    /// an `impacts` link it is the document or the ADR of the link
    /// (COLLIERY-T-0269).
    pub entity_id: Option<String>,
    /// `strategy|initiative|task|document|adr`. Null when `entity_id` is
    /// null.
    pub entity_type: Option<String>,
    /// Structured context, e.g. `"column:Draft->Active"`.
    pub details: String,
    /// RFC 3339.
    pub occurred_at: String,
    /// The short code of the item acted on (COLLIERY-T-0262), live or
    /// archived. Null when the entry is not about an item (a board, a
    /// team, a member), and when Kairos has no item with the id.
    #[serde(default)]
    pub entity_short_code: Option<String>,
    /// The title of the item acted on, as it is now. Null when
    /// `entity_short_code` is null.
    #[serde(default)]
    pub entity_title: Option<String>,
    /// When the item acted on was archived (RFC 3339). Null for a live
    /// item, and when `entity_short_code` is null.
    #[serde(default)]
    pub entity_archived_at: Option<String>,
    /// The repository of the entry, when `entity_type` is `repository`
    /// (a `forge_event`). Null for any other entry, and when Kairos has no
    /// repository with the id.
    #[serde(default)]
    pub entity_repository: Option<ActivityRepository>,
    /// The agent key (UUID) of the request that made this change, when an
    /// agent made it with the agent key of a person (KAIROS-T-0359). The
    /// actor is still the person: show "Alice (agent)". Null for a change
    /// made without an agent key, and for each change made before Kairos
    /// recorded the key.
    #[serde(default)]
    pub agent_key_id: Option<String>,
}

/// Query of `GET /api/activity` (S-0005: all filters combinable).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
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
    /// Filter: activity by the members of a team (COLLIERY-T-0265). The
    /// value is the slug or the id (UUID) of a live team. An entry passes
    /// when its actor is a member of the team at the time of the request.
    #[serde(default)]
    pub team: Option<String>,
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
                    archived_at: None,
                    done: false,
                    cancelled: false,
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
        // A LIVE neighbour carries no marker at all — `archived_at` is
        // skipped rather than serialised as null, so its mere presence
        // means "archived" to any client (KAIROS-T-0158).
        assert!(
            value["outgoing"][0]["items"][0]
                .get("archived_at")
                .is_none(),
            "a live neighbour must not carry an archived marker: {value}"
        );
        // The same for `done` (COLLIERY-T-0214): absent unless the
        // neighbour sits in a terminal column, and `true` when it does.
        assert!(
            value["outgoing"][0]["items"][0].get("done").is_none(),
            "an open neighbour must not carry a done marker: {value}"
        );
        let mut done = response.clone();
        done.outgoing[0].items[0].done = true;
        let value = serde_json::to_value(&done).expect("serializes");
        assert_eq!(value["outgoing"][0]["items"][0]["done"], true);
        let decoded: ItemRelationshipsResponse =
            serde_json::from_value(value).expect("deserializes");
        assert_eq!(decoded, done);
    }

    /// The names in the `required` list of a schema.
    fn required_of(schema: impl serde::Serialize) -> Vec<String> {
        let schema = serde_json::to_value(schema).expect("serializes");
        schema["required"]
            .as_array()
            .expect("the schema has a `required` list")
            .iter()
            .filter_map(|name| name.as_str().map(str::to_string))
            .collect()
    }

    /// COLLIERY-T-0254, the schema half: the server sends
    /// `has_done_columns` in each response, so the schema lists it as
    /// required.
    #[test]
    fn children_progress_schema_requires_has_done_columns() {
        use utoipa::PartialSchema;
        let required = required_of(ChildrenProgressResponse::schema());
        assert!(
            required.iter().any(|name| name == "has_done_columns"),
            "`has_done_columns` must be required: {required:?}"
        );
    }

    /// COLLIERY-T-0254, the read half: a response of an older server has
    /// no `has_done_columns`, and it reads as `false`.
    #[test]
    fn children_progress_without_has_done_columns_deserializes() {
        let progress: ChildrenProgressResponse = serde_json::from_value(serde_json::json!({
            "short_code": "ACME-I-0001",
            "total": 2,
            "done": 1,
            "by_column": []
        }))
        .expect("a rollup with no `has_done_columns` deserializes");
        assert!(!progress.has_done_columns);
    }

    /// COLLIERY-T-0254, the schema half: the server sends `entity_types`
    /// in each response, so the schema lists it as required.
    #[test]
    fn metadata_definition_schema_requires_entity_types() {
        use utoipa::PartialSchema;
        let required = required_of(MetadataDefinition::schema());
        assert!(
            required.iter().any(|name| name == "entity_types"),
            "`entity_types` must be required: {required:?}"
        );
    }

    /// COLLIERY-T-0254, the read half: a response of an older server has
    /// no `entity_types`, and it reads as an empty list.
    #[test]
    fn metadata_definition_without_entity_types_deserializes() {
        let definition: MetadataDefinition = serde_json::from_value(serde_json::json!({
            "id": "d",
            "name": "Priority",
            "slug": "priority",
            "field_type": "enum",
            "is_system_default": true,
            "enum_options": ["high", "low"],
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }))
        .expect("a definition with no `entity_types` deserializes");
        assert!(definition.entity_types.is_empty());
    }
}

// ---------------------------------------------------------------------------
// External references (KAIROS-T-0508, COLLIERY-I-0630)
// ---------------------------------------------------------------------------

/// One external reference of an item: a URI with a label. The API and the
/// tools show it as the relationship `external`. A URI is not an item, so
/// the reference is not an edge between two items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ExternalRef {
    /// The id of the reference (UUID). `DELETE .../external/{id}` names it.
    pub id: String,
    /// Always `external`.
    pub relationship: String,
    /// The URI, with the scheme `http`, `https` or `mailto`.
    pub uri: String,
    /// The label, when the reference has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Who made the reference (user id, UUID).
    pub created_by: String,
    /// When the reference was made, RFC 3339.
    pub created_at: String,
}

/// Body of `POST /api/{entity_type}/{short_code}/external`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateExternalRefRequest {
    /// The URI. The scheme is `http`, `https` or `mailto`; each other
    /// scheme is refused.
    pub uri: String,
    /// The label, at most 200 characters. Empty is no label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Response of `GET /api/{entity_type}/{short_code}/external`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ItemExternalRefsResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The references of the item, oldest first.
    pub external: Vec<ExternalRef>,
}

/// Response of `DELETE /api/{entity_type}/{short_code}/external/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeletedExternalRefResponse {
    /// The short code of the item.
    pub short_code: String,
    /// The URI of the reference that was removed.
    pub uri: String,
}
