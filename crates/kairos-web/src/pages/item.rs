//! Item detail (KAIROS-T-0041): one `/items/:code` route serving all five
//! entity families (the short code's type letter picks the family — see
//! [`api::Family`]), per KAIROS-A-0015's v1 scope:
//!
//! - title + markdown content editing with preview ([`editor`], renderer
//!   decision in [`markdown`]) under A-0004 optimistic concurrency — 409
//!   opens the merge UI (server-current vs. yours, keep-mine /
//!   take-theirs / manual-merge, retry carries the new version);
//! - typed metadata panel ([`metadata`], A-0003: enum dropdowns from the
//!   definitions, date inputs, strings; null clears);
//! - board/column display (name via `GET /api/boards/{id}`, linked to the
//!   T-0040 board view) and type-specific facts;
//! - relationships summary (linked; the full explorer is T-0042's
//!   `/search/relationships/:code`) and the history link (T-0044's
//!   `/activity/history/:code`);
//! - create-from-template ([`create_doc`]) and soft delete with cascade
//!   warning ([`delete`], A-0001).

mod api;
mod create_doc;
mod delete;
mod editor;
mod markdown;
mod metadata;

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select,
    Stack, Text,
};
use aurora_dark::tokens::{ApiError, token};
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use super::boards;
use super::copy_link;
use crate::auth::use_auth;
use api::{Family, ItemDetail, RelationshipGroup};
use create_doc::CreateDocumentDialog;
use delete::DeleteDialog;
use editor::ContentEditor;
use metadata::MetadataPanel;

/// `/items/:code` — parse the family from the short code and hand off.
#[component]
pub fn ItemPage() -> impl IntoView {
    let params = use_params_map();
    view! {
        {move || {
            let code = params.read().get("code").unwrap_or_default();
            match Family::of_short_code(&code) {
                Some(family) => view! { <ItemDetailView family code/> }.into_any(),
                None => view! {
                    <PageHeader title=code.clone() sub="work item"/>
                    <Panel title="Not a short code" caption="items/:code">
                        <Empty message=format!(
                            "\"{code}\" is not an item short code (expected PREFIX-S|I|T|D|A-NNNN)."
                        )/>
                    </Panel>
                }.into_any(),
            }
        }}
    }
}

/// The detail resource + the four async view states. `reload` is the
/// refetch handle mutations bump (server state is the source of truth).
#[component]
fn ItemDetailView(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    // Page-level transient notice (survives the refetch that recreates
    // the loaded subtree).
    let notice = RwSignal::new(None::<String>);

    let detail = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        api::fetch_item(auth, family, code.get_value())
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));
    let on_saved = Callback::new(move |version: i32| {
        notice.set(Some(format!("Saved — the item is now at v{version}.")));
        reload.update(|n| *n += 1);
    });
    // The move/lane controls' success path (KAIROS-T-0075/T-0077): the
    // item changed server-side; show the control's message and refetch.
    let on_moved = Callback::new(move |message: String| {
        notice.set(Some(message));
        reload.update(|n| *n += 1);
    });

    view! {
        {move || notice.get().map(|message| view! {
            <div class="kairos-item__notice">
                <Banner color=token::OK icon="✓">
                    <Group justify="between">
                        <Text size="sm">{message}</Text>
                        <Button variant="default" size="xs" on_click=Callback::new(move |_| notice.set(None))>
                            "Dismiss"
                        </Button>
                    </Group>
                </Banner>
            </div>
        })}
        {move || match detail.get() {
            None => view! { <Loading label="Loading item…"/> }.into_any(),
            Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
            Some(Ok(item)) => view! {
                <ItemLoaded item family on_saved on_moved/>
            }.into_any(),
        }}
    }
}

/// The loaded page: header + actions, the editor column, and the facts /
/// metadata / relationships column.
#[component]
fn ItemLoaded(
    item: ItemDetail,
    family: Family,
    on_saved: Callback<i32>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let create_open = RwSignal::new(false);
    let delete_open = RwSignal::new(false);

    // Pre-compute every view input so the view! closures never capture
    // `item` itself (it is not Copy).
    let facts = item.clone();
    let header_title = item.title.clone();
    // The short code renders as its own element with a copy-link button
    // (KAIROS-T-0076), not folded into the subtitle string.
    let header_code = item.short_code.clone();
    let copy_code = item.short_code.clone();
    let version_label = format!("v{}", item.version);
    let history_href = format!("/activity/history/{}", item.short_code);
    let code = item.short_code.clone();
    let delete_title = item.title.clone();
    let lane = item.work_class.clone();
    let lifecycle = item.lifecycle.clone();
    let lifecycle_badge = item.lifecycle.clone();
    let ItemDetail {
        short_code,
        title,
        content,
        version,
        board_id,
        column_id,
        ..
    } = item;

    view! {
        <PageHeader title=header_title sub=family.label()/>
        <Group justify="between">
            <Group gap="sm">
                <Text mono=true dimmed=true size="sm">{header_code}</Text>
                <copy_link::CopyLinkButton code=copy_code/>
                <Pill color=token::ICE>{version_label}</Pill>
                // The document lifecycle badge (KAIROS-T-0078): an
                // editorial label, deliberately distinct from anything
                // board-status-shaped — its own class + state color.
                {lifecycle_badge.map(|state| view! {
                    <span class="kairos-lifecycle-badge">
                        <Pill color=lifecycle_color(&state)>{format!("lifecycle: {state}")}</Pill>
                    </span>
                })}
                <TypeFacts item=facts/>
            </Group>
            <Group gap="sm">
                <Anchor href=history_href>"History"</Anchor>
                {family.is_workflow().then(|| view! {
                    <Button variant="default" size="xs" on_click=Callback::new(move |_| create_open.set(true))>
                        "New document"
                    </Button>
                })}
                <Button variant="default" size="xs" bad=true on_click=Callback::new(move |_| delete_open.set(true))>
                    "Delete"
                </Button>
            </Group>
        </Group>
        <ChildrenProgressBar family code=short_code.clone()/>
        <div class="kairos-item__grid">
            <ContentEditor
                family
                code=short_code.clone()
                initial_title=title
                initial_content=content
                initial_version=version
                on_saved
            />
            <Stack gap="sm">
                <BoardPanel family code=short_code.clone() board_id column_id
                    work_class=lane on_moved/>
                {lifecycle.map(|current| view! {
                    <LifecyclePanel code=short_code.clone() current on_moved/>
                })}
                <MetadataPanel family code=short_code.clone()/>
                <RelationshipsPanel family code=short_code/>
            </Stack>
        </div>
        <CreateDocumentDialog parent_code=code.clone() open=create_open/>
        <DeleteDialog family code title=delete_title open=delete_open/>
    }
}

/// The badge color of a document lifecycle state (KAIROS-T-0078):
/// published green, review gold, draft/archived muted — an editorial
/// palette, deliberately unlike the board-column pills.
fn lifecycle_color(state: &str) -> &'static str {
    match state {
        "published" => token::OK,
        "review" => token::GOLD,
        _ => token::MUTED,
    }
}

/// The lifecycle control (KAIROS-T-0078, documents only): a
/// free-transition select + Set button — an editorial label, never a
/// transition engine. Gating is server-side (`manage_documents` on the
/// authorization board); a 403 surfaces in the error slot.
#[component]
fn LifecyclePanel(
    #[prop(into)] code: String,
    current: String,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let current = StoredValue::new(current);
    let value = RwSignal::new(current.get_value());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);
    let submit: Callback<()> = Callback::new(move |()| {
        let lifecycle = value.get_untracked();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::set_lifecycle(auth, &code.get_value(), &lifecycle).await {
                Ok(()) => on_moved.run(format!("Lifecycle set to {lifecycle}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    view! {
        <Panel title="Lifecycle" caption="editorial state — not board status">
            <Stack gap="sm">
                <Group gap="sm">
                    <Select label="State" value=value
                        options=vec!["draft".to_string(), "review".to_string(),
                                     "published".to_string(), "archived".to_string()]/>
                    {move || {
                        let unchanged = value.get() == current.get_value();
                        let disabled = busy.get() || unchanged;
                        view! {
                            <Button size="xs" disabled=disabled on_click=submit>
                                {if busy.get_untracked() { "Setting…" } else { "Set" }}
                            </Button>
                        }
                    }}
                </Group>
                {move || error.get().map(|e| view! {
                    <Alert title="Could not set lifecycle" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </Stack>
        </Panel>
    }
}

/// The children rollup under the header (KAIROS-T-0080): a segmented bar
/// by column plus "N of M done" — or "N children" composition-only when
/// no involved board has done columns (never a misleading fraction).
/// Renders nothing for items without children; a failed read renders
/// nothing too (progress is enhancement data, not the page).
#[component]
fn ChildrenProgressBar(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let progress = LocalResource::new(move || {
        let _ = auth.token();
        api::fetch_children_progress(auth, family, code.get_value())
    });
    view! {
        {move || match progress.get() {
            Some(Ok(p)) if p.total > 0 => {
                let total = p.total;
                let label = if p.has_done_columns {
                    format!("{} of {} done", p.done, p.total)
                } else {
                    format!("{} children", p.total)
                };
                view! {
                    <div class="kairos-progress">
                        <div class="kairos-progress__track">
                            {p.by_column.iter().map(|column| {
                                let width =
                                    (column.count as f64 / total as f64 * 100.0).max(3.0);
                                view! {
                                    <span
                                        class="kairos-progress__segment"
                                        class:kairos-progress__segment--done=column.is_done
                                        style=format!("width: {width:.1}%")
                                        title=format!("{}: {}", column.column_name, column.count)
                                    ></span>
                                }
                            }).collect_view()}
                        </div>
                        <Text dimmed=true size="xs">{label}</Text>
                    </div>
                }.into_any()
            }
            _ => ().into_any(),
        }}
    }
}

/// The type-specific facts as pills (each family's extra columns).
#[component]
fn TypeFacts(item: ItemDetail) -> impl IntoView {
    let mut facts: Vec<(String, &'static str)> = Vec::new();
    if let Some(task_type) = &item.task_type {
        facts.push((task_type.clone(), token::TEAL));
    }
    // KAIROS-T-0077: the lane rides as a fact; Planned stays unmarked as
    // the default lane.
    if item.work_class.as_deref() == Some("support") {
        facts.push(("support lane".to_string(), token::GOLD));
    }
    if let Some(complexity) = &item.complexity {
        facts.push((format!("complexity: {complexity}"), token::TEAL));
    }
    if item.is_bucket == Some(true) {
        let kind = item.bucket_type.clone().unwrap_or_else(|| "bucket".into());
        facts.push((format!("bucket: {kind}"), token::GOLD));
    }
    if let Some(hypothesis) = &item.hypothesis
        && !hypothesis.is_empty()
    {
        facts.push(("has hypothesis".to_string(), token::VIOLET));
    }
    if let Some(decision_maker) = &item.decision_maker {
        facts.push((format!("decided by {decision_maker}"), token::VIOLET));
    }
    if let Some(decision_date) = &item.decision_date {
        facts.push((decision_date.clone(), token::VIOLET));
    }
    view! {
        {facts
            .into_iter()
            .map(|(label, color)| view! { <Pill color=color>{label}</Pill> })
            .collect_view()}
        <Text size="xs" dimmed=true>{format!("updated {}", item.updated_at)}</Text>
    }
}

/// Board/column display: documents never sit on boards; ADRs may not; the
/// rest always do. Board and column ids resolve to names via
/// `GET /api/boards/{id}`, linked to the T-0040 board view. On-board
/// items also get the move control here (KAIROS-T-0075) — the
/// keyboard-accessible transition path since cards are drag-only.
#[component]
fn BoardPanel(
    family: Family,
    #[prop(into)] code: String,
    board_id: Option<String>,
    column_id: Option<String>,
    /// The task's Planned/Support lane (KAIROS-T-0077) — `Some` enables
    /// the lane control.
    work_class: Option<String>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let work_class = StoredValue::new(work_class);
    view! {
        <Panel title="Board" caption="placement">
            {match board_id {
                None => {
                    let message = match family {
                        Family::Document => "Documents attach to a workflow item (supports edge), not a board.",
                        _ => "Off-board — no column, no transitions (org-admin writes only).",
                    };
                    view! { <Text size="sm" dimmed=true>{message}</Text> }.into_any()
                }
                Some(board_id) => {
                    let board = LocalResource::new(move || {
                        let _ = auth.token();
                        api::fetch_board(auth, board_id.clone())
                    });
                    let column_id = StoredValue::new(column_id);
                    view! {
                        {move || match board.get() {
                            None => view! { <Loading label="Loading board…"/> }.into_any(),
                            Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                            Some(Ok(board)) => {
                                let column = column_id.with_value(|id| {
                                    id.as_ref().and_then(|id| {
                                        board.columns.iter().find(|c| &c.id == id)
                                    })
                                    .map(|c| c.name.clone())
                                    .unwrap_or_else(|| "unknown column".to_string())
                                });
                                let slug = board.slug.clone();
                                let name = board.name.clone();
                                view! {
                                    <Stack gap="sm">
                                        <Group justify="between">
                                            <Anchor href=format!("/boards/{slug}")>{name}</Anchor>
                                            <Pill color=token::ICE>{column}</Pill>
                                        </Group>
                                        <MoveControl
                                            family
                                            code=code.get_value()
                                            board
                                            column_id=column_id.with_value(Clone::clone)
                                            work_class=work_class.get_value()
                                            on_moved
                                        />
                                    </Stack>
                                }.into_any()
                            }
                        }}
                    }.into_any()
                }
            }}
        </Panel>
    }
}

/// The keyboard-accessible transition path (KAIROS-T-0075) plus the lane
/// control (KAIROS-T-0077): cards are drag-only, so moving an item — or
/// switching a task's Planned/Support lane — without a pointer happens
/// here. Renders only when there is a legal move to offer AND the user
/// holds `transition_items` on this board — the same gate as the drag
/// affordance, via the shared powers mirror.
#[component]
fn MoveControl(
    family: Family,
    code: String,
    board: api::BoardInfo,
    column_id: Option<String>,
    work_class: Option<String>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    // Documents never transition; the other four families map onto the
    // board-item kinds the transition endpoint serves.
    let kind = match family {
        Family::Strategy => Some(boards::data::EntityKind::Strategy),
        Family::Initiative => Some(boards::data::EntityKind::Initiative),
        Family::Task => Some(boards::data::EntityKind::Task),
        Family::Adr => Some(boards::data::EntityKind::Adr),
        Family::Document => None,
    };
    // ONLY the targets this board's transitions allow from the current
    // column — invalid moves are never offered (A-0002).
    let targets: Vec<(String, String)> = column_id
        .as_ref()
        .map(|from| {
            board
                .transitions
                .iter()
                .filter(|t| &t.from_column_id == from)
                .filter_map(|t| {
                    board
                        .columns
                        .iter()
                        .find(|c| c.id == t.to_column_id)
                        .map(|c| (c.id.clone(), c.name.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let has_targets = !targets.is_empty();
    // The lane row exists for tasks only (they carry work_class) — and
    // keeps a task in a dead-end column movable across lanes.
    let lane_row = matches!(family, Family::Task).then_some(()).and(work_class);
    let (Some(kind), false) = (kind, !has_targets && lane_row.is_none()) else {
        return ().into_any();
    };

    // Same client-side capability mirror as the board (KAIROS-T-0072);
    // the server remains the authority.
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let slug = StoredValue::new(board.slug.clone());
    let team_id = StoredValue::new(board.team_id.clone());
    let can_move = Memo::new(move |_| {
        whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
            .is_some_and(|me| {
                slug.with_value(|slug| {
                    team_id.with_value(|team| {
                        boards::board_powers(&me, slug, team.as_deref(), None).transition
                    })
                })
            })
    });

    let option_names = StoredValue::new(
        targets
            .iter()
            .map(|(_, name)| name.clone())
            .collect::<Vec<String>>(),
    );
    let targets = StoredValue::new(targets);
    let target = RwSignal::new(
        option_names.with_value(|names| names.first().cloned().unwrap_or_default()),
    );
    let code = StoredValue::new(code);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    let submit: Callback<()> = Callback::new(move |()| {
        let name = target.get_untracked();
        let Some((to_column_id, column_name)) =
            targets.with_value(|t| t.iter().find(|(_, n)| *n == name).cloned())
        else {
            return;
        };
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match boards::data::transition(auth, kind, &code.get_value(), &to_column_id).await {
                // Success refetches the whole detail (on_moved bumps the
                // reload), which drops this instance — no busy reset.
                Ok(()) => on_moved.run(format!("Moved to {column_name}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });

    // The lane row (KAIROS-T-0077): tasks switch Planned/Support here —
    // the keyboard counterpart of a cross-lane drag.
    let has_lane = lane_row.is_some();
    let current_lane = StoredValue::new(lane_row.unwrap_or_default());
    let lane_value = RwSignal::new(current_lane.get_value());
    let submit_lane: Callback<()> = Callback::new(move |()| {
        let value = lane_value.get_untracked();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match boards::data::set_work_class(auth, &code.get_value(), &value).await {
                Ok(()) => on_moved.run(format!("Lane set to {value}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });

    view! {
        {move || can_move.get().then(|| view! {
            <div class="kairos-item__move">
                {has_targets.then(|| view! {
                    <Group gap="sm">
                        <Select label="Move to" value=target
                            options=option_names.get_value()/>
                        {move || {
                            let disabled = busy.get();
                            view! {
                                <Button size="xs" disabled=disabled on_click=submit>
                                    {if busy.get_untracked() { "Moving…" } else { "Move" }}
                                </Button>
                            }
                        }}
                    </Group>
                })}
                {has_lane.then(|| view! {
                    <Group gap="sm">
                        <Select label="Lane" value=lane_value
                            options=vec!["planned".to_string(), "support".to_string()]/>
                        {move || {
                            let unchanged =
                                lane_value.get() == current_lane.get_value();
                            let disabled = busy.get() || unchanged;
                            view! {
                                <Button size="xs" disabled=disabled on_click=submit_lane>
                                    {if busy.get_untracked() { "Setting…" } else { "Set lane" }}
                                </Button>
                            }
                        }}
                    </Group>
                })}
                {move || error.get().map(|e| view! {
                    <Alert title="Could not move" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </div>
        })}
    }
    .into_any()
}

/// Relationships summary: both directions, grouped, every neighbor linked
/// to its own detail route. The full graph explorer is T-0042's.
#[component]
fn RelationshipsPanel(family: Family, #[prop(into)] code: String) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    let relationships = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        api::fetch_relationships(auth, family, code.get_value())
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));

    view! {
        <Panel title="Relationships" caption="summary">
            {move || match relationships.get() {
                None => view! { <Loading label="Loading relationships…"/> }.into_any(),
                Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                Some(Ok(relationships))
                    if relationships.outgoing.is_empty() && relationships.incoming.is_empty() =>
                {
                    view! { <Empty message="No relationships yet — link items from the graph explorer (KAIROS-T-0042)."/> }
                        .into_any()
                }
                Some(Ok(relationships)) => view! {
                    <Stack gap="sm">
                        {relationships.outgoing.into_iter().map(|group| view! {
                            <RelationshipGroupView group direction="outgoing"/>
                        }).collect_view()}
                        {relationships.incoming.into_iter().map(|group| view! {
                            <RelationshipGroupView group direction="incoming"/>
                        }).collect_view()}
                        <Anchor href=format!("/search/relationships/{}", code.get_value())>
                            "Open the graph explorer"
                        </Anchor>
                    </Stack>
                }.into_any(),
            }}
        </Panel>
    }
}

/// The human name of one relationship group, read from THIS item's side —
/// an outgoing `parent` edge points at children, so labeling the group
/// "parent" read exactly backwards (UAT review nit on T-0075/T-0080
/// wave). Vocabulary matches the graph explorer's panels; direction is in
/// the words, not an arrow glyph. Pure, host-tested.
fn relationship_label(relationship: &str, outgoing: bool) -> String {
    match (relationship, outgoing) {
        // source=parent, target=child (A-0001 matrix).
        ("parent", true) => "children".to_string(),
        ("parent", false) => "parent".to_string(),
        ("blocks", true) => "blocks".to_string(),
        ("blocks", false) => "blocked by".to_string(),
        // source=workflow item, target=document/ADR (T-0018 contract).
        ("supports", true) => "supporting material".to_string(),
        ("supports", false) => "supports".to_string(),
        // source=document/ADR, target=workflow item.
        ("informs", true) => "informs".to_string(),
        ("informs", false) => "informed by".to_string(),
        ("supersedes", true) => "supersedes".to_string(),
        ("supersedes", false) => "superseded by".to_string(),
        // Unknown types stay honest about their direction.
        (other, true) => format!("{other} (outgoing)"),
        (other, false) => format!("{other} (incoming)"),
    }
}

/// One direction of one relationship type, neighbors linked.
#[component]
fn RelationshipGroupView(
    group: RelationshipGroup,
    #[prop(into)] direction: String,
) -> impl IntoView {
    let label = relationship_label(&group.relationship, direction == "outgoing");
    view! {
        <div class="kairos-relationships__group">
            <Group gap="sm">
                <Text size="xs" dimmed=true>{label}</Text>
            </Group>
            {group.items.into_iter().map(|item| view! {
                <Group gap="sm" justify="between">
                    <Anchor href=format!("/items/{}", item.short_code)>
                        {format!("{} — {}", item.short_code, item.title)}
                    </Anchor>
                    <Pill color=token::VIOLET>{item.entity_type.clone()}</Pill>
                </Group>
            }).collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The summary reads from THIS item's side: an outgoing parent edge
    /// lists children; passive voice marks the incoming direction.
    #[test]
    fn relationship_labels_read_from_the_items_side() {
        assert_eq!(relationship_label("parent", true), "children");
        assert_eq!(relationship_label("parent", false), "parent");
        assert_eq!(relationship_label("blocks", false), "blocked by");
        assert_eq!(relationship_label("supports", true), "supporting material");
        assert_eq!(relationship_label("informs", false), "informed by");
        assert_eq!(relationship_label("supersedes", false), "superseded by");
        assert_eq!(relationship_label("mystery", true), "mystery (outgoing)");
    }
}
