//! The focal flight-level graph view (KAIROS-T-0089, design in
//! KAIROS-I-0008): one intentional SVG canvas answering *"what does this
//! item depend on, what does it feed into, and where does it sit?"* —
//! replacing the five stacked panels the UAT called "a grab bag".
//!
//! Rendering rules (survey-backed, recorded in the initiative):
//! - Strategy | Initiative | Task as FIXED layered columns; layout is the
//!   pure [`super::graph_layout`] module (deterministic, unit-tested);
//! - `parent` is containment (lane bands), never an arrow;
//! - `blocks` is the ONLY drawn arrow;
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

use super::graph_layout::{self, Column, LayoutInputEdge, LayoutInputNode};
use super::{RELATIONSHIPS, data, entity_color};
use crate::api;
use crate::auth::use_auth;

/// The canvas column of an entity type; documents/ADRs return `None` and
/// render in the side panel instead.
fn column_of(entity_type: &str) -> Option<Column> {
    match entity_type {
        "strategy" => Some(Column::Strategy),
        "initiative" => Some(Column::Initiative),
        "task" => Some(Column::Task),
        _ => None,
    }
}

/// Truncate a title for its node box (SVG text does not wrap).
fn clip(title: &str, max: usize) -> String {
    if title.chars().count() <= max {
        title.to_string()
    } else {
        let clipped: String = title.chars().take(max.saturating_sub(1)).collect();
        format!("{clipped}…")
    }
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
    let hovered = RwSignal::new(None::<String>);

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
                // ---- canvas geometry (pure, deterministic) -----------------
                // `+N` = degree minus EVERY fetched incident edge (the full
                // merged set — side-panel material counts as fetched, so a
                // badge always promises an expansion that adds something).
                let mut fetched_incident: std::collections::HashMap<&str, i64> =
                    std::collections::HashMap::new();
                for edge in &edges {
                    *fetched_incident.entry(edge.source_id.as_str()).or_default() += 1;
                    *fetched_incident.entry(edge.target_id.as_str()).or_default() += 1;
                }
                let canvas_inputs: Vec<LayoutInputNode> = nodes
                    .iter()
                    .filter_map(|node| {
                        Some(LayoutInputNode {
                            id: node.id.clone(),
                            short_code: node.short_code.clone(),
                            column: column_of(&node.entity_type)?,
                            hidden_neighbors: (node.degree
                                - fetched_incident
                                    .get(node.id.as_str())
                                    .copied()
                                    .unwrap_or(0))
                            .max(0),
                        })
                    })
                    .collect();
                let on_canvas: std::collections::HashSet<&str> =
                    canvas_inputs.iter().map(|n| n.id.as_str()).collect();
                let canvas_edges: Vec<LayoutInputEdge> = edges
                    .iter()
                    .filter(|edge| {
                        on_canvas.contains(edge.source_id.as_str())
                            && on_canvas.contains(edge.target_id.as_str())
                    })
                    .map(|edge| LayoutInputEdge {
                        source_id: edge.source_id.clone(),
                        target_id: edge.target_id.clone(),
                        relationship: edge.relationship.clone(),
                    })
                    .collect();
                let geometry = graph_layout::layout(&canvas_inputs, &canvas_edges);
                let by_id = move |id: &str| nodes.iter().find(|n| n.id == id).cloned();
                let focus_code = code.get_value();

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
                    let (anchor, other, arrow) = if column_of(&target.entity_type).is_none() {
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

                // ---- SVG ----------------------------------------------------
                let lane_label = |id: &str| {
                    by_id(id).map(|n| n.short_code).unwrap_or_default()
                };
                let lanes = geometry
                    .lanes
                    .iter()
                    .map(|lane| {
                        let label = lane_label(&lane.parent_id);
                        view! {
                            <g>
                                <rect
                                    class="kairos-graph__lane"
                                    x=lane.x y=lane.y width=lane.w height=lane.h rx=8
                                ></rect>
                                <text
                                    class="kairos-graph__lane-label"
                                    x=lane.x + 6.0 y=lane.y - 3.0
                                >{label}</text>
                            </g>
                        }
                    })
                    .collect_view();
                let arrows = geometry
                    .arrows
                    .iter()
                    .map(|arrow| {
                        let source = arrow.source_id.clone();
                        let target = arrow.target_id.clone();
                        let path = arrow.path.clone();
                        let hot = move || {
                            hovered.get().is_some_and(|h| h == source || h == target)
                        };
                        view! {
                            <path
                                class=move || if hot() {
                                    "kairos-graph__edge kairos-graph__edge--hot"
                                } else {
                                    "kairos-graph__edge"
                                }
                                d=path
                                marker-end="url(#kairos-graph-arrowhead)"
                            ></path>
                        }
                    })
                    .collect_view();
                let boxes = geometry
                    .nodes
                    .iter()
                    .filter_map(|placed| {
                        let node = by_id(&placed.id)?;
                        let is_focus = node.short_code == focus_code;
                        let node_id = node.id.clone();
                        let enter_id = node_id.clone();
                        // KAIROS-T-0158/T-0163: an archived node is DRAWN
                        // (dropping it broke the paths through it), so it
                        // has to be drawn differently — otherwise the
                        // reader plans against a retired item. "put away"
                        // never "archived": for a document the status
                        // line below is the editorial lifecycle, which
                        // has its own unrelated "archived".
                        let put_away = node.archived_at.is_some();
                        let title_attr = if put_away {
                            format!("{} — {} — put away", node.title, node.status)
                        } else {
                            format!("{} — {}", node.title, node.status)
                        };
                        let node_code = node.short_code.clone();
                        let refocus_code = node_code.clone();
                        let detail_code = node_code.clone();
                        let expand_code = node_code.clone();
                        let refocus = refocus.clone();
                        let navigate = use_navigate();
                        let (x, y, w, h) = (placed.x, placed.y, placed.w, placed.h);
                        let hidden = placed.hidden_neighbors;
                        Some(view! {
                            <g
                                class=if is_focus {
                                    "kairos-graph__node kairos-graph__node--focus"
                                } else {
                                    "kairos-graph__node"
                                }
                                class:kairos-graph__node--put-away=put_away
                                on:mouseenter=move |_| hovered.set(Some(enter_id.clone()))
                                on:mouseleave=move |_| hovered.set(None)
                            >
                                <title>{title_attr}</title>
                                <rect
                                    class="kairos-graph__box"
                                    x=x y=y width=w height=h rx=8
                                    stroke=entity_color(&node.entity_type)
                                    on:click=move |_| {
                                        if !is_focus {
                                            refocus(refocus_code.clone());
                                        }
                                    }
                                ></rect>
                                <text
                                    class="kairos-graph__code"
                                    x=x + 10.0 y=y + 20.0
                                    on:click=move |_| navigate(
                                        &format!("/items/{detail_code}"),
                                        Default::default(),
                                    )
                                >{node.short_code.clone()}</text>
                                <text
                                    class="kairos-graph__title"
                                    x=x + 10.0 y=y + 36.0
                                >{clip(&node.title, 26)}</text>
                                <text
                                    class="kairos-graph__status"
                                    x=x + 10.0 y=y + 50.0
                                >{node.status.clone()}</text>
                                {put_away.then(|| view! {
                                    <text
                                        class="kairos-graph__put-away"
                                        x=x + w - 10.0 y=y + 50.0
                                    >"put away"</text>
                                })}
                                {(hidden > 0).then(|| {
                                    view! {
                                        <g
                                            class="kairos-graph__more"
                                            on:click=move |_| expand(expand_code.clone())
                                        >
                                            <title>{format!("{hidden} more linked item(s) — click to expand")}</title>
                                            <circle cx=x + w - 16.0 cy=y + 16.0 r=11></circle>
                                            <text x=x + w - 16.0 y=y + 20.0>
                                                {format!("+{hidden}")}
                                            </text>
                                        </g>
                                    }
                                })}
                            </g>
                        })
                    })
                    .collect_view();
                let headers = geometry
                    .headers
                    .iter()
                    .map(|(label, x)| {
                        view! {
                            <text class="kairos-graph__header" x=*x y=18>{*label}</text>
                        }
                    })
                    .collect_view();
                let (view_w, view_h) = (geometry.width, geometry.height);

                view! {
                    <Stack gap="md">
                        {trail_chips}
                        <Panel
                            title="Flight-level graph"
                            caption="parent = containment · blocks = arrows · depth 2, +N expands \
                                     · put-away items are drawn, marked"
                        >
                            <div class="kairos-graph">
                                <svg
                                    width=view_w
                                    height=view_h
                                    viewBox=format!("0 0 {view_w} {view_h}")
                                    role="img"
                                >
                                    <defs>
                                        <marker
                                            id="kairos-graph-arrowhead"
                                            viewBox="0 0 10 10"
                                            refX="9" refY="5"
                                            markerWidth="7" markerHeight="7"
                                            orient="auto-start-reverse"
                                        >
                                            <path d="M 0 0 L 10 5 L 0 10 z" class="kairos-graph__arrowhead"></path>
                                        </marker>
                                    </defs>
                                    {headers}
                                    {lanes}
                                    {arrows}
                                    {boxes}
                                </svg>
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
                "Enter the other item's short code.".to_string(),
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
            caption="org admin — the server enforces the A-0001 rule matrix and cycle checks"
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
