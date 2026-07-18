//! Boards area (KAIROS-T-0040): the board list (`/boards`) and the board
//! view (`/boards/:board` — slug or id) with live `/ws/events` updates.
//!
//! Shape (per docs/gui-conventions.md and A-0015):
//! - columns come from the board's configuration; items arrive grouped by
//!   column from `GET /api/boards/{id}/items` (all four board entity
//!   types: strategies, initiatives, tasks, ADRs);
//! - **transitions are click-to-move** (a per-card "Move" menu offering
//!   ONLY the targets the board's `transitions` allow from the card's
//!   current column — drag-and-drop is deferred, decision in the task
//!   doc); mutation errors surface in a page-level `Banner`;
//! - live updates: a board-filtered `/ws/events` subscription re-fetches
//!   the grouped items on every event (server state is the source of
//!   truth; `LocalResource` keeps the last value while re-fetching, so
//!   the refresh is silent — no polling, no loading flash);
//! - create-from-column makes the board level's entity type (A-0002);
//!   **documents are off-board** (no `board_id`/`column_id`), so document
//!   creation lives in the board header ("New document": a template picker
//!   and a parent picker over this board's items — POST /api/documents
//!   requires `parent_short_code`), hidden on ADR boards (documents
//!   attach to strategies/initiatives/tasks only).

mod data;
mod live;

use aurora_dark::components::{
    ActionIcon, Alert, Button, Empty, ErrorState, Group, Loading, Menu, MenuItem, Modal,
    PageHeader, Pill, Select, Stack, Text, TextInput, Textarea,
};
use aurora_dark::tokens::{ApiError, token};
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::auth::use_auth;
use data::EntityKind;

/// A short human line for a failed mutation (page-level `Banner`; load
/// failures render `ErrorState` instead, which classifies internally).
fn describe(error: &ApiError) -> String {
    match error {
        ApiError::Http { code, message, .. } => match code {
            Some(code) => format!("{code}: {message}"),
            None => message.clone(),
        },
        ApiError::Network => "Could not reach the server.".to_string(),
        ApiError::Unknown(message) => message.clone(),
    }
}

/// The accent token for a board level / entity kind.
fn level_color(level: &str) -> &'static str {
    match level {
        "strategy" => token::VIOLET,
        "initiative" => token::ICE,
        "delivery" => token::TEAL,
        "adr" => token::GOLD,
        _ => token::SKIP,
    }
}

fn kind_color(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Strategy => token::VIOLET,
        EntityKind::Initiative => token::ICE,
        EntityKind::Task => token::TEAL,
        EntityKind::Adr => token::GOLD,
    }
}

// ---------------------------------------------------------------------------
// Board list
// ---------------------------------------------------------------------------

/// `/boards` — every live board, grouped visually by its level accent.
#[component]
pub fn BoardsPage() -> impl IntoView {
    let auth = use_auth();
    let boards = LocalResource::new(move || {
        let _ = auth.token();
        data::list_boards(auth)
    });
    view! {
        <PageHeader title="Boards" sub="kanban per flight level"/>
        {move || match boards.get() {
            None => view! { <Loading label="Loading boards…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| boards.refetch())/>
            }.into_any(),
            Some(Ok(items)) if items.is_empty() => view! {
                <Empty message="No boards yet — an org admin can create one from Admin."/>
            }.into_any(),
            Some(Ok(items)) => view! {
                <div class="kairos-board-grid">
                    {items.into_iter().map(|board| {
                        let href = format!("/boards/{}", board.slug);
                        view! {
                            <a class="kairos-board-tile" href=href>
                                <Stack gap="xs">
                                    <Group justify="between">
                                        <Text bright=true bold=true>{board.name.clone()}</Text>
                                        <Pill color=level_color(&board.board_level)>
                                            {board.board_level.clone()}
                                        </Pill>
                                    </Group>
                                    <Text mono=true dimmed=true size="xs">{board.slug.clone()}</Text>
                                </Stack>
                            </a>
                        }
                    }).collect_view()}
                </div>
            }.into_any(),
        }}
    }
}

// ---------------------------------------------------------------------------
// Board view
// ---------------------------------------------------------------------------

/// `/boards/:board` — columns from the board config, items grouped, a
/// board-filtered live subscription, click-to-move, create-from-column.
#[component]
pub fn BoardPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();

    // Bumped by mutations and /ws/events messages: re-runs the resource
    // (silently — the previous value stays up while the fetch runs).
    let refresh = RwSignal::new(0u64);
    let board = LocalResource::new(move || {
        let _ = auth.token();
        let _ = refresh.get();
        let param = params.read().get("board").unwrap_or_default();
        async move { data::load_board_view(auth, &param).await }
    });
    let refetch = move || refresh.update(|n| *n += 1);

    // Failed mutations (transition/create) surface here, page-level.
    let action_error = RwSignal::new(None::<ApiError>);
    let on_error = Callback::new(move |error: ApiError| action_error.set(Some(error)));
    let on_changed = Callback::new(move |_: ()| {
        action_error.set(None);
        refetch();
    });

    // The live subscription follows whichever board is loaded; the guard
    // drops (closing the socket) on unmount or board change.
    let live_guard: StoredValue<Option<(String, live::LiveBoardGuard)>, LocalStorage> =
        StoredValue::new_local(None);
    Effect::new(move |_| {
        let Some(Ok(view)) = board.get() else {
            return;
        };
        let board_id = view.items.board.id;
        let current = live_guard.with_value(|g| g.as_ref().map(|(id, _)| id.clone()));
        if current.as_deref() == Some(board_id.as_str()) {
            return;
        }
        let guard = live::subscribe_board_events(auth, board_id.clone(), refetch);
        live_guard.set_value(Some((board_id, guard)));
    });
    on_cleanup(move || live_guard.set_value(None));

    view! {
        {move || match board.get() {
            None => view! { <Loading label="Loading board…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| refetch())/>
            }.into_any(),
            Some(Ok(view)) => {
                view! { <BoardBody view on_changed on_error/> }.into_any()
            }
        }}
        {move || action_error.get().map(|error| view! {
            <div class="kairos-board-notice">
                <Banner color=token::BAD icon="✕">
                    {describe(&error)}
                    <button
                        class="kairos-board-notice__dismiss"
                        on:click=move |_| action_error.set(None)
                    >
                        "Dismiss"
                    </button>
                </Banner>
            </div>
        })}
    }
}

/// The loaded board: header (+ document create) and the column row.
#[component]
fn BoardBody(
    view: data::BoardView,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    let data::BoardView { detail, items } = view;
    let board = items.board.clone();
    let create_kind = EntityKind::for_board_level(&board.board_level);

    // Create-from-column modal state: the target column, set by a
    // column's "+" (one modal instance for the whole page).
    let create_open = RwSignal::new(false);
    let create_column = RwSignal::new(None::<(String, String)>);

    // Document create modal ("New document" in the header) — not offered
    // on ADR boards: documents attach to strategies/initiatives/tasks.
    let doc_open = RwSignal::new(false);
    let doc_parents: Vec<(String, String)> = items
        .columns
        .iter()
        .flat_map(|group| {
            group
                .strategies
                .iter()
                .map(|s| (s.short_code.clone(), s.title.clone()))
                .chain(
                    group
                        .initiatives
                        .iter()
                        .map(|i| (i.short_code.clone(), i.title.clone())),
                )
                .chain(
                    group
                        .tasks
                        .iter()
                        .map(|t| (t.short_code.clone(), t.title.clone())),
                )
        })
        .collect();
    let documents_offered = board.board_level != "adr" && !doc_parents.is_empty();

    let sub = format!("{} board · {}", board.board_level, board.slug);
    let header_right: Children = Box::new(move || {
        if documents_offered {
            view! {
                <Button variant="default" size="xs" on_click=Callback::new(move |_| doc_open.set(true))>
                    "New document"
                </Button>
            }
            .into_any()
        } else {
            ().into_any()
        }
    });

    // Flatten the wire shape into owned view models FIRST: leptos children
    // are `'static` move closures, so views must own their data (no
    // borrowing from the response inside `view!`).
    let column_name_of = |columns: &[data::BoardColumn], id: &str| -> String {
        columns
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };
    struct CardModel {
        kind: EntityKind,
        short_code: String,
        title: String,
        meta: Vec<(String, &'static str)>,
    }
    struct ColumnModel {
        id: String,
        name: String,
        targets: Vec<(String, String)>,
        cards: Vec<CardModel>,
    }
    let columns: Vec<ColumnModel> = items
        .columns
        .iter()
        .map(|group| {
            // ONLY the targets this board's transitions allow from this
            // column — invalid moves are never offered (S-0005).
            let targets: Vec<(String, String)> = detail
                .transitions
                .iter()
                .filter(|t| t.from_column_id == group.column.id)
                .map(|t| {
                    (
                        t.to_column_id.clone(),
                        column_name_of(&detail.columns, &t.to_column_id),
                    )
                })
                .collect();
            let mut cards: Vec<CardModel> = Vec::new();
            cards.extend(group.strategies.iter().map(|item| CardModel {
                kind: EntityKind::Strategy,
                short_code: item.short_code.clone(),
                title: item.title.clone(),
                meta: Vec::new(),
            }));
            cards.extend(group.initiatives.iter().map(|item| {
                let mut meta = Vec::new();
                if let Some(complexity) = &item.complexity {
                    meta.push((format!("complexity {complexity}"), token::ICE));
                }
                if item.is_bucket {
                    let bucket = item.bucket_type.clone().unwrap_or_default();
                    meta.push((format!("bucket · {bucket}"), token::GOLD));
                }
                CardModel {
                    kind: EntityKind::Initiative,
                    short_code: item.short_code.clone(),
                    title: item.title.clone(),
                    meta,
                }
            }));
            cards.extend(group.tasks.iter().map(|item| CardModel {
                kind: EntityKind::Task,
                short_code: item.short_code.clone(),
                title: item.title.clone(),
                meta: match item.task_type.as_str() {
                    "bug" => vec![("bug".to_string(), token::BAD)],
                    "tech_debt" => vec![("tech debt".to_string(), token::GOLD)],
                    _ => Vec::new(),
                },
            }));
            cards.extend(group.adrs.iter().map(|item| {
                CardModel {
                    kind: EntityKind::Adr,
                    short_code: item.short_code.clone(),
                    title: item.title.clone(),
                    meta: item
                        .decision_date
                        .iter()
                        .map(|date| (format!("decided {date}"), token::VIOLET))
                        .collect(),
                }
            }));
            ColumnModel {
                id: group.column.id.clone(),
                name: group.column.name.clone(),
                targets,
                cards,
            }
        })
        .collect();

    view! {
        <PageHeader title=board.name.clone() sub=sub right=header_right/>
        <div class="kairos-board">
            {columns.into_iter().map(|column| {
                let ColumnModel { id, name, targets, cards } = column;
                let count = cards.len();
                let head_label = name.clone();
                let action_title = format!("New item in {name}");
                let column_for_create = (id, name);
                view! {
                    <section class="kairos-board__column">
                        <header class="kairos-board__column-head">
                            <Group justify="between">
                                <Group gap="xs">
                                    <Text bright=true bold=true size="sm">{head_label}</Text>
                                    <Text dimmed=true size="xs">{count.to_string()}</Text>
                                </Group>
                                {create_kind.map(|_| view! {
                                    <ActionIcon
                                        title=action_title
                                        on_click=Callback::new(move |_| {
                                            create_column.set(Some(column_for_create.clone()));
                                            create_open.set(true);
                                        })
                                    >
                                        "+"
                                    </ActionIcon>
                                })}
                            </Group>
                        </header>
                        <Stack gap="xs">
                            {(count == 0).then(|| view! {
                                <Text dimmed=true size="xs">"No items in this column."</Text>
                            })}
                            {cards.into_iter().map(|card| {
                                let CardModel { kind, short_code, title, meta } = card;
                                view! {
                                    <ItemCard
                                        kind short_code title meta
                                        targets=targets.clone()
                                        on_changed on_error
                                    />
                                }
                            }).collect_view()}
                        </Stack>
                    </section>
                }
            }).collect_view()}
        </div>
        {create_kind.map(|kind| view! {
            <CreateItemModal
                open=create_open
                kind
                board_id=board.id.clone()
                team_id=board.team_id.clone()
                column=create_column
                on_changed
            />
        })}
        {documents_offered.then(|| view! {
            <CreateDocumentModal open=doc_open parents=doc_parents.clone() on_changed/>
        })}
    }
}

// ---------------------------------------------------------------------------
// Cards + click-to-move
// ---------------------------------------------------------------------------

/// One board card: short code, title, type, key metadata, open link, and
/// the click-to-move menu (only when the board allows moves from here).
#[component]
fn ItemCard(
    kind: EntityKind,
    short_code: String,
    title: String,
    /// `(label, color-token)` pills — per-type key metadata.
    meta: Vec<(String, &'static str)>,
    /// `(column_id, column_name)` — the valid targets from this column.
    targets: Vec<(String, String)>,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let href = format!("/items/{short_code}");
    let code_text = short_code.clone();
    let code_for_move = short_code;
    view! {
        <article class="kairos-card">
            <Group justify="between">
                <Text mono=true dimmed=true size="xs">{code_text}</Text>
                <Pill color=kind_color(kind)>{kind.label()}</Pill>
            </Group>
            <a class="kairos-card__title" href=href>
                {title}
            </a>
            {(!meta.is_empty()).then(|| view! {
                <Group gap="xs" wrap=true>
                    {meta.into_iter().map(|(label, color)| view! {
                        <Pill color=color>{label}</Pill>
                    }).collect_view()}
                </Group>
            })}
            {(!targets.is_empty()).then(move || {
                // Each menu entry owns its short code + target column.
                let entries: Vec<_> = targets
                    .into_iter()
                    .map(|(column_id, column_name)| {
                        (code_for_move.clone(), column_id, column_name)
                    })
                    .collect();
                view! {
                    <div class="kairos-card__actions">
                        {move || busy.get().then(|| view! {
                            <Text dimmed=true size="xs">"Moving…"</Text>
                        })}
                        <Menu label="Move">
                            {entries.iter().map(|(code, column_id, column_name)| {
                                let code = code.clone();
                                let column_id = column_id.clone();
                                let label = column_name.clone();
                                view! {
                                    <MenuItem on_click=Callback::new(move |_| {
                                        let code = code.clone();
                                        let column_id = column_id.clone();
                                        busy.set(true);
                                        leptos::task::spawn_local(async move {
                                            match data::transition(auth, kind, &code, &column_id).await {
                                                Ok(()) => on_changed.run(()),
                                                Err(error) => on_error.run(error),
                                            }
                                            busy.set(false);
                                        });
                                    })>
                                        {label}
                                    </MenuItem>
                                }
                            }).collect_view()}
                        </Menu>
                    </div>
                }
            })}
        </article>
    }
}

// ---------------------------------------------------------------------------
// Create flows
// ---------------------------------------------------------------------------

/// Create-from-column: the board level's entity type with its
/// type-appropriate fields (A-0002 one item family per board level).
#[component]
fn CreateItemModal(
    open: RwSignal<bool>,
    kind: EntityKind,
    board_id: String,
    /// Delivery boards carry their team; new tasks inherit it.
    team_id: Option<String>,
    /// `(column_id, column_name)` chosen by the column's "+".
    column: RwSignal<Option<(String, String)>>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let title = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let hypothesis = RwSignal::new(String::new());
    let complexity = RwSignal::new("none".to_string());
    let task_type = RwSignal::new("task".to_string());
    let decision_maker = RwSignal::new(String::new());
    let decision_date = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    // A fresh form every time the modal opens.
    Effect::new(move |_| {
        if open.get() {
            title.set(String::new());
            content.set(String::new());
            hypothesis.set(String::new());
            complexity.set("none".to_string());
            task_type.set("task".to_string());
            decision_maker.set(String::new());
            decision_date.set(String::new());
            busy.set(false);
            error.set(None);
        }
    });

    // `Callback` is `Copy`: the modal's children (a `Fn` closure) can use
    // it without moving anything out of their environment.
    let submit: Callback<()> = Callback::new({
        let opt = |s: String| (!s.trim().is_empty()).then_some(s);
        move |()| {
            let Some((column_id, _)) = column.get_untracked() else {
                return;
            };
            let item = data::NewItem {
                title: title.get_untracked(),
                content: content.get_untracked(),
                hypothesis: opt(hypothesis.get_untracked()),
                complexity: Some(complexity.get_untracked()).filter(|c| c != "none"),
                task_type: Some(task_type.get_untracked()),
                team_id: team_id.clone(),
                decision_maker: opt(decision_maker.get_untracked()),
                decision_date: opt(decision_date.get_untracked()),
            };
            let board_id = board_id.clone();
            busy.set(true);
            error.set(None);
            leptos::task::spawn_local(async move {
                match data::create_item(auth, kind, &board_id, &column_id, &item).await {
                    Ok(()) => {
                        open.set(false);
                        on_changed.run(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    view! {
        <Modal open=open title=format!("New {}", kind.label())>
            <Stack gap="sm">
                <Text dimmed=true size="xs">
                    {move || column.get()
                        .map(|(_, name)| format!("In column: {name}"))
                        .unwrap_or_default()}
                </Text>
                <TextInput label="Title" value=title placeholder="What is it?"/>
                <Textarea label="Content (markdown)" value=content rows=5
                    placeholder="Why does it exist? What does done look like?"/>
                {matches!(kind, EntityKind::Strategy).then(|| view! {
                    <TextInput label="Hypothesis (optional)" value=hypothesis
                        placeholder="If we…, then…"/>
                })}
                {matches!(kind, EntityKind::Initiative).then(|| view! {
                    <Select label="Complexity" value=complexity
                        options=vec!["none".into(), "xs".into(), "s".into(),
                                     "m".into(), "l".into(), "xl".into()]/>
                })}
                {matches!(kind, EntityKind::Task).then(|| view! {
                    <Select label="Task type" value=task_type
                        options=vec!["task".into(), "bug".into(), "tech_debt".into()]/>
                })}
                {matches!(kind, EntityKind::Adr).then(|| view! {
                    <Stack gap="sm">
                        <TextInput label="Decision maker (optional)" value=decision_maker/>
                        <TextInput label="Decision date (optional)" value=decision_date
                            placeholder="YYYY-MM-DD"/>
                    </Stack>
                })}
                {move || error.get().map(|e| view! {
                    <Alert title="Could not create" color=token::BAD>
                        <Text size="sm">{describe(&e)}</Text>
                    </Alert>
                })}
                <Group justify="between">
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| open.set(false))>
                        "Cancel"
                    </Button>
                    {move || {
                        let disabled = busy.get() || title.get().trim().is_empty();
                        view! {
                            <Button size="xs" disabled=disabled on_click=submit>
                                {if busy.get_untracked() { "Creating…" } else { "Create" }}
                            </Button>
                        }
                    }}
                </Group>
            </Stack>
        </Modal>
    }
}

/// "New document" (board header): template picker + parent picker.
/// Documents are off-board, so this is the one create flow that is not
/// column-anchored; the server writes the `supports` edge to the parent.
#[component]
fn CreateDocumentModal(
    open: RwSignal<bool>,
    /// `(short_code, title)` of this board's eligible parents.
    parents: Vec<(String, String)>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let title = RwSignal::new(String::new());
    let template = RwSignal::new("(blank)".to_string());
    let parent = RwSignal::new(
        parents
            .first()
            .map(|(code, _)| code.clone())
            .unwrap_or_default(),
    );
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    // Templates load lazily, first time the modal opens.
    let templates = LocalResource::new(move || {
        let _ = auth.token();
        let wanted = open.get();
        async move {
            if wanted {
                data::list_templates(auth).await
            } else {
                Ok(Vec::new())
            }
        }
    });

    let first_parent = parents
        .first()
        .map(|(code, _)| code.clone())
        .unwrap_or_default();
    Effect::new(move |_| {
        if open.get() {
            title.set(String::new());
            template.set("(blank)".to_string());
            parent.set(first_parent.clone());
            busy.set(false);
            error.set(None);
        }
    });

    // `StoredValue` (Copy) so the modal's `Fn` children never move the
    // options vec out of their environment.
    let parent_options: StoredValue<Vec<String>> = StoredValue::new(
        parents
            .iter()
            .map(|(code, item_title)| format!("{code} · {item_title}"))
            .collect(),
    );

    let submit: Callback<()> = Callback::new(move |()| {
        let template_name = template.get_untracked();
        let template_id = templates.get_untracked().and_then(|result| {
            result.ok().and_then(|templates| {
                templates
                    .into_iter()
                    .find(|t| t.name == template_name)
                    .map(|t| t.id)
            })
        });
        // Options render as "CODE · title"; the code is the first token.
        let parent_code = parent
            .get_untracked()
            .split(" · ")
            .next()
            .unwrap_or_default()
            .to_string();
        let doc_title = title.get_untracked();
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match data::create_document(auth, &doc_title, template_id.as_deref(), &parent_code)
                .await
            {
                Ok(()) => {
                    open.set(false);
                    on_changed.run(());
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    });

    view! {
        <Modal open=open title="New document">
            <Stack gap="sm">
                <TextInput label="Title" value=title placeholder="e.g. PRD: …"/>
                {move || match templates.get() {
                    None => view! { <Text dimmed=true size="xs">"Loading templates…"</Text> }.into_any(),
                    Some(Err(e)) => view! {
                        <Alert title="Templates unavailable" color=token::GOLD>
                            <Text size="sm">{describe(&e)}</Text>
                        </Alert>
                    }.into_any(),
                    Some(Ok(list)) => {
                        let mut options = vec!["(blank)".to_string()];
                        options.extend(list.into_iter().map(|t| t.name));
                        view! { <Select label="Template" value=template options/> }.into_any()
                    }
                }}
                <Select label="Attach to (supports)" value=parent
                    options=parent_options.get_value()/>
                {move || error.get().map(|e| view! {
                    <Alert title="Could not create" color=token::BAD>
                        <Text size="sm">{describe(&e)}</Text>
                    </Alert>
                })}
                <Group justify="between">
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| open.set(false))>
                        "Cancel"
                    </Button>
                    {move || {
                        let disabled = busy.get() || title.get().trim().is_empty();
                        view! {
                            <Button size="xs" disabled=disabled on_click=submit>
                                {if busy.get_untracked() { "Creating…" } else { "Create" }}
                            </Button>
                        }
                    }}
                </Group>
            </Stack>
        </Modal>
    }
}
