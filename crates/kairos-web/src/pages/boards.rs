//! Boards area (KAIROS-T-0040): the board list (`/boards`) and the board
//! view (`/boards/:board` — slug or id) with live `/ws/events` updates.
//!
//! Shape (per docs/gui-conventions.md and A-0015):
//! - columns come from the board's configuration; items arrive grouped by
//!   column from `GET /api/boards/{id}/items` (all four board entity
//!   types: strategies, initiatives, tasks, ADRs);
//! - **transitions are drag-and-drop** (KAIROS-T-0064/T-0075): a card
//!   drags only to the columns the board's `transitions` allow from its
//!   current column (A-0002: invalid moves are never offered); the
//!   keyboard-accessible path is the item detail page's move control;
//!   mutation errors surface in a page-level `Banner`;
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

pub(crate) mod data;
pub(crate) mod live;

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, Modal, PageHeader, Pill, Select,
    Stack, Text, TextInput, Textarea,
};
use aurora_dark::tokens::{ApiError, token};
use aurora_dark::widgets::Banner;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{query_signal_with_options, use_params_map};

use super::copy_link;
use super::repositories;
use crate::auth::use_auth;
use data::EntityKind;
use repositories::api::NO_REPOSITORY;

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

/// The in-flight card drag (KAIROS-T-0064): which card, where it started,
/// and the column ids its CURRENT column's transitions allow as drop
/// targets — so only legal drop zones light up and accept the drop
/// (A-0002: invalid moves are never offered). `source_column` and
/// `work_class` make (column, lane) drops decidable (KAIROS-T-0077).
#[derive(Clone, Debug, PartialEq)]
struct DragData {
    kind: EntityKind,
    short_code: String,
    targets: Vec<String>,
    /// The column the card is dragged FROM.
    source_column: String,
    /// The card's current lane — `Some` for tasks, `None` for kinds that
    /// carry no lane (and therefore cannot lane-move).
    work_class: Option<String>,
}

// ---------------------------------------------------------------------------
// Planned/Support lanes (KAIROS-T-0077)
// ---------------------------------------------------------------------------

/// The two board lanes: the lane is a projection of `tasks.work_class`.
const LANE_SUPPORT: &str = "support";
const LANE_PLANNED: &str = "planned";

/// Which lane a card renders in: tasks follow their `work_class`;
/// non-task kinds always render in the Planned (default) lane. Pure,
/// host-tested.
fn card_lane(work_class: Option<&str>) -> &'static str {
    match work_class {
        Some(LANE_SUPPORT) => LANE_SUPPORT,
        _ => LANE_PLANNED,
    }
}

/// What dropping the in-flight drag onto (column, lane) would do.
/// `None` = illegal or no-op, and the zone neither lights up nor accepts.
/// Pure, host-tested: same-column cross-lane = lane write only (the rules
/// engine is never consulted); cross-column same-lane = transition as
/// before; diagonal = both; lane moves only exist for cards that carry a
/// lane (tasks).
#[derive(Clone, Debug, PartialEq)]
struct DropEffect {
    transition_to: Option<String>,
    set_work_class: Option<String>,
}

fn drop_effect(drag: &DragData, column_id: &str, lane: Option<&str>) -> Option<DropEffect> {
    let column_change = drag.source_column != column_id;
    let lane_change = match (lane, drag.work_class.as_deref()) {
        (Some(lane), Some(current)) => current != lane,
        _ => false,
    };
    match (column_change, lane_change) {
        (false, false) => None,
        (false, true) => Some(DropEffect {
            transition_to: None,
            set_work_class: lane.map(str::to_string),
        }),
        (true, _) if !drag.targets.iter().any(|t| t == column_id) => None,
        (true, false) => Some(DropEffect {
            transition_to: Some(column_id.to_string()),
            set_work_class: None,
        }),
        (true, true) => Some(DropEffect {
            transition_to: Some(column_id.to_string()),
            set_work_class: lane.map(str::to_string),
        }),
    }
}

// ---------------------------------------------------------------------------
// Client-side capability mirror (KAIROS-T-0072)
// ---------------------------------------------------------------------------

/// What the signed-in user may do on THIS board — mirrors the A-0006
/// decision (org-admin bypass, explicit grants incl. globs, and the
/// KAIROS-T-0072 team implication) so affordances the server would 403
/// never render. The server remains the authority; this is UX.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BoardPowers {
    /// May move cards (`transition_items`).
    pub(crate) transition: bool,
    /// May create this board level's entity (`manage_<family>`).
    pub(crate) create: bool,
    /// May create documents (`manage_documents`).
    pub(crate) documents: bool,
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
/// Also used by the item detail page's move control (KAIROS-T-0075) —
/// the same `transition_items` gate as the board's drag affordance.
pub(crate) fn board_powers(
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
    let team_member = board_team_id.is_some_and(|team| me.teams.iter().any(|mine| mine.id == team));
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

/// Apply one [`DropEffect`] and report through the standard board
/// callbacks — the drop handler's mutation path (the keyboard-accessible
/// path lives on the item detail page, KAIROS-T-0075). A diagonal drop is
/// two sequential writes (transition, then lane); a failure after the
/// first still refetches nothing stale — `on_error` surfaces it and the
/// next WS event reconciles.
fn run_drop(
    auth: crate::auth::Auth,
    kind: EntityKind,
    code: String,
    effect: DropEffect,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) {
    leptos::task::spawn_local(async move {
        if let Some(column_id) = effect.transition_to
            && let Err(error) = data::transition(auth, kind, &code, &column_id).await
        {
            on_error.run(error);
            return;
        }
        if let Some(work_class) = effect.set_work_class
            && let Err(error) = data::set_work_class(auth, &code, &work_class).await
        {
            on_error.run(error);
            return;
        }
        on_changed.run(());
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

/// One board-list group: `(Some((heading, /teams/:slug href)), boards)`
/// per team for the delivery band; a single unnamed group for every other
/// level.
type BoardGroup = (Option<(String, String)>, Vec<data::Board>);

/// One rendered board-list band: level heading + its tiles, with the
/// delivery band grouped by owning team.
struct BandModel {
    level: String,
    label: String,
    groups: Vec<BoardGroup>,
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
            let mut groups: Vec<BoardGroup> = Vec::new();
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

/// One card's owned view model with a content-fingerprint `key`
/// (KAIROS-T-0074): the card `<For>` diffs on it, so a card whose content
/// is unchanged keeps its DOM node — element identity, focus, transient
/// state — across refetches; a changed card is rebuilt.
#[derive(Clone, PartialEq)]
struct CardModel {
    kind: EntityKind,
    short_code: String,
    title: String,
    meta: Vec<(String, &'static str)>,
    /// `tasks.work_class` — `Some` for tasks only (KAIROS-T-0077).
    work_class: Option<String>,
    /// The bound repository's slug — tasks only (KAIROS-T-0109, A-0019).
    repository: Option<String>,
    /// Children rollup — `Some` for parents only (KAIROS-T-0080).
    progress: Option<data::ProgressCounts>,
    /// Blocked-by/blocks counts — `Some` only with live blocks edges
    /// (KAIROS-T-0091).
    blocks: Option<data::BlocksCounts>,
    key: String,
}

/// One column's owned view model. Its `key` fingerprints identity + the
/// transition-derived drop targets (config changes rebuild the column);
/// the card list is NOT in the key — cards diff independently.
#[derive(Clone, PartialEq)]
struct ColumnModel {
    id: String,
    name: String,
    targets: Vec<(String, String)>,
    cards: Vec<CardModel>,
    key: String,
}

/// Flatten the wire shape into owned, keyed view models (leptos children
/// are `'static` move closures, so views must own their data).
fn column_models(view: &data::BoardView) -> Vec<ColumnModel> {
    let column_name_of = |id: &str| -> String {
        view.detail
            .columns
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };
    let progress_of = |short_code: &str| view.items.children_progress.get(short_code).copied();
    let blocks_of = |short_code: &str| view.items.blocks_summary.get(short_code).copied();
    let card = |kind: EntityKind,
                short_code: &str,
                title: &str,
                meta: Vec<(String, &'static str)>,
                work_class: Option<String>,
                repository: Option<String>| {
        let progress = progress_of(short_code);
        let blocks = blocks_of(short_code);
        CardModel {
            kind,
            short_code: short_code.to_string(),
            title: title.to_string(),
            key: format!(
                "{short_code}|{title}|{meta:?}|{work_class:?}|{repository:?}|{progress:?}|{blocks:?}"
            ),
            work_class,
            repository,
            progress,
            blocks,
            meta,
        }
    };
    view.items
        .columns
        .iter()
        .map(|group| {
            // ONLY the targets this board's transitions allow from this
            // column — invalid moves are never offered (S-0005).
            let targets: Vec<(String, String)> = view
                .detail
                .transitions
                .iter()
                .filter(|t| t.from_column_id == group.column.id)
                .map(|t| (t.to_column_id.clone(), column_name_of(&t.to_column_id)))
                .collect();
            let mut cards: Vec<CardModel> = Vec::new();
            cards.extend(group.strategies.iter().map(|item| {
                card(
                    EntityKind::Strategy,
                    &item.short_code,
                    &item.title,
                    Vec::new(),
                    None,
                    None,
                )
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
                card(
                    EntityKind::Initiative,
                    &item.short_code,
                    &item.title,
                    meta,
                    None,
                    None,
                )
            }));
            cards.extend(group.tasks.iter().map(|item| {
                let meta = match item.task_type.as_str() {
                    "bug" => vec![("bug".to_string(), token::BAD)],
                    "tech_debt" => vec![("tech debt".to_string(), token::GOLD)],
                    "support" => vec![("support".to_string(), token::GOLD)],
                    _ => Vec::new(),
                };
                card(
                    EntityKind::Task,
                    &item.short_code,
                    &item.title,
                    meta,
                    Some(item.work_class.clone()),
                    item.repository.as_ref().map(|r| r.slug.clone()),
                )
            }));
            cards.extend(group.adrs.iter().map(|item| {
                let meta = item
                    .decision_date
                    .iter()
                    .map(|date| (format!("decided {date}"), token::VIOLET))
                    .collect();
                card(
                    EntityKind::Adr,
                    &item.short_code,
                    &item.title,
                    meta,
                    None,
                    None,
                )
            }));
            ColumnModel {
                id: group.column.id.clone(),
                name: group.column.name.clone(),
                key: format!("{}|{}|{:?}", group.column.id, group.column.name, targets),
                targets,
                cards,
            }
        })
        .collect()
}

/// `(short_code, title)` of the board's document-parent candidates
/// (strategies/initiatives/tasks — documents attach via `supports`).
fn doc_parent_options(view: &data::BoardView) -> Vec<(String, String)> {
    view.items
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
        .collect()
}

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

    // The live model behind the fine-grained board (KAIROS-T-0074):
    // refetches update THIS SIGNAL and the mounted BoardBody diffs against
    // it through memos + keyed <For>s — the DOM is not rebuilt, so
    // transient UI state (open modals, element identity) survives
    // WS-driven updates.
    let model: RwSignal<Option<data::BoardView>> = RwSignal::new(None);
    Effect::new(move |_| {
        if let Some(Ok(view)) = board.get() {
            model.set(Some(view));
        }
    });
    // One BoardBody instance per board id — navigating to another board
    // (or the first load) is the only thing that recreates it.
    let board_key =
        Memo::new(move |_| model.with(|m| m.as_ref().map(|view| view.items.board.id.clone())));
    // Powers re-derive when whoami OR the board changes, so affordances
    // appear as soon as both are known (KAIROS-T-0072). MEMOIZED
    // (KAIROS-T-0074): a plain Signal::derive notifies consumers on every
    // model refetch even when the value is identical — which re-rendered
    // every card and destroyed its DOM node, exactly what the
    // fine-grained rendering exists to prevent.
    let powers: Signal<BoardPowers> = Memo::new(move |_| {
        let identity = whoami
            .and_then(|resource| resource.get())
            .and_then(|result| result.ok());
        model
            .with(|m| {
                m.as_ref().zip(identity.as_ref()).map(|(view, me)| {
                    let board = &view.items.board;
                    board_powers(
                        me,
                        &board.slug,
                        board.team_id.as_deref(),
                        EntityKind::for_board_level(&board.board_level),
                    )
                })
            })
            .unwrap_or_default()
    })
    .into();

    view! {
        {move || match board.get() {
            // A load error renders above the (stale) board rather than
            // destroying it; retry refetches in place.
            Some(Err(error)) => Some(view! {
                <ErrorState error on_retry=Callback::new(move |_| refetch())/>
            }.into_any()),
            None if model.with_untracked(|m| m.is_none()) => Some(view! {
                <Loading label="Loading board…"/>
            }.into_any()),
            _ => None,
        }}
        {move || board_key.get().map(|_| view! {
            <BoardBody model powers on_changed on_error/>
        })}
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

/// The loaded board: header (+ create actions) and the column row.
///
/// Fine-grained rendering (KAIROS-T-0074): created once per board (keyed
/// on board id by [`BoardPage`]) and reads everything through memos over
/// the shared `model` signal, with keyed `<For>`s over columns and cards.
/// A refetch updates only what changed — an untouched card keeps its DOM
/// node across WS-driven updates (the old whole-DOM rebuild replaced
/// every element).
#[component]
fn BoardBody(
    /// The live board view model. ALWAYS `Some` while this component is
    /// mounted — [`BoardPage`] keys it on the model's board id.
    model: RwSignal<Option<data::BoardView>>,
    /// What the user may do here (KAIROS-T-0072) — gates every mutating
    /// affordance; the server stays the authority.
    powers: Signal<BoardPowers>,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    // Identity fields are stable for this instance's lifetime (a board-id
    // change recreates the whole component).
    let board = model.with_untracked(|m| {
        m.as_ref()
            .expect("BoardBody mounted without a model")
            .items
            .board
            .clone()
    });
    let create_kind = EntityKind::for_board_level(&board.board_level);
    let is_adr = board.board_level == "adr";
    let is_delivery = board.board_level == "delivery";
    let board_id = StoredValue::new(board.id.clone());
    let team_id = StoredValue::new(board.team_id.clone());

    // The one in-flight drag (KAIROS-T-0064); columns read it to decide
    // whether they are legal drop targets.
    let drag: RwSignal<Option<DragData>> = RwSignal::new(None);

    // Modal open-state lives HERE so refetches never reset it.
    let create_open = RwSignal::new(false);
    let doc_open = RwSignal::new(false);

    // ---- memos over the live model (notify only on actual change) --------
    let header_text = Memo::new(move |_| {
        model
            .with(|m| {
                m.as_ref().map(|view| {
                    let board = &view.items.board;
                    (
                        board.name.clone(),
                        format!("{} board · {}", board.board_level, board.slug),
                    )
                })
            })
            .unwrap_or_default()
    });
    // KAIROS-T-0062: creation is global and always lands in the board's
    // entry column (lowest position — Backlog/Draft/Discovery on the
    // seeded defaults). Creation is intake; flow happens by transition.
    let entry_column = Memo::new(move |_| {
        model.with(|m| {
            m.as_ref().and_then(|view| {
                view.detail
                    .columns
                    .iter()
                    .min_by_key(|column| column.position)
                    .map(|column| (column.id.clone(), column.name.clone()))
            })
        })
    });
    // Document create ("New document") — not offered on ADR boards:
    // documents attach to strategies/initiatives/tasks.
    let doc_parents =
        Memo::new(move |_| model.with(|m| m.as_ref().map(doc_parent_options).unwrap_or_default()));
    let documents_offered = Memo::new(move |_| !is_adr && !doc_parents.with(Vec::is_empty));
    // KAIROS-T-0109 (A-0019): the repository lens. `?repo=a,b` in the URL
    // is the source of truth for the selection, so it survives WS
    // refetches AND reloads; `?by_repo=1` groups the delivery board into
    // one lane per repository instead of Support/Planned. Lens writes
    // REPLACE the history entry (a filter change is not a page the back
    // button should revisit — and the prune write-back below must never
    // push, or "back" would land on the stale URL and bounce forward).
    let lens_nav = || NavigateOptions {
        replace: true,
        scroll: false,
        ..Default::default()
    };
    let (repo_query, set_repo_query) = query_signal_with_options::<String>("repo", lens_nav());
    let (by_repo_query, set_by_repo_query) =
        query_signal_with_options::<String>("by_repo", lens_nav());
    // Every repository slug present on the board, sorted — the chip set.
    let board_repos = Memo::new(move |_| {
        model.with(|m| {
            let mut slugs: Vec<String> = m
                .as_ref()
                .map(|view| {
                    view.items
                        .columns
                        .iter()
                        .flat_map(|c| c.tasks.iter())
                        .filter_map(|t| t.repository.as_ref().map(|r| r.slug.clone()))
                        .collect()
                })
                .unwrap_or_default();
            slugs.sort_unstable();
            slugs.dedup();
            slugs
        })
    });
    // What the URL asks for vs. what the board can show (KAIROS-T-0114):
    // a stale `?repo=<slug no longer here>` must not blank every task
    // with no chip lit, so the EFFECTIVE selection is pruned against the
    // board's slugs, and the pruned form is written back to the URL.
    let requested_repos = Memo::new(move |_| parse_repo_query(repo_query.get().as_deref()));
    let selected_repos = Memo::new(move |_| {
        requested_repos.with(|requested| board_repos.with(|known| prune_repos(requested, known)))
    });
    Effect::new(move |_| {
        let requested = requested_repos.get();
        let selected = selected_repos.get();
        if requested != selected {
            set_repo_query.set((!selected.is_empty()).then(|| selected.join(",")));
        }
    });
    let group_by_repo = Memo::new(move |_| by_repo_query.get().is_some_and(|v| v == "1"));
    // The lens row renders whenever the board has repositories OR a lens
    // query param is set (so a stale/odd URL always has a visible "clear");
    // the group-by toggle renders when grouping is meaningful (>1 repo) OR
    // already on (so it can always be turned off).
    let lens_active =
        Memo::new(move |_| repo_query.get().is_some() || by_repo_query.get().is_some());
    let lens_shown = Memo::new(move |_| !board_repos.with(Vec::is_empty) || lens_active.get());
    let group_toggle_shown =
        Memo::new(move |_| board_repos.with(|r| r.len() > 1) || group_by_repo.get());
    // The group-by lanes derive from the effective selection (+ unbound).
    let lanes = Memo::new(move |_| {
        selected_repos.with(|selected| board_repos.with(|known| repo_lanes(selected, known)))
    });
    let toggle_repo = move |slug: String| {
        let mut selected = selected_repos.get_untracked();
        match selected.iter().position(|s| *s == slug) {
            Some(i) => {
                selected.remove(i);
            }
            None => selected.push(slug),
        }
        set_repo_query.set((!selected.is_empty()).then(|| selected.join(",")));
    };
    let clear_lens = move || {
        set_repo_query.set(None);
        set_by_repo_query.set(None);
    };
    let columns = Memo::new(move |_| {
        let selected = selected_repos.get();
        model.with(|m| {
            let mut columns = m.as_ref().map(column_models).unwrap_or_default();
            if !selected.is_empty() {
                for column in columns.iter_mut() {
                    column.cards.retain(|card| lens_admits(card, &selected));
                }
            }
            columns
        })
    });

    let create_label = StoredValue::new(
        create_kind
            .map(|kind| format!("New {}", kind.label()))
            .unwrap_or_default(),
    );

    view! {
        {move || {
            let (title, sub) = header_text.get();
            let header_right: Children = Box::new(move || view! {
                <Group gap="xs">
                    {move || (create_kind.is_some()
                        && entry_column.with(Option::is_some)
                        && powers.get().create)
                        .then(|| view! {
                            <Button size="xs" on_click=Callback::new(move |_| create_open.set(true))>
                                {create_label.get_value()}
                            </Button>
                        })}
                    {move || (documents_offered.get() && powers.get().documents).then(|| view! {
                        <Button variant="default" size="xs" on_click=Callback::new(move |_| doc_open.set(true))>
                            "New document"
                        </Button>
                    })}
                </Group>
            }.into_any());
            view! { <PageHeader title sub right=header_right/> }
        }}
        {move || (is_delivery && lens_shown.get()).then(|| view! {
            <div class="kairos-board__lens" data-testid="repo-lens">
                <Group gap="xs" wrap=true>
                    <Text dimmed=true size="xs">"Repository"</Text>
                    <For
                        each=move || board_repos.get()
                        key=|slug| slug.clone()
                        children=move |slug: String| {
                            let on_slug = slug.clone();
                            let is_on = Memo::new(move |_| selected_repos.with(|s| s.contains(&slug)));
                            let label = on_slug.clone();
                            let attr = on_slug.clone();
                            view! {
                                <button
                                    type="button"
                                    class="kairos-board__lens-chip"
                                    class:kairos-board__lens-chip--on=move || is_on.get()
                                    aria-pressed=move || is_on.get().to_string()
                                    data-repo=attr
                                    on:click=move |_| toggle_repo(on_slug.clone())
                                >
                                    {label}
                                </button>
                            }
                        }
                    />
                    {move || group_toggle_shown.get().then(|| view! {
                        <button
                            type="button"
                            class="kairos-board__lens-chip"
                            class:kairos-board__lens-chip--on=move || group_by_repo.get()
                            aria-pressed=move || group_by_repo.get().to_string()
                            data-testid="group-by-repo"
                            on:click=move |_| {
                                set_by_repo_query.set((!group_by_repo.get_untracked()).then(|| "1".to_string()))
                            }
                        >
                            "Group by repository"
                        </button>
                    })}
                    {move || lens_active.get().then(|| view! {
                        <button
                            type="button"
                            class="kairos-board__lens-chip kairos-board__lens-chip--clear"
                            data-testid="clear-repo-lens"
                            on:click=move |_| clear_lens()
                        >
                            "Clear"
                        </button>
                    })}
                </Group>
            </div>
        })}
        {
            // KAIROS-T-0077: delivery boards split into Support (on top,
            // the expedite convention) and Planned lanes — the lane is a
            // pure projection of tasks.work_class. Other levels keep the
            // single unlaned row. The level is fixed per BoardBody
            // instance, so this branch is deliberately non-reactive.
            // KAIROS-T-0109: with `?by_repo=1` a delivery board instead
            // shows one lane per repository (plus "No repository"), and
            // drops only transition — the lane axis is not a work_class.
            // Lanes are a keyed <For> (key = slug) so a WS refetch that
            // changes the slug set diffs lanes instead of rebuilding them
            // all (KAIROS-T-0074 rule, KAIROS-T-0114).
            if is_delivery {
                view! {
                    {move || if group_by_repo.get() {
                        view! {
                            <For
                                each=move || lanes.get()
                                key=|lane: &LaneKey| lane.clone()
                                children=move |lane: LaneKey| {
                                    view! {
                                        <RepoLaneSection repo=lane columns drag powers on_changed on_error/>
                                    }
                                }
                            />
                        }
                        .into_any()
                    } else {
                        view! {
                            <LaneSection lane=LANE_SUPPORT label="Support" caption="unplanned intake"
                                color=token::GOLD columns drag powers on_changed on_error/>
                            <LaneSection lane=LANE_PLANNED label="Planned" caption="scheduled work"
                                color=token::TEAL columns drag powers on_changed on_error/>
                        }.into_any()
                    }}
                }
                .into_any()
            } else {
                view! { <LaneColumns lane=None repo=RepoLane::Any columns drag powers on_changed on_error/> }
                    .into_any()
            }
        }
        {move || create_kind.zip(entry_column.get()).map(|(kind, entry)| view! {
            <CreateItemModal
                open=create_open
                kind
                board_id=board_id.get_value()
                team_id=team_id.get_value()
                entry
                on_changed
            />
        })}
        {move || documents_offered.get().then(|| view! {
            <CreateDocumentModal open=doc_open parents=doc_parents on_changed/>
        })}
    }
}

// ---------------------------------------------------------------------------
// Lanes + columns (KAIROS-T-0077)
// ---------------------------------------------------------------------------

/// One horizontal lane on a delivery board: accent header with a live
/// card count, then the column row filtered to this lane. An empty
/// Support lane renders slim (`--empty`) but its columns stay valid drop
/// targets.
#[component]
fn LaneSection(
    lane: &'static str,
    label: &'static str,
    caption: &'static str,
    color: &'static str,
    columns: Memo<Vec<ColumnModel>>,
    drag: RwSignal<Option<DragData>>,
    powers: Signal<BoardPowers>,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    let count = Memo::new(move |_| {
        columns.with(|columns| {
            columns
                .iter()
                .flat_map(|column| column.cards.iter())
                .filter(|card| card_lane(card.work_class.as_deref()) == lane)
                .count()
        })
    });
    view! {
        <section
            class="kairos-board__lane"
            class:kairos-board__lane--support=(lane == LANE_SUPPORT)
            class:kairos-board__lane--planned=(lane == LANE_PLANNED)
            class:kairos-board__lane--empty=move || count.get() == 0
        >
            <header class="kairos-board__lane-head">
                <Group gap="xs">
                    <Pill color=color>{label}</Pill>
                    <Text dimmed=true size="xs">{move || count.get().to_string()}</Text>
                    <Text dimmed=true size="xs">{caption}</Text>
                </Group>
            </header>
            <LaneColumns lane=Some(lane) repo=RepoLane::Any columns drag powers on_changed on_error/>
        </section>
    }
}

/// Which repository a lane shows (KAIROS-T-0109).
#[derive(Clone, PartialEq)]
enum RepoLane {
    /// Every card.
    Any,
    /// Tasks bound to this slug.
    Slug(String),
    /// Tasks with no repository (and non-task cards).
    Unbound,
}

impl RepoLane {
    fn admits(&self, card: &CardModel) -> bool {
        match self {
            RepoLane::Any => true,
            RepoLane::Slug(slug) => card.repository.as_deref() == Some(slug.as_str()),
            RepoLane::Unbound => card.repository.is_none(),
        }
    }
}

// ---------------------------------------------------------------------------
// Repository lens projections (KAIROS-T-0109 / KAIROS-T-0114) — pure,
// host-tested
// ---------------------------------------------------------------------------

/// Parse `?repo=a,b` into the requested slugs: empty segments dropped,
/// duplicates removed (first occurrence wins, order kept).
fn parse_repo_query(query: Option<&str>) -> Vec<String> {
    let mut slugs: Vec<String> = Vec::new();
    for slug in query
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        if !slugs.iter().any(|s| s == slug) {
            slugs.push(slug.to_string());
        }
    }
    slugs
}

/// The effective selection: the requested slugs that are actually on the
/// board. A stale `?repo=` (re-homed, unbound, or simply mistyped) is
/// ignored rather than blanking every task with no chip lit.
fn prune_repos(requested: &[String], known: &[String]) -> Vec<String> {
    requested
        .iter()
        .filter(|slug| known.contains(slug))
        .cloned()
        .collect()
}

/// Does the lens admit this card? The filter narrows TASKS only — other
/// kinds always stay — and an empty selection admits everything.
fn lens_admits(card: &CardModel, selected: &[String]) -> bool {
    selected.is_empty()
        || card.kind != EntityKind::Task
        || card
            .repository
            .as_ref()
            .is_some_and(|slug| selected.contains(slug))
}

/// A group-by-repository lane's identity: `Some(slug)` or the unbound
/// remainder (`None`). The keyed `<For>` diffs lanes on it.
type LaneKey = Option<String>;

/// The group-by-repository lanes: one per EFFECTIVE selection (every
/// board repository when nothing is selected), in board order, then the
/// unbound remainder (`None`).
fn repo_lanes(selected: &[String], known: &[String]) -> Vec<LaneKey> {
    known
        .iter()
        .filter(|slug| selected.is_empty() || selected.contains(slug))
        .cloned()
        .map(Some)
        .chain(std::iter::once(None))
        .collect()
}

/// One repository lane on a delivery board (KAIROS-T-0109): the columns
/// narrowed to tasks bound to `repo` (`None` = the unbound remainder).
/// Drops here transition only — no lane axis to re-lane into.
#[component]
fn RepoLaneSection(
    repo: Option<String>,
    columns: Memo<Vec<ColumnModel>>,
    drag: RwSignal<Option<DragData>>,
    powers: Signal<BoardPowers>,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    let lane = match &repo {
        Some(slug) => RepoLane::Slug(slug.clone()),
        None => RepoLane::Unbound,
    };
    let count_lane = lane.clone();
    let count = Memo::new(move |_| {
        columns.with(|columns| {
            columns
                .iter()
                .flat_map(|column| column.cards.iter())
                .filter(|card| count_lane.admits(card))
                .count()
        })
    });
    let label = repo.clone().unwrap_or_else(|| "No repository".to_string());
    let attr = repo.clone().unwrap_or_default();
    view! {
        <section
            class="kairos-board__lane kairos-board__lane--repo"
            class:kairos-board__lane--empty=move || count.get() == 0
            data-repo-lane=attr
        >
            <header class="kairos-board__lane-head">
                <Group gap="xs">
                    <Pill color=token::ICE>{label}</Pill>
                    <Text dimmed=true size="xs">{move || count.get().to_string()}</Text>
                </Group>
            </header>
            <LaneColumns lane=None repo=lane columns drag powers on_changed on_error/>
        </section>
    }
}

/// The column row (KAIROS-T-0040/T-0074 fine-grained rendering), filtered
/// to one lane when `lane` is `Some` (KAIROS-T-0077). Drop zones are
/// (column, lane) pairs decided by [`drop_effect`]; the column node
/// persists across refetches and only its cards diff.
#[component]
fn LaneColumns(
    lane: Option<&'static str>,
    /// Repository narrowing for the group-by-repo view (KAIROS-T-0109).
    repo: RepoLane,
    columns: Memo<Vec<ColumnModel>>,
    drag: RwSignal<Option<DragData>>,
    powers: Signal<BoardPowers>,
    on_changed: Callback<()>,
    on_error: Callback<ApiError>,
) -> impl IntoView {
    let auth = use_auth();
    let repo = StoredValue::new(repo);
    view! {
        <div class="kairos-board">
            <For
                each=move || columns.get()
                key=|column| column.key.clone()
                children=move |column: ColumnModel| {
                    let ColumnModel { id, name, targets, key: _, cards: _ } = column;
                    // This column's live card list — this lane's slice of
                    // it: the column node itself persists across
                    // refetches; only its cards diff.
                    let cards_column_id = id.clone();
                    let cards = Memo::new(move |_| {
                        columns.with(|columns| {
                            columns
                                .iter()
                                .find(|c| c.id == cards_column_id)
                                .map(|c| {
                                    c.cards
                                        .iter()
                                        .filter(|card| repo.with_value(|r| r.admits(card)))
                                        .filter(|card| match lane {
                                            None => true,
                                            Some(lane) => {
                                                card_lane(card.work_class.as_deref()) == lane
                                            }
                                        })
                                        .cloned()
                                        .collect::<Vec<CardModel>>()
                                })
                                .unwrap_or_default()
                        })
                    });
                    // Per-handler copies of this column's id (drop target).
                    let class_id = id.clone();
                    let over_id = id.clone();
                    let drop_id = id.clone();
                    let targets_for_cards = StoredValue::new(targets);
                    let column_for_cards = StoredValue::new(id);
                    view! {
                        <section
                            class="kairos-board__column"
                            class:kairos-board__column--droppable=move || {
                                drag.with(|d| {
                                    d.as_ref()
                                        .is_some_and(|d| drop_effect(d, &class_id, lane).is_some())
                                })
                            }
                            on:dragover=move |ev: web_sys::DragEvent| {
                                // preventDefault marks this zone as a valid
                                // drop target — only when the drop would
                                // actually do something legal.
                                let legal = drag.with_untracked(|d| {
                                    d.as_ref()
                                        .is_some_and(|d| drop_effect(d, &over_id, lane).is_some())
                                });
                                if legal {
                                    ev.prevent_default();
                                }
                            }
                            on:drop=move |ev: web_sys::DragEvent| {
                                ev.prevent_default();
                                let Some(data) = drag.get_untracked() else { return };
                                drag.set(None);
                                let Some(effect) = drop_effect(&data, &drop_id, lane) else {
                                    return;
                                };
                                run_drop(auth, data.kind, data.short_code, effect, on_changed, on_error);
                            }
                        >
                            <header class="kairos-board__column-head">
                                <Group gap="xs">
                                    <Text bright=true bold=true size="sm">{name}</Text>
                                    <Text dimmed=true size="xs">{move || cards.with(Vec::len).to_string()}</Text>
                                </Group>
                            </header>
                            <Stack gap="xs">
                                {move || ((lane != Some(LANE_SUPPORT)) && cards.with(Vec::is_empty)).then(|| view! {
                                    <Text dimmed=true size="xs">"No items in this column."</Text>
                                })}
                                <For
                                    each=move || cards.get()
                                    key=|card| card.key.clone()
                                    children=move |card: CardModel| {
                                        let CardModel {
                                            kind, short_code, title, meta, work_class,
                                            repository, progress, blocks, key: _,
                                        } = card;
                                        view! {
                                            <ItemCard
                                                kind short_code title meta work_class repository
                                                progress blocks
                                                targets=targets_for_cards.get_value()
                                                source_column=column_for_cards.get_value()
                                                drag powers
                                            />
                                        }
                                    }
                                />
                            </Stack>
                        </section>
                    }
                }
            />
        </div>
    }
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

/// One board card: short code (the detail link, KAIROS-T-0076) with its
/// copy-link button, plain-text title, type, key metadata, and
/// drag-and-drop between columns (KAIROS-T-0064 — draggable only when the
/// board allows moves from here). The keyboard-accessible transition path
/// is the item detail page's move control (KAIROS-T-0075).
#[component]
fn ItemCard(
    kind: EntityKind,
    short_code: String,
    title: String,
    /// `(label, color-token)` pills — per-type key metadata.
    meta: Vec<(String, &'static str)>,
    /// `tasks.work_class` — `Some` for tasks; carried into the drag so
    /// lane drops are decidable (KAIROS-T-0077).
    work_class: Option<String>,
    /// The bound repository's slug (KAIROS-T-0109) — a chip on the card.
    repository: Option<String>,
    /// Children rollup badge (KAIROS-T-0080) — renders only when `Some`.
    progress: Option<data::ProgressCounts>,
    /// Blocked-by/blocks badges (KAIROS-T-0091) — render only when
    /// nonzero; click-through to the item's graph. Neutral accents only —
    /// red stays reserved.
    blocks: Option<data::BlocksCounts>,
    /// `(column_id, column_name)` — the valid targets from this column.
    targets: Vec<(String, String)>,
    /// The column this card currently sits in (the drag's source).
    source_column: String,
    /// The board's in-flight drag; this card writes itself here on
    /// dragstart so legal drop zones light up and accept the drop.
    drag: RwSignal<Option<DragData>>,
    /// The user's powers on this board (KAIROS-T-0072): no transition
    /// power → no drag.
    powers: Signal<BoardPowers>,
) -> impl IntoView {
    let href = format!("/items/{short_code}");
    let code_text = short_code.clone();
    let graph_code = short_code.clone();
    let code_for_copy = short_code.clone();
    let code_for_drag = short_code.clone();
    let code_for_class = short_code;
    // A card is draggable when a column move is offered OR it carries a
    // lane (KAIROS-T-0077: a task in a dead-end column can still move to
    // the other lane of the same column).
    let has_moves = !targets.is_empty() || work_class.is_some();
    let target_ids: Vec<String> = targets.iter().map(|(id, _)| id.clone()).collect();
    let drag_work_class = work_class;
    // Memoized (KAIROS-T-0074): notifies only when the decision actually
    // flips, so refetches can't needlessly rebuild the card's attributes.
    let movable = Memo::new(move |_| has_moves && powers.get().transition);
    view! {
        <article
            class="kairos-card"
            class:kairos-card--dragging=move || {
                drag.with(|d| d.as_ref().is_some_and(|d| d.short_code == code_for_class))
            }
            draggable=move || if movable.get() { "true" } else { "false" }
            on:dragstart=move |ev: web_sys::DragEvent| {
                if !(has_moves && powers.get_untracked().transition) {
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
                    source_column: source_column.clone(),
                    work_class: drag_work_class.clone(),
                }));
            }
            on:dragend=move |_| drag.set(None)
        >
            <Group justify="between" wrap=true>
                <Group gap="xs">
                    // The identifier IS the detail link (KAIROS-T-0076);
                    // `cl-mono` keeps the code scrapeable and mono-set.
                    <a class="cl-mono kairos-card__code" href=href>
                        {code_text}
                    </a>
                    <copy_link::CopyLinkButton code=code_for_copy/>
                </Group>
                <Group gap="xs">
                    {repository.map(|slug| {
                        let attr = slug.clone();
                        view! {
                            <span class="kairos-card__repo" data-repo=attr>
                                <Pill color=token::ICE>{slug}</Pill>
                            </span>
                        }
                    })}
                    <Pill color=kind_color(kind)>{kind.label()}</Pill>
                </Group>
            </Group>
            <span class="kairos-card__title">{title}</span>
            {(!meta.is_empty()).then(|| view! {
                <Group gap="xs" wrap=true>
                    {meta.into_iter().map(|(label, color)| view! {
                        <Pill color=color>{label}</Pill>
                    }).collect_view()}
                </Group>
            })}
            {progress.map(|p| {
                // KAIROS-T-0080: the N-of-M micro-badge. Without done
                // semantics on the children's boards, composition only —
                // never a misleading fraction.
                let percent = if p.has_done && p.total > 0 {
                    (p.done * 100 / p.total).clamp(0, 100)
                } else {
                    0
                };
                let label = if p.has_done {
                    format!("{}/{} done", p.done, p.total)
                } else {
                    format!("{} children", p.total)
                };
                view! {
                    <div class="kairos-card__progress" title="direct children">
                        {p.has_done.then(|| view! {
                            <span class="kairos-progress__bar">
                                <span
                                    class="kairos-progress__fill"
                                    style=format!("width: {percent}%")
                                ></span>
                            </span>
                        })}
                        <Text mono=true dimmed=true size="xs">{label}</Text>
                    </div>
                }
            })}
            {blocks.map(|counts| {
                // KAIROS-T-0091: dependency badges — Jira-style counts,
                // neutral accents (red stays reserved), linking to the
                // item's graph where the web is visible.
                let graph_href = format!("/items/{}?view=graph", graph_code);
                view! {
                    <Group gap="xs" wrap=true>
                        {(counts.blocked_by > 0).then(|| {
                            let href = graph_href.clone();
                            view! {
                                <a class="kairos-card__blocks" href=href title="open the graph">
                                    <Pill color=token::GOLD>
                                        {format!("blocked by {}", counts.blocked_by)}
                                    </Pill>
                                </a>
                            }
                        })}
                        {(counts.blocks > 0).then(|| {
                            let href = graph_href.clone();
                            view! {
                                <a class="kairos-card__blocks" href=href title="open the graph">
                                    <Pill color=token::ICE>
                                        {format!("blocks {}", counts.blocks)}
                                    </Pill>
                                </a>
                            }
                        })}
                    </Group>
                }
            })}
        </article>
    }
}

// ---------------------------------------------------------------------------
// Create flows
// ---------------------------------------------------------------------------

/// The global create flow (KAIROS-T-0062): the board level's entity type
/// with its type-appropriate fields (A-0002 one item family per board
/// level). New items ALWAYS land in the board's entry column — creation
/// is intake; movement happens by transition.
#[component]
fn CreateItemModal(
    open: RwSignal<bool>,
    kind: EntityKind,
    board_id: String,
    /// Delivery boards carry their team; new tasks inherit it.
    team_id: Option<String>,
    /// `(column_id, column_name)` — the board's entry column.
    entry: (String, String),
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let title = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let hypothesis = RwSignal::new(String::new());
    let complexity = RwSignal::new("none".to_string());
    let task_type = RwSignal::new("task".to_string());
    // KAIROS-T-0077: "auto" omits the field — the server defaults the
    // lane (support type → Support lane, else Planned).
    let work_class = RwSignal::new("auto".to_string());
    let decision_maker = RwSignal::new(String::new());
    let decision_date = RwSignal::new(String::new());
    // KAIROS-T-0124 #6b: the repository to issue the task against —
    // "(none)" omits the field. The item page keeps its own picker for
    // re-binding later.
    let repository = RwSignal::new(NO_REPOSITORY.to_string());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);

    // The owning team's repositories, loaded whenever the modal opens on
    // a task board with a team (delivery boards). The select renders only
    // when the team owns at least one.
    let repos_team = StoredValue::new(team_id.clone());
    let repos = LocalResource::new(move || {
        let _ = auth.token();
        let team = (open.get() && matches!(kind, EntityKind::Task))
            .then(|| repos_team.get_value())
            .flatten();
        async move {
            match team {
                Some(team) => repositories::api::list_repositories(auth, Some(&team))
                    .await
                    .map(|list| list.into_iter().map(|r| r.slug).collect::<Vec<String>>()),
                None => Ok(Vec::new()),
            }
        }
    });
    let repo_options = Memo::new(move |_| {
        let slugs = repos
            .get()
            .and_then(|result| result.ok())
            .unwrap_or_default();
        (!slugs.is_empty()).then(|| {
            std::iter::once(NO_REPOSITORY.to_string())
                .chain(slugs)
                .collect::<Vec<String>>()
        })
    });

    // A fresh form every time the modal opens.
    Effect::new(move |_| {
        if open.get() {
            title.set(String::new());
            content.set(String::new());
            hypothesis.set(String::new());
            complexity.set("none".to_string());
            task_type.set("task".to_string());
            work_class.set("auto".to_string());
            decision_maker.set(String::new());
            decision_date.set(String::new());
            repository.set(NO_REPOSITORY.to_string());
            busy.set(false);
            error.set(None);
        }
    });

    let (entry_id, entry_name) = entry;

    // `Callback` is `Copy`: the modal's children (a `Fn` closure) can use
    // it without moving anything out of their environment.
    let submit: Callback<()> = Callback::new({
        let opt = |s: String| (!s.trim().is_empty()).then_some(s);
        move |()| {
            let column_id = entry_id.clone();
            let item = data::NewItem {
                title: title.get_untracked(),
                content: content.get_untracked(),
                hypothesis: opt(hypothesis.get_untracked()),
                complexity: Some(complexity.get_untracked()).filter(|c| c != "none"),
                task_type: Some(task_type.get_untracked()),
                work_class: Some(work_class.get_untracked()).filter(|c| c != "auto"),
                team_id: team_id.clone(),
                repository: Some(repository.get_untracked()).filter(|r| r != NO_REPOSITORY),
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

    let entry_note = StoredValue::new(format!(
        "New {}s start in {entry_name} — the board's intake column.",
        kind.label()
    ));
    view! {
        <Modal open=open title=format!("New {}", kind.label())>
            <Stack gap="sm">
                <Text dimmed=true size="xs">
                    {move || entry_note.get_value()}
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
                        options=vec!["task".into(), "bug".into(), "tech_debt".into(),
                                     "support".into()]/>
                    // KAIROS-T-0077: the Planned/Support lane; "auto"
                    // follows the type (support → Support lane).
                    <Select label="Lane" value=work_class
                        options=vec!["auto".into(), "planned".into(), "support".into()]/>
                    // KAIROS-T-0124 #6b: only when the team owns a repository.
                    {move || repo_options.get().map(|options| view! {
                        <div data-testid="create-repository">
                            <Select label="Repository" value=repository options/>
                        </div>
                    })}
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
    /// `(short_code, title)` of this board's eligible parents — LIVE
    /// (KAIROS-T-0074): the modal instance persists across refetches, so
    /// the option list must follow the board's current items.
    parents: Memo<Vec<(String, String)>>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let title = RwSignal::new(String::new());
    let template = RwSignal::new("(blank)".to_string());
    let parent = RwSignal::new(String::new());
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

    Effect::new(move |_| {
        if open.get() {
            title.set(String::new());
            template.set("(blank)".to_string());
            parent.set(
                parents
                    .get_untracked()
                    .first()
                    .map(|(code, _)| code.clone())
                    .unwrap_or_default(),
            );
            busy.set(false);
            error.set(None);
        }
    });

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
                {move || {
                    let options: Vec<String> = parents
                        .get()
                        .iter()
                        .map(|(code, item_title)| format!("{code} · {item_title}"))
                        .collect();
                    view! { <Select label="Attach to (supports)" value=parent options/> }
                }}
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
            "user": {"id": "u-1", "display_name": "u", "email": "u@x.test"},
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
        let on_team = board_powers(
            &bob,
            "platform-delivery",
            Some("t1"),
            Some(EntityKind::Task),
        );
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

    /// KAIROS-T-0077: the lane is a pure projection of work_class;
    /// non-task kinds render in the default Planned lane.
    #[test]
    fn card_lane_projects_work_class() {
        assert_eq!(card_lane(Some("support")), LANE_SUPPORT);
        assert_eq!(card_lane(Some("planned")), LANE_PLANNED);
        assert_eq!(card_lane(None), LANE_PLANNED);
    }

    /// KAIROS-T-0077 drop semantics: same-column cross-lane = lane write
    /// only (no transition validation); cross-column same-lane =
    /// transition exactly as before; diagonal = both; column legality
    /// stays with the transitions; unlaned cards cannot lane-move.
    #[test]
    fn drop_effect_decides_column_and_lane_moves() {
        let drag = |targets: &[&str], work_class: Option<&str>| DragData {
            kind: EntityKind::Task,
            short_code: "T-1".into(),
            targets: targets.iter().map(|s| s.to_string()).collect(),
            source_column: "c-src".into(),
            work_class: work_class.map(str::to_string),
        };
        // Same column, same lane: no-op.
        assert_eq!(
            drop_effect(&drag(&["c-2"], Some("planned")), "c-src", Some("planned")),
            None
        );
        // Same column, other lane: lane write only — legal even from a
        // dead-end column (no targets).
        let effect =
            drop_effect(&drag(&[], Some("planned")), "c-src", Some("support")).expect("legal");
        assert_eq!(effect.transition_to, None);
        assert_eq!(effect.set_work_class.as_deref(), Some("support"));
        // Cross column, same lane: transition exactly as before.
        let effect =
            drop_effect(&drag(&["c-2"], Some("support")), "c-2", Some("support")).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        assert_eq!(effect.set_work_class, None);
        // Diagonal: both writes.
        let effect =
            drop_effect(&drag(&["c-2"], Some("planned")), "c-2", Some("support")).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        assert_eq!(effect.set_work_class.as_deref(), Some("support"));
        // A column outside the transition targets stays illegal, diagonal
        // or not — lane never affects transition legality.
        assert_eq!(
            drop_effect(&drag(&["c-2"], Some("planned")), "c-3", Some("support")),
            None
        );
        // Unlaned rows (non-delivery boards): pure column semantics.
        let effect = drop_effect(&drag(&["c-2"], None), "c-2", None).expect("legal");
        assert_eq!(effect.transition_to.as_deref(), Some("c-2"));
        assert_eq!(drop_effect(&drag(&["c-2"], None), "c-src", None), None);
        // A card without a lane cannot lane-move.
        assert_eq!(
            drop_effect(&drag(&[], None), "c-src", Some("support")),
            None
        );
    }

    fn card(kind: EntityKind, repository: Option<&str>) -> CardModel {
        CardModel {
            kind,
            short_code: "X-1".into(),
            title: "x".into(),
            meta: Vec::new(),
            work_class: None,
            repository: repository.map(str::to_string),
            progress: None,
            blocks: None,
            key: String::new(),
        }
    }

    fn slugs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// KAIROS-T-0114: `?repo=` parses to a deduplicated slug list and the
    /// effective selection is pruned to what the board can show — a stale
    /// slug is dropped (never a blank board), and the pruned form differs
    /// from the request exactly when a write-back is due.
    #[test]
    fn selected_repos_parse_and_prune_against_the_board() {
        assert!(parse_repo_query(None).is_empty());
        assert!(parse_repo_query(Some("")).is_empty());
        assert_eq!(parse_repo_query(Some(",a,,b,a,")), slugs(&["a", "b"]));

        let known = slugs(&["payments-api", "platform-infra"]);
        let requested = parse_repo_query(Some("platform-infra,gone,payments-api"));
        assert_eq!(
            prune_repos(&requested, &known),
            slugs(&["platform-infra", "payments-api"])
        );
        // Entirely stale → empty selection (the URL is then cleared).
        assert!(prune_repos(&slugs(&["gone"]), &known).is_empty());
        // Nothing on the board → nothing selectable.
        assert!(prune_repos(&requested, &[]).is_empty());
        // A clean request round-trips unchanged (no write-back).
        let clean = slugs(&["payments-api"]);
        assert_eq!(prune_repos(&clean, &known), clean);
    }

    /// KAIROS-T-0109: a repo lane admits exactly its tasks; the unbound
    /// lane takes repository-less cards (tasks and other kinds alike).
    #[test]
    fn repo_lane_admits_by_binding() {
        let bound = card(EntityKind::Task, Some("payments-api"));
        let other = card(EntityKind::Task, Some("platform-infra"));
        let unbound = card(EntityKind::Task, None);
        let adr = card(EntityKind::Adr, None);

        let lane = RepoLane::Slug("payments-api".into());
        assert!(lane.admits(&bound));
        assert!(!lane.admits(&other));
        assert!(!lane.admits(&unbound));
        assert!(!lane.admits(&adr));

        assert!(!RepoLane::Unbound.admits(&bound));
        assert!(RepoLane::Unbound.admits(&unbound));
        assert!(RepoLane::Unbound.admits(&adr));

        assert!(RepoLane::Any.admits(&bound));
        assert!(RepoLane::Any.admits(&adr));
    }

    /// KAIROS-T-0109: the lens retain filter narrows TASKS only — an
    /// empty selection admits everything, and non-task kinds always stay.
    #[test]
    fn lens_filter_narrows_tasks_only() {
        let selected = slugs(&["payments-api"]);
        assert!(lens_admits(
            &card(EntityKind::Task, Some("payments-api")),
            &selected
        ));
        assert!(!lens_admits(
            &card(EntityKind::Task, Some("platform-infra")),
            &selected
        ));
        assert!(!lens_admits(&card(EntityKind::Task, None), &selected));
        assert!(lens_admits(&card(EntityKind::Initiative, None), &selected));
        assert!(lens_admits(&card(EntityKind::Adr, None), &selected));
        assert!(lens_admits(&card(EntityKind::Task, None), &[]));
        assert!(lens_admits(
            &card(EntityKind::Task, Some("platform-infra")),
            &[]
        ));
    }

    /// KAIROS-T-0114: group-by lanes come from the EFFECTIVE selection
    /// (all board repositories when none is selected), in board order,
    /// plus the unbound remainder — never a lane for a chip that is off.
    #[test]
    fn repo_lanes_follow_the_effective_selection() {
        let known = slugs(&["payments-api", "platform-infra"]);
        assert_eq!(
            repo_lanes(&[], &known),
            vec![
                Some("payments-api".to_string()),
                Some("platform-infra".to_string()),
                None
            ]
        );
        assert_eq!(
            repo_lanes(&slugs(&["platform-infra"]), &known),
            vec![Some("platform-infra".to_string()), None]
        );
        // One repository on the board: a single repo lane + unbound.
        assert_eq!(
            repo_lanes(&[], &slugs(&["only"])),
            vec![Some("only".to_string()), None]
        );
        // No repositories at all: just the unbound lane.
        assert_eq!(repo_lanes(&[], &[]), vec![None]);
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
