//! The focal flight-level graph view (KAIROS-T-0089, design in
//! KAIROS-I-0008): one intentional SVG canvas answering *"what does this
//! item depend on, what does it feed into, and where does it sit?"* —
//! replacing the five stacked panels the UAT called "a grab bag".
//!
//! Rendering rules (COLLIERY-I-0626: the graph shows work dependencies):
//! - layers come from `blocks` (KAIROS-T-0492): a chain reads left to
//!   right, tasks with no edge between them stack in one column. The
//!   Aurora [`Dag`] (COLLIERY-T-1836) owns the layout (a pure,
//!   deterministic function) and the interaction: click selects (here:
//!   refocuses), double click or Enter on the focus opens the item, hover
//!   dims all but the edges of a node. [`graph_model`] maps the subgraph
//!   to its data (unit-tested);
//! - `parent` is a group box with a header, never an arrow or a layer;
//!   colour is the state of the item; zoom and pan move the stage;
//! - `blocks` is the ONLY drawn arrow, in two styles (COLLIERY-T-0233):
//!   an open blocker, and a resolved one, where an end of the edge is in
//!   a done column (the COLLIERY-T-0214 rule). The legend names both;
//! - `supports`/`informs`/`supersedes`/`amends` live in the side panel, which is
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

use aurora_dark::graph::{Dag, DagLegend};
use aurora_dark::graph_layout::layout;

use super::graph_model::{
    self, CanvasData, GroupBox, NODE_H, canvas_data, edge_styles, fit_scale, group_boxes,
    layout_options, zoom_at,
};
use super::{RELATIONSHIPS, data, entity_color};
use crate::api;
use crate::auth::use_auth;

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
    // KAIROS-T-0309: the result of the last link or unlink. It lives here
    // and not in the manage panel: a change to the graph builds the panel
    // again, and a signal of the panel would go with it, so "Unlinked ..."
    // showed for a few milliseconds only.
    let link_feedback = RwSignal::new(None::<Result<String, ApiError>>);

    // Base subgraph (depth 2) + in-place expansions, merged reactively —
    // expanding never remounts and never loses prior expansions.
    let base = LocalResource::new(move || {
        auth.track_session();
        let _ = reload.get();
        let code = code.get_value();
        async move { data::item_graph(auth, &code, None).await }
    });
    let expansions = RwSignal::new(Vec::<data::GraphResponse>::new());
    // KAIROS-T-0492: show the folded siblings of the focus.
    let unfold = RwSignal::new(false);
    // Zoom and pan live here, so a live refetch keeps them.
    let view_state = ViewState::new();
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
    // The shell fetches it once and shares it as a context.
    let whoami = use_context::<LocalResource<Result<api::Whoami, ApiError>>>();
    let is_admin = move || matches!(whoami.and_then(|w| w.get()), Some(Ok(me)) if me.organization.role == "admin");

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
                let canvas = canvas_data(&nodes, &edges, &focus_code, unfold.get());
                let by_id = |id: &str| nodes.iter().find(|n| n.id == id).cloned();
                let is_work = |entity_type: &str| {
                    matches!(entity_type, "strategy" | "initiative" | "task")
                };

                // ---- side panel rows ---------------------------------------
                // supports/informs/supersedes/amends, and the `blocks` edges with an
                // end that is a group header (not a node).
                let mut panel_rows: Vec<PanelRow> = Vec::new();
                for edge in edges
                    .iter()
                    .filter(|e| matches!(e.relationship.as_str(), "supports" | "informs" | "supersedes" | "amends"))
                {
                    let (Some(source), Some(target)) =
                        (by_id(&edge.source_id), by_id(&edge.target_id))
                    else {
                        continue;
                    };
                    // Show the document/ADR end, labeled with direction
                    // relative to the work item.
                    let (anchor, other, arrow) = if !is_work(&target.entity_type) {
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
                for (source, target) in &canvas.off_canvas_blocks {
                    let Some(other) = nodes.iter().find(|n| &n.short_code == source) else {
                        continue;
                    };
                    panel_rows.push(PanelRow {
                        direction: format!("{target} ← blocks"),
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
                let focus_for_select = focus_code.clone();
                let refocus_node = refocus.clone();
                let refocus_group = refocus.clone();
                let open_item = {
                    let navigate = use_navigate();
                    move |target: String| navigate(&format!("/items/{target}"), Default::default())
                };

                view! {
                    <Stack gap="md">
                        {trail_chips}
                        <Panel
                            title="Work graph"
                            caption="An arrow shows a block: work goes from left to right. \
                                     Items in one column can go in parallel. A box holds the \
                                     tasks of a parent. Click an item to move the focus to it. \
                                     Double-click an item to open it. Click +N to show more \
                                     items."
                        >
                            <WorkCanvas
                                canvas=canvas
                                focus=focus_code.clone()
                                view_state=view_state
                                unfold=unfold
                                on_select=Callback::new(move |target: String| {
                                    if target != focus_for_select {
                                        refocus_node(target);
                                    }
                                })
                                on_group=Callback::new(move |target: String| refocus_group(target))
                                on_open=Callback::new(open_item)
                                on_more=Callback::new(move |target: String| expand(target))
                            />
                        </Panel>
                        {(!panel_rows.is_empty()).then(|| view! {
                            <Panel
                                title="Supporting material"
                                caption="documents & ADRs — supports · informs · supersedes · amends; \
                                         blocks to or from a group"
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
                                feedback=link_feedback
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

/// Zoom and pan of the canvas (KAIROS-T-0492). The graph view owns it, so
/// a live refetch keeps it.
#[derive(Clone, Copy)]
struct ViewState {
    scale: RwSignal<f64>,
    pan: RwSignal<(f64, f64)>,
    /// The zoom of the first fit. It sets the height of the viewport.
    fit: RwSignal<f64>,
    fitted: RwSignal<bool>,
}

impl ViewState {
    fn new() -> Self {
        Self {
            scale: RwSignal::new(1.0),
            pan: RwSignal::new((0.0, 0.0)),
            fit: RwSignal::new(1.0),
            fitted: RwSignal::new(false),
        }
    }
}

/// The width of one character of a group header (10px mono).
const HEADER_CHAR_W: f64 = 6.1;
/// One zoom step.
const ZOOM_STEP: f64 = 1.2;
/// One pan step of an arrow key, in pixels.
const PAN_STEP: f64 = 48.0;

/// `text` cut to `max` characters, with an ellipsis.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// A keyboard press that works a link or a button: Enter or Space.
fn activates(ev: &leptos::ev::KeyboardEvent) -> bool {
    matches!(ev.key().as_str(), "Enter" | " " | "Spacebar")
}

/// A link in the header of a group: it moves the focus to the item.
fn header_link(
    x: f64,
    y: f64,
    text: String,
    code: String,
    on_group: Callback<String>,
) -> impl IntoView {
    let key_code = code.clone();
    view! {
        <text
            class="kairos-graph__link"
            x=x
            y=y
            role="link"
            tabindex="0"
            data-code=code.clone()
            on:click=move |_| on_group.run(code.clone())
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if activates(&ev) {
                    ev.prevent_default();
                    on_group.run(key_code.clone());
                }
            }
        >
            {text}
        </text>
    }
}

/// A button in the header of a group (SVG text, keyboard ready).
fn header_button(
    x: f64,
    y: f64,
    class: &'static str,
    text: Signal<String>,
    aria: Signal<String>,
    on_press: Callback<()>,
) -> impl IntoView {
    view! {
        <text
            class=format!("kairos-graph__link {class}")
            x=x
            y=y
            text-anchor="end"
            role="button"
            tabindex="0"
            aria-label=aria
            on:click=move |_| on_press.run(())
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if activates(&ev) {
                    ev.prevent_default();
                    on_press.run(());
                }
            }
        >
            {text}
        </text>
    }
}

/// The header of a group box: the strategy (when there is one), the parent,
/// the toggle of the folded siblings, and the `+N` of the neighbours that
/// are not fetched.
fn group_header(
    b: GroupBox,
    group: graph_model::Group,
    unfold: RwSignal<bool>,
    on_group: Callback<String>,
    on_more: Callback<String>,
) -> impl IntoView {
    let y = b.y - 4.0;
    let mut right = b.x + b.w - 6.0;
    let mut x = b.x + 6.0;

    let more = (group.more > 0).then(|| {
        let n = group.more;
        let label = format!("+{n}");
        let at = right;
        right -= (label.chars().count() + 2) as f64 * HEADER_CHAR_W;
        let codes = group.expand.clone();
        header_button(
            at,
            y,
            "kairos-graph__more",
            Signal::stored(label),
            Signal::stored(format!("Show {n} more linked items")),
            Callback::new(move |_| {
                for code in &codes {
                    on_more.run(code.clone());
                }
            }),
        )
    });
    let toggle = (group.folded > 0).then(|| {
        let n = group.folded;
        let at = right;
        right -= 13.0 * HEADER_CHAR_W;
        header_button(
            at,
            y,
            "kairos-graph__fold",
            Signal::derive(move || {
                if unfold.get() {
                    "fewer tasks".to_string()
                } else {
                    format!("+{n} tasks")
                }
            }),
            Signal::derive(move || {
                if unfold.get() {
                    format!("Hide {n} sibling tasks")
                } else {
                    format!("Show {n} more sibling tasks")
                }
            }),
            Callback::new(move |_| unfold.update(|u| *u = !*u)),
        )
    });
    let strategy = group.strategy.clone().map(|(code, _)| {
        let at = x;
        let width = code.chars().count() as f64 * HEADER_CHAR_W;
        x += width + 3.0 * HEADER_CHAR_W;
        view! {
            {header_link(at, y, code.clone(), code, on_group)}
            <text class="kairos-graph__sep" x=at + width + HEADER_CHAR_W y=y>"›"</text>
        }
    });
    let room = ((right - x) / HEADER_CHAR_W).floor().max(4.0) as usize;
    let label = clip(&format!("{} {}", group.code, group.title), room);
    let main = header_link(x, y, label, group.code.clone(), on_group);
    let class = if group.current {
        "kairos-graph__header kairos-graph__header--current"
    } else {
        "kairos-graph__header"
    };
    view! {
        <g class=class data-group=group.code.clone() data-kind=group.entity_type.clone()>
            {strategy}
            {main}
            {toggle}
            {more}
        </g>
    }
}

/// The canvas (KAIROS-T-0492): the Aurora [`Dag`] with no fixed layers,
/// so `blocks` sets the columns; the group boxes under it and their
/// headers over it; all three in a stage that zooms and pans.
#[component]
fn WorkCanvas(
    canvas: CanvasData,
    #[prop(into)] focus: String,
    view_state: ViewState,
    unfold: RwSignal<bool>,
    on_select: Callback<String>,
    on_group: Callback<String>,
    on_open: Callback<String>,
    on_more: Callback<String>,
) -> impl IntoView {
    use wasm_bindgen::JsCast;

    let CanvasData {
        nodes,
        edges,
        lanes,
        groups,
        states,
        ..
    } = canvas;
    // The same options as the `Dag`, so the boxes fit its nodes.
    let opts = layout_options();
    let geometry = layout(&nodes, &edges, &lanes, &[], &opts);
    let boxes = group_boxes(&geometry, &nodes);
    let (width, height) = (geometry.width, geometry.height);
    let used_styles: Vec<_> = edge_styles()
        .into_iter()
        .filter(|s| edges.iter().any(|e| e.style == s.name))
        .collect();
    let group_of = |code: &str| groups.iter().find(|g| g.code == code).cloned();

    let rects = boxes
        .iter()
        .map(|b| {
            let class = if group_of(&b.code).is_some_and(|g| g.current) {
                "kairos-graph__group kairos-graph__group--current"
            } else {
                "kairos-graph__group"
            };
            view! {
                <rect
                    class=class
                    data-group=b.code.clone()
                    x=b.x
                    y=b.y
                    width=b.w
                    height=b.h
                    rx="10"
                />
            }
        })
        .collect_view();
    let headers = boxes
        .iter()
        .filter(|b| b.header)
        .filter_map(|b| {
            Some(group_header(
                b.clone(),
                group_of(&b.code)?,
                unfold,
                on_group,
                on_more,
            ))
        })
        .collect_view();
    let state_legend = states
        .iter()
        .map(|state| {
            let label = state.label();
            view! {
                <span class=format!("kairos-graph__state kairos-graph__state--{label}")>
                    {label}
                </span>
            }
        })
        .collect_view();

    // ---- zoom and pan ----------------------------------------------------
    let vs = view_state;
    let viewport = NodeRef::<leptos::html::Div>::new();
    // The first fit, when the viewport has a width.
    Effect::new(move |_| {
        if vs.fitted.get_untracked() {
            return;
        }
        if let Some(el) = viewport.get() {
            let fit = fit_scale(width, f64::from(el.client_width()));
            vs.fit.set(fit);
            vs.scale.set(fit);
            vs.pan.set((0.0, 0.0));
            vs.fitted.set(true);
        }
    });
    let zoom_by = move |factor: f64, at: Option<(f64, f64)>| {
        let at = at.unwrap_or_else(|| {
            viewport
                .get_untracked()
                .map(|el| {
                    (
                        f64::from(el.client_width()) / 2.0,
                        f64::from(el.client_height()) / 2.0,
                    )
                })
                .unwrap_or_default()
        });
        let (scale, pan) = zoom_at(vs.scale.get_untracked(), vs.pan.get_untracked(), factor, at);
        vs.scale.set(scale);
        vs.pan.set(pan);
    };
    let fit = move || {
        vs.scale.set(vs.fit.get_untracked());
        vs.pan.set((0.0, 0.0));
    };
    let pan_by = move |dx: f64, dy: f64| {
        vs.pan.update(|(x, y)| {
            *x += dx;
            *y += dy;
        })
    };
    // Ctrl or Cmd and the wheel (a pinch on a trackpad) zoom at the pointer;
    // the wheel alone scrolls the page.
    let on_wheel = move |ev: leptos::ev::WheelEvent| {
        if !(ev.ctrl_key() || ev.meta_key()) {
            return;
        }
        ev.prevent_default();
        let Some(el) = viewport.get_untracked() else {
            return;
        };
        let rect = el.get_bounding_client_rect();
        let at = (
            f64::from(ev.client_x()) - rect.left(),
            f64::from(ev.client_y()) - rect.top(),
        );
        let factor = if ev.delta_y() < 0.0 { 1.1 } else { 1.0 / 1.1 };
        zoom_by(factor, Some(at));
    };
    // A drag on the background pans. A press on an item or a link does not.
    let drag = StoredValue::new(None::<(f64, f64, (f64, f64))>);
    let on_pointerdown = move |ev: leptos::ev::PointerEvent| {
        if ev.button() != 0 {
            return;
        }
        let on_control = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| {
                el.closest(".cl-dag__item, .kairos-graph__link")
                    .ok()
                    .flatten()
            })
            .is_some();
        if on_control {
            return;
        }
        drag.set_value(Some((
            f64::from(ev.client_x()),
            f64::from(ev.client_y()),
            vs.pan.get_untracked(),
        )));
        if let Some(el) = viewport.get_untracked() {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
    };
    let on_pointermove = move |ev: leptos::ev::PointerEvent| {
        if let Some((x0, y0, (px, py))) = drag.get_value() {
            vs.pan.set((
                px + f64::from(ev.client_x()) - x0,
                py + f64::from(ev.client_y()) - y0,
            ));
        }
    };
    let end_drag = move |_: leptos::ev::PointerEvent| drag.set_value(None);
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let handled = match ev.key().as_str() {
            "ArrowLeft" => {
                pan_by(PAN_STEP, 0.0);
                true
            }
            "ArrowRight" => {
                pan_by(-PAN_STEP, 0.0);
                true
            }
            "ArrowUp" => {
                pan_by(0.0, PAN_STEP);
                true
            }
            "ArrowDown" => {
                pan_by(0.0, -PAN_STEP);
                true
            }
            "+" | "=" => {
                zoom_by(ZOOM_STEP, None);
                true
            }
            "-" | "_" => {
                zoom_by(1.0 / ZOOM_STEP, None);
                true
            }
            "0" => {
                fit();
                true
            }
            _ => false,
        };
        if handled {
            ev.prevent_default();
        }
    };

    let selected = Signal::derive({
        let focus = focus.clone();
        move || Some(focus.clone())
    });
    let view_box = format!("0 0 {width} {height}");
    view! {
        <div class="kairos-graph" data-testid="item-graph">
            <div class="kairos-graph__toolbar">
                <div class="kairos-graph__legend" aria-label="States">{state_legend}</div>
                {(!used_styles.is_empty()).then(|| view! { <DagLegend styles=used_styles/> })}
                <Group gap="xs">
                    <Button variant="default" size="xs" on_click=Callback::new(move |_| zoom_by(1.0 / ZOOM_STEP, None))>
                        "Zoom out"
                    </Button>
                    <span class="kairos-graph__zoom cl-mono" data-testid="graph-zoom">
                        {move || format!("{:.0}%", vs.scale.get() * 100.0)}
                    </span>
                    <Button variant="default" size="xs" on_click=Callback::new(move |_| zoom_by(ZOOM_STEP, None))>
                        "Zoom in"
                    </Button>
                    <Button variant="default" size="xs" on_click=Callback::new(move |_| fit())>
                        "Fit"
                    </Button>
                </Group>
            </div>
            <div
                class="kairos-graph__viewport"
                node_ref=viewport
                tabindex="0"
                aria-label="Work graph. Arrow keys pan, plus and minus zoom, 0 fits."
                style:height=move || format!("{:.0}px", (height * vs.fit.get()).max(NODE_H * 3.0) + 2.0)
                on:wheel=on_wheel
                on:pointerdown=on_pointerdown
                on:pointermove=on_pointermove
                on:pointerup=end_drag
                on:pointercancel=end_drag
                on:keydown=on_keydown
            >
                <div
                    class="kairos-graph__stage"
                    style:width=format!("{width}px")
                    style:height=format!("{height}px")
                    style:transform=move || {
                        let (x, y) = vs.pan.get();
                        format!("translate({x}px, {y}px) scale({})", vs.scale.get())
                    }
                >
                    <svg
                        class="kairos-graph__underlay"
                        width=width
                        height=height
                        viewBox=view_box.clone()
                        aria-hidden="true"
                    >
                        {rects}
                    </svg>
                    <Dag
                        nodes=nodes
                        edges=edges
                        lanes=lanes
                        styles=edge_styles()
                        options=opts
                        fit=false
                        label="Work graph"
                        // KAIROS-T-0326 (Aurora 0.4.1): a stable id, so the
                        // marker of a style is `kairos-graph-arrow-<style>`;
                        // and a select refocuses the graph, so a click waits
                        // for a possible second click that opens the item.
                        id="kairos-graph"
                        defer_select=true
                        selected=selected
                        on_select=on_select
                        on_open=on_open
                        on_more=on_more
                    />
                    <svg
                        class="kairos-graph__overlay"
                        width=width
                        height=height
                        viewBox=view_box
                        role="group"
                        aria-label="Groups"
                    >
                        {headers}
                    </svg>
                </div>
            </div>
        </div>
    }
}

/// Org-admin link/unlink (ported from the old explorer): the focus item's
/// direct edges with unlink buttons, plus the create-link form. The
/// server enforces the A-0001 rule matrix; typed 422s surface here.
#[component]
fn ManagePanel(
    #[prop(into)] short_code: String,
    /// The result of the last link or unlink. The owner keeps it, so it
    /// stays when the panel is built again (KAIROS-T-0309).
    feedback: RwSignal<Option<Result<String, ApiError>>>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(short_code);
    let reload = RwSignal::new(0u32);

    let rels = LocalResource::new(move || {
        auth.track_session();
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
                Ok(outcome) => {
                    feedback.set(Some(Ok(match outcome {
                        data::LinkOutcome::Proposed(proposed) => proposed.message,
                        data::LinkOutcome::Linked(_) => format!(
                            "Linked {} \u{2014}{}\u{2192} {}.",
                            request.source_short_code,
                            request.relationship,
                            request.target_short_code
                        ),
                    })));
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
                <Group align="end" gap="sm" wrap=true>
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
                        options={RELATIONSHIPS.iter().map(|r| r.to_string()).collect::<Vec<_>>()}
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
    use graph_model::blocks_resolved;

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

    #[test]
    fn clip_keeps_short_text_and_cuts_long_text() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}
