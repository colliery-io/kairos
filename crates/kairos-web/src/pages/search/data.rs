//! Data layer for the T-0042 search + relationships views: mirror DTOs
//! and domain fetchers over [`crate::api`]'s `get_json`/`post_json`/
//! `delete_json` helpers.
//!
//! Kept inside the T-0042 page module (rather than appended to the shared
//! `api.rs`) so the concurrent GUI fan-out tasks don't contend over one
//! file; the mirror-DTO rules from docs/gui-conventions.md §4 apply
//! unchanged — every mirror is partial on purpose and carries a
//! `mirror of:` line, request mirrors serialize the exact S-0005 field
//! names (the server is `deny_unknown_fields`), and decode/encode tests
//! run on the host (`cargo test -p kairos-web`).

use std::collections::BTreeMap;

use aurora_dark::tokens::ApiError;
use serde::{Deserialize, Serialize};

use crate::api;
use crate::auth::Auth;

// ---------------------------------------------------------------------------
// Short-code → API family plumbing
// ---------------------------------------------------------------------------

/// The API family (plural path segment) for a short code's type letter
/// (`PREFIX-S-0001` → `strategies`, per `kairos_core::short_code`).
pub fn family_of(short_code: &str) -> Option<&'static str> {
    let mut parts = short_code.rsplit('-');
    let _number = parts.next()?;
    match parts.next()? {
        "S" => Some("strategies"),
        "I" => Some("initiatives"),
        "T" => Some("tasks"),
        "D" => Some("documents"),
        "A" => Some("adrs"),
        _ => None,
    }
}

/// [`family_of`] as an [`ApiError`] for fetchers that need a family.
fn family_or_err(short_code: &str) -> Result<&'static str, ApiError> {
    family_of(short_code)
        .ok_or_else(|| ApiError::Unknown(format!("{short_code:?} is not a Kairos short code")))
}

// ---------------------------------------------------------------------------
// Unified search (POST /api/search, KAIROS-A-0007 / S-0005)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_search::SearchRequest` (partial —
/// `sort` omitted, the server default `created_at desc` is what the view
/// wants; the server accepts any subset).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SearchRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<SearchFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traverse: Option<SearchTraverse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
}

/// mirror of: `kairos_client::types_search::SearchFilter` (partial — only
/// the filter-builder fields; `team_id`/`is_bucket`/`include_deleted`
/// aren't in the T-0042 builder).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SearchFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_type: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_type: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<String>,
}

impl SearchFilter {
    /// Does this filter constrain anything? (An all-`None` filter is
    /// omitted from the request — the server requires at least one
    /// constraining capability.)
    pub fn is_empty(&self) -> bool {
        self == &SearchFilter::default()
    }
}

/// mirror of: `kairos_client::types_search::SearchTraverse`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchTraverse {
    pub from: SearchTraverseFrom,
    pub relationships: Vec<String>,
    pub direction: String,
    pub depth: u32,
}

/// mirror of: `kairos_client::types_search::SearchTraverseFrom` (partial —
/// the view always starts from a short code, never a raw id).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchTraverseFrom {
    pub short_code: String,
}

/// mirror of: `kairos_client::types_search::SearchResponse`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SearchResponse {
    pub results: SearchResultGroups,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// mirror of: `kairos_client::types_search::SearchResultGroups` (partial —
/// every group decodes into the one [`Hit`] row shape the list renders;
/// empty groups are omitted from the wire, hence the defaults).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct SearchResultGroups {
    #[serde(default)]
    pub strategies: Vec<Hit>,
    #[serde(default)]
    pub initiatives: Vec<Hit>,
    #[serde(default)]
    pub tasks: Vec<Hit>,
    #[serde(default)]
    pub documents: Vec<Hit>,
    #[serde(default)]
    pub adrs: Vec<Hit>,
}

/// One result row. mirror of: `kairos_client::types::{Strategy, Initiative,
/// Task, Document, Adr}` (partial — the common fields every row renders,
/// plus the per-type badges; serde ignores each type's other fields).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Hit {
    pub short_code: String,
    pub title: String,
    /// Tasks only (`task|bug|tech_debt`).
    #[serde(default)]
    pub task_type: Option<String>,
    /// Initiatives only (standing buckets, KAIROS-A-0001).
    #[serde(default)]
    pub is_bucket: Option<bool>,
    /// RFC 3339.
    pub created_at: String,
}

/// `POST /api/search`.
pub async fn search(auth: Auth, request: &SearchRequest) -> Result<SearchResponse, ApiError> {
    api::post_json(auth, "/api/search", request).await
}

// ---------------------------------------------------------------------------
// Boards (filter-builder options)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types::ListEnvelope<Board>` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardList {
    pub items: Vec<Board>,
}

/// mirror of: `kairos_client::types_org::Board` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Board {
    pub id: String,
    pub slug: String,
    pub board_level: String,
}

/// mirror of: `kairos_client::types_org::BoardDetail` (partial — only the
/// column list; the flattened board fields are ignored).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumns {
    pub columns: Vec<BoardColumn>,
}

/// mirror of: `kairos_client::types_org::BoardColumn` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BoardColumn {
    pub id: String,
    pub name: String,
}

/// `GET /api/boards` (first page is plenty — a tenant has a handful).
pub async fn boards(auth: Auth) -> Result<Vec<Board>, ApiError> {
    api::get_json::<BoardList>(auth, "/api/boards?limit=100")
        .await
        .map(|list| list.items)
}

/// `GET /api/boards/{id}` → its columns, in position order.
pub async fn board_columns(auth: Auth, board_id: &str) -> Result<Vec<BoardColumn>, ApiError> {
    api::get_json::<BoardColumns>(auth, &format!("/api/boards/{board_id}"))
        .await
        .map(|detail| detail.columns)
}

// ---------------------------------------------------------------------------
// Relationships (T-0020 endpoints, graph semantics per A-0001)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types_meta::ItemRelationshipsResponse`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ItemRelationships {
    /// Edges where the item is the SOURCE (children via `parent`, items it
    /// blocks via `blocks`, …).
    #[serde(default)]
    pub outgoing: Vec<RelationshipGroup>,
    /// Edges where the item is the TARGET (its parent via `parent`, items
    /// blocking it via `blocks`, …).
    #[serde(default)]
    pub incoming: Vec<RelationshipGroup>,
}

impl ItemRelationships {
    /// The neighbors of one relationship type in one direction.
    pub fn group(&self, relationship: &str, outgoing: bool) -> Vec<RelatedItem> {
        let groups = if outgoing {
            &self.outgoing
        } else {
            &self.incoming
        };
        groups
            .iter()
            .filter(|g| g.relationship == relationship)
            .flat_map(|g| g.items.iter().cloned())
            .collect()
    }
}

/// mirror of: `kairos_client::types_meta::RelationshipGroup`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RelationshipGroup {
    pub relationship: String,
    pub items: Vec<RelatedItem>,
}

/// mirror of: `kairos_client::types_meta::RelatedItem` (partial — `id` is
/// not needed, edges are deleted by `relationship_id`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RelatedItem {
    pub relationship_id: String,
    pub short_code: String,
    pub entity_type: String,
    pub title: String,
}

/// mirror of: `kairos_client::types_meta::CreateRelationshipRequest`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CreateRelationship {
    pub source_short_code: String,
    pub target_short_code: String,
    pub relationship: String,
}

/// mirror of: `kairos_client::types_meta::Relationship` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CreatedRelationship {
    pub id: String,
}

/// mirror of: `kairos_client::types_meta::DeletedResponse` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DeletedRelationship {
    pub id: String,
}

/// `GET /api/{family}/{short_code}/relationships`.
pub async fn relationships(auth: Auth, short_code: &str) -> Result<ItemRelationships, ApiError> {
    let family = family_or_err(short_code)?;
    api::get_json(auth, &format!("/api/{family}/{short_code}/relationships")).await
}

/// `POST /api/relationships` (org admin; the typed 422s — `RELATIONSHIP_RULE`,
/// `CYCLE_DETECTED`, `ALREADY_LINKED` — arrive as `ApiError::Http{code}`).
pub async fn create_relationship(
    auth: Auth,
    request: &CreateRelationship,
) -> Result<CreatedRelationship, ApiError> {
    api::post_json(auth, "/api/relationships", request).await
}

/// `DELETE /api/relationships/{id}` (org admin).
pub async fn delete_relationship(
    auth: Auth,
    relationship_id: &str,
) -> Result<DeletedRelationship, ApiError> {
    api::delete_json(auth, &format!("/api/relationships/{relationship_id}")).await
}

// ---------------------------------------------------------------------------
// Item summary + parent chain (the breadcrumb)
// ---------------------------------------------------------------------------

/// mirror of: `kairos_client::types::{Strategy, Initiative, Task, Document,
/// Adr}` (partial — the header fields of any `GET /api/{family}/{code}`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ItemSummary {
    pub short_code: String,
    pub title: String,
}

/// `GET /api/{family}/{short_code}` — just the header fields.
pub async fn item_summary(auth: Auth, short_code: &str) -> Result<ItemSummary, ApiError> {
    let family = family_or_err(short_code)?;
    api::get_json(auth, &format!("/api/{family}/{short_code}")).await
}

/// One breadcrumb ancestor.
#[derive(Clone, Debug, PartialEq)]
pub struct Crumb {
    pub short_code: String,
    pub entity_type: String,
    pub title: String,
}

/// Walk `parent` edges upward (each item's parent is the source of its
/// incoming `parent` edge) and return the chain root-first. Bounded and
/// cycle-guarded — `parent` is acyclic server-side (A-0001), this is just
/// client hygiene.
pub async fn parent_chain(auth: Auth, short_code: &str) -> Result<Vec<Crumb>, ApiError> {
    let mut chain: Vec<Crumb> = Vec::new();
    let mut seen: Vec<String> = vec![short_code.to_string()];
    let mut current = short_code.to_string();
    for _ in 0..10 {
        let rels = relationships(auth, &current).await?;
        let parents = rels.group("parent", false);
        let Some(parent) = parents.first() else {
            break;
        };
        if seen.contains(&parent.short_code) {
            break;
        }
        seen.push(parent.short_code.clone());
        chain.push(Crumb {
            short_code: parent.short_code.clone(),
            entity_type: parent.entity_type.clone(),
            title: parent.title.clone(),
        });
        current = parent.short_code.clone();
    }
    chain.reverse();
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Type letters map to the S-0004 families; junk maps to none.
    #[test]
    fn family_of_maps_type_letters() {
        assert_eq!(family_of("DEMO-S-0001"), Some("strategies"));
        assert_eq!(family_of("DEMO-I-0002"), Some("initiatives"));
        assert_eq!(family_of("ACME-T-0042"), Some("tasks"));
        assert_eq!(family_of("K-D-9999"), Some("documents"));
        assert_eq!(family_of("DEMO-A-0001"), Some("adrs"));
        assert_eq!(family_of("DEMO-X-0001"), None);
        assert_eq!(family_of("not a code"), None);
        assert_eq!(family_of(""), None);
    }

    /// The request mirror serializes the exact S-0005 field names (the
    /// server rejects unknown fields) and omits everything unset.
    #[test]
    fn search_request_serializes_s0005_field_names() {
        let request = SearchRequest {
            q: Some("authentication".into()),
            filter: Some(SearchFilter {
                entity_type: Some(vec!["task".into()]),
                board_id: Some("b-uuid".into()),
                column_id: Some("c-uuid".into()),
                task_type: Some(vec!["bug".into(), "tech_debt".into()]),
                metadata: Some(BTreeMap::from([("priority".into(), "critical".into())])),
                created_after: Some("2026-01-01T00:00:00Z".into()),
                created_before: Some("2026-03-01T00:00:00Z".into()),
            }),
            traverse: Some(SearchTraverse {
                from: SearchTraverseFrom {
                    short_code: "DEMO-S-0001".into(),
                },
                relationships: vec!["parent".into()],
                direction: "outbound".into(),
                depth: 3,
            }),
            limit: Some(25),
            offset: Some(0),
        };
        let value = serde_json::to_value(&request).expect("serializes");
        assert_eq!(
            value,
            serde_json::json!({
                "q": "authentication",
                "filter": {
                    "entity_type": ["task"],
                    "board_id": "b-uuid",
                    "column_id": "c-uuid",
                    "task_type": ["bug", "tech_debt"],
                    "metadata": {"priority": "critical"},
                    "created_after": "2026-01-01T00:00:00Z",
                    "created_before": "2026-03-01T00:00:00Z"
                },
                "traverse": {
                    "from": {"short_code": "DEMO-S-0001"},
                    "relationships": ["parent"],
                    "direction": "outbound",
                    "depth": 3
                },
                "limit": 25,
                "offset": 0
            })
        );

        // Unset fields vanish (a bare `{}` would be a server-side 400,
        // which the page never sends — see `SearchPage::build_request`).
        let empty = serde_json::to_value(SearchRequest::default()).expect("serializes");
        assert_eq!(empty, serde_json::json!({}));
    }

    /// The response mirror decodes a realistic grouped body — typed extra
    /// fields are ignored, omitted groups default to empty.
    #[test]
    fn search_response_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "results": {
                "tasks": [{
                    "id": "0193a1c2-0000-7000-8000-000000000009",
                    "short_code": "DEMO-T-0007",
                    "title": "Sign-up form drops UTF-8 names",
                    "content": "Names with combining characters…",
                    "board_id": "0193a1c2-0000-7000-8000-000000000001",
                    "column_id": "0193a1c2-0000-7000-8000-000000000002",
                    "task_type": "bug",
                    "team_id": null,
                    "version": 1,
                    "created_by": "u", "updated_by": "u",
                    "created_at": "2026-07-01T10:00:00Z",
                    "updated_at": "2026-07-01T10:00:00Z"
                }],
                "initiatives": [{
                    "id": "i", "short_code": "DEMO-I-0003", "title": "Bugs",
                    "content": "", "board_id": "b", "column_id": "c",
                    "complexity": null, "is_bucket": true, "bucket_type": "bug",
                    "version": 1, "created_by": "u", "updated_by": "u",
                    "created_at": "2026-07-01T09:00:00Z",
                    "updated_at": "2026-07-01T09:00:00Z"
                }]
            },
            "total": 2,
            "limit": 25,
            "offset": 0
        });
        let response: SearchResponse = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(response.total, 2);
        assert_eq!(response.results.tasks[0].short_code, "DEMO-T-0007");
        assert_eq!(response.results.tasks[0].task_type.as_deref(), Some("bug"));
        assert_eq!(response.results.initiatives[0].is_bucket, Some(true));
        assert!(response.results.strategies.is_empty());
        assert!(response.results.documents.is_empty());
        assert!(response.results.adrs.is_empty());
    }

    /// The relationships mirror decodes the T-0020 grouped shape, and
    /// `group()` picks one relationship in one direction.
    #[test]
    fn relationships_mirror_decodes_server_shape() {
        let body = serde_json::json!({
            "short_code": "DEMO-I-0002",
            "outgoing": [
                {"relationship": "parent", "items": [
                    {"relationship_id": "e1", "id": "x", "short_code": "DEMO-T-0001",
                     "entity_type": "task", "title": "Sign-up form UI skeleton"}
                ]},
                {"relationship": "supports", "items": [
                    {"relationship_id": "e2", "id": "y", "short_code": "DEMO-D-0001",
                     "entity_type": "document", "title": "PRD: Portal sign-up flow"}
                ]}
            ],
            "incoming": [
                {"relationship": "parent", "items": [
                    {"relationship_id": "e3", "id": "z", "short_code": "DEMO-S-0001",
                     "entity_type": "strategy", "title": "Self-serve customer onboarding"}
                ]}
            ]
        });
        let rels: ItemRelationships = serde_json::from_value(body).expect("mirror decodes");
        assert_eq!(rels.group("parent", true).len(), 1);
        assert_eq!(rels.group("parent", false)[0].short_code, "DEMO-S-0001");
        assert_eq!(rels.group("supports", true)[0].entity_type, "document");
        assert!(rels.group("blocks", true).is_empty());
    }
}
