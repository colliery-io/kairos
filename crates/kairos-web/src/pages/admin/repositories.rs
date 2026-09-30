//! `/admin/repositories` — the repository directory (KAIROS-T-0109,
//! decision KAIROS-A-0019): register, edit and re-home repositories, and
//! wire their forge webhooks. Every repository has exactly one owning
//! team. The owner does not choose where tasks go: a task on any team's
//! board may link to any repository, and the open count of a row covers all
//! boards (COLLIERY-T-0219, COLLIERY-A-0023).
//!
//! The webhook secret is DERIVED server-side and shown exactly once at
//! connect time (KAIROS-T-0097) — this page renders it in a notice the
//! operator copies into the forge and never re-reads. Rotation is
//! disconnect + connect (a new connection id = a new URL and secret).
//!
//! COLLIERY-T-0269: each row shows the documents and the ADRs that impact
//! the repository (its vision, its architecture, the decisions about it),
//! and makes a new document for the repository
//! ([`crate::pages::repositories::documents`]).
//!
//! COLLIERY-T-0267: the two forms give the rule of the full name, of the
//! URL and of the default branch before the user sends them. A refusal of
//! the server about one of these fields shows next to that field.
//! COLLIERY-T-0265: the slug has its rule and its refusal too.

use aurora_dark::components::{
    Button, Code, Divider, Empty, ErrorState, Group, Loading, PageHeader, Panel, Select, Stack,
    Text, TextInput,
};
use aurora_dark::tokens::ApiError;
use leptos::prelude::*;

use super::api;
use super::{
    MutationNotice, MutationOutcome, REPOSITORY_SLUG_HINT, refusal_for, run_form_mutation,
    run_mutation,
};
use crate::auth::use_auth;

/// What the operator pastes into the forge — shown once, then gone.
#[derive(Clone, Debug, PartialEq)]
struct Secret {
    slug: String,
    webhook_url: String,
    webhook_secret: String,
}

/// The rule of the full name for `forge`. A mirror of
/// `kairos_core::repositories::full_name_rule`: this crate does not depend
/// on `kairos-core`.
fn full_name_hint(forge: &str) -> &'static str {
    match forge {
        "github" => {
            "Full name: 2 parts for the forge github, for example acme/payments-api. It \
             has no space, and it does not end with .git."
        }
        "gitlab" => {
            "Full name: 2 or more parts for the forge gitlab, for example \
             acme/portal/web. It has no space, and it does not end with .git."
        }
        _ => {
            "Full name: 1 or more parts for the forge other, for example acme/site. It has \
             no space, and it does not end with .git."
        }
    }
}

/// The rule of the URL.
const URL_HINT: &str = "URL: an absolute http or https URL, with no space. Do not put a user \
                        name or a password in it, because each member can read it.";

/// The rule of the default branch.
const BRANCH_HINT: &str = "Default branch: a branch name that git accepts, with no space.";

/// The fields that have a refusal next to them. The slug is one of them
/// since COLLIERY-T-0265.
const FORM_FIELDS: [&str; 4] = ["repo_full_name", "repo_url", "default_branch", "slug"];

/// `/admin/repositories`.
#[component]
pub fn AdminRepositoriesPage() -> impl IntoView {
    let auth = use_auth();
    let reload = RwSignal::new(0u32);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let secret: RwSignal<Option<Secret>> = RwSignal::new(None);
    let refused: RwSignal<Option<api::Refusal>> = RwSignal::new(None);

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
    // The owning-team picker's option list, and its default: the first
    // team once the directory has loaded, set from an Effect — never a
    // signal write inside a tracked render (KAIROS-T-0114).
    let team_slugs = Memo::new(move |_| {
        teams
            .get()
            .and_then(|t| t.ok())
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.slug)
            .collect::<Vec<String>>()
    });
    Effect::new(move |_| {
        if let Some(first) = team_slugs.with(|slugs| slugs.first().cloned())
            && team.get_untracked().is_empty()
        {
            team.set(first);
        }
    });

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
        run_form_mutation(
            busy,
            outcome,
            reload,
            refused,
            &FORM_FIELDS,
            format!("Kairos added the repository \"{n}\"."),
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
        <PageHeader title="Repositories" sub="The repositories that tasks link to, and \
                                              their webhooks."/>
        <Stack gap="md">
            <MutationNotice outcome/>
            {move || secret.get().map(|s| view! {
                <Panel title="The webhook is connected. Copy these values now." caption="Kairos \
                                                                                         shows \
                                                                                         the \
                                                                                         secret \
                                                                                         one \
                                                                                         time \
                                                                                         only.">
                    <Stack gap="xs" attr:data-testid="webhook-secret">
                        <Text size="sm">{format!("Repository {}: put these values in the \
                                                  webhook settings of the forge.", s.slug)}</Text>
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
            <Panel title="All repositories" caption="Each repository has one team that \
                                                     owns it. The count of open tasks \
                                                     includes all boards.">
                {move || match repos.get() {
                    None => view! { <Loading/> }.into_any(),
                    Some(Err(error)) => view! {
                        <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                    }.into_any(),
                    Some(Ok(items)) if items.is_empty() => view! {
                        <Empty message="The organization has no repositories. Add one \
                                        below."/>
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
            <Panel title="Register repository" caption="An organization admin adds a \
                                                        repository here. A member of the \
                                                        owner team adds one with the CLI \
                                                        (kairos repos create). An agent \
                                                        adds one with the MCP tool \
                                                        add_repository.">
                <Stack gap="sm">
                    <Group gap="sm" wrap=true top=true>
                        <Select label="Forge" value=forge
                            options=vec!["github".to_string(), "gitlab".to_string(), "other".to_string()]/>
                        {move || view! {
                            <TextInput label="Full name" value=name
                                placeholder="e.g. acme/payments-api"
                                error=refusal_for(refused, "repo_full_name")/>
                        }}
                        {move || view! {
                            <TextInput label="URL" value=url
                                placeholder="https://github.com/acme/payments-api"
                                error=refusal_for(refused, "repo_url")/>
                        }}
                        {move || view! {
                            <Select label="Owner team" value=team options=team_slugs.get()/>
                        }}
                    </Group>
                    <Group gap="sm" wrap=true top=true>
                        {move || view! {
                            <TextInput label="Slug (optional)" value=slug
                                placeholder="Kairos makes it from the full name"
                                error=refusal_for(refused, "slug")/>
                        }}
                        {move || view! {
                            <TextInput label="Default branch (optional)" value=branch
                                placeholder="main"
                                error=refusal_for(refused, "default_branch")/>
                        }}
                        <TextInput label="How to work here (optional)" value=description
                            placeholder="What an agent must know before it works in this \
                                         repository"/>
                    </Group>
                    <Stack gap="xs" attr:data-testid="repository-rules">
                        <Text dimmed=true size="xs">{move || full_name_hint(&forge.get())}</Text>
                        <Text dimmed=true size="xs">{URL_HINT}</Text>
                        <Text dimmed=true size="xs">
                            {format!("{BRANCH_HINT} The default is main.")}
                        </Text>
                        <Text dimmed=true size="xs">{REPOSITORY_SLUG_HINT}</Text>
                    </Stack>
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
    let refused: RwSignal<Option<api::Refusal>> = RwSignal::new(None);
    let has_webhook = repo.has_webhook;
    let connectable = repo.forge != "other";
    let name = format!("{} · {}", repo.forge, repo.repo_full_name);
    let counts = format!("{} open", repo.open_tasks);
    let row_attr = repo.slug.clone();
    let code_slug = repo.slug.clone();
    let owner = format!("owner: {}", repo.team.slug);
    let repo_url = repo.repo_url.clone();
    let description = repo.description.clone();
    let documents_slug = repo.slug.clone();
    let owner_team = repo.team.id.clone();

    let on_save = move |_| {
        let reference = slug.get_value();
        let (s, u, b, t, d) = (
            edit_slug.get_untracked(),
            edit_url.get_untracked(),
            edit_branch.get_untracked(),
            edit_team.get_untracked(),
            edit_description.get_untracked(),
        );
        // The editor stays open: a refusal shows next to its field
        // (COLLIERY-T-0267). A success reads the list again, and the new
        // row has a closed editor.
        run_form_mutation(
            busy,
            outcome,
            reload,
            refused,
            &FORM_FIELDS,
            format!("Kairos changed the repository \"{s}\"."),
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
            format!("Kairos deleted the repository \"{reference}\"."),
            async move { api::delete_repository(auth, &reference).await.map(|_| ()) },
        );
    };
    let on_connect = move |_| {
        let reference = slug.get_value();
        run_mutation(
            busy,
            outcome,
            reload,
            format!("Kairos connected the webhook of \"{reference}\"."),
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
            format!("Kairos disconnected the webhook of \"{reference}\"."),
            async move {
                let detail = api::repository_detail(auth, &reference).await?;
                // The row's `has_webhook` may be stale (another operator
                // disconnected first): say so rather than reporting a
                // disconnect that never happened.
                let Some(id) = detail.connection_id else {
                    return Err(ApiError::Unknown(format!(
                        "\"{reference}\" has no forge connection, so it has no webhook to \
                         disconnect."
                    )));
                };
                api::disconnect_webhook(auth, &id).await?;
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
            <crate::pages::repositories::documents::RepositoryDocuments
                slug=documents_slug owner_team/>
            <Show when=move || editing.get()>
                <Stack gap="xs">
                    <Group gap="sm" wrap=true top=true>
                        {move || view! {
                            <TextInput label="Slug" value=edit_slug
                                error=refusal_for(refused, "slug")/>
                        }}
                        {move || view! {
                            <TextInput label="URL" value=edit_url
                                error=refusal_for(refused, "repo_url")/>
                        }}
                        {move || view! {
                            <TextInput label="Default branch" value=edit_branch
                                error=refusal_for(refused, "default_branch")/>
                        }}
                        <Select label="Owner team" value=edit_team options=team_slugs.get_value()/>
                    </Group>
                    <Group gap="sm" wrap=true top=true>
                        <TextInput label="How to work here" value=edit_description/>
                        <Button size="xs" on_click=Callback::new(on_save)>"Save"</Button>
                    </Group>
                    <Text dimmed=true size="xs">{URL_HINT}</Text>
                    <Text dimmed=true size="xs">{BRANCH_HINT}</Text>
                    <Text dimmed=true size="xs">{REPOSITORY_SLUG_HINT}</Text>
                    <Text dimmed=true size="xs" attr:style="color: var(--gold)">
                        "A new owning team does not change the tasks. Each task stays on its board and keeps its link."
                    </Text>
                </Stack>
            </Show>
            <Divider/>
        </Stack>
    }
}
