//! `/admin/teams` — teams CRUD + membership (KAIROS-T-0043). Creating a
//! team also creates its delivery board (`{slug}-delivery`, KAIROS-A-0002);
//! the success notice surfaces it with a link into board configuration.

use aurora_dark::components::{
    Alert, Anchor, Button, Code, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel,
    Pill, Select, Stack, Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api;
use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::use_auth;

const TEAM_TYPES: [&str; 4] = [
    "stream_aligned",
    "platform",
    "enabling",
    "complicated_subsystem",
];

/// `/admin/teams`.
#[component]
pub fn AdminTeamsPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    // The team created by the last successful create (its delivery board
    // gets surfaced prominently, per the AC).
    let created = RwSignal::new(None::<api::Team>);
    // Which team's member panel is expanded.
    let expanded = RwSignal::new(None::<String>);

    let teams = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_teams(auth)
    });

    let name = RwSignal::new(String::new());
    let slug = RwSignal::new(String::new());
    let team_type = RwSignal::new("stream_aligned".to_string());

    let on_create = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let (n, s, t) = (
            name.get_untracked(),
            slug.get_untracked(),
            team_type.get_untracked(),
        );
        leptos::task::spawn_local(async move {
            let result = api::create_team(auth, &n, &s, &t).await;
            busy.set(false);
            match result {
                Ok(team) => {
                    created.set(Some(team));
                    outcome.set(None);
                    reload.update(|count| *count += 1);
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
        });
    };

    view! {
        <PageHeader title="Teams" sub="each team owns a delivery board"/>
        <Stack gap="md">
            <MutationNotice outcome/>
            {move || created.get().map(|team| view! {
                <Alert title="Team created" color=token::OK>
                    <Stack gap="xs">
                        <Text size="sm">
                            {format!(
                                "\"{}\" is ready — its delivery board \"{}-delivery\" was created with it.",
                                team.name, team.slug
                            )}
                        </Text>
                        {team.delivery_board_id.clone().map(|board_id| view! {
                            <Anchor href=format!("/admin/boards/{board_id}")>
                                "Configure the new delivery board"
                            </Anchor>
                        })}
                        <Group>
                            <Button variant="subtle" size="xs"
                                on_click=Callback::new(move |_| created.set(None))>
                                "Dismiss"
                            </Button>
                        </Group>
                    </Stack>
                </Alert>
            })}
            <Panel title="All teams" caption="expand a team to manage its members">
                {move || match teams.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(items)) if items.is_empty() => view! {
                        <Empty message="No teams yet — create one below (its delivery board comes with it)."/>
                    }.into_any(),
                    Some(Ok(items)) => items.into_iter().map(|team| {
                        view! { <TeamRow team busy outcome reload expanded/> }
                    }).collect_view().into_any(),
                }}
            </Panel>
            <Panel title="Create team"
                caption="also creates the team's delivery board from the system defaults">
                <Stack gap="sm">
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Name" value=name placeholder="e.g. Payments"/>
                        <TextInput label="Slug" value=slug placeholder="e.g. payments"/>
                        <Select label="Type"
                            options=TEAM_TYPES.iter().map(|t| t.to_string()).collect()
                            value=team_type/>
                    </Group>
                    <Group>
                        <Button on_click=Callback::new(on_create)>"Create team"</Button>
                    </Group>
                </Stack>
            </Panel>
        </Stack>
    }
}

/// One team row: identity + delivery-board link, inline edit, delete, and
/// the expandable member panel.
#[component]
fn TeamRow(
    team: api::Team,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
    expanded: RwSignal<Option<String>>,
) -> impl IntoView {
    let auth = use_auth();
    let team_id = StoredValue::new(team.id.clone());
    let editing = RwSignal::new(false);
    let edit_name = RwSignal::new(team.name.clone());
    let edit_slug = RwSignal::new(team.slug.clone());
    let edit_type = RwSignal::new(team.team_type.clone());
    let team_name = team.name.clone();
    let deleted_name = team.name.clone();

    let on_save = move |_| {
        let id = team_id.get_value();
        let (n, s, t) = (
            edit_name.get_untracked(),
            edit_slug.get_untracked(),
            edit_type.get_untracked(),
        );
        editing.set(false);
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Team \"{n}\" updated."),
            async move { api::update_team(auth, &id, &n, &s, &t).await.map(|_| ()) },
        );
    };
    let on_delete = move |_| {
        let id = team_id.get_value();
        let deleted_name = deleted_name.clone();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Team \"{deleted_name}\" deleted."),
            async move { api::delete_team(auth, &id).await.map(|_| ()) },
        );
    };
    let toggle_members = move |_| {
        let id = team_id.get_value();
        expanded.update(|current| {
            *current = if current.as_deref() == Some(id.as_str()) {
                None
            } else {
                Some(id)
            };
        });
    };
    let is_expanded = move || expanded.get().as_deref() == Some(team_id.get_value().as_str());

    view! {
        <Stack gap="xs">
            <Group justify="between" wrap=true>
                <Group gap="sm">
                    <Text bright=true>{team_name}</Text>
                    <Code>{team.slug.clone()}</Code>
                    <Pill color=token::VIOLET>{team.team_type.clone()}</Pill>
                    {team.delivery_board_id.clone().map(|board_id| view! {
                        <Anchor href=format!("/admin/boards/{board_id}")>"delivery board"</Anchor>
                    })}
                </Group>
                <Group gap="xs">
                    <Button variant="default" size="xs" on_click=Callback::new(toggle_members)>
                        {move || if is_expanded() { "Hide members" } else { "Members" }}
                    </Button>
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| editing.update(|open| *open = !*open))>
                        {move || if editing.get() { "Close" } else { "Edit" }}
                    </Button>
                    <Button variant="default" size="xs" bad=true on_click=Callback::new(on_delete)>
                        "Delete"
                    </Button>
                </Group>
            </Group>
            <Show when=move || editing.get()>
                <Group gap="sm" wrap=true top=true>
                    <TextInput label="Name" value=edit_name/>
                    <TextInput label="Slug" value=edit_slug/>
                    <Select label="Type"
                        options=TEAM_TYPES.iter().map(|t| t.to_string()).collect()
                        value=edit_type/>
                    <Button size="xs" on_click=Callback::new(on_save)>"Save"</Button>
                </Group>
            </Show>
            <Show when=is_expanded>
                <TeamMembersPanel team_id=team_id.get_value() busy outcome reload/>
            </Show>
            <Divider/>
        </Stack>
    }
}

/// Members of one team: list, add (org-member picker), remove.
#[component]
fn TeamMembersPanel(
    team_id: String,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let team = StoredValue::new(team_id);
    let members = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        let id = team.get_value();
        async move { api::team_members(auth, &id).await }
    });
    let org_members = LocalResource::new(move || {
        let _ = auth.token();
        api::list_org_members(auth)
    });
    let add_email = RwSignal::new(String::new());

    let on_add = move |_| {
        let email = add_email.get_untracked();
        let user_id = org_members
            .get()
            .and_then(|result| result.ok())
            .and_then(|list| {
                list.iter()
                    .find(|member| member.email == email)
                    .map(|member| member.user_id.clone())
            });
        let Some(user_id) = user_id else {
            outcome.set(Some(Err(aurora_dark::tokens::ApiError::Unknown(
                "Pick an organization member to add.".to_string(),
            ))));
            return;
        };
        let id = team.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("{email} added to the team."),
            async move { api::add_team_member(auth, &id, &user_id).await.map(|_| ()) },
        );
    };

    view! {
        <Stack gap="sm">
            {move || match members.get() {
                None => view! { <Loading/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! {
                    <Empty message="No members yet — add one below."/>
                }.into_any(),
                Some(Ok(list)) => list.into_iter().map(|member| {
                    let user_id = StoredValue::new(member.user_id);
                    let email = member.email.clone();
                    let on_remove = move |_| {
                        let id = team.get_value();
                        let user_id = user_id.get_value();
                        let email = email.clone();
                        run_mutation(
                            busy, outcome, reload,
                            format!("{email} removed from the team."),
                            async move {
                                api::remove_team_member(auth, &id, &user_id).await.map(|_| ())
                            },
                        );
                    };
                    view! {
                        <Group justify="between" wrap=true>
                            <Group gap="sm">
                                <Text>{member.display_name.clone()}</Text>
                                <Text dimmed=true size="sm">{member.email.clone()}</Text>
                            </Group>
                            <Button variant="default" size="xs" bad=true
                                on_click=Callback::new(on_remove)>
                                "Remove"
                            </Button>
                        </Group>
                    }
                }).collect_view().into_any(),
            }}
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
                    <Group gap="sm" top=true>
                        <Select label="Add member" options value=add_email/>
                        <Button size="xs" on_click=Callback::new(on_add)>"Add"</Button>
                    </Group>
                }
            }}
        </Stack>
    }
}
