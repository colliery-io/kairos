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
    Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Stack, Text,
};
use aurora_dark::tokens::token;
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

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
                <ItemLoaded item family on_saved/>
            }.into_any(),
        }}
    }
}

/// The loaded page: header + actions, the editor column, and the facts /
/// metadata / relationships column.
#[component]
fn ItemLoaded(item: ItemDetail, family: Family, on_saved: Callback<i32>) -> impl IntoView {
    let create_open = RwSignal::new(false);
    let delete_open = RwSignal::new(false);

    // Pre-compute every view input so the view! closures never capture
    // `item` itself (it is not Copy).
    let facts = item.clone();
    let header_title = item.title.clone();
    let header_sub = format!("{} · {}", family.label(), item.short_code);
    let version_label = format!("v{}", item.version);
    let history_href = format!("/activity/history/{}", item.short_code);
    let code = item.short_code.clone();
    let delete_title = item.title.clone();
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
        <PageHeader title=header_title sub=header_sub/>
        <Group justify="between">
            <Group gap="sm">
                <Pill color=token::ICE>{version_label}</Pill>
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
                <BoardPanel family board_id column_id/>
                <MetadataPanel family code=short_code.clone()/>
                <RelationshipsPanel family code=short_code/>
            </Stack>
        </div>
        <CreateDocumentDialog parent_code=code.clone() open=create_open/>
        <DeleteDialog family code title=delete_title open=delete_open/>
    }
}

/// The type-specific facts as pills (each family's extra columns).
#[component]
fn TypeFacts(item: ItemDetail) -> impl IntoView {
    let mut facts: Vec<(String, &'static str)> = Vec::new();
    if let Some(task_type) = &item.task_type {
        facts.push((task_type.clone(), token::TEAL));
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
/// `GET /api/boards/{id}`, linked to the T-0040 board view.
#[component]
fn BoardPanel(
    family: Family,
    board_id: Option<String>,
    column_id: Option<String>,
) -> impl IntoView {
    let auth = use_auth();
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
                                view! {
                                    <Group justify="between">
                                        <Anchor href=format!("/boards/{}", board.slug)>{board.name.clone()}</Anchor>
                                        <Pill color=token::ICE>{column}</Pill>
                                    </Group>
                                }.into_any()
                            }
                        }}
                    }.into_any()
                }
            }}
        </Panel>
    }
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

/// One direction of one relationship type, neighbors linked.
#[component]
fn RelationshipGroupView(
    group: RelationshipGroup,
    #[prop(into)] direction: String,
) -> impl IntoView {
    let arrow = if direction == "outgoing" {
        "→"
    } else {
        "←"
    };
    view! {
        <div class="kairos-relationships__group">
            <Group gap="sm">
                <Text mono=true size="xs" dimmed=true>{arrow.to_string()}</Text>
                <Text size="xs" dimmed=true>{group.relationship.clone()}</Text>
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
