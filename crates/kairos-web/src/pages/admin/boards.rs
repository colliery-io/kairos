//! `/admin/boards` (list, create, delete) and `/admin/boards/:board`
//! (columns, transitions, members + capability grants) — KAIROS-T-0043.
//!
//! Board-config rule violations arrive as typed 422s (`COLUMN_NOT_EMPTY`,
//! `DUPLICATE_COLUMN_NAME`, `DUPLICATE_COLUMN_POSITION`,
//! `DUPLICATE_TRANSITION`, `BOARD_NOT_EMPTY`) and render through
//! [`super::MutationNotice`] → `ErrorState`, which shows the server's
//! message plus the `code:` line — components never inspect statuses.

use aurora_dark::components::{
    Anchor, Button, Code, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill,
    Select, Stack, Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use super::api;
use super::capabilities::{CapabilityEditor, CapabilityPills, EditorState};
use super::gating;
use super::{
    BOARD_FORM_FIELDS, MutationNotice, MutationOutcome, PREFIX_HINT, SLUG_HINT, refusal_for,
    run_form_mutation, run_mutation,
};
use crate::auth::use_auth;

const LEVELS: [&str; 4] = ["strategy", "initiative", "delivery", "adr"];

/// The level of a delivery board. Every other level is a board of the
/// organization.
const DELIVERY: &str = "delivery";

/// The level of an ADR board. It can have a team (COLLIERY-T-3102).
const ADR: &str = "adr";

/// What the create form says when a delivery board has no team
/// (COLLIERY-T-0230). The server refuses the same request with a 422. The
/// form says it first, in the words of the form, and sends nothing.
const SELECT_TEAM: &str = "Select the team that owns the board. A delivery board must \
                           have a team.";

/// Why the create form cannot send this board, or `None` when it can
/// (COLLIERY-T-0230). A delivery board needs a team. A board of the
/// organization has no delivery team, so nothing is necessary for it.
/// Pure, host-tested.
fn create_refusal(level: &str, team_id: Option<&str>) -> Option<&'static str> {
    (level == DELIVERY && team_id.is_none()).then_some(SELECT_TEAM)
}

/// The team to send with a new board. A delivery board has a delivery team
/// (COLLIERY-T-0230), and an ADR board can have a team (COLLIERY-T-3102).
/// The team control stays set after the level changes, so the level
/// decides, not the control. Pure, host-tested.
fn team_for_level(level: &str, team_id: Option<String>) -> Option<String> {
    team_id.filter(|_| level == DELIVERY || level == ADR)
}

/// The label of the team control: required for a delivery board, optional
/// for an ADR board (COLLIERY-T-3102). Pure, host-tested.
fn team_label(level: &str) -> &'static str {
    if level == ADR {
        "Team (optional; the ADR board of a team has the prefix of the team)"
    } else {
        "Owner team (required)"
    }
}

/// The title and the caption of the members panel of a board
/// (COLLIERY-T-0230). Every board has a team. The team of a delivery board
/// is its delivery team, which has a page of its own, so the panel keeps
/// its words. The team of a board of the organization is the list of the
/// members of the board: the panel IS the team, and says so. Pure,
/// host-tested.
fn members_panel_words(level: &str) -> (&'static str, &'static str) {
    if level == DELIVERY {
        (
            "Members and capabilities",
            "A person can write only with a capability grant (A-0006). Each member can read.",
        )
    } else {
        (
            "Team of this board",
            "The members of this board are its team. To put a person in the team, add the \
             person to the board.",
        )
    }
}

/// What the members panel says when the board has no member
/// (COLLIERY-T-0230). Pure, host-tested.
fn no_members_message(level: &str) -> &'static str {
    if level == DELIVERY {
        "This board has no capability grants. Add a member below."
    } else {
        "This board has no team members. Add a member below."
    }
}

/// The name of the team, when the server refuses the delete of this board
/// with `LAST_DELIVERY_BOARD` (COLLIERY-T-0251). The server refuses the
/// delete of the only live delivery board of a live team
/// (COLLIERY-T-0241). `boards` and `teams` are the live lists that the page
/// has. A team of old data can have 2 or more delivery boards, and the
/// delete of one of them is permitted. Pure, host-tested.
fn only_delivery_board_of(
    board: &api::Board,
    boards: &[api::Board],
    teams: &[api::Team],
) -> Option<String> {
    if board.board_level != DELIVERY {
        return None;
    }
    let team_id = board.team_id.as_deref()?;
    let team = teams.iter().find(|team| team.id == team_id)?;
    let delivery_boards = boards
        .iter()
        .filter(|other| other.board_level == DELIVERY)
        .filter(|other| other.team_id.as_deref() == Some(team_id))
        .count();
    (delivery_boards == 1).then(|| team.name.clone())
}

/// What the list says in the place of a delete that the server refuses
/// (COLLIERY-T-0251). Pure, host-tested.
fn only_delivery_board_note(team_name: &str) -> String {
    format!(
        "This board is the only delivery board of the team \"{team_name}\". \
         To remove the board, delete the team."
    )
}

/// `/admin/boards` — every live board, plus create/delete.
#[component]
pub fn AdminBoardsPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome = RwSignal::new(None);
    let busy = RwSignal::new(false);

    let boards = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_boards(auth)
    });
    let teams = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_teams(auth)
    });
    // Board create/delete are org-admin-only (tenant-level lifecycle,
    // A-0006); the board LIST and per-board config links stay open to
    // capability holders admitted by the shell gate (KAIROS-T-0052). The
    // server enforces this regardless — the gate below is UX only.
    let whoami = LocalResource::new(move || {
        let _ = auth.token();
        crate::api::whoami(auth)
    });
    let is_admin = move || matches!(whoami.get(), Some(Ok(me)) if gating::is_org_admin(&me));

    // Create form state.
    let name = RwSignal::new(String::new());
    let slug = RwSignal::new(String::new());
    let code_prefix = RwSignal::new(String::new());
    let level = RwSignal::new("initiative".to_string());
    let team_slug = RwSignal::new(String::new());
    // COLLIERY-T-0265: the refusal of the create form that is about a
    // field. It shows below the field.
    let refused: RwSignal<Option<api::Refusal>> = RwSignal::new(None);

    let on_create = move |_| {
        let team_id = teams
            .get()
            .and_then(|result| result.ok())
            .and_then(|teams| {
                teams
                    .iter()
                    .find(|team| team.slug == team_slug.get_untracked())
                    .map(|team| team.id.clone())
            });
        let (n, s, l, p) = (
            name.get_untracked(),
            slug.get_untracked(),
            level.get_untracked(),
            code_prefix.get_untracked(),
        );
        let team_id = team_for_level(&l, team_id);
        // COLLIERY-T-0230: a delivery board needs a team. The server
        // refuses the request too; the form does not send it.
        if let Some(message) = create_refusal(&l, team_id.as_deref()) {
            outcome.set(Some(Err(aurora_dark::tokens::ApiError::Unknown(
                message.to_string(),
            ))));
            return;
        }
        run_form_mutation(
            busy,
            outcome,
            reload,
            refused,
            &BOARD_FORM_FIELDS,
            format!("Kairos made the board \"{n}\" with the default columns of the level {l}."),
            async move {
                api::create_board(auth, &n, &s, &l, team_id.as_deref(), &p)
                    .await
                    .map(|_| ())
            },
        );
    };

    view! {
        <PageHeader title="Boards" sub="Make, delete, and configure the boards."/>
        <Stack gap="md">
            <MutationNotice outcome/>
            <Panel title="All boards" caption="Click a board to configure it.">
                {move || match boards.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(items)) if items.is_empty() => view! {
                        <Empty message="The organization has no boards. Make one below."/>
                    }.into_any(),
                    Some(Ok(items)) => {
                        let live_teams = teams.get().and_then(|result| result.ok()).unwrap_or_default();
                        let all = items.clone();
                        // COLLIERY-T-1836: a gap between the rows.
                        let rows = items.into_iter().map(move |board| {
                        let config_href = format!("/admin/boards/{}", board.id);
                        // COLLIERY-T-0251: the server refuses the delete of
                        // the only delivery board of a team. The page says
                        // so, and does not offer the delete.
                        let refused_for = StoredValue::new(
                            only_delivery_board_of(&board, &all, &live_teams),
                        );
                        // StoredValue keeps the delete handler Copy so the
                        // admin-gated `Show` child (a `Fn`) can rebuild it.
                        let board_id = StoredValue::new(board.id.clone());
                        let board_name = StoredValue::new(board.name.clone());
                        view! {
                            <Group justify="between" wrap=true>
                                <Group gap="sm">
                                    <Anchor href=config_href>{board.name.clone()}</Anchor>
                                    <Code>{board.slug.clone()}</Code>
                                    <Pill color=token::TEAL>{board.board_level.clone()}</Pill>
                                    // COLLIERY-T-3099: the prefix of the codes.
                                    <Code attr:data-testid="board-prefix">
                                        {board.code_prefix.clone()}
                                    </Code>
                                </Group>
                                // Delete is org-admin-only (KAIROS-T-0052).
                                <Show when=move || is_admin()>
                                    {
                                        let on_delete = move |_| {
                                            let board_id = board_id.get_value();
                                            let board_name = board_name.get_value();
                                            run_mutation(
                                                busy, outcome, reload,
                                                format!("Kairos deleted the board \
                                                         \"{board_name}\"."),
                                                async move {
                                                    api::delete_board(auth, &board_id).await.map(|_| ())
                                                },
                                            );
                                        };
                                        match refused_for.get_value() {
                                            Some(team_name) => view! {
                                                <Group gap="sm" wrap=true>
                                                    <Text size="xs" dimmed=true>
                                                        {only_delivery_board_note(&team_name)}
                                                    </Text>
                                                    <Anchor href="/admin/teams">"Open the teams"</Anchor>
                                                    <Button variant="default" size="xs" bad=true
                                                        disabled=true>
                                                        "Delete"
                                                    </Button>
                                                </Group>
                                            }.into_any(),
                                            None => view! {
                                                <Button variant="default" size="xs" bad=true
                                                    on_click=Callback::new(on_delete)>
                                                    "Delete"
                                                </Button>
                                            }.into_any(),
                                        }
                                    }
                                </Show>
                            </Group>
                        }
                    }).collect_view();
                        view! { <Stack gap="sm">{rows}</Stack> }.into_any()
                    }
                }}
            </Panel>
            // Board creation is org-admin-only (tenant-level, A-0006);
            // capability holders configure existing boards but do not create
            // them (KAIROS-T-0052).
            <Show when=move || is_admin()>
                <Panel title="Create board" caption="A new board has the default columns \
                                                     and transitions of its level.">
                    <Stack gap="sm">
                        <Group gap="sm" wrap=true top=true>
                            <TextInput label="Name" value=name placeholder="e.g. Platform Initiatives"/>
                            {move || view! {
                                <TextInput label="Slug" value=slug
                                    placeholder="e.g. platform-initiatives"
                                    error=refusal_for(refused, "slug")/>
                            }}
                            {move || view! {
                                <TextInput label="Prefix" value=code_prefix
                                    placeholder="e.g. PLAT"
                                    error=refusal_for(refused, "code_prefix")/>
                            }}
                            <Select label="Level" options=LEVELS.iter().map(|l| l.to_string()).collect() value=level/>
                        </Group>
                        <Text dimmed=true size="xs" attr:data-testid="slug-rule">{SLUG_HINT}</Text>
                        <Text dimmed=true size="xs" attr:data-testid="prefix-rule">{PREFIX_HINT}</Text>
                        <Show when=move || matches!(level.get().as_str(), DELIVERY | ADR)>
                            {move || {
                                let options = match teams.get() {
                                    Some(Ok(teams)) => {
                                        let mut slugs: Vec<String> =
                                            teams.iter().map(|team| team.slug.clone()).collect();
                                        slugs.insert(0, String::new());
                                        slugs
                                    }
                                    _ => vec![String::new()],
                                };
                                view! {
                                    <Select label=team_label(&level.get()) options value=team_slug/>
                                }
                            }}
                        </Show>
                        <Group>
                            <Button on_click=Callback::new(on_create)>"Create board"</Button>
                        </Group>
                    </Stack>
                </Panel>
            </Show>
        </Stack>
    }
}

/// `/admin/boards/:board` — one board's configuration: columns (add /
/// rename / reorder / remove), transitions (add / remove), and members
/// with A-0006 capability grants.
#[component]
pub fn AdminBoardPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let board_id = Memo::new(move |_| params.read().get("board").unwrap_or_default());

    let reload = RwSignal::new(0u32);
    let outcome = RwSignal::new(None);
    let busy = RwSignal::new(false);
    // The selection of the transition form. It is here and not in
    // `TransitionsPanel`, because each reload makes a new panel: a
    // selection in the panel went back to the first columns when a
    // mutation completed (COLLIERY-T-0238).
    let from_name = RwSignal::new(String::new());
    let to_name = RwSignal::new(String::new());

    let detail = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        let id = board_id.get();
        async move { api::board_detail(auth, &id).await }
    });

    view! {
        {move || match detail.get() {
            None => view! { <Loading label="Loading board…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
            }.into_any(),
            Some(Ok(detail)) => {
                let id = detail.board.id.clone();
                let level = detail.board.board_level.clone();
                view! {
                    <PageHeader title=detail.board.name.clone() sub="board configuration"/>
                    <Stack gap="md">
                        <Group gap="sm">
                            <Code>{detail.board.slug.clone()}</Code>
                            <Pill color=token::TEAL>{detail.board.board_level.clone()}</Pill>
                            // COLLIERY-T-3099: the prefix of the codes.
                            <Text size="xs" dimmed=true attr:data-testid="board-prefix">
                                {format!("Prefix {}", detail.board.code_prefix)}
                            </Text>
                            <Anchor href="/admin/boards">"All boards"</Anchor>
                        </Group>
                        <MutationNotice outcome/>
                        <ColumnsPanel board_id=id.clone() columns=detail.columns.clone()
                            transitions=detail.transitions.clone() busy outcome reload/>
                        <TransitionsPanel board_id=id.clone() columns=detail.columns.clone()
                            transitions=detail.transitions.clone() busy outcome reload
                            from_name to_name/>
                        <MembersPanel board_id=id board_level=level
                            busy outcome reload/>
                    </Stack>
                }.into_any()
            }
        }}
    }
}

/// Columns: position-ordered rows with rename / move / remove / done
/// toggle (KAIROS-T-0080), plus an append form. Non-empty-column removals
/// surface the server's `COLUMN_NOT_EMPTY` 422 (message includes the live
/// item count). The dead-end heuristic only ever SUGGESTS the done flag
/// (a gold "dead end" pill) — marking is an explicit admin click.
#[component]
fn ColumnsPanel(
    board_id: String,
    columns: Vec<api::BoardColumn>,
    transitions: Vec<api::BoardTransition>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let last = columns.len().saturating_sub(1) as i32;
    let new_name = RwSignal::new(String::new());
    let board = StoredValue::new(board_id);

    let on_add = move |_| {
        let board_id = board.get_value();
        let name = new_name.get_untracked();
        let position = last + 1;
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Kairos added the column \"{name}\"."),
            async move {
                api::add_column(auth, &board_id, &name, position)
                    .await
                    .map(|_| ())
            },
        );
    };

    let rows = columns
        .into_iter()
        .map(|column| {
            let rename = RwSignal::new(column.name.clone());
            let position = column.position;
            let is_done = column.is_done;
            // The heuristic suggestion (KAIROS-T-0080): a column with no
            // outbound transitions is either terminal or misconfigured —
            // surface it, never act on it.
            let is_dead_end = !transitions.iter().any(|t| t.from_column_id == column.id);
            let column_id = StoredValue::new(column.id);
            let on_toggle_done = move |_| {
                let board_id = board.get_value();
                let column_id = column_id.get_value();
                run_mutation(
                    busy,
                    outcome,
                    reload,
                    format!(
                        "Kairos marked the column as {}.",
                        if is_done { "not done" } else { "done" }
                    ),
                    async move {
                        api::update_column(auth, &board_id, &column_id, None, None, Some(!is_done))
                            .await
                            .map(|_| ())
                    },
                );
            };
            let on_rename = move |_| {
                let board_id = board.get_value();
                let column_id = column_id.get_value();
                let name = rename.get_untracked();
                run_mutation(
                    busy,
                    outcome,
                    reload,
                    format!("Kairos changed the name of the column to \"{name}\"."),
                    async move {
                        api::update_column(auth, &board_id, &column_id, Some(&name), None, None)
                            .await
                            .map(|_| ())
                    },
                );
            };
            let move_to = move |target: i32| {
                if target < 0 || target > last {
                    return; // already at the edge — nothing to move
                }
                let board_id = board.get_value();
                let column_id = column_id.get_value();
                run_mutation(
                    busy,
                    outcome,
                    reload,
                    format!("Kairos moved the column to the position {target}."),
                    async move {
                        api::update_column(auth, &board_id, &column_id, None, Some(target), None)
                            .await
                            .map(|_| ())
                    },
                );
            };
            let on_remove = move |_| {
                let board_id = board.get_value();
                let column_id = column_id.get_value();
                run_mutation(
                    busy,
                    outcome,
                    reload,
                    "Kairos removed the column.".to_string(),
                    async move {
                        api::remove_column(auth, &board_id, &column_id)
                            .await
                            .map(|_| ())
                    },
                );
            };
            view! {
                <Group justify="between" wrap=true>
                    <Group gap="sm">
                        <Pill color=token::ICE>{position}</Pill>
                        <TextInput value=rename/>
                        <Button variant="default" size="xs"
                            on_click=Callback::new(on_rename)>"Rename"</Button>
                        {is_done.then(|| view! {
                            <Pill color=token::OK>"done"</Pill>
                        })}
                        {(is_dead_end && !is_done).then(|| view! {
                            <Pill color=token::GOLD>"dead end"</Pill>
                        })}
                    </Group>
                    <Group gap="xs">
                        <Button variant="default" size="xs"
                            on_click=Callback::new(on_toggle_done)>
                            {if is_done { "Unmark done" } else { "Mark done" }}
                        </Button>
                        <Button variant="default" size="xs"
                            on_click=Callback::new(move |_| move_to(position - 1))>
                            "Up"
                        </Button>
                        <Button variant="default" size="xs"
                            on_click=Callback::new(move |_| move_to(position + 1))>
                            "Down"
                        </Button>
                        <Button variant="default" size="xs" bad=true
                            on_click=Callback::new(on_remove)>
                            "Remove"
                        </Button>
                    </Group>
                </Group>
            }
        })
        .collect_view();

    view! {
        <Panel title="Columns" caption="The board shows the columns in this order, from \
                                        left to right.">
            <Stack gap="sm">
                {rows}
                <Divider/>
                <Group gap="sm">
                    <TextInput value=new_name placeholder="New column name"/>
                    <Button on_click=Callback::new(on_add)>"Add column"</Button>
                </Group>
            </Stack>
        </Panel>
    }
}

/// The new value of a selection of the transition form, or `None` when
/// the selection stays (COLLIERY-T-0238). The selection stays when it
/// names a column of the board. If not, it becomes the column at
/// `default_index`, or empty for a board with too few columns.
fn kept_or_default(names: &[String], selected: &str, default_index: usize) -> Option<String> {
    if names.iter().any(|name| name == selected) {
        return None;
    }
    Some(names.get(default_index).cloned().unwrap_or_default())
}

/// Transition edges: which column-to-column moves the board allows.
#[component]
fn TransitionsPanel(
    board_id: String,
    columns: Vec<api::BoardColumn>,
    transitions: Vec<api::BoardTransition>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
    from_name: RwSignal<String>,
    to_name: RwSignal<String>,
) -> impl IntoView {
    let auth = use_auth();
    let board = StoredValue::new(board_id);
    let column_name = {
        let columns = columns.clone();
        move |id: &str| {
            columns
                .iter()
                .find(|column| column.id == id)
                .map(|column| column.name.clone())
                .unwrap_or_else(|| id.to_string())
        }
    };

    let names: Vec<String> = columns.iter().map(|column| column.name.clone()).collect();
    // The page owns the selection, so it stays across a reload
    // (COLLIERY-T-0238). A selection that names no column of the board
    // (the first load, or a column that was renamed or removed) goes to
    // the default.
    if let Some(name) = kept_or_default(&names, &from_name.get_untracked(), 0) {
        from_name.set(name);
    }
    if let Some(name) = kept_or_default(&names, &to_name.get_untracked(), 1) {
        to_name.set(name);
    }
    let by_name = StoredValue::new(
        columns
            .iter()
            .map(|column| (column.name.clone(), column.id.clone()))
            .collect::<Vec<_>>(),
    );
    let id_of = move |name: &str| {
        by_name
            .get_value()
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| id.clone())
    };

    let on_add = move |_| {
        let board_id = board.get_value();
        let from = from_name.get_untracked();
        let to = to_name.get_untracked();
        let (Some(from_id), Some(to_id)) = (id_of(&from), id_of(&to)) else {
            return;
        };
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Kairos added the transition from {from} to {to}."),
            async move {
                api::add_transition(auth, &board_id, &from_id, &to_id)
                    .await
                    .map(|_| ())
            },
        );
    };

    let rows = transitions
        .into_iter()
        .map(|transition| {
            let from = column_name(&transition.from_column_id);
            let to = column_name(&transition.to_column_id);
            let transition_id = StoredValue::new(transition.id);
            let label = format!("{from} → {to}");
            let done = label.clone();
            let on_remove = move |_| {
                let board_id = board.get_value();
                let transition_id = transition_id.get_value();
                let done = done.clone();
                run_mutation(
                    busy,
                    outcome,
                    reload,
                    format!("Kairos removed the transition {done}."),
                    async move {
                        api::remove_transition(auth, &board_id, &transition_id)
                            .await
                            .map(|_| ())
                    },
                );
            };
            view! {
                <Group justify="between" wrap=true>
                    <Text mono=true size="sm">{label}</Text>
                    <Button variant="default" size="xs" bad=true
                        on_click=Callback::new(on_remove)>
                        "Remove"
                    </Button>
                </Group>
            }
        })
        .collect_view();

    view! {
        <Panel title="Transitions" caption="A card can move only along a transition of \
                                            this list.">
            <Stack gap="sm">
                {rows}
                <Divider/>
                <Group gap="sm" wrap=true>
                    <Select label="From" options=names.clone() value=from_name/>
                    <Select label="To" options=names.clone() value=to_name/>
                    <Button on_click=Callback::new(on_add)>"Add transition"</Button>
                </Group>
            </Stack>
        </Panel>
    }
}

/// Board members and their A-0006 capability grants: list with pills, an
/// inline grant editor per member (PATCH replaces the full set), removal,
/// and an add-member flow (org-member picker + the same grant editor).
///
/// On a board of the organization the panel is the team of the board
/// (COLLIERY-T-0230), and its title and caption say so: see
/// [`members_panel_words`]. The mechanics are the same for every level.
#[component]
fn MembersPanel(
    board_id: String,
    board_level: String,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let board = StoredValue::new(board_id);

    let members = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        let id = board.get_value();
        async move { api::board_members(auth, &id).await }
    });
    let org_members = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_org_members(auth)
    });

    // Add-member flow state.
    let add_email = RwSignal::new(String::new());
    let add_editor = EditorState::new();

    let on_add = move |_| {
        let email = add_email.get_untracked();
        let user_id = org_members
            .get()
            .and_then(|result| result.ok())
            .and_then(|members| {
                members
                    .iter()
                    .find(|member| member.email == email)
                    .map(|member| member.user_id.clone())
            });
        let Some(user_id) = user_id else {
            outcome.set(Some(Err(aurora_dark::tokens::ApiError::Unknown(
                "Select a member of the organization.".to_string(),
            ))));
            return;
        };
        let capabilities = add_editor.selection();
        if capabilities.is_empty() {
            outcome.set(Some(Err(aurora_dark::tokens::ApiError::Unknown(
                "Select one capability or more. A person can write only with a capability \
                 grant (A-0006)."
                    .to_string(),
            ))));
            return;
        }
        let board_id = board.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!(
                "Kairos added {email} with these capabilities: {}.",
                capabilities.join(", ")
            ),
            async move {
                api::add_board_member(auth, &board_id, &user_id, &capabilities)
                    .await
                    .map(|_| ())
            },
        );
    };

    let (title, caption) = members_panel_words(&board_level);
    let empty_message = no_members_message(&board_level);

    view! {
        <Panel title=title caption=caption>
            <Stack gap="md">
                {move || match members.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(list)) if list.is_empty() => view! {
                        <Empty message=empty_message/>
                    }.into_any(),
                    Some(Ok(list)) => list.into_iter().map(|member| {
                        let editor = EditorState::from_capabilities(&member.capabilities);
                        let editing = RwSignal::new(false);
                        let user = StoredValue::new(member.user_id);
                        let email = member.email.clone();
                        // StoredValue keeps the handlers Copy — Show children
                        // must stay `Fn`, so nothing owned may move into them.
                        let member_email = StoredValue::new(email.clone());
                        let on_save = move |_| {
                            let capabilities = editor.selection();
                            if capabilities.is_empty() {
                                outcome.set(Some(Err(aurora_dark::tokens::ApiError::Unknown(
                                    "A member must have one capability or more. To remove \
                                     the member from the board, use Remove."
                                        .to_string(),
                                ))));
                                return;
                            }
                            let board_id = board.get_value();
                            let user_id = user.get_value();
                            let saved_email = member_email.get_value();
                            run_mutation(
                                busy, outcome, reload,
                                format!("{saved_email} has these capabilities: {}.", capabilities.join(", ")),
                                async move {
                                    api::replace_capabilities(auth, &board_id, &user_id, &capabilities)
                                        .await
                                        .map(|_| ())
                                },
                            );
                        };
                        let on_remove = move |_| {
                            let board_id = board.get_value();
                            let user_id = user.get_value();
                            let removed_email = member_email.get_value();
                            run_mutation(
                                busy, outcome, reload,
                                format!("Kairos removed {removed_email} from the board. \
                                         The person has no grants on the board."),
                                async move {
                                    api::remove_board_member(auth, &board_id, &user_id)
                                        .await
                                        .map(|_| ())
                                },
                            );
                        };
                        view! {
                            <Stack gap="xs">
                                <Group justify="between" wrap=true>
                                    <Group gap="sm">
                                        <Text bright=true>{member.display_name.clone()}</Text>
                                        <Text dimmed=true size="sm">{email.clone()}</Text>
                                        <CapabilityPills capabilities=member.capabilities.clone()/>
                                    </Group>
                                    <Group gap="xs">
                                        <Button variant="default" size="xs"
                                            on_click=Callback::new(move |_| editing.update(|open| *open = !*open))>
                                            {move || if editing.get() { "Close" } else { "Edit grants" }}
                                        </Button>
                                        <Button variant="default" size="xs" bad=true
                                            on_click=Callback::new(on_remove)>
                                            "Remove"
                                        </Button>
                                    </Group>
                                </Group>
                                <Show when=move || editing.get()>
                                    <Stack gap="sm">
                                        <CapabilityEditor state=editor/>
                                        <Group>
                                            <Button on_click=Callback::new(on_save)>
                                                "Save grants"
                                            </Button>
                                        </Group>
                                    </Stack>
                                </Show>
                                <Divider/>
                            </Stack>
                        }
                    }).collect_view().into_any(),
                }}
                <Text bright=true bold=true size="sm">"Add member"</Text>
                {move || {
                    let taken: Vec<String> = members
                        .get()
                        .and_then(|result| result.ok())
                        .map(|list| list.into_iter().map(|member| member.email).collect())
                        .unwrap_or_default();
                    let mut options: Vec<String> = org_members
                        .get()
                        .and_then(|result| result.ok())
                        .map(|list| {
                            list.into_iter()
                                .map(|member| member.email)
                                .filter(|email| !taken.contains(email))
                                .collect()
                        })
                        .unwrap_or_default();
                    options.insert(0, String::new());
                    view! {
                        <Select label="Organization member" options value=add_email/>
                    }
                }}
                <CapabilityEditor state=add_editor/>
                <Group>
                    <Button on_click=Callback::new(on_add)>"Add member with grants"</Button>
                </Group>
            </Stack>
        </Panel>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COLLIERY-T-0230: the form does not send a delivery board with no
    /// team, and the refusal names the missing team.
    #[test]
    fn a_delivery_board_with_no_team_is_not_sent() {
        let refusal = create_refusal("delivery", None).expect("a refusal");
        assert!(refusal.contains("team"), "{refusal}");
        assert_eq!(create_refusal("delivery", Some("t1")), None);
    }

    /// A board of the organization has no delivery team, so the form asks
    /// for none and sends none, whatever the team control holds.
    #[test]
    fn a_board_of_the_organization_is_sent_with_no_team() {
        for level in ["strategy", "initiative"] {
            assert_eq!(create_refusal(level, None), None, "{level}");
            assert_eq!(team_for_level(level, Some("t1".into())), None, "{level}");
        }
        assert_eq!(
            team_for_level("delivery", Some("t1".into())),
            Some("t1".to_string())
        );
    }

    /// COLLIERY-T-3102: an ADR board can have a team, and the team is
    /// optional.
    #[test]
    fn an_adr_board_is_sent_with_its_team_or_with_none() {
        assert_eq!(create_refusal("adr", None), None);
        assert_eq!(
            team_for_level("adr", Some("t1".into())),
            Some("t1".to_string())
        );
        assert_eq!(team_for_level("adr", None), None);
        assert!(team_label("adr").contains("optional"));
        assert!(team_label("delivery").contains("required"));
    }

    /// COLLIERY-T-0230: on a board of the organization the members are the
    /// team of the board, and the panel says so. A delivery board keeps the
    /// words it had: its team is the delivery team.
    #[test]
    fn the_members_of_a_board_of_the_organization_are_its_team() {
        for level in ["strategy", "initiative", "adr"] {
            let (title, caption) = members_panel_words(level);
            assert_eq!(title, "Team of this board", "{level}");
            assert!(
                caption.contains("members of this board are its team"),
                "{level}: {caption}"
            );
            assert!(no_members_message(level).contains("team"), "{level}");
        }
        let (title, caption) = members_panel_words("delivery");
        assert_eq!(title, "Members and capabilities");
        assert!(
            caption.contains("only with a capability grant"),
            "{caption}"
        );
        assert_eq!(
            no_members_message("delivery"),
            "This board has no capability grants. Add a member below."
        );
    }

    fn board(id: &str, level: &str, team_id: Option<&str>) -> api::Board {
        api::Board {
            id: id.to_string(),
            name: id.to_string(),
            slug: id.to_string(),
            board_level: level.to_string(),
            team_id: team_id.map(str::to_string),
            code_prefix: id.to_uppercase(),
        }
    }

    fn team(id: &str, name: &str) -> api::Team {
        api::Team {
            id: id.to_string(),
            name: name.to_string(),
            slug: name.to_lowercase(),
            team_type: "stream_aligned".to_string(),
            delivery_board_id: None,
        }
    }

    /// COLLIERY-T-0251: the page does not offer the delete of the only
    /// delivery board of a team, and the note names the team.
    #[test]
    fn the_only_delivery_board_of_a_team_has_no_delete() {
        let teams = [team("t1", "Platform"), team("t2", "Web")];
        let boards = [
            board("b1", "delivery", Some("t1")),
            board("b2", "delivery", Some("t2")),
            board("b3", "initiative", None),
        ];
        assert_eq!(
            only_delivery_board_of(&boards[0], &boards, &teams),
            Some("Platform".to_string())
        );
        assert_eq!(
            only_delivery_board_of(&boards[1], &boards, &teams),
            Some("Web".to_string())
        );
        // A board of the organization has no delivery team.
        assert_eq!(only_delivery_board_of(&boards[2], &boards, &teams), None);
        let note = only_delivery_board_note("Platform");
        assert!(note.contains("\"Platform\""), "{note}");
        assert!(note.contains("delete the team"), "{note}");
    }

    /// COLLIERY-T-0251: a team of old data can have 2 delivery boards, and
    /// the server permits the delete of one. A delivery board of a team
    /// that is not live, or with no team, is not in the rule.
    #[test]
    fn a_team_with_two_delivery_boards_can_delete_one() {
        let teams = [team("t1", "Platform")];
        let boards = [
            board("b1", "delivery", Some("t1")),
            board("b2", "delivery", Some("t1")),
            board("b3", "delivery", Some("gone")),
            board("b4", "delivery", None),
            // A board of a different level does not count as a second
            // delivery board.
            board("b5", "initiative", Some("t1")),
        ];
        for board in &boards {
            assert_eq!(
                only_delivery_board_of(board, &boards, &teams),
                None,
                "{}",
                board.id
            );
        }
        // One of the two is deleted: the other is now the only one.
        assert_eq!(
            only_delivery_board_of(&boards[0], &boards[..1], &teams),
            Some("Platform".to_string())
        );
    }

    /// COLLIERY-T-0238: the selection of the transition form stays when
    /// it names a column of the board, and goes to the default when it
    /// does not.
    #[test]
    fn the_selection_of_the_transition_form_stays_across_a_reload() {
        let names: Vec<String> = ["Backlog", "Todo", "Active", "Review"]
            .iter()
            .map(|name| name.to_string())
            .collect();
        assert_eq!(kept_or_default(&names, "Active", 0), None);
        assert_eq!(kept_or_default(&names, "Review", 1), None);
        // The first load, and a column that was removed.
        assert_eq!(kept_or_default(&names, "", 0), Some("Backlog".to_string()));
        assert_eq!(kept_or_default(&names, "Gone", 1), Some("Todo".to_string()));
        // A board with too few columns has no default.
        assert_eq!(kept_or_default(&names[..1], "", 1), Some(String::new()));
        assert_eq!(kept_or_default(&[], "", 0), Some(String::new()));
    }
}
