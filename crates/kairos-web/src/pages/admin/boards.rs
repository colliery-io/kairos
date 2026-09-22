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
use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::use_auth;

const LEVELS: [&str; 4] = ["strategy", "initiative", "delivery", "adr"];

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
    let level = RwSignal::new("initiative".to_string());
    let team_slug = RwSignal::new(String::new());

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
        let (n, s, l) = (
            name.get_untracked(),
            slug.get_untracked(),
            level.get_untracked(),
        );
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Board \"{n}\" created with the {l} default columns."),
            async move {
                api::create_board(auth, &n, &s, &l, team_id.as_deref())
                    .await
                    .map(|_| ())
            },
        );
    };

    view! {
        <PageHeader title="Boards" sub="create, delete, configure"/>
        <Stack gap="md">
            <MutationNotice outcome/>
            <Panel title="All boards" caption="click a board to configure it">
                {move || match boards.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(items)) if items.is_empty() => view! {
                        <Empty message="No boards yet — create one below."/>
                    }.into_any(),
                    Some(Ok(items)) => items.into_iter().map(|board| {
                        let config_href = format!("/admin/boards/{}", board.id);
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
                                </Group>
                                // Delete is org-admin-only (KAIROS-T-0052).
                                <Show when=move || is_admin()>
                                    {
                                        let on_delete = move |_| {
                                            let board_id = board_id.get_value();
                                            let board_name = board_name.get_value();
                                            run_mutation(
                                                busy, outcome, reload,
                                                format!("Board \"{board_name}\" deleted."),
                                                async move {
                                                    api::delete_board(auth, &board_id).await.map(|_| ())
                                                },
                                            );
                                        };
                                        view! {
                                            <Button variant="default" size="xs" bad=true
                                                on_click=Callback::new(on_delete)>
                                                "Delete"
                                            </Button>
                                        }
                                    }
                                </Show>
                            </Group>
                        }
                    }).collect_view().into_any(),
                }}
            </Panel>
            // Board creation is org-admin-only (tenant-level, A-0006);
            // capability holders configure existing boards but do not create
            // them (KAIROS-T-0052).
            <Show when=move || is_admin()>
                <Panel title="Create board" caption="seeded with the level's default columns and transitions">
                    <Stack gap="sm">
                        <Group gap="sm" wrap=true top=true>
                            <TextInput label="Name" value=name placeholder="e.g. Platform Initiatives"/>
                            <TextInput label="Slug" value=slug placeholder="e.g. platform-initiatives"/>
                            <Select label="Level" options=LEVELS.iter().map(|l| l.to_string()).collect() value=level/>
                        </Group>
                        <Show when=move || level.get() == "delivery">
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
                                    <Select label="Owning team (delivery boards)" options value=team_slug/>
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
                view! {
                    <PageHeader title=detail.board.name.clone() sub="board configuration"/>
                    <Stack gap="md">
                        <Group gap="sm">
                            <Code>{detail.board.slug.clone()}</Code>
                            <Pill color=token::TEAL>{detail.board.board_level.clone()}</Pill>
                            <Anchor href="/admin/boards">"All boards"</Anchor>
                        </Group>
                        <MutationNotice outcome/>
                        <ColumnsPanel board_id=id.clone() columns=detail.columns.clone()
                            transitions=detail.transitions.clone() busy outcome reload/>
                        <TransitionsPanel board_id=id.clone() columns=detail.columns.clone()
                            transitions=detail.transitions.clone() busy outcome reload/>
                        <MembersPanel board_id=id busy outcome reload/>
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
            format!("Column \"{name}\" added."),
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
                        "Column marked {}.",
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
                    format!("Column renamed to \"{name}\"."),
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
                    format!("Column moved to position {target}."),
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
                    "Column removed.".to_string(),
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
        <Panel title="Columns" caption="ordered left to right on the board">
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

/// Transition edges: which column-to-column moves the board allows.
#[component]
fn TransitionsPanel(
    board_id: String,
    columns: Vec<api::BoardColumn>,
    transitions: Vec<api::BoardTransition>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
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
    let from_name = RwSignal::new(names.first().cloned().unwrap_or_default());
    let to_name = RwSignal::new(names.get(1).cloned().unwrap_or_default());
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
            format!("Transition {from} → {to} added."),
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
                    format!("Transition {done} removed."),
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
        <Panel title="Transitions" caption="allowed column-to-column moves (anything absent is not offered on the board)">
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
#[component]
fn MembersPanel(
    board_id: String,
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
                "Pick an organization member to add.".to_string(),
            ))));
            return;
        };
        let capabilities = add_editor.selection();
        if capabilities.is_empty() {
            outcome.set(Some(Err(aurora_dark::tokens::ApiError::Unknown(
                "Select at least one capability — grants are whitelist-only (A-0006).".to_string(),
            ))));
            return;
        }
        let board_id = board.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("{email} added with: {}.", capabilities.join(", ")),
            async move {
                api::add_board_member(auth, &board_id, &user_id, &capabilities)
                    .await
                    .map(|_| ())
            },
        );
    };

    view! {
        <Panel title="Members and capabilities"
            caption="write access is whitelist-only (A-0006); reads are open tenant-wide">
            <Stack gap="md">
                {move || match members.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(list)) if list.is_empty() => view! {
                        <Empty message="No capability grants on this board yet — add a member below."/>
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
                                    "A member needs at least one capability — use Remove to revoke membership."
                                        .to_string(),
                                ))));
                                return;
                            }
                            let board_id = board.get_value();
                            let user_id = user.get_value();
                            let saved_email = member_email.get_value();
                            run_mutation(
                                busy, outcome, reload,
                                format!("{saved_email} now has: {}.", capabilities.join(", ")),
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
                                format!("{removed_email} removed from the board (all grants revoked)."),
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
