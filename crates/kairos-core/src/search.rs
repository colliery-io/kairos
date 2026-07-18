//! Pure unified-search request semantics (KAIROS-A-0007, KAIROS-T-0014):
//! the typed `POST /api/search` request model (mirroring the KAIROS-S-0005
//! Unified Search shape exactly), request validation, server caps, and the
//! metadata-glob LIKE translation.
//!
//! Per KAIROS-A-0009 everything here is a function over in-memory data — no
//! diesel, no I/O. `kairos-db::search::execute_search` validates a request
//! through [`validate`] and then runs the A-0007 pipeline (traverse → type
//! resolution → bounded hydration → filter → full-text → sort/paginate).
//!
//! # Validation contract
//!
//! - At least one of `q`/`filter`/`traverse` must be present and
//!   *constraining*: a blank `q` or a `filter` with no constraining field
//!   does not count ([`SearchValidationError::NoCapability`]).
//! - `traverse.depth` is required (A-0007) and server-capped at
//!   [`MAX_TRAVERSE_DEPTH`]; `traverse.from` names exactly one of
//!   `short_code`/`id`; `traverse.relationships` must be non-empty.
//! - `limit` is capped at [`MAX_LIMIT`] (default [`DEFAULT_LIMIT`]);
//!   `offset` must be non-negative.
//! - Date ranges must be sane: `created_after < created_before` when both
//!   are given.
//! - Unknown entity types, task types, relationships, directions, and sort
//!   fields are unrepresentable: the vocabulary is typed enums, so serde
//!   rejects them at the boundary.
//!
//! # Metadata globs (the T-0011 LIKE translation, exactly)
//!
//! Metadata string values support glob patterns with `*`
//! (KAIROS-A-0007/S-0005, e.g. `"component": "auth*"`). The sanctioned glob
//! position is a trailing `*`; [`metadata_like_pattern`] mirrors the
//! KAIROS-T-0011 capability-glob translation: LIKE's own metacharacters
//! (`%`, `_`, `\`) in the value are escaped so they match themselves
//! literally, then every `*` becomes `%`. A value without `*` therefore
//! translates to a LIKE pattern equivalent to equality.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::short_code::ItemType;

/// Server cap on `traverse.depth` (A-0007: "server-capped to prevent
/// runaway queries").
pub const MAX_TRAVERSE_DEPTH: u32 = 10;

/// Server cap on `limit`.
pub const MAX_LIMIT: i64 = 100;

/// Default `limit` when the request omits it (the S-0005 examples' page
/// size).
pub const DEFAULT_LIMIT: i64 = 25;

// ---------------------------------------------------------------------------
// Request model (S-0005 Unified Search, field for field)
// ---------------------------------------------------------------------------

/// The `POST /api/search` request body (KAIROS-A-0007 / S-0005). All
/// top-level fields are optional; at least one of `q`/`filter`/`traverse`
/// must be present ([`validate`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    /// Full-text search over the `searchable_items` view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Structured filter over entity attributes and metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<SearchFilter>,
    /// Graph traversal over `item_relationships`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traverse: Option<Traverse>,
    /// Sort order for the combined result set (default: `created_at desc`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<Sort>,
    /// Page size (default [`DEFAULT_LIMIT`], capped at [`MAX_LIMIT`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// Offset into the combined result set (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
}

impl SearchRequest {
    /// The page size to use: `limit` or [`DEFAULT_LIMIT`].
    pub fn effective_limit(&self) -> i64 {
        self.limit.unwrap_or(DEFAULT_LIMIT)
    }

    /// The offset to use: `offset` or 0.
    pub fn effective_offset(&self) -> i64 {
        self.offset.unwrap_or(0)
    }

    /// The sort to use: `sort` or `created_at desc`.
    pub fn effective_sort(&self) -> Sort {
        self.sort.unwrap_or(Sort {
            field: SortField::CreatedAt,
            order: SortOrder::Desc,
        })
    }
}

/// The `filter` capability (A-0007: fields are AND with each other; array
/// values are OR within a field).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchFilter {
    /// Restrict to these entity types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_type: Option<Vec<SearchEntityType>>,
    /// Restrict to items on this board (excludes documents, which do not
    /// live on boards).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_id: Option<Uuid>,
    /// Restrict to items in this board column (excludes documents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_id: Option<Uuid>,
    /// Restrict to tasks assigned to this team (`team_id` is a task-level
    /// attribute; other entity types are excluded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<Uuid>,
    /// Restrict to tasks of these types (excludes non-task entities).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<Vec<SearchTaskType>>,
    /// Restrict to (non-)bucket initiatives (`is_bucket` is an
    /// initiative-level attribute; other entity types are excluded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_bucket: Option<bool>,
    /// Metadata equality/glob conditions, keyed by `metadata_definitions`
    /// slug; string values support trailing-`*` globs (module docs). All
    /// entries must match (AND).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    /// Only items created strictly after this instant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_after: Option<DateTime<Utc>>,
    /// Only items created strictly before this instant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_before: Option<DateTime<Utc>>,
    /// Include soft-deleted items (default false — soft-deleted are
    /// excluded everywhere by default).
    #[serde(default)]
    pub include_deleted: bool,
}

impl SearchFilter {
    /// Whether this filter narrows results at all. `include_deleted` alone
    /// does not constrain — it widens — so a filter carrying only it does
    /// not satisfy the at-least-one-capability rule.
    pub fn is_constraining(&self) -> bool {
        self.entity_type.is_some()
            || self.board_id.is_some()
            || self.column_id.is_some()
            || self.team_id.is_some()
            || self.task_type.is_some()
            || self.is_bucket.is_some()
            || self.metadata.as_ref().is_some_and(|m| !m.is_empty())
            || self.created_after.is_some()
            || self.created_before.is_some()
    }
}

/// The `filter.entity_type` vocabulary (the five S-0004 entity tables).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchEntityType {
    /// `strategies`.
    Strategy,
    /// `initiatives`.
    Initiative,
    /// `tasks`.
    Task,
    /// `documents`.
    Document,
    /// `adrs`.
    Adr,
}

impl SearchEntityType {
    /// The corresponding [`ItemType`].
    pub fn item_type(self) -> ItemType {
        match self {
            SearchEntityType::Strategy => ItemType::Strategy,
            SearchEntityType::Initiative => ItemType::Initiative,
            SearchEntityType::Task => ItemType::Task,
            SearchEntityType::Document => ItemType::Document,
            SearchEntityType::Adr => ItemType::Adr,
        }
    }
}

/// The `filter.task_type` vocabulary (`tasks.task_type` CHECK set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchTaskType {
    /// `'task'`.
    Task,
    /// `'bug'`.
    Bug,
    /// `'tech_debt'`.
    TechDebt,
}

impl SearchTaskType {
    /// The TEXT value stored in `tasks.task_type`.
    pub fn as_str(self) -> &'static str {
        match self {
            SearchTaskType::Task => "task",
            SearchTaskType::Bug => "bug",
            SearchTaskType::TechDebt => "tech_debt",
        }
    }
}

/// The `traverse` capability (A-0007: recursive walk of
/// `item_relationships` from a starting node).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traverse {
    /// The starting entity, identified by `short_code` or `id` (exactly
    /// one).
    pub from: TraverseFrom,
    /// Which relationship types to follow (non-empty).
    pub relationships: Vec<SearchRelationship>,
    /// Which edge direction(s) to walk.
    pub direction: Direction,
    /// Maximum traversal depth — required (A-0007), `1..=`
    /// [`MAX_TRAVERSE_DEPTH`]. `Option` so a missing depth is the typed
    /// [`SearchValidationError::TraverseDepthRequired`] rather than a serde
    /// error.
    #[serde(default)]
    pub depth: Option<u32>,
}

/// `traverse.from`: exactly one of `short_code`/`id` (enforced by
/// [`validate`], not serde, so the error is typed).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraverseFrom {
    /// The starting entity's short code (e.g. `"ACME-S-0001"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_code: Option<String>,
    /// The starting entity's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
}

/// The `traverse.relationships` vocabulary (`item_relationships.
/// relationship` CHECK set, KAIROS-A-0001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchRelationship {
    /// `'parent'` (strategy → initiative, initiative → task).
    Parent,
    /// `'supports'` (workflow item → document/ADR).
    Supports,
    /// `'informs'` (document/ADR → workflow item).
    Informs,
    /// `'supersedes'` (ADR → ADR).
    Supersedes,
    /// `'blocks'` (workflow item → workflow item).
    Blocks,
}

impl SearchRelationship {
    /// The TEXT value stored in `item_relationships.relationship`.
    pub fn as_str(self) -> &'static str {
        match self {
            SearchRelationship::Parent => "parent",
            SearchRelationship::Supports => "supports",
            SearchRelationship::Informs => "informs",
            SearchRelationship::Supersedes => "supersedes",
            SearchRelationship::Blocks => "blocks",
        }
    }
}

/// `traverse.direction` (A-0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Follow edges source → target.
    Outbound,
    /// Follow edges target → source.
    Inbound,
    /// Follow edges in both directions.
    Both,
}

/// The `sort` clause; applies to the combined cross-type result set before
/// pagination (A-0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sort {
    /// The column to sort by.
    pub field: SortField,
    /// Ascending or descending.
    pub order: SortOrder,
}

/// Sortable fields — attributes every entity type carries, so the combined
/// sort is total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    /// `created_at`.
    CreatedAt,
    /// `updated_at`.
    UpdatedAt,
    /// `title` (lexicographic, case-sensitive).
    Title,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// A structurally invalid search request (HTTP 400 at the API layer).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SearchValidationError {
    /// None of `q`/`filter`/`traverse` is present and constraining.
    #[error("at least one of q, filter, or traverse must be provided")]
    NoCapability,
    /// `q` is present but blank.
    #[error("q must not be blank")]
    BlankQuery,
    /// `filter.entity_type` is present but empty (would match nothing by
    /// accident; an explicit empty OR-list is almost certainly a bug).
    #[error("filter.entity_type must not be an empty list")]
    EmptyEntityTypes,
    /// `filter.task_type` is present but empty.
    #[error("filter.task_type must not be an empty list")]
    EmptyTaskTypes,
    /// A `filter.metadata` key is blank.
    #[error("filter.metadata keys must not be blank")]
    BlankMetadataKey,
    /// `created_after`/`created_before` do not form a sane range.
    #[error("created_after ({after}) must be before created_before ({before})")]
    InvertedDateRange {
        /// The submitted `created_after`.
        after: DateTime<Utc>,
        /// The submitted `created_before`.
        before: DateTime<Utc>,
    },
    /// `traverse.from` names neither `short_code` nor `id`.
    #[error("traverse.from must name a short_code or an id")]
    TraverseFromMissing,
    /// `traverse.from` names both `short_code` and `id`.
    #[error("traverse.from must name exactly one of short_code or id, not both")]
    TraverseFromAmbiguous,
    /// `traverse.relationships` is empty.
    #[error("traverse.relationships must not be empty")]
    NoRelationships,
    /// `traverse.depth` is missing (required per A-0007).
    #[error("traverse.depth is required")]
    TraverseDepthRequired,
    /// `traverse.depth` is outside `1..=` [`MAX_TRAVERSE_DEPTH`].
    #[error("traverse.depth {depth} is out of range (must be 1..={cap})")]
    TraverseDepthOutOfRange {
        /// The submitted depth.
        depth: u32,
        /// The server cap ([`MAX_TRAVERSE_DEPTH`]).
        cap: u32,
    },
    /// `limit` is outside `1..=` [`MAX_LIMIT`].
    #[error("limit {limit} is out of range (must be 1..={cap})")]
    LimitOutOfRange {
        /// The submitted limit.
        limit: i64,
        /// The server cap ([`MAX_LIMIT`]).
        cap: i64,
    },
    /// `offset` is negative.
    #[error("offset {offset} must not be negative")]
    NegativeOffset {
        /// The submitted offset.
        offset: i64,
    },
}

/// Validate a [`SearchRequest`] against the module-docs contract. Pure;
/// `kairos-db::search::execute_search` calls this before touching the
/// database.
pub fn validate(request: &SearchRequest) -> Result<(), SearchValidationError> {
    if let Some(q) = &request.q
        && q.trim().is_empty()
    {
        return Err(SearchValidationError::BlankQuery);
    }

    if let Some(filter) = &request.filter {
        if let Some(types) = &filter.entity_type
            && types.is_empty()
        {
            return Err(SearchValidationError::EmptyEntityTypes);
        }
        if let Some(types) = &filter.task_type
            && types.is_empty()
        {
            return Err(SearchValidationError::EmptyTaskTypes);
        }
        if let Some(metadata) = &filter.metadata
            && metadata.keys().any(|k| k.trim().is_empty())
        {
            return Err(SearchValidationError::BlankMetadataKey);
        }
        if let (Some(after), Some(before)) = (filter.created_after, filter.created_before)
            && after >= before
        {
            return Err(SearchValidationError::InvertedDateRange { after, before });
        }
    }

    if let Some(traverse) = &request.traverse {
        match (&traverse.from.short_code, &traverse.from.id) {
            (None, None) => return Err(SearchValidationError::TraverseFromMissing),
            (Some(_), Some(_)) => return Err(SearchValidationError::TraverseFromAmbiguous),
            _ => {}
        }
        if traverse.relationships.is_empty() {
            return Err(SearchValidationError::NoRelationships);
        }
        match traverse.depth {
            None => return Err(SearchValidationError::TraverseDepthRequired),
            Some(depth) if depth == 0 || depth > MAX_TRAVERSE_DEPTH => {
                return Err(SearchValidationError::TraverseDepthOutOfRange {
                    depth,
                    cap: MAX_TRAVERSE_DEPTH,
                });
            }
            Some(_) => {}
        }
    }

    if let Some(limit) = request.limit
        && !(1..=MAX_LIMIT).contains(&limit)
    {
        return Err(SearchValidationError::LimitOutOfRange {
            limit,
            cap: MAX_LIMIT,
        });
    }
    if let Some(offset) = request.offset
        && offset < 0
    {
        return Err(SearchValidationError::NegativeOffset { offset });
    }

    let has_q = request.q.is_some(); // blank q already rejected above
    let has_filter = request
        .filter
        .as_ref()
        .is_some_and(SearchFilter::is_constraining);
    let has_traverse = request.traverse.is_some();
    if !(has_q || has_filter || has_traverse) {
        return Err(SearchValidationError::NoCapability);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Metadata glob translation (T-0011 semantics)
// ---------------------------------------------------------------------------

/// Translate a metadata filter value into a SQL LIKE pattern (module docs):
/// escape LIKE's metacharacters (`\`, `%`, `_`) so they match themselves,
/// then turn every `*` into `%`. PostgreSQL's default LIKE escape character
/// is `\`, so the result is used with a plain `LIKE`.
pub fn metadata_like_pattern(value: &str) -> String {
    let mut pattern = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => pattern.push_str("\\\\"),
            '%' => pattern.push_str("\\%"),
            '_' => pattern.push_str("\\_"),
            '*' => pattern.push('%'),
            other => pattern.push(other),
        }
    }
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> SearchRequest {
        SearchRequest {
            q: Some(text.to_string()),
            ..Default::default()
        }
    }

    fn traverse(depth: Option<u32>) -> Traverse {
        Traverse {
            from: TraverseFrom {
                short_code: Some("ACME-S-0001".into()),
                id: None,
            },
            relationships: vec![SearchRelationship::Parent],
            direction: Direction::Outbound,
            depth,
        }
    }

    // -- at-least-one capability ------------------------------------------------

    #[test]
    fn empty_request_is_no_capability() {
        assert_eq!(
            validate(&SearchRequest::default()),
            Err(SearchValidationError::NoCapability)
        );
    }

    #[test]
    fn non_constraining_filter_is_no_capability() {
        // A bare `{}` filter and an `include_deleted`-only filter widen, not
        // narrow: they do not count as a capability.
        for filter in [
            SearchFilter::default(),
            SearchFilter {
                include_deleted: true,
                ..Default::default()
            },
            SearchFilter {
                metadata: Some(BTreeMap::new()),
                ..Default::default()
            },
        ] {
            let request = SearchRequest {
                filter: Some(filter),
                ..Default::default()
            };
            assert_eq!(validate(&request), Err(SearchValidationError::NoCapability));
        }
    }

    #[test]
    fn each_capability_alone_is_valid() {
        assert_eq!(validate(&q("authentication")), Ok(()));
        let filtered = SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec![SearchEntityType::Task]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(validate(&filtered), Ok(()));
        let traversed = SearchRequest {
            traverse: Some(traverse(Some(3))),
            ..Default::default()
        };
        assert_eq!(validate(&traversed), Ok(()));
    }

    #[test]
    fn blank_q_is_rejected_even_alongside_other_capabilities() {
        assert_eq!(validate(&q("")), Err(SearchValidationError::BlankQuery));
        assert_eq!(validate(&q("   ")), Err(SearchValidationError::BlankQuery));
        let mut request = q("  ");
        request.traverse = Some(traverse(Some(1)));
        assert_eq!(validate(&request), Err(SearchValidationError::BlankQuery));
    }

    // -- filter field validation ----------------------------------------------

    #[test]
    fn empty_or_lists_are_rejected() {
        let request = SearchRequest {
            filter: Some(SearchFilter {
                entity_type: Some(vec![]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            validate(&request),
            Err(SearchValidationError::EmptyEntityTypes)
        );
        let request = SearchRequest {
            filter: Some(SearchFilter {
                task_type: Some(vec![]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            validate(&request),
            Err(SearchValidationError::EmptyTaskTypes)
        );
    }

    #[test]
    fn blank_metadata_keys_are_rejected() {
        let request = SearchRequest {
            filter: Some(SearchFilter {
                metadata: Some(BTreeMap::from([(" ".to_string(), "x".to_string())])),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            validate(&request),
            Err(SearchValidationError::BlankMetadataKey)
        );
    }

    #[test]
    fn date_ranges_must_be_sane() {
        let after: DateTime<Utc> = "2026-03-01T00:00:00Z".parse().unwrap();
        let before: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
        let request = SearchRequest {
            filter: Some(SearchFilter {
                created_after: Some(after),
                created_before: Some(before),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            validate(&request),
            Err(SearchValidationError::InvertedDateRange { after, before })
        );
        // Equal endpoints are an empty (insane) range too.
        let request = SearchRequest {
            filter: Some(SearchFilter {
                created_after: Some(after),
                created_before: Some(after),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(matches!(
            validate(&request),
            Err(SearchValidationError::InvertedDateRange { .. })
        ));
        // Open-ended ranges are fine.
        let request = SearchRequest {
            filter: Some(SearchFilter {
                created_after: Some(after),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(validate(&request), Ok(()));
    }

    // -- traverse validation -----------------------------------------------------

    #[test]
    fn traverse_depth_is_required_and_capped() {
        let request = |depth| SearchRequest {
            traverse: Some(traverse(depth)),
            ..Default::default()
        };
        assert_eq!(
            validate(&request(None)),
            Err(SearchValidationError::TraverseDepthRequired)
        );
        assert_eq!(
            validate(&request(Some(0))),
            Err(SearchValidationError::TraverseDepthOutOfRange { depth: 0, cap: 10 })
        );
        assert_eq!(
            validate(&request(Some(MAX_TRAVERSE_DEPTH + 1))),
            Err(SearchValidationError::TraverseDepthOutOfRange { depth: 11, cap: 10 })
        );
        assert_eq!(validate(&request(Some(1))), Ok(()));
        assert_eq!(validate(&request(Some(MAX_TRAVERSE_DEPTH))), Ok(()));
    }

    #[test]
    fn traverse_from_names_exactly_one_reference() {
        let request = |from| SearchRequest {
            traverse: Some(Traverse {
                from,
                relationships: vec![SearchRelationship::Parent],
                direction: Direction::Outbound,
                depth: Some(1),
            }),
            ..Default::default()
        };
        assert_eq!(
            validate(&request(TraverseFrom::default())),
            Err(SearchValidationError::TraverseFromMissing)
        );
        assert_eq!(
            validate(&request(TraverseFrom {
                short_code: Some("ACME-S-0001".into()),
                id: Some(Uuid::new_v4()),
            })),
            Err(SearchValidationError::TraverseFromAmbiguous)
        );
        assert_eq!(
            validate(&request(TraverseFrom {
                short_code: None,
                id: Some(Uuid::new_v4()),
            })),
            Ok(())
        );
    }

    #[test]
    fn traverse_relationships_must_be_non_empty() {
        let request = SearchRequest {
            traverse: Some(Traverse {
                relationships: vec![],
                ..traverse(Some(1))
            }),
            ..Default::default()
        };
        assert_eq!(
            validate(&request),
            Err(SearchValidationError::NoRelationships)
        );
    }

    // -- limit / offset -----------------------------------------------------------

    #[test]
    fn limit_is_capped_and_offset_non_negative() {
        let request = |limit, offset| SearchRequest {
            limit,
            offset,
            ..q("x")
        };
        assert_eq!(
            validate(&request(Some(0), None)),
            Err(SearchValidationError::LimitOutOfRange { limit: 0, cap: 100 })
        );
        assert_eq!(
            validate(&request(Some(MAX_LIMIT + 1), None)),
            Err(SearchValidationError::LimitOutOfRange {
                limit: 101,
                cap: 100
            })
        );
        assert_eq!(
            validate(&request(Some(-5), None)),
            Err(SearchValidationError::LimitOutOfRange {
                limit: -5,
                cap: 100
            })
        );
        assert_eq!(
            validate(&request(None, Some(-1))),
            Err(SearchValidationError::NegativeOffset { offset: -1 })
        );
        assert_eq!(validate(&request(Some(MAX_LIMIT), Some(0))), Ok(()));
    }

    #[test]
    fn effective_defaults() {
        let request = q("x");
        assert_eq!(request.effective_limit(), DEFAULT_LIMIT);
        assert_eq!(request.effective_offset(), 0);
        let sort = request.effective_sort();
        assert_eq!(sort.field, SortField::CreatedAt);
        assert_eq!(sort.order, SortOrder::Desc);
    }

    // -- serde shape (S-0005 field for field) -------------------------------------

    #[test]
    fn full_s0005_request_deserializes() {
        // The S-0005 "Unified Search" request example, verbatim (uuids made
        // concrete).
        let json = r#"{
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
        }"#;
        let request: SearchRequest = serde_json::from_str(json).expect("S-0005 example parses");
        assert_eq!(validate(&request), Ok(()));
        let filter = request.filter.as_ref().unwrap();
        assert_eq!(
            filter.entity_type,
            Some(vec![SearchEntityType::Task, SearchEntityType::Initiative])
        );
        assert_eq!(
            filter.task_type,
            Some(vec![SearchTaskType::Bug, SearchTaskType::TechDebt])
        );
        assert!(!filter.include_deleted);
        let traverse = request.traverse.as_ref().unwrap();
        assert_eq!(traverse.from.short_code.as_deref(), Some("S-0001"));
        assert_eq!(traverse.relationships, vec![SearchRelationship::Parent]);
        assert_eq!(traverse.direction, Direction::Outbound);
        assert_eq!(traverse.depth, Some(3));
        // Round-trips.
        let json = serde_json::to_string(&request).unwrap();
        let back: SearchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, request);
    }

    #[test]
    fn unknown_vocabulary_is_rejected_by_serde() {
        assert!(
            serde_json::from_str::<SearchRequest>(r#"{"filter": {"entity_type": ["epic"]}}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<SearchRequest>(r#"{"filter": {"task_type": ["story"]}}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<SearchRequest>(
                r#"{"traverse": {"from": {"id": "0193a1c2-0000-7000-8000-000000000001"},
                "relationships": ["depends_on"], "direction": "outbound", "depth": 1}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<SearchRequest>(
                r#"{"traverse": {"from": {"id": "0193a1c2-0000-7000-8000-000000000001"},
                "relationships": ["parent"], "direction": "sideways", "depth": 1}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<SearchRequest>(r#"{"sort": {"field": "id", "order": "asc"}}"#)
                .is_err()
        );
        // Unknown top-level / nested fields are typos, not silently ignored.
        assert!(serde_json::from_str::<SearchRequest>(r#"{"query": "auth"}"#).is_err());
        assert!(serde_json::from_str::<SearchRequest>(r#"{"filter": {"boards": []}}"#).is_err());
    }

    // -- metadata glob translation (T-0011 semantics) ------------------------------

    #[test]
    fn metadata_like_pattern_translates_globs_and_escapes_metacharacters() {
        // Trailing glob — the sanctioned position.
        assert_eq!(metadata_like_pattern("auth*"), "auth%");
        // No glob: equivalent to equality under LIKE.
        assert_eq!(metadata_like_pattern("critical"), "critical");
        // LIKE metacharacters in the value match themselves literally.
        assert_eq!(metadata_like_pattern("100%"), "100\\%");
        assert_eq!(metadata_like_pattern("a_b"), "a\\_b");
        assert_eq!(metadata_like_pattern("a\\b"), "a\\\\b");
        // Every '*' translates (mirroring the T-0011 SQL translation), even
        // mid-string.
        assert_eq!(metadata_like_pattern("a*b*"), "a%b%");
        assert_eq!(metadata_like_pattern("*"), "%");
    }
}
