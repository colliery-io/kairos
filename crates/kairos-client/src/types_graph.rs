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
    /// The node's TOTAL edge count — clients render `+N` where
    /// `N = degree - edges shown` for undisplayed neighbors. Archived
    /// neighbours count, because they are drawn.
    pub degree: i64,
    /// When this node was archived, RFC 3339; absent while it is live.
    /// The subgraph is archived-INCLUSIVE (KAIROS-T-0158) — omitting a
    /// node used to break the paths THROUGH it, leaving the far side
    /// floating with no route back to the focus. Clients must draw a
    /// marked node distinctly; drawing it as live is the one wrong
    /// answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// `true` while this node is in a terminal column, a column with
    /// `is_done` (COLLIERY-T-0233). Done work does not block, and nothing
    /// blocks done work: a `blocks` edge with a done node at one end is
    /// history. Clients must draw that edge differently from an open
    /// blocker. `false` for a node with no column.
    ///
    /// The server sends it in each response, so the schema shows it as
    /// required (COLLIERY-T-0247). The default is for the client only: a
    /// response of an older server has no `done`, and it reads as `false`.
    #[serde(default)]
    #[schema(required = true)]
    pub done: bool,
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
    /// Every node within `depth` hops (any relationship type, either
    /// direction), focus included at depth 0; ordered by short code.
    /// Archived nodes are present and marked (`archived_at`), never
    /// dropped.
    pub nodes: Vec<GraphNode>,
    /// ALL edges among the returned nodes — cross-links included, not
    /// just the discovery tree.
    pub edges: Vec<GraphEdge>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::PartialSchema;

    /// COLLIERY-T-0247, the schema half: the server sends `done` in each
    /// response, so the schema lists it as required. `archived_at` is
    /// absent on a live node, so it is not in the list.
    #[test]
    fn graph_node_schema_requires_done() {
        let schema = serde_json::to_value(GraphNode::schema()).expect("serializes");
        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("the schema has a `required` list")
            .iter()
            .filter_map(|name| name.as_str())
            .collect();
        assert!(
            required.contains(&"done"),
            "`done` must be required: {required:?}"
        );
        assert!(
            !required.contains(&"archived_at"),
            "`archived_at` must stay optional: {required:?}"
        );
    }

    /// COLLIERY-T-0247, the read half: a response of an older server has no
    /// `done`. The client reads it, and the node is not done.
    #[test]
    fn graph_node_without_done_deserializes() {
        let node: GraphNode = serde_json::from_value(serde_json::json!({
            "id": "i",
            "short_code": "ACME-T-0001",
            "entity_type": "task",
            "title": "t",
            "status": "Todo",
            "depth": 0,
            "degree": 1
        }))
        .expect("a node with no `done` deserializes");
        assert!(!node.done);
        assert_eq!(node.archived_at, None);
    }
}
