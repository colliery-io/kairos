//! Unified-search wire types (KAIROS-T-0021): the `POST /api/search`
//! request and response bodies, mirroring the KAIROS-S-0005 "Unified
//! Search" section field for field (contract per KAIROS-A-0007).
//!
//! Same dependency discipline as [`crate::types`] (KAIROS-A-0015): ids
//! travel as canonical hyphenated UUID strings, timestamps as RFC 3339
//! strings, and the closed vocabularies (`entity_type`, `task_type`,
//! `relationships`, `direction`, `sort.field`, `sort.order`) as their
//! S-0005 string values. The server converts to the typed
//! `kairos_core::search` request and rejects malformed or out-of-vocabulary
//! values with 400 `VALIDATION` carrying field-level detail
//! (`details.field`).
//!
//! Request-side structs are `deny_unknown_fields` — a typo'd field is a 400
//! at the boundary, never silently ignored (matching the core model).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::{Adr, Document, Initiative, Strategy, Task};

// ---------------------------------------------------------------------------
// Request (S-0005 Unified Search, field for field)
// ---------------------------------------------------------------------------

/// Body of `POST /api/search`. All fields optional; at least one of
/// `q`/`filter`/`traverse` must be present and constraining (400
/// `VALIDATION` otherwise).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    /// Full-text search (PostgreSQL websearch semantics; quoted phrases,
    /// `OR`, `-negation`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Structured filter over entity attributes and metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<SearchFilter>,
    /// Graph traversal from a starting entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traverse: Option<SearchTraverse>,
    /// Sort order for the combined result set (default: `created_at`
    /// `desc`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<SearchSort>,
    /// Page size (default 25, max 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// Offset into the combined result set (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
}

/// The `filter` capability. Fields are AND with each other; array values
/// are OR within a field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchFilter {
    /// Restrict to these entity types
    /// (`strategy|initiative|task|document|adr`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_type: Option<Vec<String>>,
    /// Restrict to items on this board (UUID; excludes documents, which do
    /// not live on boards).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
    /// Restrict to items in this board column (UUID; excludes documents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_id: Option<String>,
    /// Restrict to tasks assigned to this team (UUID; task-level attribute,
    /// other entity types are excluded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Restrict to tasks issued against this repository (UUID; task-level
    /// attribute, other entity types are excluded — KAIROS-T-0104).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// Restrict to tasks of these types (`task|bug|tech_debt|support`;
    /// excludes non-task entities).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<Vec<String>>,
    /// Restrict to tasks in these Planned/Support lanes
    /// (`planned|support`, KAIROS-T-0077; task-level attribute, other
    /// entity types are excluded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_class: Option<Vec<String>>,
    /// Restrict to (non-)bucket initiatives (initiative-level attribute;
    /// other entity types are excluded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_bucket: Option<bool>,
    /// Metadata conditions keyed by definition slug; string values support
    /// trailing-`*` globs (e.g. `"component": "auth*"`). All entries must
    /// match (AND).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    /// Only items created strictly after this instant (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_after: Option<String>,
    /// Only items created strictly before this instant (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_before: Option<String>,
    /// Include soft-deleted items (default false).
    #[serde(default)]
    pub include_deleted: bool,
}

/// The `traverse` capability: recursive walk of the relationship graph from
/// a starting node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchTraverse {
    /// The starting entity, identified by exactly one of
    /// `short_code`/`id`.
    pub from: SearchTraverseFrom,
    /// Relationship types to follow (non-empty;
    /// `parent|supports|informs|supersedes|blocks`).
    pub relationships: Vec<String>,
    /// Edge direction(s) to walk (`outbound|inbound|both`).
    pub direction: String,
    /// Maximum traversal depth — required, `1..=10` (server-capped). Typed
    /// as optional so a missing depth is a field-level 400, not a parse
    /// error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
}

/// `traverse.from`: exactly one of `short_code`/`id`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchTraverseFrom {
    /// The starting entity's short code (e.g. `"ACME-S-0001"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_code: Option<String>,
    /// The starting entity's id (UUID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// The `sort` clause, applied to the combined cross-type result set before
/// pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchSort {
    /// `created_at|updated_at|title`.
    pub field: String,
    /// `asc|desc`.
    pub order: String,
}

// ---------------------------------------------------------------------------
// Response (S-0005: results grouped by type, empty groups omitted)
// ---------------------------------------------------------------------------

/// Response of `POST /api/search`: results grouped by entity type plus the
/// pagination envelope. `total` counts matches BEFORE `limit`/`offset` are
/// applied.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SearchResponse {
    /// Matches grouped by entity type; empty groups are omitted from the
    /// JSON.
    pub results: SearchResultGroups,
    /// Total matches before pagination.
    pub total: i64,
    /// The applied page size.
    pub limit: i64,
    /// The applied offset.
    pub offset: i64,
}

/// The `results` object: one fully-typed group per entity type, each group
/// omitted entirely when empty. Group order follows the request's combined
/// sort.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SearchResultGroups {
    /// Matching strategies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strategies: Vec<Strategy>,
    /// Matching initiatives.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub initiatives: Vec<Initiative>,
    /// Matching tasks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Task>,
    /// Matching documents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<Document>,
    /// Matching ADRs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adrs: Vec<Adr>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The S-0005 Unified Search request example parses field for field and
    /// round-trips.
    #[test]
    fn s0005_request_round_trips() {
        let json = serde_json::json!({
            "q": "authentication",
            "filter": {
                "entity_type": ["task", "initiative"],
                "board_id": "0193a1c2-0000-7000-8000-000000000001",
                "column_id": "0193a1c2-0000-7000-8000-000000000002",
                "team_id": "0193a1c2-0000-7000-8000-000000000003",
                "task_type": ["bug", "tech_debt"],
                "is_bucket": false,
                "metadata": {"priority": "critical", "component": "auth*"},
                "created_after": "2026-01-01T00:00:00Z",
                "created_before": "2026-03-01T00:00:00Z",
                "include_deleted": false
            },
            "traverse": {
                "from": {"short_code": "S-0001"},
                "relationships": ["parent"],
                "direction": "outbound",
                "depth": 3
            },
            "sort": {"field": "created_at", "order": "desc"},
            "limit": 25,
            "offset": 0
        });
        let request: SearchRequest =
            serde_json::from_value(json.clone()).expect("S-0005 example parses");
        assert_eq!(request.q.as_deref(), Some("authentication"));
        let traverse = request.traverse.as_ref().expect("traverse");
        assert_eq!(traverse.depth, Some(3));
        assert_eq!(serde_json::to_value(&request).expect("serializes"), json);
    }

    /// Unknown fields are rejected, mirroring the core model.
    #[test]
    fn unknown_fields_are_rejected() {
        assert!(
            serde_json::from_value::<SearchRequest>(serde_json::json!({"query": "x"})).is_err()
        );
        assert!(
            serde_json::from_value::<SearchRequest>(serde_json::json!({"filter": {"boards": []}}))
                .is_err()
        );
    }

    /// Empty groups vanish from the serialized response; present groups and
    /// the envelope fields survive a round trip.
    #[test]
    fn empty_groups_are_omitted() {
        let response = SearchResponse {
            results: SearchResultGroups::default(),
            total: 0,
            limit: 25,
            offset: 0,
        };
        let value = serde_json::to_value(&response).expect("serializes");
        assert_eq!(value["results"], serde_json::json!({}));
        assert_eq!(value["total"], 0);
        assert_eq!(value["limit"], 25);
        assert_eq!(value["offset"], 0);
        let back: SearchResponse = serde_json::from_value(value).expect("deserializes");
        assert_eq!(back, response);
    }
}
