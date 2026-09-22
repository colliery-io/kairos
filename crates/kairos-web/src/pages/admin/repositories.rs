//! `/admin/repositories` — the repository directory (KAIROS-T-0109,
//! decision KAIROS-A-0019): register, edit and re-home repositories, and
//! wire their forge webhooks. Every repository has exactly one owning
//! team; tasks filed against it land on that team's delivery board.
//!
//! The webhook secret is DERIVED server-side and shown exactly once at
//! connect time (KAIROS-T-0097) — this page renders it in a notice the
//! operator copies into the forge and never re-reads. Rotation is
//! disconnect + connect (a new connection id = a new URL and secret).

use aurora_dark::components::{
    Button, Code, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Select, Stack,
    Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api;
use super::{MutationNotice, MutationOutcome, run_mutation};
use crate::auth::use_auth;

/// What the operator pastes into the forge — shown once, then gone.
#[derive(Clone, Debug, PartialEq)]
struct Secret {
    slug: String,
    webhook_url: String,
    webhook_secret: String,
}

/// `/admin/repositories`.
#[component]
pub fn AdminRepositoriesPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let secret: RwSignal<Option<Secret>> = RwSignal::new(None);

    let repos = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        api::list_repositories(auth, None)
    });
    let teams = LocalResource::new(move || {
        let _ = auth.token();
        api::list_teams(auth)
    });

    let forge = RwSignal::new("github".to_string());
    let name = RwSignal::new(String::new());
    let url = RwSignal::new(String::new());
    let team = RwSignal::new(String::new());
    let slug = RwSignal::new(String::new());
    let branch = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());

    let on_create = move |_| {
        let (f, n, u, t, s, b, d) = (
            forge.get_untracked(),
            name.get_untracked(),
            url.get_untracked(),
            team.get_untracked(),
            slug.get_untracked(),
            branch.get_untracked(),
            description.get_untracked(),
        );
        let s = (!s.is_empty()).then_some(s);
        let b = (!b.is_empty()).then_some(b);
        let d = (!d.is_empty()).then_some(d);
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Repository \"{n}\" registered."),
            async move {
                api::create_repository(
                    auth,
                    s.as_deref(),
                    &f,
                    &n,
                    &u,
                    &t,
                    b.as_deref(),
                    d.as_deref(),
                )
                .await
                .map(|_| ())
            },
        );
    };

    view! {
        <PageHeader title="Repositories" sub="the codebases tickets are issued against, and their webhooks"/>
        <Stack gap="md">
            <MutationNotice outcome/>
            {move || secret.get().map(|s| view! {
                <Panel title="Webhook connected — copy these now" caption="the secret is shown once and cannot be re-read">
                    <Stack gap="xs" attr:data-testid="webhook-secret">
                        <Text size="sm">{format!("Repository {}: paste into the forge's webhook settings.", s.slug)}</Text>
                        <Group gap="sm" wrap=true>
                            <Text dimmed=true size="xs">"Payload URL"</Text>
                            <Code>{s.webhook_url.clone()}</Code>
                        </Group>
                        <Group gap="sm" wrap=true>
                            <Text dimmed=true size="xs">"Secret"</Text>
                            <Code>{s.webhook_secret.clone()}</Code>
                        </Group>
                        <Group>
                            <Button variant="default" size="xs"
                                on_click=Callback::new(move |_| secret.set(None))>
                                "I have copied them"
                            </Button>
                        </Group>
                    </Stack>
                </Panel>
            })}
            <Panel title="All repositories" caption="one owning team each; tasks route to that team's delivery board">
                {move || match repos.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(items)) if items.is_empty() => view! {
                        <Empty message="No repositories registered yet — register one below."/>
                    }.into_any(),
                    Some(Ok(items)) => {
                        let team_options = teams.get().and_then(|t| t.ok()).unwrap_or_default();
                        items.into_iter().map(|repo| {
                            let team_options = team_options.clone();
                            view! { <RepositoryRow repo team_options busy outcome reload secret/> }
                        }).collect_view().into_any()
                    }
                }}
            </Panel>
            <Panel title="Register repository" caption="any member of the owning team may also do this from the CLI (kairos repos create)">
                <Stack gap="sm">
                    <Group gap="sm" wrap=true top=true>
                        <Select label="Forge" value=forge
                            options=vec!["github".to_string(), "gitlab".to_string(), "other".to_string()]/>
                        <TextInput label="Full name" value=name placeholder="e.g. acme/payments-api"/>
                        <TextInput label="URL" value=url placeholder="https://github.com/acme/payments-api"/>
                        {move || {
                            let options: Vec<String> = teams
                                .get()
                                .and_then(|t| t.ok())
                                .unwrap_or_default()
                                .into_iter()
                                .map(|t| t.slug)
                                .collect();
                            if team.get_untracked().is_empty()
                                && let Some(first) = options.first()
                            {
                                team.set(first.clone());
                            }
                            view! { <Select label="Owning team" value=team options=options/> }
                        }}
                    </Group>
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Slug (optional)" value=slug placeholder="derived from the full name"/>
                        <TextInput label="Default branch (optional)" value=branch placeholder="main"/>
                        <TextInput label="How to work here (optional)" value=description
                            placeholder="what an agent should know before working in this repo"/>
                    </Group>
                    <Group>
                        <Button on_click=Callback::new(on_create)>"Register repository"</Button>
                    </Group>
                </Stack>
            </Panel>
        </Stack>
    }
}

/// One repository row: identity and counts, inline edit (incl. re-home),
/// webhook connect/disconnect, delete (refused server-side while
/// referenced — the notice says by what).
#[component]
fn RepositoryRow(
    repo: api::Repository,
    team_options: Vec<api::Team>,
    busy: RwSignal<bool>,
    outcome: RwSignal<MutationOutcome>,
    reload: RwSignal<u32>,
    secret: RwSignal<Option<Secret>>,
) -> impl IntoView {
    let auth = use_auth();
    let slug = StoredValue::new(repo.slug.clone());
    let editing = RwSignal::new(false);
    let edit_slug = RwSignal::new(repo.slug.clone());
    let edit_url = RwSignal::new(repo.repo_url.clone());
    let edit_branch = RwSignal::new(repo.default_branch.clone());
    let edit_team = RwSignal::new(repo.team.slug.clone());
    let edit_description = RwSignal::new(repo.description.clone());
    let team_slugs = StoredValue::new(
        team_options
            .into_iter()
            .map(|t| t.slug)
            .collect::<Vec<String>>(),
    );
    let has_webhook = repo.has_webhook;
    let connectable = repo.forge != "other";
    let name = format!("{} · {}", repo.forge, repo.repo_full_name);
    let counts = format!("{} open", repo.open_tasks);
    let row_attr = repo.slug.clone();
    let code_slug = repo.slug.clone();
    let owner = format!("owner: {}", repo.team.slug);
    let repo_url = repo.repo_url.clone();
    let description = repo.description.clone();

    let on_save = move |_| {
        let reference = slug.get_value();
        let (s, u, b, t, d) = (
            edit_slug.get_untracked(),
            edit_url.get_untracked(),
            edit_branch.get_untracked(),
            edit_team.get_untracked(),
            edit_description.get_untracked(),
        );
        editing.set(false);
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Repository \"{s}\" updated."),
            async move {
                api::update_repository(
                    auth,
                    &reference,
                    Some(&s),
                    Some(&u),
                    Some(&b),
                    Some(&t),
                    Some(&d),
                )
                .await
                .map(|_| ())
            },
        );
    };
    let on_delete = move |_| {
        let reference = slug.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Repository \"{reference}\" deleted."),
            async move { api::delete_repository(auth, &reference).await.map(|_| ()) },
        );
    };
    let on_connect = move |_| {
        let reference = slug.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Webhook connected for \"{reference}\"."),
            async move {
                let created = api::connect_webhook(auth, &reference).await?;
                secret.set(Some(Secret {
                    slug: reference,
                    webhook_url: created.webhook_url,
                    webhook_secret: created.webhook_secret,
                }));
                Ok(())
            },
        );
    };
    let on_disconnect = move |_| {
        let reference = slug.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Webhook disconnected for \"{reference}\"."),
            async move {
                let detail = api::repository_detail(auth, &reference).await?;
                if let Some(id) = detail.connection_id {
                    api::disconnect_webhook(auth, &id).await?;
                }
                Ok(())
            },
        );
    };

    view! {
        <Stack gap="xs" attr:data-repo=row_attr>
            <Group justify="between" wrap=true>
                <Group gap="sm" wrap=true>
                    <Code>{code_slug}</Code>
                    <a class="cl-anchor" href=repo_url target="_blank" rel="noopener noreferrer">
                        {name}
                    </a>
                    <Text dimmed=true size="sm">{owner}</Text>
                    <Text dimmed=true size="xs">{counts}</Text>
                    {has_webhook.then(|| view! { <Code>"webhooks"</Code> })}
                </Group>
                <Group gap="xs">
                    {(connectable && !has_webhook).then(|| view! {
                        <Button variant="default" size="xs" on_click=Callback::new(on_connect)>
                            "Connect webhook"
                        </Button>
                    })}
                    {has_webhook.then(|| view! {
                        <Button variant="default" size="xs" on_click=Callback::new(on_disconnect)>
                            "Disconnect"
                        </Button>
                    })}
                    <Button variant="default" size="xs"
                        on_click=Callback::new(move |_| editing.update(|open| *open = !*open))>
                        {move || if editing.get() { "Close" } else { "Edit" }}
                    </Button>
                    <Button variant="default" size="xs" bad=true on_click=Callback::new(on_delete)>
                        "Delete"
                    </Button>
                </Group>
            </Group>
            {(!description.is_empty()).then(|| view! {
                <Text dimmed=true size="sm">{description}</Text>
            })}
            <Show when=move || editing.get()>
                <Stack gap="xs">
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="Slug" value=edit_slug/>
                        <TextInput label="URL" value=edit_url/>
                        <TextInput label="Default branch" value=edit_branch/>
                        <Select label="Owning team" value=edit_team options=team_slugs.get_value()/>
                    </Group>
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="How to work here" value=edit_description/>
                        <Button size="xs" on_click=Callback::new(on_save)>"Save"</Button>
                    </Group>
                    <Text dimmed=true size="xs" attr:style=format!("color: {}", token::GOLD)>
                        "Re-homing to another team does not move its tasks; they are re-checked on their next write."
                    </Text>
                </Stack>
            </Show>
            <Divider/>
        </Stack>
    }
}
