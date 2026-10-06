//! The focal flight-level graph view (KAIROS-T-0089, design in
//! KAIROS-I-0008): one intentional SVG canvas answering *"what does this
//! item depend on, what does it feed into, and where does it sit?"* —
//! replacing the five stacked panels the UAT called "a grab bag".
//!
//! Rendering rules (survey-backed, recorded in the initiative):
//! - Strategy | Initiative | Task as FIXED layered columns; the Aurora
//!   [`Dag`] (COLLIERY-T-1836) owns the layout (a pure, deterministic
//!   function) and the interaction: click selects (here: refocuses),
//!   double click or Enter on the focus opens the item, hover dims all but
//!   the edges of a node. This module maps the subgraph to its data
//!   ([`canvas_data`], unit-tested);
//! - `parent` is containment (lane bands), never an arrow;
//! - `blocks` is the ONLY drawn arrow, in two styles (COLLIERY-T-0233):
//!   an open blocker, and a resolved one, where an end of the edge is in
//!   a done column (the COLLIERY-T-0214 rule). The legend names both;
//! - `supports`/`informs`/`supersedes` live in the side panel, which is
//!   suppressed when empty;
//! - depth 2 by default; `+N` badges expand in place (merge, no remount);
//! - refocus pushes a history entry and extends the `?trail=` breadcrumb,
//!   so browser back returns to the prior focus;
//! - red is used nowhere (reserved for violated/at-risk by convention);
//! - archived neighbours are **drawn and marked "put away"**
//!   (KAIROS-T-0158/T-0163, ADR-20): the subgraph is archived-inclusive
//!   because dropping a node broke every path THROUGH it, and `degree`
//!   counts them, so the `+N` arithmetic already balances.

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, Panel, Pill, SegmentedControl,
    Select, Stack, Text, TextInput,
};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};

use aurora_dark::graph::{Align, Dag, DagEdge, DagLane, DagNode, EdgeStyle, Hue};

use super::{RELATIONSHIPS, data, entity_color};
use crate::api;
use crate::auth::use_auth;

/// The canvas layer of an entity type (Strategy | Initiative | Task, the
/// fixed columns); documents/ADRs return `None` and render in the side
/// panel instead.
fn layer_of(entity_type: &str) -> Option<usize> {
    match entity_type {
        "strategy" => Some(0),
        "initiative" => Some(1),
        "task" => Some(2),
        _ => None,
    }
}

/// The hue of a canvas node: the same meaning as [`entity_color`].
fn hue_of(entity_type: &str) -> Hue {
    match entity_type {
        "strategy" => Hue::Violet,
        "initiative" => Hue::Ice,
        "task" => Hue::Teal,
        _ => Hue::Muted,
    }
}

/// The size of a node box. Three lines: title, short code, column.
const NODE_W: f64 = 220.0;
const NODE_H: f64 = 58.0;

/// The name of the edge style of an open `blocks` arrow.
const OPEN: &str = "open";
/// The name of the edge style of a resolved `blocks` arrow.
const RESOLVED: &str = "resolved";

/// The two styles of a `blocks` arrow (COLLIERY-T-0233). The dash carries
/// the difference for a reader who cannot see the colour. Red stays
/// unused (reserved for violated/at-risk by convention).
fn edge_styles() -> Vec<EdgeStyle> {
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

/// What the canvas draws: the product data for the Aurora [`Dag`].
#[derive(Debug, Clone, PartialEq)]
struct CanvasData {
    nodes: Vec<DagNode>,
    edges: Vec<DagEdge>,
    lanes: Vec<DagLane>,
}

/// Map the merged subgraph onto the canvas (pure, so it is unit-tested):
///
/// - only strategy/initiative/task nodes; the node id is the SHORT CODE,
///   so the callbacks give back the code and `data-id` is stable;
/// - `parent` (source = parent) is containment: a node has ONE lane, the
///   on-canvas parent in the previous layer with the lowest short code;
/// - `blocks` is the only arrow, open or resolved (COLLIERY-T-0233);
/// - `+N` = degree minus EVERY fetched incident edge (the full merged set
///   — side-panel material counts as fetched, so a badge always promises
///   an expansion that adds something).
fn canvas_data(nodes: &[data::GraphNode], edges: &[data::GraphEdge], focus: &str) -> CanvasData {
    use std::collections::{BTreeSet, HashMap};

    let mut fetched_incident: HashMap<&str, i64> = HashMap::new();
    for edge in edges {
        *fetched_incident.entry(edge.source_id.as_str()).or_default() += 1;
        *fetched_incident.entry(edge.target_id.as_str()).or_default() += 1;
    }
    // Canvas nodes by wire id: (node, layer).
    let on_canvas: HashMap<&str, (&data::GraphNode, usize)> = nodes
        .iter()
        .filter_map(|node| Some((node.id.as_str(), (node, layer_of(&node.entity_type)?))))
        .collect();

    // The lane of each node: its lowest-code parent one layer up.
    let mut lane_of: HashMap<&str, &str> = HashMap::new();
    for edge in edges.iter().filter(|e| e.relationship == "parent") {
        let (Some(&(parent, parent_layer)), Some(&(child, child_layer))) = (
            on_canvas.get(edge.source_id.as_str()),
            on_canvas.get(edge.target_id.as_str()),
        ) else {
            continue;
        };
        if parent_layer + 1 != child_layer {
            continue;
        }
        let entry = lane_of
            .entry(child.id.as_str())
            .or_insert(parent.short_code.as_str());
        if parent.short_code.as_str() < *entry {
            *entry = parent.short_code.as_str();
        }
    }
    let lanes: Vec<DagLane> = lane_of
        .values()
        .copied()
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .map(|parent| DagLane::new(parent, parent).anchor(parent))
        .collect();

    let mut dag_nodes: Vec<DagNode> = nodes
        .iter()
        .filter_map(|node| {
            let (_, layer) = on_canvas.get(node.id.as_str())?;
            let put_away = node.archived_at.is_some();
            let hidden =
                (node.degree - fetched_incident.get(node.id.as_str()).copied().unwrap_or(0)).max(0);
            // The old hover text: "put away" never "archived" — for a
            // document the status is the editorial lifecycle, which has
            // its own unrelated "archived" (KAIROS-T-0163).
            let mut tooltip = format!("{} — {}", node.title, node.status);
            if node.done {
                tooltip.push_str(" — done");
            }
            if put_away {
                tooltip.push_str(" — put away");
            }
            let mut dag = DagNode::new(node.short_code.clone(), node.title.clone())
                .sublabel(node.short_code.clone())
                .detail(node.status.clone())
                .kind(node.entity_type.clone())
                .layer(*layer)
                .status(hue_of(&node.entity_type))
                .sort_key(node.short_code.clone())
                .current(node.short_code == focus)
                .tooltip(tooltip)
                .more(u32::try_from(hidden).unwrap_or(u32::MAX));
            if let Some(parent) = lane_of.get(node.id.as_str()) {
                dag = dag.lane(*parent);
            }
            // The first mark sits at the right end: "put away", then "done".
            if put_away {
                dag = dag.archived(true).mark("put away", Hue::Gold);
            }
            if node.done {
                dag = dag.done(true).mark("done", Hue::Ok);
            }
            Some(dag)
        })
        .collect();
    dag_nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let dag_edges = edges
        .iter()
        .filter(|edge| edge.relationship == "blocks")
        .filter_map(|edge| {
            let (source, _) = on_canvas.get(edge.source_id.as_str())?;
            let (target, _) = on_canvas.get(edge.target_id.as_str())?;
            let edge = DagEdge::new(source.short_code.clone(), target.short_code.clone());
            Some(if blocks_resolved(nodes, &source.id, &target.id) {
                edge.style(RESOLVED)
                    .label("Resolved: one end is in a done column. This edge does not block.")
            } else {
                edge.style(OPEN).label("Open blocker")
            })
        })
        .collect();

    CanvasData {
        nodes: dag_nodes,
        edges: dag_edges,
        lanes,
    }
}

/// Whether a `blocks` arrow is resolved (COLLIERY-T-0233): an end of the
/// edge is in a done column, so the edge is history and not a blocker
/// (the COLLIERY-T-0214 rule, which the board counts follow). An end
/// that is not in the node set counts as not done.
fn blocks_resolved(nodes: &[data::GraphNode], source_id: &str, target_id: &str) -> bool {
    nodes
        .iter()
        .any(|node| node.done && (node.id == source_id || node.id == target_id))
}

/// One row of the supporting-material side panel.
#[derive(Clone, Debug, PartialEq)]
struct PanelRow {
    direction: String,
    entity_type: String,
    short_code: String,
    title: String,
    status: String,
    /// The ADR-20 state (KAIROS-T-0158): `Some(rfc3339)` = put away.
    /// Careful — for a DOCUMENT, `status` above is its editorial
    /// lifecycle, which can independently read "archived"
    /// (KAIROS-T-0078). Two different states, and this panel is where
    /// both can land in one row, so the marker never says only
    /// "archived".
    archived_at: Option<String>,
}

/// The focal graph, standalone: `short_code` is its only input (T-0090
/// mounts the same component on the item detail's Graph tab).
#[component]
pub fn GraphView(#[prop(into)] short_code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(short_code);
    let reload = RwSignal::new(0u32);

    // Base subgraph (depth 2) + in-place expansions, merged reactively —
    // expanding never remounts and never loses prior expansions.
    let base = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        let code = code.get_value();
        async move { data::item_graph(auth, &code, None).await }
    });
    let expansions = RwSignal::new(Vec::<data::GraphResponse>::new());
    let merged = Memo::new(move |_| {
        base.get().and_then(|result| result.ok()).map(|response| {
            let mut nodes = response.nodes;
            let mut edges = response.edges;
            for expansion in expansions.get() {
                for node in expansion.nodes {
                    if !nodes.iter().any(|existing| existing.id == node.id) {
                        nodes.push(node);
                    }
                }
                for edge in expansion.edges {
                    let duplicate = edges.iter().any(|existing| {
                        existing.source_id == edge.source_id
                            && existing.target_id == edge.target_id
                            && existing.relationship == edge.relationship
                    });
                    if !duplicate {
                        edges.push(edge);
                    }
                }
            }
            nodes.sort_by(|a, b| a.short_code.cmp(&b.short_code));
            (nodes, edges)
        })
    });

    let expand = move |expand_code: String| {
        leptos::task::spawn_local(async move {
            if let Ok(more) = data::item_graph(auth, &expand_code, Some(1)).await {
                expansions.update(|list| list.push(more));
            }
        });
    };

    // WS live (KAIROS-T-0090, T-0074 pattern): whole-tenant thin events;
    // refetch when the event names a VISIBLE node (reconnect reconciles
    // unconditionally). Deterministic layout means unchanged subgraphs
    // re-render at identical positions; expansions ride the merge memo,
    // so they survive the refetch.
    let live_guard: StoredValue<Option<crate::pages::boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(None);
    let guard = crate::pages::boards::live::subscribe_all_events(auth, move |event_code| {
        let concerns_view = match event_code {
            Some(event_code) => merged
                .try_get_untracked()
                .flatten()
                .is_some_and(|(nodes, _)| nodes.iter().any(|n| n.short_code == event_code)),
            None => true,
        };
        if concerns_view {
            reload.update(|n| *n += 1);
        }
    });
    live_guard.set_value(Some(guard));
    on_cleanup(move || live_guard.set_value(None));

    // Whoami drives the org-admin manage panel (roles from whoami, §6).
    let whoami = LocalResource::new(move || {
        let _ = auth.token();
        api::whoami(auth)
    });
    let is_admin = move || matches!(whoami.get(), Some(Ok(me)) if me.organization.role == "admin");

    // The visited-focus trail rides `?trail=` so back returns cleanly.
    let query = use_query_map();
    let trail = move || -> Vec<String> {
        query
            .read()
            .get("trail")
            .map(|raw| {
                raw.split(',')
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let navigate = use_navigate();
    let refocus = move |target: String| {
        let mut breadcrumbs = trail();
        breadcrumbs.push(code.get_value());
        let href = format!(
            "/search/relationships/{target}?trail={}",
            breadcrumbs.join(",")
        );
        navigate(&href, Default::default());
    };

    view! {
        {move || match merged.get() {
            None => match base.get() {
                Some(Err(error)) => view! {
                    <ErrorState
                        error
                        on_retry=Callback::new(move |_| reload.update(|n| *n += 1))
                    />
                }.into_any(),
                _ => view! { <Loading label="Walking the graph…"/> }.into_any(),
            },
            Some((nodes, edges)) => {
                // ---- canvas data (Aurora `Dag` owns layout + interaction) --
                let focus_code = code.get_value();
                let canvas = canvas_data(&nodes, &edges, &focus_code);
                let by_id = move |id: &str| nodes.iter().find(|n| n.id == id).cloned();

                // ---- side panel rows (supports/informs/supersedes) ---------
                let mut panel_rows: Vec<PanelRow> = Vec::new();
                for edge in edges
                    .iter()
                    .filter(|e| matches!(e.relationship.as_str(), "supports" | "informs" | "supersedes"))
                {
                    let (Some(source), Some(target)) =
                        (by_id(&edge.source_id), by_id(&edge.target_id))
                    else {
                        continue;
                    };
                    // Show the NON-canvas end (doc/ADR material), labeled
                    // with direction relative to the canvas item.
                    let (anchor, other, arrow) = if layer_of(&target.entity_type).is_none() {
                        (source, target, "→")
                    } else {
                        (target, source, "←")
                    };
                    panel_rows.push(PanelRow {
                        direction: format!("{} {arrow} {}", anchor.short_code, edge.relationship),
                        entity_type: other.entity_type.clone(),
                        short_code: other.short_code.clone(),
                        title: other.title.clone(),
                        status: other.status.clone(),
                        archived_at: other.archived_at.clone(),
                    });
                }
                panel_rows.sort_by(|a, b| {
                    a.short_code.cmp(&b.short_code).then(a.direction.cmp(&b.direction))
                });
                panel_rows.dedup();

                // ---- trail breadcrumbs -------------------------------------
                let breadcrumbs = trail();
                let trail_focus = focus_code.clone();
                let trail_chips = (!breadcrumbs.is_empty()).then(move || {
                    let chips = breadcrumbs
                        .iter()
                        .enumerate()
                        .map(|(index, crumb)| {
                            let href = if index == 0 {
                                format!("/search/relationships/{crumb}")
                            } else {
                                format!(
                                    "/search/relationships/{crumb}?trail={}",
                                    breadcrumbs[..index].join(",")
                                )
                            };
                            let crumb = crumb.clone();
                            view! {
                                <Group gap="xs">
                                    <Anchor href=href>
                                        <span class="cl-mono">{crumb}</span>
                                    </Anchor>
                                    <Text dimmed=true size="sm">"›"</Text>
                                </Group>
                            }
                        })
                        .collect_view();
                    view! {
                        <Group gap="xs" wrap=true>
                            <Text dimmed=true size="xs">"trail:"</Text>
                            {chips}
                            <Text bright=true size="sm">
                                <span class="cl-mono">{trail_focus}</span>
                            </Text>
                        </Group>
                    }
                });

                // ---- canvas ------------------------------------------------
                let CanvasData { nodes: dag_nodes, edges: dag_edges, lanes: dag_lanes } = canvas;
                let focus_for_select = focus_code.clone();
                let refocus = refocus.clone();
                let selected = Signal::derive({
                    let focus = focus_code.clone();
                    move || Some(focus.clone())
                });
                let open_item = {
                    let navigate = use_navigate();
                    move |target: String| navigate(&format!("/items/{target}"), Default::default())
                };

                view! {
                    <Stack gap="md">
                        {trail_chips}
                        <Panel
                            title="Flight-level graph"
                            caption="A band holds the children of a parent. An arrow shows a \
                                     block. Click a box to move the focus to it. Double-click a \
                                     box to open the item. Click +N to show more linked items."
                        >
                            <div class="kairos-graph" data-testid="item-graph">
                                <Dag
                                    nodes=dag_nodes
                                    edges=dag_edges
                                    lanes=dag_lanes
                                    layers=vec![
                                        "Strategy".to_string(),
                                        "Initiative".to_string(),
                                        "Task".to_string(),
                                    ]
                                    styles=edge_styles()
                                    legend=true
                                    align=Align::Start
                                    node_w=NODE_W
                                    node_h=NODE_H
                                    label="Flight-level graph"
                                    selected=selected
                                    on_select=Callback::new(move |target: String| {
                                        if target != focus_for_select {
                                            refocus(target);
                                        }
                                    })
                                    on_open=Callback::new(open_item)
                                    on_more=Callback::new(move |target: String| expand(target))
                                />
                            </div>
                        </Panel>
                        {(!panel_rows.is_empty()).then(|| view! {
                            <Panel
                                title="Supporting material"
                                caption="documents & ADRs — supports · informs · supersedes"
                            >
                                <Stack gap="xs">
                                    {panel_rows.into_iter().map(|row| {
                                        let detail = format!("/items/{}", row.short_code);
                                        let put_away = row.archived_at.map(|_| view! {
                                            <span class="kairos-archived-badge">
                                                <Pill color=token::GOLD>"put away"</Pill>
                                            </span>
                                        });
                                        view! {
                                            <Group gap="sm" wrap=true>
                                                <Pill color=token::MUTED>{row.direction}</Pill>
                                                <Pill color=entity_color(&row.entity_type)>
                                                    {row.entity_type.clone()}
                                                </Pill>
                                                <Anchor href=detail>
                                                    <span class="cl-mono">{row.short_code.clone()}</span>
                                                </Anchor>
                                                <Text size="sm">{row.title.clone()}</Text>
                                                <Pill color=token::MUTED>{row.status.clone()}</Pill>
                                                {put_away}
                                            </Group>
                                        }
                                    }).collect_view()}
                                </Stack>
                            </Panel>
                        })}
                        {move || is_admin().then(|| view! {
                            <ManagePanel
                                short_code=code.get_value()
                                on_changed=Callback::new(move |_| {
                                    expansions.set(Vec::new());
                                    reload.update(|n| *n += 1);
                                })
                            />
                        })}
                    </Stack>
                }.into_any()
            }
        }}
    }
}

/// Org-admin link/unlink (ported from the old explorer): the focus item's
/// direct edges with unlink buttons, plus the create-link form. The
/// server enforces the A-0001 rule matrix; typed 422s surface here.
#[component]
fn ManagePanel(#[prop(into)] short_code: String, on_changed: Callback<()>) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(short_code);
    let reload = RwSignal::new(0u32);
    let feedback = RwSignal::new(None::<Result<String, ApiError>>);

    let rels = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        let code = code.get_value();
        async move { data::relationships(auth, &code).await }
    });

    let role = RwSignal::new("source".to_string());
    let other = RwSignal::new(String::new());
    let rel_kind = RwSignal::new("parent".to_string());
    let busy = RwSignal::new(false);

    let create_link = move || {
        let this = code.get_value();
        let other_code = other.get_untracked().trim().to_uppercase();
        if other_code.is_empty() {
            feedback.set(Some(Err(ApiError::Unknown(
                "Enter the short code of the other item.".to_string(),
            ))));
            return;
        }
        let (source, target) = if role.get_untracked() == "source" {
            (this, other_code)
        } else {
            (other_code, this)
        };
        let request = data::CreateRelationship {
            source_short_code: source,
            target_short_code: target,
            relationship: rel_kind.get_untracked(),
        };
        busy.set(true);
        leptos::task::spawn_local(async move {
            match data::create_relationship(auth, &request).await {
                Ok(_) => {
                    feedback.set(Some(Ok(format!(
                        "Linked {} \u{2014}{}\u{2192} {}.",
                        request.source_short_code, request.relationship, request.target_short_code
                    ))));
                    reload.update(|n| *n += 1);
                    on_changed.run(());
                }
                Err(error) => feedback.set(Some(Err(error))),
            }
            busy.set(false);
        });
    };
    let unlink = move |edge_id: String, edge_desc: String| {
        leptos::task::spawn_local(async move {
            match data::delete_relationship(auth, &edge_id).await {
                Ok(_) => {
                    feedback.set(Some(Ok(format!("Unlinked {edge_desc}."))));
                    reload.update(|n| *n += 1);
                    on_changed.run(());
                }
                Err(error) => feedback.set(Some(Err(error))),
            }
        });
    };

    view! {
        <Panel
            title="Manage links"
            caption="org admin — the server checks the relationship rules and refuses a cycle"
        >
            <Stack gap="sm">
                {move || feedback.get().map(|outcome| match outcome {
                    Ok(message) => view! {
                        <Alert title="Done" color=token::OK>
                            <Text size="sm">{message}</Text>
                        </Alert>
                    }.into_any(),
                    Err(error) => view! { <ErrorState error=error/> }.into_any(),
                })}
                {move || match rels.get() {
                    Some(Ok(body)) => {
                        // (edge id, description, title, put away?) — the
                        // last because relationship lists are
                        // archived-inclusive (KAIROS-T-0158) and an admin
                        // about to unlink an edge should know that its
                        // far end is already put away.
                        let mut rows: Vec<(String, String, String, bool)> = Vec::new();
                        for (direction, groups) in
                            [("→", &body.outgoing), ("←", &body.incoming)]
                        {
                            for group in groups {
                                for item in &group.items {
                                    rows.push((
                                        item.relationship_id.clone(),
                                        format!(
                                            "{} {direction} {}",
                                            group.relationship, item.short_code
                                        ),
                                        item.title.clone(),
                                        item.archived_at.is_some(),
                                    ));
                                }
                            }
                        }
                        if rows.is_empty() {
                            view! { <Empty message="No edges on this item yet."/> }.into_any()
                        } else {
                            view! {
                                <Stack gap="xs">
                                    {rows.into_iter().map(|(edge_id, description, title, put_away)| {
                                        let edge_desc = description.clone();
                                        view! {
                                            <Group justify="between" wrap=true>
                                                <Group gap="sm" wrap=true>
                                                    <Pill color=token::MUTED>{description}</Pill>
                                                    <Text size="sm" dimmed=true>{title}</Text>
                                                    {put_away.then(|| view! {
                                                        <span class="kairos-archived-badge">
                                                            <Pill color=token::GOLD>"put away"</Pill>
                                                        </span>
                                                    })}
                                                </Group>
                                                <Button
                                                    variant="default"
                                                    size="xs"
                                                    bad=true
                                                    on_click=Callback::new(move |_| {
                                                        unlink(edge_id.clone(), edge_desc.clone());
                                                    })
                                                >
                                                    "Unlink"
                                                </Button>
                                            </Group>
                                        }
                                    }).collect_view()}
                                </Stack>
                            }.into_any()
                        }
                    }
                    _ => view! { <Loading label="Loading edges…"/> }.into_any(),
                }}
                <Group gap="sm" wrap=true top=true>
                    <Stack gap="xs">
                        <Text dimmed=true size="xs">
                            {format!("Role of {} in the new edge", code.get_value())}
                        </Text>
                        <SegmentedControl
                            options=vec!["source".to_string(), "target".to_string()]
                            value=role
                        />
                    </Stack>
                    <TextInput
                        label="Other item (short code)"
                        placeholder="DEMO-T-0001"
                        value=other
                    />
                    <Select
                        label="Relationship"
                        options=RELATIONSHIPS.iter().map(|r| r.to_string()).collect()
                        value=rel_kind
                    />
                    {move || view! {
                        <Button
                            disabled=busy.get()
                            on_click=Callback::new(move |_| create_link())
                        >
                            {if busy.get() { "Linking…" } else { "Create link" }}
                        </Button>
                    }}
                </Group>
            </Stack>
        </Panel>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, done: bool) -> data::GraphNode {
        data::GraphNode {
            id: id.to_string(),
            short_code: format!("DEMO-T-{id}"),
            entity_type: "task".to_string(),
            title: String::new(),
            status: String::new(),
            depth: 0,
            degree: 0,
            archived_at: None,
            done,
        }
    }

    /// COLLIERY-T-0233: a `blocks` arrow is resolved when the node at
    /// one end is in a done column, and open only when no end is.
    #[test]
    fn a_blocks_arrow_with_a_done_end_is_resolved() {
        let nodes = vec![
            node("open", false),
            node("other", false),
            node("done", true),
        ];
        assert!(!blocks_resolved(&nodes, "open", "other"));
        assert!(blocks_resolved(&nodes, "done", "open"), "done blocker");
        assert!(blocks_resolved(&nodes, "open", "done"), "done blocked item");
        assert!(!blocks_resolved(&nodes, "open", "absent"), "unknown end");
    }

    fn typed(id: &str, entity_type: &str) -> data::GraphNode {
        data::GraphNode {
            short_code: id.to_string(),
            entity_type: entity_type.to_string(),
            title: format!("title {id}"),
            status: "Active".to_string(),
            ..node(id, false)
        }
    }

    fn edge(source: &str, target: &str, relationship: &str) -> data::GraphEdge {
        data::GraphEdge {
            source_id: source.to_string(),
            target_id: target.to_string(),
            relationship: relationship.to_string(),
            depth: 1,
        }
    }

    /// COLLIERY-T-1836: a node has ONE lane — of its parents one layer up,
    /// the one with the lowest short code. A parent two layers up, and a
    /// document, give no lane; the document is not on the canvas.
    #[test]
    fn a_node_has_one_lane_the_lowest_parent_code() {
        let nodes = vec![
            typed("A-S-1", "strategy"),
            typed("A-I-2", "initiative"),
            typed("A-I-1", "initiative"),
            typed("A-T-1", "task"),
            typed("A-T-2", "task"),
            typed("A-D-1", "document"),
        ];
        let edges = vec![
            edge("A-I-2", "A-T-1", "parent"),
            edge("A-I-1", "A-T-1", "parent"),
            edge("A-S-1", "A-T-2", "parent"),
            edge("A-S-1", "A-I-2", "parent"),
            edge("A-D-1", "A-I-1", "supports"),
        ];
        let canvas = canvas_data(&nodes, &edges, "A-T-1");
        let lane = |id: &str| {
            canvas
                .nodes
                .iter()
                .find(|n| n.id == id)
                .and_then(|n| n.lane.clone())
        };
        assert_eq!(lane("A-T-1").as_deref(), Some("A-I-1"));
        assert_eq!(lane("A-T-2"), None, "a parent two layers up is no lane");
        assert_eq!(lane("A-I-2").as_deref(), Some("A-S-1"));
        let lanes: Vec<&str> = canvas.lanes.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(lanes, vec!["A-I-1", "A-S-1"]);
        assert!(canvas.nodes.iter().all(|n| n.id != "A-D-1"), "no documents");
        assert!(canvas.edges.is_empty(), "parent and supports draw no arrow");
        let focus: Vec<&str> = canvas
            .nodes
            .iter()
            .filter(|n| n.current)
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(focus, vec!["A-T-1"]);
    }

    /// Blocks arrows get the open or the resolved style; marks, the
    /// archived flag and `+N` come from the wire node; the node order does
    /// not depend on the input order.
    #[test]
    fn blocks_styles_marks_and_more() {
        let mut done = typed("A-T-1", "task");
        done.done = true;
        done.archived_at = Some("2026-01-01T00:00:00Z".to_string());
        let mut open = typed("A-T-2", "task");
        open.degree = 4;
        let nodes = vec![open, done, typed("A-T-3", "task")];
        let edges = vec![
            edge("A-T-1", "A-T-2", "blocks"),
            edge("A-T-3", "A-T-2", "blocks"),
        ];
        let canvas = canvas_data(&nodes, &edges, "A-T-2");
        let styles: Vec<(&str, &str)> = canvas
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.style.as_str()))
            .collect();
        assert_eq!(styles, vec![("A-T-1", RESOLVED), ("A-T-3", OPEN)]);
        let ids: Vec<&str> = canvas.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["A-T-1", "A-T-2", "A-T-3"]);
        let first = &canvas.nodes[0];
        assert!(first.archived && first.done);
        let marks: Vec<&str> = first.marks.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(marks, vec!["put away", "done"]);
        assert_eq!(canvas.nodes[1].more, 2, "degree 4 minus 2 fetched edges");
        assert_eq!(canvas.nodes[2].more, 0);

        let mut reversed = nodes.clone();
        reversed.reverse();
        assert_eq!(canvas_data(&reversed, &edges, "A-T-2").nodes, canvas.nodes);
    }
}
