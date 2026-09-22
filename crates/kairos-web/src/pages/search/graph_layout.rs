//! Deterministic layered layout for the focal flight-level graph
//! (KAIROS-T-0089, design in KAIROS-I-0008). Pure code — no leptos, no
//! DOM — so determinism is provable with native unit tests.
//!
//! The strict A-0001 type matrix makes this cheap: `ItemType` statically
//! assigns the layer (Strategy | Initiative | Task fixed columns — no
//! layer-assignment algorithm), so the only decisions are ORDER within a
//! column (one barycenter pass over visible parents, ties broken by
//! short code) and lane grouping (parent = containment, never arrows).
//! Documents and ADRs are never placed — they live in the side panel.
//!
//! No force-directed anything: identical input yields identical geometry,
//! so positions build spatial memory and survive WS refetches unchanged.

/// The three canvas columns, in fixed left-to-right order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Strategy,
    Initiative,
    Task,
}

impl Column {
    fn index(self) -> usize {
        match self {
            Column::Strategy => 0,
            Column::Initiative => 1,
            Column::Task => 2,
        }
    }
}

/// One node the layout places (the component maps wire mirrors to this).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutInputNode {
    pub id: String,
    pub short_code: String,
    pub column: Column,
    /// Neighbors that exist but are NOT YET FETCHED (`degree` minus every
    /// incident edge in the merged response — side-panel material counts
    /// as fetched, so `+N` never advertises an expansion that would add
    /// nothing). Computed by the component; the layout passes it through.
    pub hidden_neighbors: i64,
}

/// One edge the layout considers (already filtered to canvas nodes).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutInputEdge {
    pub source_id: String,
    pub target_id: String,
    /// `parent` edges become containment; `blocks` become arrows; other
    /// relationships are ignored here (side panel).
    pub relationship: String,
}

/// A placed node box.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedNode {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Neighbors that exist (per `degree`) but are not on the canvas —
    /// rendered as a `+N` badge when positive.
    pub hidden_neighbors: i64,
}

/// A containment lane: the band a visible parent draws around its
/// children in the next column.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLane {
    pub parent_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A blocks arrow, as a ready-to-render SVG path between box edges.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedArrow {
    pub source_id: String,
    pub target_id: String,
    pub path: String,
}

/// The finished geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphLayout {
    pub width: f64,
    pub height: f64,
    /// `(label, x)` for the three column headers.
    pub headers: Vec<(&'static str, f64)>,
    pub lanes: Vec<PlacedLane>,
    pub nodes: Vec<PlacedNode>,
    pub arrows: Vec<PlacedArrow>,
}

pub const NODE_W: f64 = 220.0;
pub const NODE_H: f64 = 56.0;
const COLUMN_GAP: f64 = 90.0;
const ROW_GAP: f64 = 18.0;
const LANE_PAD: f64 = 8.0;
const MARGIN_X: f64 = 24.0;
const HEADER_H: f64 = 34.0;
/// Extra vertical gap between lane groups so bands never touch.
const GROUP_GAP: f64 = 26.0;

fn column_x(column: Column) -> f64 {
    MARGIN_X + column.index() as f64 * (NODE_W + COLUMN_GAP)
}

/// Order a column: barycenter over the average placed-parent row, ties
/// (and parentless nodes) by short code. `parent_row` maps node id → the
/// mean row index of its visible parents in the PREVIOUS column.
fn order_column(
    mut ids: Vec<usize>,
    nodes: &[LayoutInputNode],
    parent_row: &std::collections::HashMap<&str, f64>,
) -> Vec<usize> {
    ids.sort_by(|&a, &b| {
        let bary_a = parent_row.get(nodes[a].id.as_str()).copied();
        let bary_b = parent_row.get(nodes[b].id.as_str()).copied();
        match (bary_a, bary_b) {
            (Some(x), Some(y)) => x
                .partial_cmp(&y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| nodes[a].short_code.cmp(&nodes[b].short_code)),
            // Parented nodes sort before orphans; orphans by short code.
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => nodes[a].short_code.cmp(&nodes[b].short_code),
        }
    });
    ids
}

/// Lay out the subgraph. Deterministic: same input, same geometry.
pub fn layout(nodes: &[LayoutInputNode], edges: &[LayoutInputEdge]) -> GraphLayout {
    use std::collections::HashMap;

    let index_of: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();
    // Visible parent(s) per node (parent edges between canvas nodes).
    let mut parents_of: HashMap<&str, Vec<usize>> = HashMap::new();
    for edge in edges.iter().filter(|e| e.relationship == "parent") {
        if let (Some(&parent), Some(&child)) = (
            index_of.get(edge.source_id.as_str()),
            index_of.get(edge.target_id.as_str()),
        ) {
            parents_of
                .entry(nodes[child].id.as_str())
                .or_default()
                .push(parent);
        }
    }

    let ids_in = |column: Column| -> Vec<usize> {
        nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.column == column)
            .map(|(index, _)| index)
            .collect()
    };

    // --- column ordering ----------------------------------------------------
    // Strategies: short code (no in-canvas parents).
    let strategies = order_column(ids_in(Column::Strategy), nodes, &HashMap::new());
    let strategy_row: HashMap<&str, f64> = strategies
        .iter()
        .enumerate()
        .map(|(row, &index)| (nodes[index].id.as_str(), row as f64))
        .collect();
    // Initiatives: barycenter over strategy rows.
    let initiative_bary: HashMap<&str, f64> = nodes
        .iter()
        .filter(|node| node.column == Column::Initiative)
        .filter_map(|node| {
            let parents = parents_of.get(node.id.as_str())?;
            let rows: Vec<f64> = parents
                .iter()
                .filter_map(|&p| strategy_row.get(nodes[p].id.as_str()).copied())
                .collect();
            (!rows.is_empty()).then(|| {
                (
                    node.id.as_str(),
                    rows.iter().sum::<f64>() / rows.len() as f64,
                )
            })
        })
        .collect();
    let initiatives = order_column(ids_in(Column::Initiative), nodes, &initiative_bary);

    // --- vertical placement with lane grouping ------------------------------
    // Tasks group CONTIGUOUSLY under their (first, by initiative order)
    // visible initiative parent; ungrouped tasks trail, by short code.
    let initiative_order: HashMap<&str, usize> = initiatives
        .iter()
        .enumerate()
        .map(|(row, &index)| (nodes[index].id.as_str(), row))
        .collect();
    let mut task_groups: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
    {
        let mut grouped: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
        for index in ids_in(Column::Task) {
            let group = parents_of
                .get(nodes[index].id.as_str())
                .and_then(|parents| {
                    parents
                        .iter()
                        .filter_map(|&p| initiative_order.get(nodes[p].id.as_str()).copied())
                        .min()
                });
            grouped.entry(group).or_default().push(index);
        }
        let mut keys: Vec<Option<usize>> = grouped.keys().copied().collect();
        // Grouped tasks in initiative order; the orphan group (None) last.
        keys.sort_by_key(|key| (key.is_none(), key.unwrap_or(usize::MAX)));
        for key in keys {
            let mut members = grouped.remove(&key).unwrap_or_default();
            members.sort_by(|&a, &b| nodes[a].short_code.cmp(&nodes[b].short_code));
            task_groups.push((key, members));
        }
    }

    // Same banding for initiatives under strategies.
    let strategy_order: HashMap<&str, usize> = strategies
        .iter()
        .enumerate()
        .map(|(row, &index)| (nodes[index].id.as_str(), row))
        .collect();
    let mut initiative_groups: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
    {
        let mut grouped: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
        for &index in &initiatives {
            let group = parents_of
                .get(nodes[index].id.as_str())
                .and_then(|parents| {
                    parents
                        .iter()
                        .filter_map(|&p| strategy_order.get(nodes[p].id.as_str()).copied())
                        .min()
                });
            grouped.entry(group).or_default().push(index);
        }
        let mut keys: Vec<Option<usize>> = grouped.keys().copied().collect();
        keys.sort_by_key(|key| (key.is_none(), key.unwrap_or(usize::MAX)));
        for key in keys {
            let mut members = grouped.remove(&key).unwrap_or_default();
            members.sort_by(|&a, &b| nodes[a].short_code.cmp(&nodes[b].short_code));
            initiative_groups.push((key, members));
        }
    }

    // Place: strategies as one flat stack; initiatives and tasks stack by
    // group with a lane band per visible parent.
    let mut placed: HashMap<&str, (f64, f64)> = HashMap::new();
    let mut lanes: Vec<PlacedLane> = Vec::new();
    let top = HEADER_H + 12.0;

    let mut y = top;
    for &index in &strategies {
        placed.insert(nodes[index].id.as_str(), (column_x(Column::Strategy), y));
        y += NODE_H + ROW_GAP;
    }
    let mut max_y = y;

    let mut place_grouped = |groups: &[(Option<usize>, Vec<usize>)],
                             column: Column,
                             parent_ids: &[usize],
                             lanes: &mut Vec<PlacedLane>|
     -> f64 {
        let mut y = top;
        for (group, members) in groups {
            let start = y;
            for &index in members {
                placed.insert(nodes[index].id.as_str(), (column_x(column), y));
                y += NODE_H + ROW_GAP;
            }
            if let Some(parent_row) = group {
                let parent_index = parent_ids[*parent_row];
                lanes.push(PlacedLane {
                    parent_id: nodes[parent_index].id.clone(),
                    x: column_x(column) - LANE_PAD,
                    y: start - LANE_PAD,
                    w: NODE_W + 2.0 * LANE_PAD,
                    h: (y - ROW_GAP - start) + 2.0 * LANE_PAD,
                });
            }
            y += GROUP_GAP;
        }
        y
    };
    let initiatives_bottom = place_grouped(
        &initiative_groups,
        Column::Initiative,
        &strategies,
        &mut lanes,
    );
    let tasks_bottom = place_grouped(&task_groups, Column::Task, &initiatives, &mut lanes);
    max_y = max_y.max(initiatives_bottom).max(tasks_bottom);

    // --- arrows (blocks only) ----------------------------------------------
    let mut arrows = Vec::new();
    for edge in edges.iter().filter(|e| e.relationship == "blocks") {
        let (Some(&(sx, sy)), Some(&(tx, ty))) = (
            placed.get(edge.source_id.as_str()),
            placed.get(edge.target_id.as_str()),
        ) else {
            continue;
        };
        let (source_mid_y, target_mid_y) = (sy + NODE_H / 2.0, ty + NODE_H / 2.0);
        let path = if (sx - tx).abs() < f64::EPSILON {
            // Same column: bow out to the right of the boxes.
            let x = sx + NODE_W;
            let bow = x + 46.0;
            format!(
                "M {x} {source_mid_y} C {bow} {source_mid_y}, {bow} {target_mid_y}, {x} {target_mid_y}"
            )
        } else if sx < tx {
            let from = sx + NODE_W;
            let mid = (from + tx) / 2.0;
            format!(
                "M {from} {source_mid_y} C {mid} {source_mid_y}, {mid} {target_mid_y}, {tx} {target_mid_y}"
            )
        } else {
            let to = tx + NODE_W;
            let mid = (sx + to) / 2.0;
            format!(
                "M {sx} {source_mid_y} C {mid} {source_mid_y}, {mid} {target_mid_y}, {to} {target_mid_y}"
            )
        };
        arrows.push(PlacedArrow {
            source_id: edge.source_id.clone(),
            target_id: edge.target_id.clone(),
            path,
        });
    }

    let placed_nodes = nodes
        .iter()
        .filter_map(|node| {
            let &(x, y) = placed.get(node.id.as_str())?;
            Some(PlacedNode {
                id: node.id.clone(),
                x,
                y,
                w: NODE_W,
                h: NODE_H,
                hidden_neighbors: node.hidden_neighbors,
            })
        })
        .collect();

    GraphLayout {
        width: column_x(Column::Task) + NODE_W + COLUMN_GAP,
        height: max_y + 12.0,
        headers: vec![
            ("Strategy", column_x(Column::Strategy)),
            ("Initiative", column_x(Column::Initiative)),
            ("Task", column_x(Column::Task)),
        ],
        lanes,
        nodes: placed_nodes,
        arrows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, code: &str, column: Column, hidden_neighbors: i64) -> LayoutInputNode {
        LayoutInputNode {
            id: id.into(),
            short_code: code.into(),
            column,
            hidden_neighbors,
        }
    }

    fn edge(source: &str, target: &str, relationship: &str) -> LayoutInputEdge {
        LayoutInputEdge {
            source_id: source.into(),
            target_id: target.into(),
            relationship: relationship.into(),
        }
    }

    fn fixture() -> (Vec<LayoutInputNode>, Vec<LayoutInputEdge>) {
        (
            vec![
                node("s1", "A-S-0001", Column::Strategy, 0),
                node("i1", "A-I-0001", Column::Initiative, 0),
                node("i2", "A-I-0002", Column::Initiative, 0),
                node("t1", "A-T-0001", Column::Task, 0),
                node("t2", "A-T-0002", Column::Task, 0),
                node("t3", "A-T-0003", Column::Task, 1),
            ],
            vec![
                edge("s1", "i1", "parent"),
                edge("s1", "i2", "parent"),
                edge("i1", "t1", "parent"),
                edge("i1", "t2", "parent"),
                edge("i2", "t3", "parent"),
                edge("t1", "t3", "blocks"),
            ],
        )
    }

    /// Identical input → identical geometry, twice over (the no-force
    /// determinism AC).
    #[test]
    fn layout_is_deterministic() {
        let (nodes, edges) = fixture();
        let first = layout(&nodes, &edges);
        let second = layout(&nodes, &edges);
        assert_eq!(first, second);
        // Shuffled input order changes nothing either: ordering is by
        // short code + barycenter, never by arrival order.
        let mut reversed = nodes.clone();
        reversed.reverse();
        let third = layout(&reversed, &edges);
        let sort = |mut layout: GraphLayout| {
            layout.nodes.sort_by(|a, b| a.id.cmp(&b.id));
            layout.lanes.sort_by(|a, b| a.parent_id.cmp(&b.parent_id));
            layout
        };
        assert_eq!(sort(first), sort(third));
    }

    /// Columns are fixed by type; lanes band children under their parent.
    #[test]
    fn columns_lanes_and_arrows() {
        let (nodes, edges) = fixture();
        let result = layout(&nodes, &edges);
        let placed = |id: &str| result.nodes.iter().find(|n| n.id == id).unwrap();
        // Fixed columns, left to right.
        assert!(placed("s1").x < placed("i1").x);
        assert!(placed("i1").x < placed("t1").x);
        assert_eq!(placed("i1").x, placed("i2").x);
        // t1/t2 band under i1, contiguous and above i2's group (i1 sorts
        // first); t3 under i2.
        assert!(placed("t1").y < placed("t2").y);
        assert!(placed("t2").y < placed("t3").y);
        // Lanes exist for both initiatives and the strategy.
        let lane_parents: Vec<&str> = result.lanes.iter().map(|l| l.parent_id.as_str()).collect();
        assert!(lane_parents.contains(&"i1"));
        assert!(lane_parents.contains(&"i2"));
        assert!(lane_parents.contains(&"s1"));
        // The i1 lane spans t1+t2.
        let lane = result.lanes.iter().find(|l| l.parent_id == "i1").unwrap();
        assert!(lane.y <= placed("t1").y && lane.y + lane.h >= placed("t2").y + NODE_H);
        // Exactly one arrow: the blocks edge; parent edges draw none.
        assert_eq!(result.arrows.len(), 1);
        assert_eq!(result.arrows[0].source_id, "t1");
        // +N passes through untouched (the component computes it over the
        // FULL merged edge set, side-panel edges included).
        assert_eq!(placed("t3").hidden_neighbors, 1);
        assert_eq!(placed("t1").hidden_neighbors, 0);
    }
}
