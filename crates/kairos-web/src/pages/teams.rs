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
//! - directory: one `list_teams` + `team_members` per team (member counts;
//!   org scale is tens of teams, so the fan-out is fine at this size);
//! - detail: team by slug from `list_teams`, roster via `team_members`,
//!   delivery board resolved through `list_board_refs`, and stream
//!   membership by checking each stream's `stream_teams` (no reverse
//!   endpoint; stream counts are small at org scale).

pub(crate) mod api;

use aurora_dark::components::{
    Anchor, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Stack, Text,
};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::auth::use_auth;
use api::team_type_color;

/// Lifecycle chip accent — the same mapping as the item detail's
/// lifecycle panel (KAIROS-T-0078), so the states read identically.
fn lifecycle_color(state: &str) -> &'static str {
    use aurora_dark::tokens::token;
    match state {
        "published" => token::OK,
        "review" => token::GOLD,
        _ => token::MUTED,
    }
}

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

/// `/teams` — every team: name, type, member count, linking to detail.
#[component]
pub fn TeamsPage() -> impl IntoView {
    let auth = use_auth();
    let rows = LocalResource::new(move || {
        let _ = auth.token();
        async move {
            let teams = api::list_teams(auth).await?;
            let mut rows = Vec::with_capacity(teams.len());
            for team in teams {
                let members = api::team_members(auth, &team.id).await?;
                rows.push(DirectoryRow {
                    name: team.name,
                    slug: team.slug,
                    team_type: team.team_type,
                    member_count: members.len(),
                });
            }
            Ok::<_, aurora_dark::tokens::ApiError>(rows)
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
                            <a class="kairos-board-tile" href=href>
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
                            </a>
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
    /// Documents attached to the team's work (KAIROS-T-0084, derived).
    work_documents: Vec<api::WorkDocument>,
}

/// Load everything the detail page shows. The slug resolves through the
/// teams list (the API keys teams by id; slugs are the human handle).
async fn load_team_view(
    auth: crate::auth::Auth,
    slug: &str,
) -> Result<TeamView, aurora_dark::tokens::ApiError> {
    let teams = api::list_teams(auth).await?;
    let team = teams
        .into_iter()
        .find(|team| team.slug == slug)
        .ok_or_else(|| aurora_dark::tokens::ApiError::Http {
            status: 404,
            message: format!("no team with slug {slug:?}"),
            code: Some("NOT_FOUND".to_string()),
        })?;
    let members = api::team_members(auth, &team.id).await?;

    let delivery_board = match &team.delivery_board_id {
        Some(board_id) => api::list_board_refs(auth)
            .await?
            .into_iter()
            .find(|board| &board.id == board_id)
            .map(|board| (board.name, board.slug)),
        None => None,
    };

    // Stream membership has no reverse endpoint: check each stream's team
    // list. Org-scale stream counts keep this cheap; a failing stream read
    // fails the whole load (consistent error surface beats a silent gap).
    let mut streams = Vec::new();
    for stream in api::list_streams(auth).await? {
        let stream_team_ids = api::stream_teams(auth, &stream.id).await?;
        if stream_team_ids.iter().any(|t| t.id == team.id) {
            streams.push(stream);
        }
    }

    let work_documents = api::team_work_documents(auth, &team.id).await?;

    Ok(TeamView {
        team,
        members,
        delivery_board,
        streams,
        work_documents,
    })
}

/// `/teams/:slug` — roster, delivery board, and stream membership.
#[component]
pub fn TeamPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let team = LocalResource::new(move || {
        let _ = auth.token();
        let slug = params.read().get("slug").unwrap_or_default();
        async move { load_team_view(auth, &slug).await }
    });
    view! {
        {move || match team.get() {
            None => view! { <Loading label="Loading team…"/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| team.refetch())/>
            }.into_any(),
            Some(Ok(view_model)) => view! { <TeamBody view_model/> }.into_any(),
        }}
    }
}

/// The loaded team detail.
#[component]
fn TeamBody(view_model: TeamView) -> impl IntoView {
    let TeamView {
        team,
        members,
        delivery_board,
        streams,
        work_documents,
    } = view_model;
    let sub = format!("team · {}", team.slug);
    let type_pill = team.team_type.clone();
    let header_right: Children = Box::new(move || {
        view! { <Pill color=team_type_color(&type_pill)>{type_pill.clone()}</Pill> }.into_any()
    });

    view! {
        <PageHeader title=team.name.clone() sub=sub right=header_right/>
        <Stack gap="md">
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
            <Panel title="Work documents" caption="documents attached to this team's work items">
                {if work_documents.is_empty() {
                    view! {
                        <Empty message="No documents attached to this team's work items yet — documents under org-level items live with their parent."/>
                    }.into_any()
                } else {
                    view! {
                        <Stack gap="sm">
                            {work_documents.into_iter().map(|doc| {
                                let api::WorkDocument {
                                    short_code, title, lifecycle,
                                    parent_short_code, parent_title, ..
                                } = doc;
                                view! {
                                    <Stack gap="xs">
                                        <Group justify="between">
                                            <Anchor href=format!("/items/{short_code}")>
                                                {format!("{short_code} — {title}")}
                                            </Anchor>
                                            <Pill color=lifecycle_color(&lifecycle)>{lifecycle.clone()}</Pill>
                                        </Group>
                                        <Group gap="xs">
                                            <Text dimmed=true size="xs">"supports"</Text>
                                            <Anchor href=format!("/items/{parent_short_code}")>
                                                <Text dimmed=true size="xs">
                                                    {format!("{parent_short_code} — {parent_title}")}
                                                </Text>
                                            </Anchor>
                                        </Group>
                                    </Stack>
                                }
                            }).collect_view()}
                        </Stack>
                    }.into_any()
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
        </Stack>
    }
}
