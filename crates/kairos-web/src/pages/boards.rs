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
    ActionIcon, Alert, Anchor, Button, Empty, ErrorState, Group, Loading, Menu, MenuItem, Modal,
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

/// The in-flight card drag (KAIROS-T-0064): which card, and the column ids
/// its CURRENT column's transitions allow as drop targets — so only legal
/// columns light up and accept the drop (A-0002: invalid moves are never
/// offered).
#[derive(Clone, Debug, PartialEq)]
struct DragData {
    kind: EntityKind,
    short_code: String,
    targets: Vec<String>,
}

// ---------------------------------------------------------------------------
// Client-side capability mirror (KAIROS-T-0072)
// ---------------------------------------------------------------------------

/// What the signed-in user may do on THIS board — mirrors the A-0006
/// decision (org-admin bypass, explicit grants incl. globs, and the
/// KAIROS-T-0072 team implication) so affordances the server would 403
/// never render. The server remains the authority; this is UX.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BoardPowers {
    /// May move cards (`transition_items`).
    transition: bool,
    /// May create this board level's entity (`manage_<family>`).
    create: bool,
    /// May create documents (`manage_documents`).
    documents: bool,
}

/// The `manage_*` capability that creating this kind requires.
fn create_capability(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Strategy => "manage_strategies",
        EntityKind::Initiative => "manage_initiatives",
        EntityKind::Task => "manage_tasks",
        EntityKind::Adr => "manage_adrs",
    }
}

/// Does a stored grant cover `required`? Client mirror of the A-0006
/// matching semantics for the sanctioned grant forms (exact, `*`, and
/// trailing-`*` globs).
fn grant_covers(grant: &str, required: &str) -> bool {
    grant == "*"
        || grant == required
        || grant
            .strip_suffix('*')
            .is_some_and(|prefix| required.starts_with(prefix))
}

/// The KAIROS-T-0072 implied set (mirror of
/// `kairos_core::abac::TEAM_IMPLIED_CAPABILITIES`).
fn team_implies(required: &str) -> bool {
    matches!(
        required,
        "manage_tasks" | "manage_documents" | "transition_items"
    )
}

/// Compute [`BoardPowers`] from the whoami identity. Pure, host-tested.
fn board_powers(
    me: &crate::api::Whoami,
    board_slug: &str,
    board_team_id: Option<&str>,
    create_kind: Option<EntityKind>,
) -> BoardPowers {
    if me.organization.role == "admin" {
        return BoardPowers {
            transition: true,
            create: create_kind.is_some(),
            documents: true,
        };
    }
    let team_member =
        board_team_id.is_some_and(|team| me.teams.iter().any(|mine| mine.id == team));
    let has = |required: &str| {
        (team_member && team_implies(required))
            || me
                .capabilities
                .iter()
                .filter(|board| board.board_slug == board_slug)
                .flat_map(|board| board.grants.iter())
                .any(|grant| grant_covers(grant, required))
    };
    BoardPowers {
        transition: has("transition_items"),
        create: create_kind.is_some_and(|kind| has(create_capability(kind))),
        documents: has("manage_documents"),
    }
}

/// Run one transition and report through the standard board callbacks —
/// shared by the move menu (accessibility fallback) and the drop handler.
fn run_transition(
    auth: crate::auth::Auth,
    kind: EntityKind,
    code: String,
    column_id: String,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) {
    leptos::task::spawn_local(async move {
        match data::transition(auth, kind, &code, &column_id).await {
            Ok(()) => on_changed.run(()),
            Err(error) => on_error.run(error),
        }
    });
}

// ---------------------------------------------------------------------------
// Board list
// ---------------------------------------------------------------------------

/// The flight-level band order for `/boards` (KAIROS-T-0069/T-0063:
/// strategy feeds initiatives feed delivery; ADRs record the decisions).
const LEVEL_BANDS: &[(&str, &str)] = &[
    ("strategy", "Strategy"),
    ("initiative", "Initiatives"),
    ("delivery", "Delivery"),
    ("adr", "Decisions"),
];

/// One rendered board-list band: level heading + its tiles, with the
/// delivery band grouped by owning team.
struct BandModel {
    level: String,
    label: String,
    /// `(heading, /teams/:slug href, boards)` — one group per team for the
    /// delivery band; a single unnamed group for every other level.
    groups: Vec<(Option<(String, String)>, Vec<data::Board>)>,
}

/// Bucket boards into level bands (strategy → initiative → delivery →
/// adr, unknown levels last) and the delivery band by owning team.
/// Teamless delivery boards keep a group of their own — nothing becomes
/// unreachable. Pure, host-tested.
fn band_models(
    boards: Vec<data::Board>,
    teams: &[crate::pages::teams::api::Team],
) -> Vec<BandModel> {
    let mut bands: Vec<BandModel> = Vec::new();
    let known: Vec<&str> = LEVEL_BANDS.iter().map(|(level, _)| *level).collect();
    for (level, label) in LEVEL_BANDS {
        let of_level: Vec<data::Board> = boards
            .iter()
            .filter(|b| b.board_level == *level)
            .cloned()
            .collect();
        if of_level.is_empty() {
            continue;
        }
        let groups = if *level == "delivery" {
            // One group per team (team order = teams list order), then
            // teamless boards under their own heading.
            let mut groups: Vec<(Option<(String, String)>, Vec<data::Board>)> = Vec::new();
            for team in teams {
                let of_team: Vec<data::Board> = of_level
                    .iter()
                    .filter(|b| b.team_id.as_deref() == Some(team.id.as_str()))
                    .cloned()
                    .collect();
                if !of_team.is_empty() {
                    groups.push((
                        Some((team.name.clone(), format!("/teams/{}", team.slug))),
                        of_team,
                    ));
                }
            }
            let known_team = |id: &Option<String>| {
                id.as_deref()
                    .is_some_and(|id| teams.iter().any(|t| t.id == id))
            };
            let orphans: Vec<data::Board> = of_level
                .iter()
                .filter(|b| !known_team(&b.team_id))
                .cloned()
                .collect();
            if !orphans.is_empty() {
                groups.push((Some(("No team".to_string(), String::new())), orphans));
            }
            groups
        } else {
            vec![(None, of_level)]
        };
        bands.push(BandModel {
            level: level.to_string(),
            label: label.to_string(),
            groups,
        });
    }
    // Anything with a level outside the known vocabulary still renders.
    let unknown: Vec<data::Board> = boards
        .iter()
        .filter(|b| !known.contains(&b.board_level.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        bands.push(BandModel {
            level: "other".to_string(),
            label: "Other".to_string(),
            groups: vec![(None, unknown)],
        });
    }
    bands
}

/// `/boards` — boards in flight-level bands (strategy above initiatives
/// above delivery, KAIROS-T-0069/T-0063), the delivery band grouped by
/// owning team with headings linking into `/teams/:slug`.
#[component]
pub fn BoardsPage() -> impl IntoView {
    let auth = use_auth();
    let boards = LocalResource::new(move || {
        let _ = auth.token();
        data::list_boards(auth)
    });
    // Team names for the delivery grouping. Optional enhancement data: a
    // failed teams read degrades to "No team" grouping, never a dead page.
    let teams = LocalResource::new(move || {
        let _ = auth.token();
        crate::pages::teams::api::list_teams(auth)
    });
    view! {
        <PageHeader title="Boards" sub="kanban per flight level — strategy feeds initiatives feed delivery"/>
        {move || match boards.get() {
            None => view! { <Loading label="Loading boards…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| boards.refetch())/>
            }.into_any(),
            Some(Ok(items)) if items.is_empty() => view! {
                <Empty message="No boards yet — an org admin can create one from Admin."/>
            }.into_any(),
            Some(Ok(items)) => {
                let team_list = teams.get().and_then(|r| r.ok()).unwrap_or_default();
                let bands = band_models(items, &team_list);
                view! {
                    <Stack gap="md">
                        {bands.into_iter().map(|band| {
                            let BandModel { level, label, groups } = band;
                            view! {
                                <section class="kairos-board-band">
                                    <Group gap="sm">
                                        <Pill color=level_color(&level)>{label}</Pill>
                                    </Group>
                                    <Stack gap="sm">
                                        {groups.into_iter().map(|(heading, group_boards)| view! {
                                            {heading.map(|(team_name, team_href)| {
                                                if team_href.is_empty() {
                                                    view! {
                                                        <Text dimmed=true size="xs">{team_name}</Text>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <Anchor href=team_href>{team_name}</Anchor>
                                                    }.into_any()
                                                }
                                            })}
                                            <div class="kairos-board-grid">
                                                {group_boards.into_iter().map(|board| {
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
                                        }).collect_view()}
                                    </Stack>
                                </section>
                            }
                        }).collect_view()}
                    </Stack>
                }.into_any()
            }
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
    // The shell's shared identity (KAIROS-T-0072): powers derive from it.
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();

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
                // Powers re-derive when whoami lands/refreshes, so the
                // affordances appear without waiting for a board refetch.
                let board_slug = view.items.board.slug.clone();
                let team_id = view.items.board.team_id.clone();
                let kind = EntityKind::for_board_level(&view.items.board.board_level);
                let powers = Signal::derive(move || {
                    whoami
                        .and_then(|resource| resource.get())
                        .and_then(|result| result.ok())
                        .map(|me| board_powers(&me, &board_slug, team_id.as_deref(), kind))
                        .unwrap_or_default()
                });
                view! { <BoardBody view powers on_changed on_error/> }.into_any()
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
    /// What the user may do here (KAIROS-T-0072) — gates every mutating
    /// affordance; the server stays the authority.
    powers: Signal<BoardPowers>,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    let auth = use_auth();
    let data::BoardView { detail, items } = view;
    let board = items.board.clone();
    let create_kind = EntityKind::for_board_level(&board.board_level);

    // The one in-flight drag (KAIROS-T-0064); columns read it to decide
    // whether they are legal drop targets.
    let drag: RwSignal<Option<DragData>> = RwSignal::new(None);

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
        view! {
            {move || (documents_offered && powers.get().documents).then(|| view! {
                <Button variant="default" size="xs" on_click=Callback::new(move |_| doc_open.set(true))>
                    "New document"
                </Button>
            })}
        }
        .into_any()
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
                // Per-handler copies of this column's id (the drop target).
                let class_id = id.clone();
                let over_id = id.clone();
                let drop_id = id.clone();
                let column_for_create = (id, name);
                view! {
                    <section
                        class="kairos-board__column"
                        class:kairos-board__column--droppable=move || {
                            drag.with(|d| d.as_ref().is_some_and(|d| d.targets.contains(&class_id)))
                        }
                        on:dragover=move |ev: web_sys::DragEvent| {
                            // preventDefault marks the column as a valid
                            // drop target — only for legal transitions.
                            let legal = drag.with_untracked(|d| {
                                d.as_ref().is_some_and(|d| d.targets.contains(&over_id))
                            });
                            if legal {
                                ev.prevent_default();
                            }
                        }
                        on:drop=move |ev: web_sys::DragEvent| {
                            ev.prevent_default();
                            let Some(data) = drag.get_untracked() else { return };
                            drag.set(None);
                            if !data.targets.contains(&drop_id) {
                                return;
                            }
                            run_transition(
                                auth,
                                data.kind,
                                data.short_code,
                                drop_id.clone(),
                                on_changed,
                                on_error,
                            );
                        }
                    >
                        <header class="kairos-board__column-head">
                            <Group justify="between">
                                <Group gap="xs">
                                    <Text bright=true bold=true size="sm">{head_label}</Text>
                                    <Text dimmed=true size="xs">{count.to_string()}</Text>
                                </Group>
                                {
                                    let column_for_create = StoredValue::new(column_for_create);
                                    let action_title = StoredValue::new(action_title);
                                    move || (create_kind.is_some() && powers.get().create).then(|| view! {
                                        <ActionIcon
                                            title=action_title.get_value()
                                            on_click=Callback::new(move |_| {
                                                create_column.set(Some(column_for_create.get_value()));
                                                create_open.set(true);
                                            })
                                        >
                                            "+"
                                        </ActionIcon>
                                    })
                                }
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
                                        drag powers
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

/// One board card: short code, title, type, key metadata, open link,
/// drag-and-drop between columns (KAIROS-T-0064 — draggable only when the
/// board allows moves from here), and the click-to-move menu kept as the
/// keyboard/accessibility fallback.
#[component]
fn ItemCard(
    kind: EntityKind,
    short_code: String,
    title: String,
    /// `(label, color-token)` pills — per-type key metadata.
    meta: Vec<(String, &'static str)>,
    /// `(column_id, column_name)` — the valid targets from this column.
    targets: Vec<(String, String)>,
    /// The board's in-flight drag; this card writes itself here on
    /// dragstart so legal columns light up and accept the drop.
    drag: RwSignal<Option<DragData>>,
    /// The user's powers on this board (KAIROS-T-0072): no transition
    /// power → no drag, no move menu.
    powers: Signal<BoardPowers>,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    let auth = use_auth();
    let busy = RwSignal::new(false);
    let href = format!("/items/{short_code}");
    let code_text = short_code.clone();
    let code_for_drag = short_code.clone();
    let code_for_class = short_code.clone();
    let code_for_move = short_code;
    let has_targets = !targets.is_empty();
    let target_ids: Vec<String> = targets.iter().map(|(id, _)| id.clone()).collect();
    let movable = move || has_targets && powers.get().transition;
    view! {
        <article
            class="kairos-card"
            class:kairos-card--dragging=move || {
                drag.with(|d| d.as_ref().is_some_and(|d| d.short_code == code_for_class))
            }
            draggable=move || if movable() { "true" } else { "false" }
            on:dragstart=move |ev: web_sys::DragEvent| {
                if !(has_targets && powers.get_untracked().transition) {
                    return;
                }
                // dataTransfer content is required for some engines to
                // start a drag at all; the real payload is the signal.
                if let Some(dt) = ev.data_transfer() {
                    let _ = dt.set_data("text/plain", &code_for_drag);
                    dt.set_effect_allowed("move");
                }
                drag.set(Some(DragData {
                    kind,
                    short_code: code_for_drag.clone(),
                    targets: target_ids.clone(),
                }));
            }
            on:dragend=move |_| drag.set(None)
        >
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
            {
                // Each menu entry owns its short code + target column;
                // stored once, re-cloned per reactive render.
                let entries: StoredValue<Vec<(String, String, String)>> = StoredValue::new(
                    targets
                        .into_iter()
                        .map(|(column_id, column_name)| {
                            (code_for_move.clone(), column_id, column_name)
                        })
                        .collect(),
                );
                move || movable().then(|| view! {
                    <div class="kairos-card__actions">
                        {move || busy.get().then(|| view! {
                            <Text dimmed=true size="xs">"Moving…"</Text>
                        })}
                        <Menu label="Move">
                            {entries.get_value().into_iter().map(|(code, column_id, label)| {
                                let label_text = label.clone();
                                view! {
                                    <MenuItem on_click=Callback::new(move |_| {
                                        busy.set(true);
                                        let done = Callback::new(move |()| {
                                            busy.set(false);
                                            on_changed.run(());
                                        });
                                        let fail = Callback::new(move |error| {
                                            busy.set(false);
                                            on_error.run(error);
                                        });
                                        run_transition(
                                            auth,
                                            kind,
                                            code.clone(),
                                            column_id.clone(),
                                            done,
                                            fail,
                                        );
                                    })>
                                        {label_text}
                                    </MenuItem>
                                }
                            }).collect_view()}
                        </Menu>
                    </div>
                })
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::teams::api::Team;

    fn board(id: &str, level: &str, team_id: Option<&str>) -> data::Board {
        data::Board {
            id: id.to_string(),
            name: id.to_string(),
            slug: id.to_string(),
            board_level: level.to_string(),
            team_id: team_id.map(str::to_string),
        }
    }

    fn team(id: &str, slug: &str) -> Team {
        Team {
            id: id.to_string(),
            name: slug.to_string(),
            slug: slug.to_string(),
            team_type: "platform".to_string(),
            delivery_board_id: None,
        }
    }

    /// Bands come out in flight-level order, the delivery band grouped by
    /// team with teamless boards kept reachable in their own group.
    #[test]
    fn band_models_orders_levels_and_groups_delivery_by_team() {
        let boards = vec![
            board("adr-board", "adr", None),
            board("web-delivery", "delivery", Some("t2")),
            board("main-strategy", "strategy", None),
            board("platform-delivery", "delivery", Some("t1")),
            board("initiatives", "initiative", None),
            board("orphan-delivery", "delivery", None),
        ];
        let teams = vec![team("t1", "platform"), team("t2", "web")];
        let bands = band_models(boards, &teams);

        let levels: Vec<&str> = bands.iter().map(|b| b.level.as_str()).collect();
        assert_eq!(levels, vec!["strategy", "initiative", "delivery", "adr"]);

        let delivery = &bands[2];
        let headings: Vec<Option<&str>> = delivery
            .groups
            .iter()
            .map(|(h, _)| h.as_ref().map(|(name, _)| name.as_str()))
            .collect();
        assert_eq!(
            headings,
            vec![Some("platform"), Some("web"), Some("No team")]
        );
        let platform_group = &delivery.groups[0];
        assert_eq!(
            platform_group.0.as_ref().map(|(_, href)| href.as_str()),
            Some("/teams/platform")
        );
        assert_eq!(platform_group.1[0].slug, "platform-delivery");
    }

    fn me(role: &str, team_ids: &[&str], grants: &[(&str, &[&str])]) -> crate::api::Whoami {
        serde_json::from_value(serde_json::json!({
            "user": {"display_name": "u", "email": "u@x.test"},
            "organization": {"slug": "demo", "role": role},
            "teams": team_ids.iter().map(|id| serde_json::json!({
                "id": id, "slug": id, "name": id
            })).collect::<Vec<_>>(),
            "capabilities": grants.iter().map(|(slug, caps)| serde_json::json!({
                "board_slug": slug, "grants": caps
            })).collect::<Vec<_>>(),
        }))
        .expect("test whoami")
    }

    /// KAIROS-T-0072 client mirror: team membership implies the delivery
    /// set on the team's board — and only there, and only that set.
    #[test]
    fn board_powers_mirror_team_implication() {
        let bob = me("member", &["t1"], &[]);
        // On the team's delivery board: transition + create tasks + docs.
        let on_team = board_powers(&bob, "platform-delivery", Some("t1"), Some(EntityKind::Task));
        assert!(on_team.transition && on_team.create && on_team.documents);
        // A team-owned STRATEGY board: transition/docs implied, create is
        // manage_strategies — not implied.
        let strat = board_powers(&bob, "s", Some("t1"), Some(EntityKind::Strategy));
        assert!(strat.transition && strat.documents && !strat.create);
        // Someone else's board: nothing.
        let other = board_powers(&bob, "web-delivery", Some("t2"), Some(EntityKind::Task));
        assert_eq!(other, BoardPowers::default());
        // Org-wide (teamless) board: nothing.
        let orgwide = board_powers(&bob, "strategy", None, Some(EntityKind::Strategy));
        assert_eq!(orgwide, BoardPowers::default());
    }

    /// Explicit grants (incl. globs) and the admin bypass keep working.
    #[test]
    fn board_powers_mirror_grants_and_admin() {
        let admin = me("admin", &[], &[]);
        let p = board_powers(&admin, "any", None, Some(EntityKind::Adr));
        assert!(p.transition && p.create && p.documents);

        let granted = me("member", &[], &[("adrs", &["transition_*", "manage_adrs"])]);
        let p = board_powers(&granted, "adrs", None, Some(EntityKind::Adr));
        assert!(p.transition && p.create && !p.documents);
        // Grants are board-scoped: elsewhere they mean nothing.
        let elsewhere = board_powers(&granted, "strategy", None, Some(EntityKind::Strategy));
        assert_eq!(elsewhere, BoardPowers::default());

        assert!(grant_covers("*", "manage_tasks"));
        assert!(grant_covers("manage_*", "manage_tasks"));
        assert!(!grant_covers("manage_*", "transition_items"));
        assert!(!grant_covers("manage_tasks", "manage_taskss"));
    }

    /// A board whose team id names an unknown team lands in "No team"
    /// (a stale/failed teams read must never hide boards).
    #[test]
    fn band_models_keeps_unknown_team_boards_reachable() {
        let boards = vec![board("d", "delivery", Some("gone"))];
        let bands = band_models(boards, &[]);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].groups.len(), 1);
        assert_eq!(
            bands[0].groups[0].0.as_ref().map(|(name, _)| name.as_str()),
            Some("No team")
        );
        assert_eq!(bands[0].groups[0].1.len(), 1);
    }
}
