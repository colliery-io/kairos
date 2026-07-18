//! `/admin/streams` — delivery streams CRUD + team membership
//! (KAIROS-T-0043).

use aurora_dark::components::{
    Button, Code, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Select, Stack,
    Text, TextInput,
};
use leptos::prelude::*;

use super::api;
use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::use_auth;

/// `/admin/streams`.
#[component]
pub fn AdminStreamsPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let expanded = RwSignal::new(None::<String>);

    let streams = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_streams(auth)
    });

    let name = RwSignal::new(String::new());
    let slug = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());

    let on_create = move |_| {
        let (n, s, d) = (
            name.get_untracked(),
            slug.get_untracked(),
            description.get_untracked(),
        );
        let d = (!d.is_empty()).then_some(d);
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Delivery stream \"{n}\" created."),
            async move {
                api::create_stream(auth, &n, &s, d.as_deref())
                    .await
                    .map(|_| ())
            },
        );
    };

    view! {
        <PageHeader title="Delivery streams" sub="value streams and the teams feeding them"/>
        <Stack gap="md">
            <MutationNotice outcome/>
            <Panel title="All streams" caption="expand a stream to manage its teams">
                {move || match streams.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(items)) if items.is_empty() => view! {
                        <Empty message="No delivery streams yet — create one below."/>
                    }.into_any(),
                    Some(Ok(items)) => items.into_iter().map(|stream| {
                        view! { <StreamRow stream busy outcome reload expanded/> }
                    }).collect_view().into_any(),
                }}
            </Panel>
            <Panel title="Create delivery stream">
                <Stack gap="sm">
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Name" value=name placeholder="e.g. Checkout"/>
                        <TextInput label="Slug" value=slug placeholder="e.g. checkout"/>
                        <TextInput label="Description (optional)" value=description/>
                    </Group>
                    <Group>
                        <Button on_click=Callback::new(on_create)>"Create stream"</Button>
                    </Group>
                </Stack>
            </Panel>
        </Stack>
    }
}

/// One stream row: identity, inline edit, delete, expandable teams panel.
#[component]
fn StreamRow(
    stream: api::DeliveryStream,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
    expanded: RwSignal<Option<String>>,
) -> impl IntoView {
    let auth = use_auth();
    let stream_id = StoredValue::new(stream.id.clone());
    let editing = RwSignal::new(false);
    let edit_name = RwSignal::new(stream.name.clone());
    let edit_slug = RwSignal::new(stream.slug.clone());
    let edit_description = RwSignal::new(stream.description.clone().unwrap_or_default());
    let deleted_name = stream.name.clone();

    let on_save = move |_| {
        let id = stream_id.get_value();
        let (n, s, d) = (
            edit_name.get_untracked(),
            edit_slug.get_untracked(),
            edit_description.get_untracked(),
        );
        let d = (!d.is_empty()).then_some(d);
        editing.set(false);
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Stream \"{n}\" updated."),
            async move {
                api::update_stream(auth, &id, &n, &s, d.as_deref())
                    .await
                    .map(|_| ())
            },
        );
    };
    let on_delete = move |_| {
        let id = stream_id.get_value();
        let deleted_name = deleted_name.clone();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Stream \"{deleted_name}\" deleted."),
            async move { api::delete_stream(auth, &id).await.map(|_| ()) },
        );
    };
    let toggle_teams = move |_| {
        let id = stream_id.get_value();
        expanded.update(|current| {
            *current = if current.as_deref() == Some(id.as_str()) {
                None
            } else {
                Some(id)
            };
        });
    };
    let is_expanded = move || expanded.get().as_deref() == Some(stream_id.get_value().as_str());

    view! {
        <Stack gap="xs">
            <Group justify="between" wrap=true>
                <Group gap="sm">
                    <Text bright=true>{stream.name.clone()}</Text>
                    <Code>{stream.slug.clone()}</Code>
                    {stream.description.clone().map(|description| view! {
                        <Text dimmed=true size="sm">{description}</Text>
                    })}
                </Group>
                <Group gap="xs">
                    <Button variant="default" size="xs" on_click=Callback::new(toggle_teams)>
                        {move || if is_expanded() { "Hide teams" } else { "Teams" }}
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
                    <TextInput label="Description" value=edit_description/>
                    <Button size="xs" on_click=Callback::new(on_save)>"Save"</Button>
                </Group>
            </Show>
            <Show when=is_expanded>
                <StreamTeamsPanel stream_id=stream_id.get_value() busy outcome reload/>
            </Show>
            <Divider/>
        </Stack>
    }
}

/// Teams feeding one stream: list, add (team picker), remove.
#[component]
fn StreamTeamsPanel(
    stream_id: String,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let auth = use_auth();
    let stream = StoredValue::new(stream_id);
    let member_teams = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        let id = stream.get_value();
        async move { api::stream_teams(auth, &id).await }
    });
    let all_teams = LocalResource::new(move || {
        let _ = auth.token();
        api::list_teams(auth)
    });
    let add_slug = RwSignal::new(String::new());

    let on_add = move |_| {
        let slug = add_slug.get_untracked();
        let team_id = all_teams
            .get()
            .and_then(|result| result.ok())
            .and_then(|teams| {
                teams
                    .iter()
                    .find(|team| team.slug == slug)
                    .map(|team| team.id.clone())
            });
        let Some(team_id) = team_id else {
            outcome.set(Some(Err(aurora_dark::tokens::ApiError::Unknown(
                "Pick a team to add.".to_string(),
            ))));
            return;
        };
        let id = stream.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Team \"{slug}\" added to the stream."),
            async move { api::add_stream_team(auth, &id, &team_id).await.map(|_| ()) },
        );
    };

    view! {
        <Stack gap="sm">
            {move || match member_teams.get() {
                None => view! { <Loading/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! {
                    <Empty message="No teams in this stream yet — add one below."/>
                }.into_any(),
                Some(Ok(list)) => list.into_iter().map(|team| {
                    let team_id = StoredValue::new(team.id);
                    let team_name = team.name.clone();
                    let on_remove = move |_| {
                        let id = stream.get_value();
                        let team_id = team_id.get_value();
                        let team_name = team_name.clone();
                        run_mutation(
                            busy, outcome, reload,
                            format!("Team \"{team_name}\" removed from the stream."),
                            async move {
                                api::remove_stream_team(auth, &id, &team_id).await.map(|_| ())
                            },
                        );
                    };
                    view! {
                        <Group justify="between" wrap=true>
                            <Group gap="sm">
                                <Text>{team.name.clone()}</Text>
                                <Code>{team.slug.clone()}</Code>
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
                let taken: Vec<String> = member_teams
                    .get()
                    .and_then(|result| result.ok())
                    .map(|list| list.into_iter().map(|team| team.slug).collect())
                    .unwrap_or_default();
                let mut options: Vec<String> = all_teams
                    .get()
                    .and_then(|result| result.ok())
                    .map(|teams| {
                        teams
                            .into_iter()
                            .map(|team| team.slug)
                            .filter(|slug| !taken.contains(slug))
                            .collect()
                    })
                    .unwrap_or_default();
                options.insert(0, String::new());
                view! {
                    <Group gap="sm" top=true>
                        <Select label="Add team" options value=add_slug/>
                        <Button size="xs" on_click=Callback::new(on_add)>"Add"</Button>
                    </Group>
                }
            }}
        </Stack>
    }
}
