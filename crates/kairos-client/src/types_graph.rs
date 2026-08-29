//! Focal-subgraph DTOs (KAIROS-T-0088, design in KAIROS-I-0008): the
//! wire contract behind `GET /api/{family}/{code}/graph?depth=N` — nodes
//! AND typed directed edges, so clients can draw a real graph instead of
//! re-deriving structure from membership lists.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One hydrated node of the focal subgraph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GraphNode {
    /// Entity id (UUID).
    pub id: String,
    pub short_code: String,
    /// `strategy|initiative|task|document|adr`.
    pub entity_type: String,
    pub title: String,
    /// Board column name for workflow items; editorial lifecycle for
    /// documents (the A-0018 two-vocabulary split); `off-board` for ADRs
    /// without a board position.
    pub status: String,
    /// Minimum hop distance from the focus (0 = the focus itself).
    pub depth: i32,
    /// The node's TOTAL live-edge count — clients render `+N` where
    /// `N = degree - edges shown` for undisplayed neighbors.
    pub degree: i64,
}

/// One typed directed edge between two returned nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GraphEdge {
    /// Source entity id (UUID).
    pub source_id: String,
    /// Target entity id (UUID).
    pub target_id: String,
    /// `parent|supports|informs|supersedes|blocks`.
    pub relationship: String,
    /// Minimum view depth at which BOTH endpoints are visible.
    pub depth: i32,
}

/// Response of `GET /api/{family}/{code}/graph`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GraphResponse {
    /// The focal item's short code.
    pub focus: String,
    /// Effective depth bound the walk used (default 2, capped at the
    /// server's MAX_TRAVERSE_DEPTH).
    pub depth: u32,
    /// Every live node within `depth` hops (any relationship type,
    /// either direction), focus included at depth 0; ordered by short
    /// code.
    pub nodes: Vec<GraphNode>,
    /// ALL live edges among the returned nodes — cross-links included,
    /// not just the discovery tree.
    pub edges: Vec<GraphEdge>,
}
