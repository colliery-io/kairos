//! `/teams/:slug/pages/{path…}` (KAIROS-T-0086): view and edit one team
//! page at its slug path, or index a folder's children — with the
//! generalized markdown editor (Edit/Preview + toolbar + versioned saves
//! + 409 merge dialog) and the manage affordances (create page/folder,
//! rename, move, soft-delete) under the team-member-or-admin permission
//! model. The charter shows no destructive controls (server-enforced,
//! UI-honest).
//!
//! Decisions (recorded on KAIROS-T-0086): folder indexes are the create
//! surface (root-level sections ship with the scaffold; creating new
//! ROOT nodes is deliberately not offered in v1); deletes confirm with a
//! second click rather than a dialog; after a delete the router goes up
//! one level.

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Stack, Text,
    TextInput,
};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};
use std::rc::Rc;

use super::api;
use crate::auth::use_auth;
use crate::pages::editor::{MarkdownEditor, Saver};
use crate::pages::item::api::error_text;
use crate::pages::item::markdown;

/// The fetched unit: the team and its full (flat) page tree.
#[derive(Clone, Debug, PartialEq)]
struct DocView {
    team: api::Team,
    pages: Vec<api::TeamPageNode>,
}

/// Resolve `segments` against the tree: each segment is a slug under the
/// previous node (roots for the first). `None` = no such path.
fn resolve_path<'a>(
    pages: &'a [api::TeamPageNode],
    segments: &[&str],
) -> Option<&'a api::TeamPageNode> {
    let mut parent: Option<&str> = None;
    let mut found: Option<&api::TeamPageNode> = None;
    for segment in segments {
        let node = pages
            .iter()
            .find(|p| p.parent_id.as_deref() == parent && p.slug == *segment)?;
        parent = Some(&node.id);
        found = Some(node);
    }
    found
}

/// The slug path of a node (ancestors walked through the flat list).
fn path_of(pages: &[api::TeamPageNode], node: &api::TeamPageNode) -> String {
    let mut segments = vec![node.slug.clone()];
    let mut cursor = node.parent_id.clone();
    while let Some(parent_id) = cursor {
        match pages.iter().find(|p| p.id == parent_id) {
            Some(parent) => {
                segments.push(parent.slug.clone());
                cursor = parent.parent_id.clone();
            }
            None => break,
        }
    }
    segments.reverse();
    segments.join("/")
}

/// `/teams/:slug/pages/{path…}`.
#[component]
pub fn TeamDocPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let data = LocalResource::new(move || {
        let _ = auth.token();
        let slug = params.read().get("slug").unwrap_or_default();
        async move {
            let team = api::team_by_slug(auth, &slug).await?;
            let pages = api::team_pages(auth, &team.id).await?;
            Ok::<_, ApiError>(DocView { team, pages })
        }
    });
    view! {
        {move || match data.get() {
            None => view! { <Loading label="Loading page…"/> }.into_any(),
            Some(Err(ApiError::Http { status: 404, .. })) => view! {
                <PageHeader title="Team not found" sub="teams"/>
                <Panel title="Not found" caption="nothing lives at this address">
                    <Anchor href="/teams">"Back to the team directory"</Anchor>
                </Panel>
            }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| data.refetch())/>
            }.into_any(),
            Some(Ok(doc_view)) => {
                let path = params.read().get("path").unwrap_or_default();
                // Read whoami HERE, inside the reactive closure: when the
                // identity lands after the page fetch, the body re-renders
                // with the write affordances (server stays the authority).
                let manage = whoami
                    .and_then(|resource| resource.get())
                    .and_then(|result| result.ok())
                    .map(|me| {
                        me.organization.role == "admin"
                            || me.teams.iter().any(|t| t.id == doc_view.team.id)
                    })
                    .unwrap_or(false);
                view! {
                    <DocBody
                        doc_view
                        path
                        manage
                        on_changed=Callback::new(move |_| data.refetch())
                    />
                }.into_any()
            }
        }}
    }
}

/// The resolved page/folder view with breadcrumbs and management.
#[component]
fn DocBody(
    doc_view: DocView,
    path: String,
    manage: bool,
    on_changed: Callback<()>,
) -> impl IntoView {
    let DocView { team, pages } = doc_view;
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let Some(node) = resolve_path(&pages, &segments).cloned() else {
        let team_href = format!("/teams/{}", team.slug);
        return view! {
            <PageHeader title="Page not found" sub=format!("team · {}", team.slug)/>
            <Panel title="Not found" caption="no page lives at this path">
                <Empty message=format!("Nothing at {path:?} — it may have been renamed or moved.")/>
                <Anchor href=team_href>"Back to the team page"</Anchor>
            </Panel>
        }
        .into_any();
    };

    // Breadcrumbs: team page, then every ancestor path prefix.
    let team_href = format!("/teams/{}", team.slug);
    let team_name = team.name.clone();
    let mut crumb_views = vec![view! {
        <Anchor href=team_href.clone()>{team_name}</Anchor>
    }
    .into_any()];
    let mut prefix = String::new();
    for (index, segment) in segments.iter().enumerate() {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(segment);
        let title = resolve_path(&pages, &segments[..=index])
            .map(|n| n.title.clone())
            .unwrap_or_else(|| (*segment).to_string());
        crumb_views.push(view! { <Text dimmed=true size="sm">"/"</Text> }.into_any());
        if index + 1 == segments.len() {
            crumb_views.push(view! { <Text bright=true size="sm">{title}</Text> }.into_any());
        } else {
            let href = format!("/teams/{}/pages/{prefix}", team.slug);
            crumb_views.push(view! { <Anchor href=href>{title}</Anchor> }.into_any());
        }
    }

    let body = if node.kind == "folder" {
        view! {
            <FolderIndex
                team=team.clone()
                pages=pages.clone()
                folder=node.clone()
                manage
                on_changed
            />
        }
        .into_any()
    } else {
        view! {
            <PageView
                team=team.clone()
                pages=pages.clone()
                node=node.clone()
                manage
                on_changed
            />
        }
        .into_any()
    };

    view! {
        <PageHeader
            title=node.title.clone()
            sub=format!("team · {} · {}", team.slug, if node.kind == "folder" { "folder" } else { "page" })
        />
        <Stack gap="md">
            <Group gap="xs">{crumb_views}</Group>
            {body}
        </Stack>
    }
    .into_any()
}

/// A folder: its children as links, plus the create surface.
#[component]
fn FolderIndex(
    team: api::Team,
    pages: Vec<api::TeamPageNode>,
    folder: api::TeamPageNode,
    manage: bool,
    on_changed: Callback<()>,
) -> impl IntoView {
    let children = {
        let mut children = pages
            .iter()
            .filter(|p| p.parent_id.as_deref() == Some(folder.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        children.sort_by(|a, b| a.position.cmp(&b.position).then(a.title.cmp(&b.title)));
        children
    };
    let child_rows = children
        .iter()
        .map(|child| {
            let href = format!("/teams/{}/pages/{}", team.slug, path_of(&pages, child));
            let label = if child.kind == "folder" {
                format!("📁 {}", child.title)
            } else {
                child.title.clone()
            };
            view! { <Anchor href=href>{label}</Anchor> }.into_any()
        })
        .collect::<Vec<_>>();

    view! {
        <Panel title="Contents" caption="pages and folders inside">
            {if child_rows.is_empty() {
                view! { <Empty message="This folder is empty."/> }.into_any()
            } else {
                view! { <Stack gap="xs">{child_rows}</Stack> }.into_any()
            }}
        </Panel>
        {manage.then(|| view! {
            <CreateForm team_id=team.id.clone() parent_id=folder.id.clone() on_changed/>
            <StructurePanel team pages node=folder on_changed/>
        })}
    }
}

/// A page: rendered markdown, or the generalized editor when editing.
#[component]
fn PageView(
    team: api::Team,
    pages: Vec<api::TeamPageNode>,
    node: api::TeamPageNode,
    manage: bool,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let editing = RwSignal::new(false);
    let team_id = team.id.clone();
    let page_id = node.id.clone();
    let saver: Saver = Rc::new(move |title, content, version| {
        let team_id = team_id.clone();
        let page_id = page_id.clone();
        Box::pin(async move {
            api::save_page_content(auth, &team_id, &page_id, &title, &content, version).await
        })
    });
    // `new_local`: the Rc saver rides into a reactive closure (!Send).
    let saver = StoredValue::new_local(saver);
    let rendered = markdown::to_html(&node.content);
    let version_pill = format!("v{}", node.version);
    let initial_title = node.title.clone();
    let initial_content = node.content.clone();
    let initial_version = node.version;

    view! {
        {move || if editing.get() {
            let saver = saver.get_value();
            let initial_title = initial_title.clone();
            let initial_content = initial_content.clone();
            view! {
                <MarkdownEditor
                    initial_title
                    initial_content
                    initial_version
                    saver
                    on_saved=Callback::new(move |_| {
                        editing.set(false);
                        on_changed.run(());
                    })
                />
            }.into_any()
        } else {
            let rendered = rendered.clone();
            let version_pill = version_pill.clone();
            view! {
                <Panel title="Content" caption="markdown">
                    <Stack gap="sm">
                        <Group justify="between">
                            <Pill color=token::ICE>{version_pill}</Pill>
                            {manage.then(|| view! {
                                <Button variant="default" on_click=Callback::new(move |_| editing.set(true))>
                                    "Edit"
                                </Button>
                            })}
                        </Group>
                        <div class="kairos-markdown" inner_html=rendered></div>
                    </Stack>
                </Panel>
            }.into_any()
        }}
        {(manage && !node.is_protected).then(|| view! {
            <StructurePanel team=team.clone() pages=pages.clone() node=node.clone() on_changed/>
        })}
        {(manage && node.is_protected).then(|| view! {
            <Text dimmed=true size="xs">
                "The charter is protected: its content can be edited, but it cannot be renamed, moved, or deleted."
            </Text>
        })}
    }
}

/// The create surface inside a folder (KAIROS-T-0086): kind + slug +
/// title → POST.
#[component]
fn CreateForm(team_id: String, parent_id: String, on_changed: Callback<()>) -> impl IntoView {
    let auth = use_auth();
    let team_id = StoredValue::new(team_id);
    let parent_id = StoredValue::new(parent_id);
    let kind = RwSignal::new("page".to_string());
    let slug = RwSignal::new(String::new());
    let title = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let create = move |_| {
        let slug_value = slug.get_untracked().trim().to_string();
        let title_value = title.get_untracked().trim().to_string();
        if slug_value.is_empty() || title_value.is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::create_page(
                auth,
                &team_id.get_value(),
                Some(&parent_id.get_value()),
                &kind.get_untracked(),
                &slug_value,
                &title_value,
            )
            .await;
            busy.set(false);
            match result {
                Ok(_) => {
                    slug.set(String::new());
                    title.set(String::new());
                    on_changed.run(());
                }
                Err(e) => error.set(Some(error_text(&e))),
            }
        });
    };

    view! {
        <Panel title="New page or folder" caption="created inside this folder">
            <Stack gap="sm">
                {move || error.get().map(|message| view! {
                    <Alert title="Create failed" color=token::BAD>
                        <Text size="sm" dimmed=true>{message}</Text>
                    </Alert>
                })}
                <Group gap="sm">
                    <select
                        class="cl-input"
                        prop:value=move || kind.get()
                        on:change=move |e| kind.set(event_target_value(&e))
                    >
                        <option value="page">"page"</option>
                        <option value="folder">"folder"</option>
                    </select>
                    <TextInput label="Slug" value=slug/>
                    <TextInput label="Title" value=title/>
                    <button
                        class="cl-btn cl-btn--filled"
                        disabled=move || {
                            busy.get()
                                || slug.get().trim().is_empty()
                                || title.get().trim().is_empty()
                        }
                        on:click=create
                    >
                        {move || if busy.get() { "Creating…" } else { "Create" }}
                    </button>
                </Group>
            </Stack>
        </Panel>
    }
}

/// Rename / move / delete for an unprotected node (KAIROS-T-0086). The
/// move select offers every folder EXCEPT the node and its descendants
/// (a folder inside itself is a cycle — the server refuses it too) plus
/// the tree root. Delete confirms with a second click; folders with live
/// children surface the server's FOLDER_NOT_EMPTY count.
#[component]
fn StructurePanel(
    team: api::Team,
    pages: Vec<api::TeamPageNode>,
    node: api::TeamPageNode,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let navigate = use_navigate();
    let team_slug = team.slug.clone();
    let team_id = StoredValue::new(team.id.clone());
    let page_id = StoredValue::new(node.id.clone());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let new_slug = RwSignal::new(node.slug.clone());
    let confirm_delete = RwSignal::new(false);

    // Folders the node could move into: not itself, not its descendants.
    let descendants = {
        let mut blocked = vec![node.id.clone()];
        loop {
            let more = pages
                .iter()
                .filter(|p| {
                    p.parent_id
                        .as_ref()
                        .is_some_and(|parent| blocked.contains(parent))
                        && !blocked.contains(&p.id)
                })
                .map(|p| p.id.clone())
                .collect::<Vec<_>>();
            if more.is_empty() {
                break;
            }
            blocked.extend(more);
        }
        blocked
    };
    let mut move_targets = pages
        .iter()
        .filter(|p| p.kind == "folder" && !descendants.contains(&p.id))
        .map(|p| (p.id.clone(), path_of(&pages, p)))
        .collect::<Vec<_>>();
    move_targets.sort_by(|a, b| a.1.cmp(&b.1));
    let move_choice = RwSignal::new(node.parent_id.clone().unwrap_or_default());

    let rename = move |_| {
        let slug_value = new_slug.get_untracked().trim().to_string();
        if slug_value.is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result =
                api::rename_page(auth, &team_id.get_value(), &page_id.get_value(), &slug_value)
                    .await;
            busy.set(false);
            match result {
                Ok(_) => on_changed.run(()),
                Err(e) => error.set(Some(error_text(&e))),
            }
        });
    };
    let do_move = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let choice = move_choice.get_untracked();
            let parent = (!choice.is_empty()).then_some(choice);
            let result = api::move_page(
                auth,
                &team_id.get_value(),
                &page_id.get_value(),
                parent.as_deref(),
            )
            .await;
            busy.set(false);
            match result {
                Ok(_) => on_changed.run(()),
                Err(e) => error.set(Some(error_text(&e))),
            }
        });
    };
    let parent_href = {
        let parent_path = node
            .parent_id
            .as_ref()
            .and_then(|parent_id| pages.iter().find(|p| &p.id == parent_id))
            .map(|parent| format!("/teams/{team_slug}/pages/{}", path_of(&pages, parent)));
        parent_path.unwrap_or_else(|| format!("/teams/{team_slug}"))
    };
    let delete = move |_| {
        if !confirm_delete.get_untracked() {
            confirm_delete.set(true);
            return;
        }
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        let parent_href = parent_href.clone();
        leptos::task::spawn_local(async move {
            let result =
                api::delete_page(auth, &team_id.get_value(), &page_id.get_value()).await;
            busy.set(false);
            match result {
                Ok(()) => navigate(&parent_href, Default::default()),
                Err(e) => {
                    confirm_delete.set(false);
                    error.set(Some(error_text(&e)));
                }
            }
        });
    };

    view! {
        <Panel title="Manage" caption="rename · move · delete">
            <Stack gap="sm">
                {move || error.get().map(|message| view! {
                    <Alert title="Change failed" color=token::BAD>
                        <Text size="sm" dimmed=true>{message}</Text>
                    </Alert>
                })}
                <Group gap="sm">
                    <TextInput label="Slug" value=new_slug/>
                    <Button variant="default" on_click=Callback::new(rename)>"Rename"</Button>
                </Group>
                <Group gap="sm">
                    <label class="cl-field__label">"Move to"</label>
                    <select
                        class="cl-input"
                        prop:value=move || move_choice.get()
                        on:change=move |e| move_choice.set(event_target_value(&e))
                    >
                        <option value="">"(root)"</option>
                        {move_targets.iter().map(|(id, label)| {
                            let id = id.clone();
                            let label = label.clone();
                            view! { <option value=id>{label}</option> }
                        }).collect_view()}
                    </select>
                    <Button variant="default" on_click=Callback::new(do_move)>"Move"</Button>
                </Group>
                <Group gap="sm" justify="end">
                    <button
                        class="cl-btn"
                        disabled=move || busy.get()
                        on:click=delete
                    >
                        {move || if confirm_delete.get() { "Really delete?" } else { "Delete" }}
                    </button>
                </Group>
            </Stack>
        </Panel>
    }
}
