//! User-facing team pages (KAIROS-T-0067, KAIROS-I-0006): the `/teams`
//! directory and the `/teams/:slug` detail — the member-readable answer to
//! "who is on my team, what is our board, which streams are we in".
//!
//! Read/navigate only: team CRUD and membership management stay in
//! `/admin/teams` (initiative non-goal — no second write surface). Every
//! read here is plain-authenticated (MANAGE gates writes alone), so these
//! pages render for non-admins.
//!
//! Data (`self::api`, the shared team data layer):
//! - directory: one `list_teams`, then `team_members` of every team at
//!   once (member counts; KAIROS-T-0622: together, not one after another,
//!   so 20 teams are two round trips and not 21);
//! - detail: team by slug from `list_teams`, roster via `team_members`,
//!   delivery board resolved through `list_board_refs`, and stream
//!   membership by checking each stream's `stream_teams` (no reverse
//!   endpoint; stream counts are small at org scale).

pub(crate) mod api;
pub(crate) mod archive;
pub(crate) mod doc;
pub(crate) mod tree;

use aurora_dark::components::{
    ActionIcon, Alert, Anchor, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill,
    Stack, Text, Textarea,
};
use aurora_dark::data::RelativeTime;
use aurora_dark::frame::Card;
use aurora_dark::tokens::ApiError;
use futures_util::future::{join_all, try_join_all};
use futures_util::join;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::auth::use_auth;
use crate::pages::item::api::error_text;
use crate::pages::item::markdown;
use api::team_type_color;

// ---------------------------------------------------------------------------
// Directory
// ---------------------------------------------------------------------------

/// One directory row, fully resolved before rendering.
#[derive(Clone, Debug, PartialEq)]
struct DirectoryRow {
    name: String,
    slug: String,
    team_type: String,
    member_count: usize,
}

/// The directory rows: each team with its member count, in the order of
/// `list_teams`. `counts[i]` is the count of `teams[i]`. Pure, host-tested.
fn directory_rows(teams: Vec<api::Team>, counts: Vec<usize>) -> Vec<DirectoryRow> {
    teams
        .into_iter()
        .zip(counts)
        .map(|(team, member_count)| DirectoryRow {
            name: team.name,
            slug: team.slug,
            team_type: team.team_type,
            member_count,
        })
        .collect()
}

/// `/teams` — every team: name, type, member count, linking to detail.
#[component]
pub fn TeamsPage() -> impl IntoView {
    let auth = use_auth();
    let rows = LocalResource::new(move || {
        auth.track_session();
        async move {
            let teams = api::list_teams(auth).await?;
            // KAIROS-T-0622: the members of every team at once.
            let members =
                try_join_all(teams.iter().map(|team| api::team_members(auth, &team.id))).await?;
            let counts = members.iter().map(Vec::len).collect();
            Ok::<_, aurora_dark::tokens::ApiError>(directory_rows(teams, counts))
        }
    });
    view! {
        <PageHeader title="Teams" sub="who does what — the team directory"/>
        {move || match rows.get() {
            None => view! { <Loading label="Loading teams…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| rows.refetch())/>
            }.into_any(),
            Some(Ok(rows)) if rows.is_empty() => view! {
                <Empty message="No teams yet — an org admin can create one from Admin."/>
            }.into_any(),
            Some(Ok(rows)) => view! {
                <div class="kairos-board-grid">
                    {rows.into_iter().map(|row| {
                        let DirectoryRow { name, slug, team_type, member_count } = row;
                        let href = format!("/teams/{slug}");
                        let members = match member_count {
                            1 => "1 member".to_string(),
                            n => format!("{n} members"),
                        };
                        view! {
                            // COLLIERY-T-1836: an Aurora `Card`, as on /boards.
                            <div class="kairos-board-tile">
                                <Card href=href>
                                    <Stack gap="xs">
                                        <Group justify="between">
                                            <Text bright=true bold=true>{name}</Text>
                                            <Pill color=team_type_color(&team_type)>{team_type}</Pill>
                                        </Group>
                                        <Group justify="between">
                                            <Text mono=true dimmed=true size="xs">{slug}</Text>
                                            <Text dimmed=true size="xs">{members}</Text>
                                        </Group>
                                    </Stack>
                                </Card>
                            </div>
                        }
                    }).collect_view()}
                </div>
            }.into_any(),
        }}
    }
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------

/// The fully-resolved `/teams/:slug` view model (fetched as one unit so
/// the page renders whole, per the async-view convention).
#[derive(Clone, Debug, PartialEq)]
struct TeamView {
    team: api::Team,
    members: Vec<api::TeamMember>,
    /// `(name, slug)` of the team's delivery board, when resolvable.
    delivery_board: Option<(String, String)>,
    /// Streams this team participates in.
    streams: Vec<api::DeliveryStream>,
    /// The documents and ADRs of the team, with their places in the tree
    /// (KAIROS-T-0437; they replace the "Work documents" panel).
    items: Vec<api::TreeItem>,
    /// The full page tree, flat: the charter and the pages of the team.
    pages: Vec<api::TeamPageNode>,
    /// Pinned first, newest first (server ordering).
    announcements: Vec<api::Announcement>,
    /// In-flight forge links across the team's work (KAIROS-T-0101).
    links: Vec<api::TeamLink>,
    /// In-flight branches and pull requests of the team's repositories
    /// that no item is linked to (KAIROS-T-0480).
    unlinked: Vec<crate::pages::repositories::api::ForgeRef>,
    /// The repositories this team owns (KAIROS-T-0109, A-0019). Empty
    /// when the directory read failed — the panel degrades, the page
    /// does not (KAIROS-T-0114).
    repositories: Vec<crate::pages::repositories::api::Repository>,
}

/// The team's delivery board as `(name, slug)`, resolved through the
/// board list (`None` when the team has no board or it is not listed).
async fn load_delivery_board(
    auth: crate::auth::Auth,
    delivery_board_id: Option<&str>,
) -> Result<Option<(String, String)>, aurora_dark::tokens::ApiError> {
    let Some(board_id) = delivery_board_id else {
        return Ok(None);
    };
    Ok(api::list_board_refs(auth)
        .await?
        .into_iter()
        .find(|board| board.id == board_id)
        .map(|board| (board.name, board.slug)))
}

/// Stream membership has no reverse endpoint: check each stream's team
/// list (concurrently — org-scale stream counts keep this cheap). A
/// failing stream read fails the whole load (consistent error surface
/// beats a silent gap).
async fn load_streams(
    auth: crate::auth::Auth,
    team_id: &str,
) -> Result<Vec<api::DeliveryStream>, aurora_dark::tokens::ApiError> {
    let streams = api::list_streams(auth).await?;
    let memberships = join_all(
        streams
            .iter()
            .map(|stream| api::stream_teams(auth, &stream.id)),
    )
    .await;
    let mut mine = Vec::new();
    for (stream, teams) in streams.into_iter().zip(memberships) {
        if teams?.iter().any(|t| t.id == team_id) {
            mine.push(stream);
        }
    }
    Ok(mine)
}

/// Load everything the detail page shows, resolving the slug through
/// `GET /api/teams/by-slug/{slug}` (KAIROS-T-0085 — no directory scan).
/// The per-team reads run concurrently once the team is known
/// (KAIROS-T-0114); only the repositories read degrades to empty on
/// failure — everything else is load-bearing for the layout.
async fn load_team_view(
    auth: crate::auth::Auth,
    slug: &str,
) -> Result<TeamView, aurora_dark::tokens::ApiError> {
    let team = api::team_by_slug(auth, slug).await?;
    let team_id = team.id.as_str();

    let (
        members,
        delivery_board,
        streams,
        items,
        pages,
        announcements,
        links,
        repositories,
        unlinked,
    ) = join!(
        api::team_members(auth, team_id),
        load_delivery_board(auth, team.delivery_board_id.as_deref()),
        load_streams(auth, team_id),
        api::tree_items(auth, team_id),
        api::team_pages(auth, team_id),
        api::team_announcements(auth, team_id),
        api::team_links(auth, team_id),
        crate::pages::repositories::api::list_repositories(auth, Some(team_id)),
        api::team_unlinked_refs(auth, team_id),
    );
    let members = members?;
    let delivery_board = delivery_board?;
    let streams = streams?;
    let items = items?;
    let mut pages = pages?;
    // KAIROS-T-0629: the list has no content; read the charter.
    if let Some(charter) = pages.iter_mut().find(|p| p.is_charter()) {
        charter.content = api::team_page(auth, team_id, &charter.id).await?.content;
    }
    let announcements = announcements?;
    let links = links?;
    let repositories = repositories.unwrap_or_default();
    // An older server has no route: no unlinked rows.
    let unlinked = unlinked.unwrap_or_default();

    Ok(TeamView {
        team,
        members,
        delivery_board,
        streams,
        items,
        pages,
        announcements,
        links,
        repositories,
        unlinked,
    })
}

/// `/teams/:slug` — the fixed v1 landing layout (KAIROS-T-0085):
/// header, Charter, Announcements, Members, Delivery board, Streams,
/// Documentation tree, Work documents.
#[component]
pub fn TeamPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let team = LocalResource::new(move || {
        auth.track_session();
        let slug = params.read().get("slug").unwrap_or_default();
        async move { load_team_view(auth, &slug).await }
    });
    view! {
        {move || match team.get() {
            None => view! { <Loading label="Loading team…"/> }.into_any(),
            // Unknown slug: a clean not-found, not a generic error wall.
            Some(Err(aurora_dark::tokens::ApiError::Http { status: 404, .. })) => {
                let slug = params.read().get("slug").unwrap_or_default();
                view! {
                    <PageHeader title="Team not found" sub="teams"/>
                    <Panel title="Not found" caption="nothing lives at this address">
                        <Empty message=format!("There is no team with slug {slug:?} — check the directory.")/>
                        <Anchor href="/teams">"Back to the team directory"</Anchor>
                    </Panel>
                }.into_any()
            }
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| team.refetch())/>
            }.into_any(),
            Some(Ok(view_model)) => view! {
                <TeamBody view_model on_changed=Callback::new(move |_| team.refetch())/>
            }.into_any(),
        }}
    }
}

/// The loaded team detail — panels in the fixed v1 order.
#[component]
fn TeamBody(view_model: TeamView, on_changed: Callback<()>) -> impl IntoView {
    let TeamView {
        team,
        members,
        delivery_board,
        streams,
        items,
        pages,
        announcements,
        links,
        repositories,
        unlinked,
    } = view_model;
    // KAIROS-T-0094: who may add a root-level page. Same rule and same source as
    // the announcement composer further down — org admin, or a member of THIS
    // team. Read from the shared whoami resource rather than refetched.
    let whoami_for_gate = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let gate_team_id = StoredValue::new(team.id.clone());
    let can_author = move || {
        whoami_for_gate
            .and_then(|resource| resource.get())
            .and_then(|result| {
                result.ok().map(|me| {
                    me.organization.role == "admin"
                        || me.teams.iter().any(|t| t.id == gate_team_id.get_value())
                })
            })
            .unwrap_or(false)
    };
    let sub = format!("team · {}", team.slug);
    let type_pill = team.team_type.clone();
    // KAIROS-T-0724: the stand-up of the team, for each reader.
    let standup_href = super::standup::standup_href(&team.slug);
    let header_right: Children = Box::new(move || {
        view! {
            <Group gap="xs">
                <Button variant="default" size="xs" href=standup_href.clone()>"Stand-up"</Button>
                <Pill color=team_type_color(&type_pill)>{type_pill.clone()}</Pill>
            </Group>
        }
        .into_any()
    });
    let charter = pages.iter().find(|p| p.is_charter()).cloned();
    let team_id = team.id.clone();
    let team_slug = team.slug.clone();
    let charter_href = format!("/teams/{team_slug}/pages/charter");

    view! {
        <PageHeader title=team.name.clone() sub=sub right=header_right/>
        <Stack gap="md">
            <Panel title="Charter" caption="why this team exists">
                {match charter {
                    Some(page) => view! {
                        <Stack gap="xs">
                            <div class="kairos-markdown" inner_html=markdown::to_html(&page.content)></div>
                            <Anchor href=charter_href>"Open / edit the charter"</Anchor>
                        </Stack>
                    }.into_any(),
                    None => view! {
                        <Empty message="This team has no charter page."/>
                    }.into_any(),
                }}
            </Panel>
            <AnnouncementsPanel team_id=team_id.clone() announcements on_changed/>
            <Panel title="Members" caption="the roster">
                {if members.is_empty() {
                    view! {
                        <Empty message="No members yet — an org admin can add some from Admin → Teams."/>
                    }.into_any()
                } else {
                    view! {
                        <Stack gap="xs">
                            {members.into_iter().map(|member| view! {
                                <Group justify="between">
                                    <Text bright=true size="sm">{member.display_name}</Text>
                                    <Text mono=true dimmed=true size="xs">{member.email}</Text>
                                </Group>
                            }).collect_view()}
                        </Stack>
                    }.into_any()
                }}
            </Panel>
            <Panel title="Delivery board" caption="where the work flows">
                {match delivery_board {
                    Some((name, slug)) => view! {
                        <Group justify="between">
                            <Anchor href=format!("/boards/{slug}")>{name}</Anchor>
                            <Text mono=true dimmed=true size="xs">{slug.clone()}</Text>
                        </Group>
                    }.into_any(),
                    None => view! {
                        <Empty message="This team has no delivery board."/>
                    }.into_any(),
                }}
            </Panel>
            <Panel title="Delivery streams" caption="cross-team streams this team works in">
                {if streams.is_empty() {
                    view! { <Empty message="This team is not part of any delivery stream."/> }
                        .into_any()
                } else {
                    view! {
                        <Stack gap="xs">
                            {streams.into_iter().map(|stream| view! {
                                <Group justify="between">
                                    <Text bright=true size="sm">{stream.name}</Text>
                                    {stream.description.map(|description| view! {
                                        <Text dimmed=true size="xs">{description}</Text>
                                    })}
                                </Group>
                            }).collect_view()}
                        </Stack>
                    }.into_any()
                }}
            </Panel>
            // KAIROS-T-0438: the tree is grown in place. A member (or an org
            // admin) gets "+ Page" and "+ Folder" in each folder and at the
            // root, and moves nodes by drag and drop. Re-rendered when the
            // identity lands, like the announcement composer.
            <Panel title="Documentation" caption="the team's page tree — folders expand, pages open">
                // KAIROS-T-0439: the Pages view holds the tree and the archive.
                <Anchor href=format!("/teams/{}/pages/", team_slug)>
                    <Text dimmed=true size="xs">"All pages, the archive and restore"</Text>
                </Anchor>
                {
                    let team_id_for_tree = team.id.clone();
                    let team_slug_for_tree = team_slug.clone();
                    let pages_for_tree = pages.clone();
                    let items_for_tree = items.clone();
                    move || view! {
                        <tree::TeamTree
                            team_id=team_id_for_tree.clone()
                            team_slug=team_slug_for_tree.clone()
                            pages=pages_for_tree.clone()
                            items=items_for_tree.clone()
                            manage=can_author()
                            on_changed
                        />
                    }
                }
            </Panel>
            <Panel title="Repositories" caption="the codebases this team owns; open counts cover all boards">
                {if repositories.is_empty() {
                    view! {
                        <Empty message="This team has no repositories. An organization admin adds one on the page Admin, Repositories. A member of this team adds one with the CLI (kairos repos create). An agent adds one with the MCP tool add_repository."/>
                    }.into_any()
                } else {
                    view! {
                        <Stack gap="sm">
                            {repositories.into_iter().map(|repo| {
                                let name = format!("{} · {}", repo.forge, repo.repo_full_name);
                                let open = format!("{} open", repo.open_tasks);
                                let webhook = if repo.has_webhook { "webhooks" } else { "no webhooks" };
                                let slug_attr = repo.slug.clone();
                                let documents_slug = repo.slug.clone();
                                let refs_slug = repo.slug.clone();
                                let code_index_slug = repo.slug.clone();
                                let owner_team_slug = repo.team.slug.clone();
                                let site = repo.site.clone();
                                // KAIROS-T-0124 #6a: the "how to work here"
                                // blurb agents read over MCP is visible to the
                                // humans on the team page too.
                                let description = (!repo.description.trim().is_empty())
                                    .then(|| repo.description.trim().to_string());
                                view! {
                                    <Stack gap="xs" attr:data-repo=slug_attr>
                                        <Group justify="between" wrap=true>
                                            <Group gap="sm" wrap=true>
                                                <Pill color=aurora_dark::tokens::token::ICE>{repo.slug.clone()}</Pill>
                                                <Anchor href=repo.repo_url.clone() attr:target="_blank" attr:rel="noopener noreferrer">
                                                    {name}
                                                </Anchor>
                                            </Group>
                                            <Group gap="sm">
                                                <Text dimmed=true size="xs">{open}</Text>
                                                <Text dimmed=true size="xs">{webhook}</Text>
                                            </Group>
                                        </Group>
                                        {description.map(|text| view! {
                                            <div class="kairos-team__repo-description">
                                                <Text dimmed=true size="sm">{text}</Text>
                                            </div>
                                        })}
                                        // COLLIERY-T-0269: the vision, the
                                        // architecture and the decisions
                                        // about the repository.
                                        <crate::pages::repositories::documents::RepositoryDocuments
                                            slug=documents_slug/>
                                        // KAIROS-T-0480: open work with no item.
                                        <crate::pages::repositories::refs::UnlinkedRefs
                                            slug=refs_slug/>
                                        <crate::pages::repositories::site::RepositorySiteLine site/>
                                        <crate::pages::repositories::code_index::CodeIndexPanel
                                            slug=code_index_slug owner_team=owner_team_slug/>
                                    </Stack>
                                }
                            }).collect_view()}
                        </Stack>
                    }.into_any()
                }}
            </Panel>
            <Panel title="In flight" caption="open work across this team's repos">
                {if links.is_empty() {
                    view! {
                        <Empty message="Nothing open across this team's repos — this counts pull requests and branches on this team's work items, plus repos attributed to the team."/>
                    }.into_any()
                } else {
                    view! {
                        <Stack gap="sm">
                            {links.into_iter().map(|link| {
                                let label = if link.kind == "pull_request" {
                                    format!("#{} {}", link.external_id, link.title)
                                } else {
                                    link.title.clone()
                                };
                                let repo = format!("{} · {}", link.forge, link.repo_full_name);
                                let item_href = format!("/items/{}", link.item_short_code);
                                let item_label = format!(
                                    "{} — {}",
                                    link.item_short_code, link.item_title
                                );
                                view! {
                                    <Stack gap="xs">
                                        <Group justify="between" wrap=true>
                                            <Group gap="sm" wrap=true>
                                                <Pill color=link_state_color(&link.state)>
                                                    {link.state.clone()}
                                                </Pill>
                                                <Anchor href=link.url.clone() attr:target="_blank" attr:rel="noopener noreferrer">
                                                    {label}
                                                </Anchor>
                                            </Group>
                                            <Text mono=true dimmed=true size="xs">{repo}</Text>
                                        </Group>
                                        <Group gap="xs">
                                            <Text dimmed=true size="xs">"on"</Text>
                                            <Anchor href=item_href>
                                                <Text dimmed=true size="xs">{item_label}</Text>
                                            </Anchor>
                                        </Group>
                                    </Stack>
                                }
                            }).collect_view()}
                        </Stack>
                    }.into_any()
                }}
                // KAIROS-T-0480: open branches and pull requests with no item.
                {(!unlinked.is_empty()).then(|| view! {
                    <Stack gap="xs" attr:data-testid="team-unlinked-refs">
                        <Text size="sm">"Not linked to an item"</Text>
                        {unlinked.into_iter().map(|forge_ref| view! {
                            <crate::pages::repositories::refs::ForgeRefRow forge_ref/>
                        }).collect_view()}
                    </Stack>
                })}
            </Panel>
        </Stack>
    }
}

/// The accent for a forge link's state — same mapping as the item
/// detail's Development panel (KAIROS-T-0100). Red stays reserved.
pub(crate) fn link_state_color(state: &str) -> &'static str {
    use aurora_dark::tokens::token;
    match state {
        "open" => token::ICE,
        "merged" => token::VIOLET,
        _ => token::MUTED,
    }
}

// ---------------------------------------------------------------------------
// Announcements (KAIROS-T-0085): one-way, pinned-first, append-only
// ---------------------------------------------------------------------------

/// Pinned-first feed with a member/org-admin post box and an author-or-
/// admin delete affordance. Deliberately NO comments or reactions — the
/// feed is one-way by design (KAIROS-I-0007). The server stays the
/// authority on every gate; whoami only decides what to show.
#[component]
fn AnnouncementsPanel(
    team_id: String,
    announcements: Vec<api::Announcement>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let auth = use_auth();
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let team_id = StoredValue::new(team_id);
    let draft = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    // `(my user id, org admin?, member of THIS team?)` once whoami lands.
    let identity = move || {
        whoami
            .and_then(|resource| resource.get())
            .and_then(|result| {
                result.ok().map(|me| {
                    let is_admin = me.organization.role == "admin";
                    let is_member = me.teams.iter().any(|t| t.id == team_id.get_value());
                    (me.user.id, is_admin, is_member)
                })
            })
    };

    let post = move |_: ()| {
        let body = draft.get_untracked();
        if body.trim().is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result =
                api::post_announcement(auth, &team_id.get_value(), body.trim(), false).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    draft.set(String::new());
                    on_changed.run(());
                }
                Err(e) => error.set(Some(error_text(&e))),
            }
        });
    };
    let delete = move |announcement_id: String| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result =
                api::delete_announcement(auth, &team_id.get_value(), &announcement_id).await;
            busy.set(false);
            match result {
                Ok(()) => on_changed.run(()),
                Err(e) => error.set(Some(error_text(&e))),
            }
        });
    };

    view! {
        <Panel title="Announcements" caption="one-way — no comments, no reactions">
            <Stack gap="sm">
                {move || error.get().map(|message| view! {
                    <Alert title="Announcement call failed" color=aurora_dark::tokens::token::BAD>
                        <Text size="sm" dimmed=true>{message}</Text>
                    </Alert>
                })}
                {if announcements.is_empty() {
                    view! { <Empty message="Nothing announced yet."/> }.into_any()
                } else {
                    view! {
                        <Stack gap="sm">
                            {announcements.into_iter().map(|announcement| {
                                let api::Announcement { id, body, pinned, created_by, created_at } = announcement;
                                let delete_id = id.clone();
                                view! {
                                    <Stack gap="xs">
                                        <Group justify="between">
                                            <Group gap="xs">
                                                {pinned.then(|| view! {
                                                    <Pill color=aurora_dark::tokens::token::GOLD>"pinned"</Pill>
                                                })}
                                                <Text dimmed=true size="xs"><RelativeTime iso=created_at/></Text>
                                            </Group>
                                            {move || {
                                                let mine_or_admin = identity()
                                                    .map(|(my_id, is_admin, _)| is_admin || my_id == created_by)
                                                    .unwrap_or(false);
                                                let delete_id = delete_id.clone();
                                                mine_or_admin.then(|| view! {
                                                    <ActionIcon
                                                        title="Delete this announcement"
                                                        on_click=Callback::new(move |_| delete(delete_id.clone()))
                                                    >
                                                        "×"
                                                    </ActionIcon>
                                                })
                                            }}
                                        </Group>
                                        <div class="kairos-markdown" inner_html=markdown::to_html(&body)></div>
                                    </Stack>
                                }
                            }).collect_view()}
                        </Stack>
                    }.into_any()
                }}
                {move || {
                    let can_post = identity()
                        .map(|(_, is_admin, is_member)| is_admin || is_member)
                        .unwrap_or(false);
                    can_post.then(|| view! {
                        <Stack gap="xs">
                            <Textarea
                                label="Post an announcement (markdown)"
                                value=draft
                                rows=6
                                mono=true
                            />
                            <Group justify="end">
                                <Button
                                    disabled=move || draft.get().trim().is_empty()
                                    loading=busy
                                    loading_label="Posting…"
                                    on_click=Callback::new(post)
                                >
                                    "Post"
                                </Button>
                            </Group>
                        </Stack>
                    })
                }}
            </Stack>
        </Panel>
    }
}

#[cfg(test)]
mod tests {
    use super::{DirectoryRow, api, directory_rows};

    fn team(slug: &str) -> api::Team {
        serde_json::from_value(serde_json::json!({
            "id": format!("id-{slug}"),
            "name": slug.to_uppercase(),
            "slug": slug,
            "team_type": "stream_aligned",
            "delivery_board_id": null,
        }))
        .expect("a team")
    }

    /// KAIROS-T-0622: the counts read together land on their own teams, in
    /// the order of the team list.
    #[test]
    fn each_count_lands_on_its_own_team() {
        let rows = directory_rows(vec![team("web"), team("platform")], vec![3, 1]);
        assert_eq!(
            rows,
            [
                DirectoryRow {
                    name: "WEB".to_string(),
                    slug: "web".to_string(),
                    team_type: "stream_aligned".to_string(),
                    member_count: 3,
                },
                DirectoryRow {
                    name: "PLATFORM".to_string(),
                    slug: "platform".to_string(),
                    team_type: "stream_aligned".to_string(),
                    member_count: 1,
                },
            ]
        );
        assert!(directory_rows(Vec::new(), Vec::new()).is_empty());
    }
}
