//! The data of the work-dependency graph (COLLIERY-I-0626, KAIROS-T-0492).
//! Pure code, so it is unit-tested natively.
//!
//! - The canvas is about `blocks`. The nodes have no fixed layer: the Aurora
//!   layout ranks each node one step right of each node that blocks it, so
//!   a chain reads left to right and nodes with no edge share a column.
//! - `parent` is a group, not a layer. The parent of a node (its lowest
//!   code parent one level up) is a group box with a header; a parent with
//!   members is not a node. A strategy above a group initiative is a part
//!   of the header of the group.
//! - Colour is the state of the item: todo, active, done, blocked.
//! - When the focus is a task, at most [`SIBLING_LIMIT`] sibling tasks with
//!   no `blocks` edge to the focus show; the rest fold into `+N`.
//! - Group boxes come from the Aurora geometry (the lane bands of a group
//!   over all layers), see [`group_boxes`]. Zoom and pan are a transform of
//!   the stage, see [`zoom_at`].

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use aurora_dark::graph::{DagEdge, DagLane, DagNode, EdgeStyle, Hue};
use aurora_dark::graph_layout::{Layout, LayoutOptions};

use super::data;

/// The size of a node box. Three lines: title, short code, column.
pub const NODE_W: f64 = 220.0;
pub const NODE_H: f64 = 58.0;

/// The name of the edge style of an open `blocks` arrow.
pub const OPEN: &str = "open";
/// The name of the edge style of a resolved `blocks` arrow.
pub const RESOLVED: &str = "resolved";

/// The number of sibling tasks with no `blocks` edge to the focus that
/// show before the rest fold into `+N` (Dylan, 2026-10-15).
pub const SIBLING_LIMIT: usize = 5;

/// The two styles of a `blocks` arrow (COLLIERY-T-0233). The dash carries
/// the difference for a reader who cannot see the colour. Red stays
/// unused (reserved for violated/at-risk by convention).
pub fn edge_styles() -> Vec<EdgeStyle> {
    vec![
        EdgeStyle::new(OPEN, "Open blocker", Hue::Gold),
        EdgeStyle::new(
            RESOLVED,
            "Resolved: one end is in a done column",
            Hue::Muted,
        )
        .dashed(),
    ]
}

/// The lane of the tasks with no initiative (KAIROS-T-0778). It is not a
/// short code, so it never names an item.
pub const NO_INITIATIVE: &str = "~no-initiative";

/// The margin of the canvas. The left margin holds the arrows between two
/// lanes (KAIROS-T-0778).
pub const GUTTER: f64 = 40.0;

/// The layout options of the canvas. The `Dag` and [`group_boxes`] must
/// use the same options, so the boxes fit the nodes. Each lane is one row
/// over all the columns (KAIROS-T-0778, Aurora `lane_rows`).
pub fn layout_options() -> LayoutOptions {
    LayoutOptions {
        node_w: NODE_W,
        node_h: NODE_H,
        margin: GUTTER,
        lane_rows: true,
        ..LayoutOptions::default()
    }
}

/// The state of an item on the canvas: its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkState {
    Todo,
    Active,
    Blocked,
    Done,
}

impl WorkState {
    #[cfg(test)]
    pub const ALL: [WorkState; 4] = [
        WorkState::Todo,
        WorkState::Active,
        WorkState::Blocked,
        WorkState::Done,
    ];

    pub fn hue(self) -> Hue {
        match self {
            WorkState::Todo => Hue::Neutral,
            WorkState::Active => Hue::Ice,
            WorkState::Blocked => Hue::Gold,
            WorkState::Done => Hue::Ok,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WorkState::Todo => "todo",
            WorkState::Active => "active",
            WorkState::Blocked => "blocked",
            WorkState::Done => "done",
        }
    }
}

/// The state of an item. Done: a done column. Blocked: the Blocked column,
/// or an open blocker. Todo: a column where the work has not started.
/// Each other column is active.
pub fn work_state(status: &str, done: bool, open_blocker: bool) -> WorkState {
    let status = status.trim().to_lowercase();
    if done {
        WorkState::Done
    } else if status == "blocked" || open_blocker {
        WorkState::Blocked
    } else if matches!(
        status.as_str(),
        "backlog" | "todo" | "to do" | "ready" | "discovery" | "design" | "draft" | "new" | "open"
    ) {
        WorkState::Todo
    } else {
        WorkState::Active
    }
}

/// A group: the parent of nodes, drawn as a box with a header.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The short code of the parent, or [`NO_INITIATIVE`]. It is also the
    /// lane id.
    pub code: String,
    pub title: String,
    pub entity_type: String,
    /// The strategy above the group, shown in the header: (code, title).
    pub strategy: Option<(String, String)>,
    /// The focus is the parent, the strategy of the header, or a node of
    /// the group. The lane of the focus is first (KAIROS-T-0778).
    pub current: bool,
    /// The number of sibling tasks folded into `+N`.
    pub folded: usize,
    /// The neighbours of the parent and of its strategy that are not
    /// fetched: the `+N` of the header.
    pub more: u32,
    /// The codes to expand for that `+N`.
    pub expand: Vec<String>,
}

/// What the canvas draws: the product data for the Aurora `Dag`.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasData {
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
    pub lanes: Vec<DagLane>,
    pub groups: Vec<Group>,
    /// The states that the nodes show, for the legend.
    pub states: Vec<WorkState>,
    /// `blocks` edges with an end that is not a node (a group header):
    /// (source code, target code). The side panel lists them.
    pub off_canvas_blocks: Vec<(String, String)>,
    /// The short codes of the nodes, for the side panel.
    pub node_codes: BTreeSet<String>,
    /// `blocks` edges between two groups (two lanes): from, to, and true
    /// when the edge is resolved. The canvas draws them in the left margin,
    /// from lane to lane (KAIROS-T-0778).
    pub lane_edges: Vec<LaneEdge>,
}

/// A `blocks` edge between two lanes (KAIROS-T-0778).
#[derive(Clone, Debug, PartialEq)]
pub struct LaneEdge {
    pub from: String,
    pub to: String,
    pub resolved: bool,
}

fn level(entity_type: &str) -> Option<usize> {
    match entity_type {
        "strategy" => Some(0),
        "initiative" => Some(1),
        "task" => Some(2),
        _ => None,
    }
}

/// Whether a `blocks` arrow is resolved (COLLIERY-T-0233): an end of the
/// edge is in a done column, so the edge is history and not a blocker
/// (the COLLIERY-T-0214 rule, which the board counts follow). An end
/// that is not in the node set counts as not done.
pub fn blocks_resolved(nodes: &[data::GraphNode], source_id: &str, target_id: &str) -> bool {
    nodes
        .iter()
        .any(|node| node.done && (node.id == source_id || node.id == target_id))
}

/// Map the merged subgraph onto the canvas. `unfold` shows all the
/// siblings of the focus.
///
/// - Canvas items: strategy, initiative and task. Documents and ADRs stay
///   in the side panel. The node id is the SHORT CODE.
/// - The group of an item: its parent one level up with the lowest short
///   code (task → initiative or strategy; initiative → strategy).
/// - A parent with members is a group, not a node. A strategy that is the
///   parent of a group initiative is the prefix of its header, not a node.
/// - `blocks` is the only arrow, open or resolved (COLLIERY-T-0233).
/// - `+N` on a node = degree minus EVERY fetched incident edge.
pub fn canvas_data(
    nodes: &[data::GraphNode],
    edges: &[data::GraphEdge],
    focus: &str,
    unfold: bool,
) -> CanvasData {
    let mut fetched_incident: HashMap<&str, i64> = HashMap::new();
    for edge in edges {
        *fetched_incident.entry(edge.source_id.as_str()).or_default() += 1;
        *fetched_incident.entry(edge.target_id.as_str()).or_default() += 1;
    }
    let by_id: HashMap<&str, &data::GraphNode> = nodes
        .iter()
        .filter(|node| level(&node.entity_type).is_some())
        .map(|node| (node.id.as_str(), node))
        .collect();

    // The parent of each item: one level up, lowest short code.
    let mut parent_of: HashMap<&str, &data::GraphNode> = HashMap::new();
    for edge in edges.iter().filter(|e| e.relationship == "parent") {
        let (Some(&parent), Some(&child)) = (
            by_id.get(edge.source_id.as_str()),
            by_id.get(edge.target_id.as_str()),
        ) else {
            continue;
        };
        let (Some(parent_level), Some(child_level)) =
            (level(&parent.entity_type), level(&child.entity_type))
        else {
            continue;
        };
        // A task under a strategy is a member too (no initiative between).
        if parent_level >= child_level {
            continue;
        }
        let entry = parent_of.entry(child.id.as_str()).or_insert(parent);
        if parent.short_code < entry.short_code {
            *entry = parent;
        }
    }

    // Groups and nodes. An initiative is a group when an item names it as
    // parent. Then a strategy is a header (the parent of a group) or a
    // group (the parent of a node).
    let initiative_groups: HashSet<&str> = parent_of
        .iter()
        .filter(|(child, parent)| {
            parent.entity_type == "initiative"
                && by_id.get(*child).is_some_and(|c| c.entity_type == "task")
        })
        .map(|(_, parent)| parent.id.as_str())
        .collect();
    let is_node = |id: &str| -> bool {
        let Some(node) = by_id.get(id) else {
            return false;
        };
        match node.entity_type.as_str() {
            "task" => true,
            "initiative" => !initiative_groups.contains(id),
            // A strategy is a node only when no item names it as parent.
            _ => !parent_of.values().any(|parent| parent.id == id),
        }
    };
    let mut node_ids: Vec<&str> = by_id.keys().copied().filter(|id| is_node(id)).collect();
    node_ids.sort_by(|a, b| by_id[a].short_code.cmp(&by_id[b].short_code));

    // Open blockers: a `blocks` edge with no end in a done column.
    let blocks: Vec<&data::GraphEdge> = edges
        .iter()
        .filter(|e| e.relationship == "blocks")
        .collect();
    let open_blocked: HashSet<&str> = blocks
        .iter()
        .filter(|e| !blocks_resolved(nodes, &e.source_id, &e.target_id))
        .map(|e| e.target_id.as_str())
        .collect();

    // The fold: the siblings of a task focus with no blocks edge to it.
    let focus_node = nodes.iter().find(|n| n.short_code == focus);
    let mut folded_ids: HashSet<&str> = HashSet::new();
    let mut folded_group: Option<(&str, usize)> = None;
    if let Some(focus_node) = focus_node.filter(|n| n.entity_type == "task")
        && let Some(group) = parent_of.get(focus_node.id.as_str())
    {
        let touches_focus = |id: &str| {
            blocks.iter().any(|e| {
                (e.source_id == id && e.target_id == focus_node.id)
                    || (e.target_id == id && e.source_id == focus_node.id)
            })
        };
        let has_blocks = |id: &str| {
            blocks
                .iter()
                .any(|e| e.source_id == id || e.target_id == id)
        };
        let mut siblings: Vec<&data::GraphNode> = node_ids
            .iter()
            .filter(|id| **id != focus_node.id)
            .filter(|id| parent_of.get(*id).is_some_and(|p| p.id == group.id))
            .filter(|id| !touches_focus(id))
            .map(|id| by_id[id])
            .collect();
        // Keep the siblings in a chain first, then open work, then by code.
        siblings.sort_by(|a, b| {
            (!has_blocks(&a.id), a.done, &a.short_code).cmp(&(
                !has_blocks(&b.id),
                b.done,
                &b.short_code,
            ))
        });
        if siblings.len() > SIBLING_LIMIT {
            let rest = &siblings[SIBLING_LIMIT..];
            folded_group = Some((group.id.as_str(), rest.len()));
            if !unfold {
                folded_ids.extend(rest.iter().map(|n| n.id.as_str()));
            }
        }
    }
    node_ids.retain(|id| !folded_ids.contains(id));
    let node_set: HashSet<&str> = node_ids.iter().copied().collect();

    let hidden_of = |node: &data::GraphNode| {
        let fetched = fetched_incident.get(node.id.as_str()).copied().unwrap_or(0);
        u32::try_from((node.degree - fetched).max(0)).unwrap_or(u32::MAX)
    };

    // The groups that hold a node, with their headers.
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for id in &node_ids {
        let Some(parent) = parent_of.get(id) else {
            continue;
        };
        groups.entry(parent.short_code.clone()).or_insert_with(|| {
            let strategy_node = parent_of
                .get(parent.id.as_str())
                .filter(|s| s.entity_type == "strategy");
            let strategy = strategy_node.map(|s| (s.short_code.clone(), s.title.clone()));
            let mut more = 0;
            let mut expand = Vec::new();
            for item in std::iter::once(*parent).chain(strategy_node.copied()) {
                let hidden = hidden_of(item);
                if hidden > 0 {
                    more += hidden;
                    expand.push(item.short_code.clone());
                }
            }
            let current = parent.short_code == focus
                || strategy.as_ref().is_some_and(|(code, _)| code == focus)
                || focus_node.is_some_and(|f| {
                    parent_of
                        .get(f.id.as_str())
                        .is_some_and(|p| p.id == parent.id)
                });
            let folded = match folded_group {
                Some((group, count)) if group == parent.id => count,
                _ => 0,
            };
            Group {
                code: parent.short_code.clone(),
                title: parent.title.clone(),
                entity_type: parent.entity_type.clone(),
                strategy,
                current,
                folded,
                more,
                expand,
            }
        });
    }

    // The tasks with no group get a lane of their own when the canvas has
    // groups (KAIROS-T-0778).
    let loose_tasks = !groups.is_empty()
        && node_ids.iter().any(|id| {
            by_id[id].entity_type == "task"
                && !parent_of
                    .get(id)
                    .is_some_and(|p| groups.contains_key(&p.short_code))
        });

    let mut states: BTreeSet<WorkState> = BTreeSet::new();
    let dag_nodes: Vec<DagNode> = node_ids
        .iter()
        .map(|id| {
            let node = by_id[id];
            let put_away = node.archived_at.is_some();
            let hidden = hidden_of(node);
            let state = work_state(&node.status, node.done, open_blocked.contains(id));
            states.insert(state);
            // "put away" never "archived" (KAIROS-T-0163).
            let mut tooltip = format!("{} — {} — {}", node.title, node.status, state.label());
            if put_away {
                tooltip.push_str(" — put away");
            }
            let group = parent_of
                .get(id)
                .map(|p| p.short_code.as_str())
                .filter(|code| groups.contains_key(*code))
                .or_else(|| (loose_tasks && node.entity_type == "task").then_some(NO_INITIATIVE));
            let mut dag = DagNode::new(node.short_code.clone(), node.title.clone())
                .sublabel(node.short_code.clone())
                .detail(node.status.clone())
                .kind(node.entity_type.clone())
                .status(state.hue())
                .sort_key(format!("{}/{}", group.unwrap_or("~"), node.short_code))
                .current(node.short_code == focus)
                .tooltip(tooltip)
                .more(hidden);
            if let Some(group) = group {
                dag = dag.lane(group);
            }
            // The first mark sits at the right end: "put away", then the state.
            if put_away {
                dag = dag.archived(true).mark("put away", Hue::Gold);
            }
            if node.done {
                dag = dag.done(true).mark("done", Hue::Ok);
            } else if state == WorkState::Blocked {
                dag = dag.mark("blocked", Hue::Gold);
            }
            dag
        })
        .collect();

    let mut dag_edges = Vec::new();
    let mut off_canvas_blocks = Vec::new();
    let mut lane_edges = Vec::new();
    for edge in &blocks {
        let (Some(source), Some(target)) = (
            by_id.get(edge.source_id.as_str()),
            by_id.get(edge.target_id.as_str()),
        ) else {
            continue;
        };
        let (source_on, target_on) = (
            node_set.contains(edge.source_id.as_str()),
            node_set.contains(edge.target_id.as_str()),
        );
        if folded_ids.contains(edge.source_id.as_str())
            || folded_ids.contains(edge.target_id.as_str())
        {
            continue;
        }
        if !source_on
            && !target_on
            && groups.contains_key(&source.short_code)
            && groups.contains_key(&target.short_code)
        {
            lane_edges.push(LaneEdge {
                from: source.short_code.clone(),
                to: target.short_code.clone(),
                resolved: blocks_resolved(nodes, &edge.source_id, &edge.target_id),
            });
            continue;
        }
        if !(source_on && target_on) {
            off_canvas_blocks.push((source.short_code.clone(), target.short_code.clone()));
            continue;
        }
        let dag = DagEdge::new(source.short_code.clone(), target.short_code.clone());
        dag_edges.push(
            if blocks_resolved(nodes, &edge.source_id, &edge.target_id) {
                dag.style(RESOLVED)
                    .label("Resolved: one end is in a done column. This edge does not block.")
            } else {
                dag.style(OPEN).label("Open blocker")
            },
        );
    }
    dag_edges.sort_by(|a: &DagEdge, b: &DagEdge| (&a.from, &a.to).cmp(&(&b.from, &b.to)));

    lane_edges.sort_by(|a: &LaneEdge, b: &LaneEdge| (&a.from, &a.to).cmp(&(&b.from, &b.to)));

    // The lanes: the lane of the focus first, then by short code, then the
    // tasks with no initiative (KAIROS-T-0778).
    let mut groups: Vec<Group> = groups.into_values().collect();
    groups.sort_by_key(|g| !g.current);
    if loose_tasks {
        groups.push(Group {
            code: NO_INITIATIVE.to_string(),
            title: "No initiative".to_string(),
            entity_type: "none".to_string(),
            strategy: None,
            current: false,
            folded: 0,
            more: 0,
            expand: Vec::new(),
        });
    }
    let lanes = groups
        .iter()
        .map(|g| DagLane::new(g.code.clone(), g.code.clone()))
        .collect();
    let node_codes = dag_nodes.iter().map(|n| n.id.clone()).collect();
    CanvasData {
        nodes: dag_nodes,
        edges: dag_edges,
        lanes,
        groups,
        states: states.into_iter().collect(),
        off_canvas_blocks,
        node_codes,
        lane_edges,
    }
}

/// A group box on the canvas, in the coordinates of the `Dag` SVG.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupBox {
    /// The short code of the group.
    pub code: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The box that carries the header: the first box of the group.
    pub header: bool,
}

#[derive(Clone, Copy)]
struct Rect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Rect {
    fn overlaps(&self, other: &Rect) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }
}

/// The room above a box for its header.
pub const HEADER_ROOM: f64 = 14.0;

/// The group boxes of a layout. A group gets one box: the union of its lane
/// bands over all layers. When that box would cover a node outside the
/// group, or the box of a different group, the group gets one box for
/// each band, and the first band carries the header.
pub fn group_boxes(layout: &Layout, nodes: &[DagNode]) -> Vec<GroupBox> {
    let lane_of: HashMap<&str, Option<&str>> = nodes
        .iter()
        .map(|n| (n.id.as_str(), n.lane.as_deref()))
        .collect();
    let mut unions: BTreeMap<&str, Rect> = BTreeMap::new();
    for lane in &layout.lanes {
        let rect = Rect {
            x0: lane.x,
            y0: lane.y,
            x1: lane.x + lane.w,
            y1: lane.y + lane.h,
        };
        unions
            .entry(lane.id.as_str())
            .and_modify(|u| {
                u.x0 = u.x0.min(rect.x0);
                u.y0 = u.y0.min(rect.y0);
                u.x1 = u.x1.max(rect.x1);
                u.y1 = u.y1.max(rect.y1);
            })
            .or_insert(rect);
    }
    let covers_other = |code: &str, rect: &Rect| {
        layout.nodes.iter().any(|placed| {
            lane_of.get(placed.id.as_str()).copied().flatten() != Some(code)
                && rect.overlaps(&Rect {
                    x0: placed.x,
                    y0: placed.y,
                    x1: placed.x + placed.w,
                    y1: placed.y + placed.h,
                })
        })
    };
    let split: HashSet<&str> = unions
        .iter()
        .filter(|(code, rect)| {
            covers_other(code, rect)
                || unions.iter().any(|(other, other_rect)| {
                    other != *code && {
                        // The header room counts as part of the box.
                        let a = Rect {
                            y0: rect.y0 - HEADER_ROOM,
                            ..**rect
                        };
                        let b = Rect {
                            y0: other_rect.y0 - HEADER_ROOM,
                            ..*other_rect
                        };
                        a.overlaps(&b)
                    }
                })
        })
        .map(|(code, _)| *code)
        .collect();

    let mut boxes = Vec::new();
    for (code, rect) in &unions {
        if split.contains(code) {
            let mut first = true;
            for lane in layout.lanes.iter().filter(|l| l.id == *code) {
                boxes.push(GroupBox {
                    code: code.to_string(),
                    x: lane.x,
                    y: lane.y,
                    w: lane.w,
                    h: lane.h,
                    header: first,
                });
                first = false;
            }
        } else {
            boxes.push(GroupBox {
                code: code.to_string(),
                x: rect.x0,
                y: rect.y0,
                w: rect.x1 - rect.x0,
                h: rect.y1 - rect.y0,
                header: true,
            });
        }
    }
    boxes
}

/// The smallest and the largest zoom.
pub const ZOOM_MIN: f64 = 0.25;
pub const ZOOM_MAX: f64 = 3.0;

/// Zoom by `factor` around the point `at` of the viewport, so the point
/// under the pointer stays where it is. `pan` is the offset of the stage in
/// the viewport. The zoom stays in [`ZOOM_MIN`]..=[`ZOOM_MAX`].
pub fn zoom_at(scale: f64, pan: (f64, f64), factor: f64, at: (f64, f64)) -> (f64, (f64, f64)) {
    let next = (scale * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    let ratio = next / scale;
    let pan = (at.0 - (at.0 - pan.0) * ratio, at.1 - (at.1 - pan.1) * ratio);
    (next, pan)
}

/// The zoom that fits a canvas of `width` into a viewport of `viewport`
/// pixels: never larger than 1.
pub fn fit_scale(width: f64, viewport: f64) -> f64 {
    if width <= 0.0 || viewport <= 0.0 {
        1.0
    } else {
        (viewport / width).clamp(ZOOM_MIN, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_dark::graph_layout::layout;

    fn item(code: &str, entity_type: &str) -> data::GraphNode {
        data::GraphNode {
            id: code.to_string(),
            short_code: code.to_string(),
            entity_type: entity_type.to_string(),
            title: format!("title {code}"),
            status: "Todo".to_string(),
            depth: 1,
            degree: 0,
            archived_at: None,
            done: false,
        }
    }

    fn task(code: &str) -> data::GraphNode {
        item(code, "task")
    }

    fn edge(source: &str, target: &str, relationship: &str) -> data::GraphEdge {
        data::GraphEdge {
            source_id: source.to_string(),
            target_id: target.to_string(),
            relationship: relationship.to_string(),
            depth: 1,
        }
    }

    fn placed(canvas: &CanvasData) -> Layout {
        layout(
            &canvas.nodes,
            &canvas.edges,
            &canvas.lanes,
            &[],
            &layout_options(),
        )
    }

    fn x_of(layout: &Layout, code: &str) -> f64 {
        layout.node(code).unwrap_or_else(|| panic!("{code}")).x
    }

    /// The acceptance criterion: a chain A→B→C reads left to right; two
    /// tasks with no edge are in one column.
    #[test]
    fn a_chain_reads_left_to_right_and_parallel_tasks_stack() {
        let nodes = vec![
            item("I-1", "initiative"),
            task("T-A"),
            task("T-B"),
            task("T-C"),
            task("T-D"),
            task("T-E"),
        ];
        let mut edges: Vec<_> = ["T-A", "T-B", "T-C", "T-D", "T-E"]
            .iter()
            .map(|t| edge("I-1", t, "parent"))
            .collect();
        edges.push(edge("T-A", "T-B", "blocks"));
        edges.push(edge("T-B", "T-C", "blocks"));
        let canvas = canvas_data(&nodes, &edges, "T-B", false);
        let geometry = placed(&canvas);
        let (a, b, c) = (
            x_of(&geometry, "T-A"),
            x_of(&geometry, "T-B"),
            x_of(&geometry, "T-C"),
        );
        assert!(a < b && b < c, "serial work is a row: {a} {b} {c}");
        assert_eq!(x_of(&geometry, "T-D"), x_of(&geometry, "T-E"));
        assert_eq!(
            x_of(&geometry, "T-D"),
            a,
            "parallel work stacks in a column"
        );
        let (d, e) = (geometry.node("T-D").unwrap(), geometry.node("T-E").unwrap());
        assert!((d.y - e.y).abs() >= NODE_H, "no overlap");
    }

    /// The initiative is a group (header), not a node; its strategy is the
    /// prefix of the header. The tasks are in the lane of the group.
    #[test]
    fn parents_are_groups_not_nodes() {
        // The strategy has two more neighbours than the walk fetched.
        let mut strategy = item("S-1", "strategy");
        strategy.degree = 3;
        let nodes = vec![
            strategy,
            item("I-1", "initiative"),
            task("T-1"),
            task("T-2"),
            item("D-1", "document"),
        ];
        let edges = vec![
            edge("S-1", "I-1", "parent"),
            edge("I-1", "T-1", "parent"),
            edge("I-1", "T-2", "parent"),
            edge("D-1", "I-1", "supports"),
        ];
        let canvas = canvas_data(&nodes, &edges, "T-1", false);
        let ids: Vec<&str> = canvas.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["T-1", "T-2"]);
        assert!(
            canvas
                .nodes
                .iter()
                .all(|n| n.lane.as_deref() == Some("I-1"))
        );
        assert_eq!(canvas.groups.len(), 1);
        let group = &canvas.groups[0];
        assert_eq!(group.code, "I-1");
        assert_eq!(
            group.strategy,
            Some(("S-1".to_string(), "title S-1".to_string()))
        );
        // KAIROS-T-0778: the lane of the focus task is the current one.
        assert!(group.current);
        assert_eq!(
            (group.more, group.expand.clone()),
            (2, vec!["S-1".to_string()]),
            "the +N of the header"
        );
        assert!(canvas.edges.is_empty(), "parent and supports draw no arrow");

        // With the initiative as the focus, its group is the current one.
        let canvas = canvas_data(&nodes, &edges, "I-1", false);
        assert!(canvas.groups[0].current);
    }

    /// An initiative with no task in view is a node in the group of its
    /// strategy.
    #[test]
    fn an_initiative_without_tasks_is_a_node() {
        let nodes = vec![
            item("S-1", "strategy"),
            item("I-1", "initiative"),
            item("I-2", "initiative"),
        ];
        let edges = vec![edge("S-1", "I-1", "parent"), edge("S-1", "I-2", "parent")];
        let canvas = canvas_data(&nodes, &edges, "I-1", false);
        let ids: Vec<&str> = canvas.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["I-1", "I-2"]);
        assert_eq!(canvas.groups.len(), 1);
        assert_eq!(canvas.groups[0].code, "S-1");
        // KAIROS-T-0778: the lane that holds the focus is the current one.
        assert!(canvas.groups[0].current);
        assert!(canvas.nodes[0].current);
    }

    /// Colour by state; an open blocker makes its target blocked; a done
    /// end makes the arrow resolved and the target is not blocked.
    #[test]
    fn colour_is_the_state() {
        let mut done = task("T-1");
        done.done = true;
        done.status = "Completed".to_string();
        let mut active = task("T-2");
        active.status = "Active".to_string();
        let waiting = task("T-3");
        let mut column = task("T-4");
        column.status = "Blocked".to_string();
        let after_done = task("T-5");
        let nodes = vec![done, active, waiting, column, after_done];
        let edges = vec![edge("T-2", "T-3", "blocks"), edge("T-1", "T-5", "blocks")];
        let canvas = canvas_data(&nodes, &edges, "T-2", false);
        let hue = |id: &str| canvas.nodes.iter().find(|n| n.id == id).unwrap().status;
        assert_eq!(hue("T-1"), Hue::Ok);
        assert_eq!(hue("T-2"), Hue::Ice);
        assert_eq!(hue("T-3"), Hue::Gold, "an open blocker");
        assert_eq!(hue("T-4"), Hue::Gold, "the Blocked column");
        assert_eq!(hue("T-5"), Hue::Neutral, "a resolved blocker");
        let styles: Vec<(&str, &str)> = canvas
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.style.as_str()))
            .collect();
        assert_eq!(styles, vec![("T-1", RESOLVED), ("T-2", OPEN)]);
        assert_eq!(canvas.states, WorkState::ALL.to_vec());
    }

    #[test]
    fn work_state_of_columns() {
        assert_eq!(work_state("Backlog", false, false), WorkState::Todo);
        assert_eq!(work_state("Todo", false, false), WorkState::Todo);
        assert_eq!(work_state("Active", false, false), WorkState::Active);
        assert_eq!(work_state("Decompose", false, false), WorkState::Active);
        assert_eq!(work_state("Blocked", false, false), WorkState::Blocked);
        assert_eq!(work_state("Todo", false, true), WorkState::Blocked);
        assert_eq!(work_state("Blocked", true, true), WorkState::Done);
    }

    /// Up to 5 siblings with no blocks edge to the focus show; the rest
    /// fold into `+N`. A sibling with a blocks edge to the focus always
    /// shows; `unfold` shows all.
    #[test]
    fn siblings_fold_after_five() {
        let mut nodes = vec![item("I-1", "initiative"), task("T-00")];
        let mut edges = vec![edge("I-1", "T-00", "parent")];
        for n in 1..=8 {
            let code = format!("T-{n:02}");
            nodes.push(task(&code));
            edges.push(edge("I-1", &code, "parent"));
        }
        edges.push(edge("T-08", "T-00", "blocks"));
        let canvas = canvas_data(&nodes, &edges, "T-00", false);
        let ids: Vec<&str> = canvas.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["T-00", "T-01", "T-02", "T-03", "T-04", "T-05", "T-08"]
        );
        assert_eq!(canvas.groups[0].folded, 2);

        let canvas = canvas_data(&nodes, &edges, "T-00", true);
        assert_eq!(canvas.nodes.len(), 9);
        assert_eq!(canvas.groups[0].folded, 2, "the toggle still shows");
    }

    /// A blocks edge to a group header is not an arrow: the side panel
    /// lists it.
    #[test]
    fn blocks_to_a_group_go_to_the_panel() {
        let nodes = vec![
            item("I-1", "initiative"),
            item("I-2", "initiative"),
            task("T-1"),
        ];
        let edges = vec![edge("I-1", "T-1", "parent"), edge("I-2", "I-1", "blocks")];
        let canvas = canvas_data(&nodes, &edges, "T-1", false);
        assert!(canvas.edges.is_empty());
        assert_eq!(
            canvas.off_canvas_blocks,
            vec![("I-2".to_string(), "I-1".to_string())]
        );
    }

    /// `+N`, marks and the archived flag come from the wire node; the
    /// output does not depend on the input order.
    #[test]
    fn marks_more_and_determinism() {
        let mut done = task("T-1");
        done.done = true;
        done.archived_at = Some("2026-01-01T00:00:00Z".to_string());
        let mut open = task("T-2");
        open.degree = 4;
        let nodes = vec![open, done, task("T-3")];
        let edges = vec![edge("T-1", "T-2", "blocks"), edge("T-3", "T-2", "blocks")];
        let canvas = canvas_data(&nodes, &edges, "T-2", false);
        let first = &canvas.nodes[0];
        assert!(first.archived && first.done);
        let marks: Vec<&str> = first.marks.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(marks, vec!["put away", "done"]);
        assert_eq!(canvas.nodes[1].more, 2, "degree 4 minus 2 fetched edges");
        let mut reversed = nodes.clone();
        reversed.reverse();
        assert_eq!(canvas_data(&reversed, &edges, "T-2", false), canvas);
    }

    /// One group: one box around all its nodes, over all layers.
    #[test]
    fn a_group_is_one_box_over_its_layers() {
        let nodes = vec![item("I-1", "initiative"), task("T-A"), task("T-B")];
        let edges = vec![
            edge("I-1", "T-A", "parent"),
            edge("I-1", "T-B", "parent"),
            edge("T-A", "T-B", "blocks"),
        ];
        let canvas = canvas_data(&nodes, &edges, "T-A", false);
        let geometry = placed(&canvas);
        let boxes = group_boxes(&geometry, &canvas.nodes);
        assert_eq!(boxes.len(), 1);
        let b = &boxes[0];
        for placed in &geometry.nodes {
            assert!(b.x <= placed.x && placed.x + placed.w <= b.x + b.w);
            assert!(b.y <= placed.y && placed.y + placed.h <= b.y + b.h);
        }
        assert!(b.header);
    }

    /// Two groups whose union boxes would cover the nodes of the other fall
    /// back to one box for each band; no box covers a node of a different
    /// group.
    #[test]
    fn interleaved_groups_split_into_bands() {
        // I-1: T-1 → (I-2: T-2) → T-3 — I-1 spans the column of T-2.
        let nodes = vec![
            item("I-1", "initiative"),
            item("I-2", "initiative"),
            task("T-1"),
            task("T-2"),
            task("T-3"),
            task("T-4"),
        ];
        let edges = vec![
            edge("I-1", "T-1", "parent"),
            edge("I-2", "T-2", "parent"),
            edge("I-1", "T-3", "parent"),
            edge("I-2", "T-4", "parent"),
            edge("T-1", "T-2", "blocks"),
            edge("T-2", "T-3", "blocks"),
            edge("T-4", "T-3", "blocks"),
        ];
        let canvas = canvas_data(&nodes, &edges, "T-2", false);
        let geometry = placed(&canvas);
        let boxes = group_boxes(&geometry, &canvas.nodes);
        for b in &boxes {
            for placed in &geometry.nodes {
                let lane = canvas
                    .nodes
                    .iter()
                    .find(|n| n.id == placed.id)
                    .and_then(|n| n.lane.clone());
                if lane.as_deref() == Some(b.code.as_str()) {
                    continue;
                }
                let inside = placed.x < b.x + b.w
                    && b.x < placed.x + placed.w
                    && placed.y < b.y + b.h
                    && b.y < placed.y + placed.h;
                assert!(!inside, "{} covers {}", b.code, placed.id);
            }
        }
        for code in ["I-1", "I-2"] {
            let headers = boxes.iter().filter(|b| b.code == code && b.header).count();
            assert_eq!(headers, 1, "{code}");
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let (scale, pan) = zoom_at(1.0, (10.0, 20.0), 2.0, (110.0, 120.0));
        assert_eq!(scale, 2.0);
        // The stage point under the pointer: (at - pan) / scale = (100, 100).
        assert_eq!(
            ((110.0 - pan.0) / scale, (120.0 - pan.1) / scale),
            (100.0, 100.0)
        );
        let (scale, _) = zoom_at(2.5, (0.0, 0.0), 2.0, (0.0, 0.0));
        assert_eq!(scale, ZOOM_MAX);
        let (scale, _) = zoom_at(0.3, (0.0, 0.0), 0.5, (0.0, 0.0));
        assert_eq!(scale, ZOOM_MIN);
        assert_eq!(fit_scale(2000.0, 1000.0), 0.5);
        assert_eq!(fit_scale(500.0, 1000.0), 1.0);
        assert_eq!(fit_scale(0.0, 1000.0), 1.0);
    }

    /// KAIROS-T-0778: the lane of the focus is first, then the lanes by
    /// short code, then the tasks with no initiative. Each lane is one row
    /// over all the columns, and the rows stack in that order.
    #[test]
    fn lanes_are_rows_the_focus_first_and_loose_tasks_last() {
        let nodes = vec![
            item("I-1", "initiative"),
            item("I-2", "initiative"),
            task("T-1"),
            task("T-2"),
            task("T-3"),
            task("T-9"),
        ];
        let edges = vec![
            edge("I-1", "T-1", "parent"),
            edge("I-1", "T-2", "parent"),
            edge("I-2", "T-3", "parent"),
            edge("T-1", "T-2", "blocks"),
        ];
        let canvas = canvas_data(&nodes, &edges, "T-3", false);
        let lanes: Vec<&str> = canvas.lanes.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(lanes, vec!["I-2", "I-1", NO_INITIATIVE]);
        let groups: Vec<&str> = canvas.groups.iter().map(|g| g.code.as_str()).collect();
        assert_eq!(groups, lanes, "a header for each lane, in the same order");
        assert!(canvas.groups[0].current, "the lane of the focus is marked");
        assert!(!canvas.groups[1].current);
        let loose = canvas.nodes.iter().find(|n| n.id == "T-9").expect("T-9");
        assert_eq!(loose.lane.as_deref(), Some(NO_INITIATIVE));

        let geometry = placed(&canvas);
        let boxes = group_boxes(&geometry, &canvas.nodes);
        let band = |code: &str| {
            let found: Vec<&GroupBox> = boxes.iter().filter(|b| b.code == code).collect();
            assert_eq!(found.len(), 1, "one box for {code}");
            found[0].clone()
        };
        let (b2, b1, b0) = (band("I-2"), band("I-1"), band(NO_INITIATIVE));
        assert!(
            b2.y + b2.h <= b1.y && b1.y + b1.h <= b0.y,
            "rows stack in order"
        );
        assert_eq!((b2.x, b2.w), (b1.x, b1.w), "each row is full width");
        assert_eq!((b1.x, b1.w), (b0.x, b0.w));
        // Inside a lane, the dependency order reads left to right.
        assert!(x_of(&geometry, "T-1") < x_of(&geometry, "T-2"));
        // The left margin holds the arrows between lanes.
        assert!(b1.x >= GUTTER - layout_options().lane_pad - 0.5);
    }

    /// KAIROS-T-0778: a blocks edge between two initiatives with lanes runs
    /// between the lanes; it is not an off-canvas edge.
    #[test]
    fn a_blocks_edge_between_two_lanes_is_drawn_between_them() {
        let mut done = item("I-3", "initiative");
        done.done = true;
        let nodes = vec![
            item("I-1", "initiative"),
            item("I-2", "initiative"),
            done,
            task("T-1"),
            task("T-2"),
            task("T-3"),
        ];
        let edges = vec![
            edge("I-1", "T-1", "parent"),
            edge("I-2", "T-2", "parent"),
            edge("I-3", "T-3", "parent"),
            edge("I-1", "I-2", "blocks"),
            edge("I-3", "I-2", "blocks"),
        ];
        let canvas = canvas_data(&nodes, &edges, "I-2", false);
        assert_eq!(
            canvas.lane_edges,
            vec![
                LaneEdge {
                    from: "I-1".into(),
                    to: "I-2".into(),
                    resolved: false
                },
                LaneEdge {
                    from: "I-3".into(),
                    to: "I-2".into(),
                    resolved: true
                },
            ]
        );
        assert!(canvas.off_canvas_blocks.is_empty());
        assert_eq!(canvas.lanes[0].id, "I-2", "the focus initiative is first");
    }

    /// KAIROS-T-0778: with no group, the tasks get no lane; with many
    /// initiatives, each one is a lane (no folding).
    #[test]
    fn loose_tasks_alone_get_no_lane_and_many_lanes_do_not_fold() {
        let nodes = vec![task("T-1"), task("T-2")];
        let canvas = canvas_data(&nodes, &[edge("T-1", "T-2", "blocks")], "T-1", false);
        assert!(canvas.lanes.is_empty());
        assert!(canvas.nodes.iter().all(|n| n.lane.is_none()));

        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for i in 0..12 {
            let (initiative, task_code) = (format!("I-{i:02}"), format!("T-{i:02}"));
            nodes.push(item(&initiative, "initiative"));
            nodes.push(task(&task_code));
            edges.push(edge(&initiative, &task_code, "parent"));
        }
        let canvas = canvas_data(&nodes, &edges, "I-05", false);
        assert_eq!(canvas.lanes.len(), 12);
        assert_eq!(canvas.lanes[0].id, "I-05");
        let geometry = placed(&canvas);
        let boxes = group_boxes(&geometry, &canvas.nodes);
        assert_eq!(boxes.len(), 12, "one row for each initiative");
    }
}
