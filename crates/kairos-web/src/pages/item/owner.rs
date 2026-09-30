//! The owner board of a document, and the repositories that a document or
//! an ADR impacts (COLLIERY-T-0269, COLLIERY-I-0019).
//!
//! The owner decided the model on 2026-09-29. A document has two links,
//! and they say two different things:
//!
//! - the OWNER BOARD gives the right to edit the document
//!   ([`OwnerBoardPanel`]). The document is not a card of that board: it
//!   has no column, and the panel shows none;
//! - an IMPACTS link says which repository the document is about
//!   ([`ImpactsPanel`]). It gives no right.
//!
//! The two panels have different gates, as the server has:
//!
//! - the change of the owner board is a MOVE. The user needs
//!   `manage_documents` on the board that owns the document now and on the
//!   new board, and the creator of the document gets no right to it;
//! - an impacts link is an EDIT. The edit rule applies
//!   ([`boards::may_edit_item`]): the creator of the item, or the
//!   `manage_<type>` capability on its board.
//!
//! The server remains the authority. A refusal shows in the panel.

use aurora_dark::components::{Alert, Anchor, Button, Group, Panel, Pill, Stack, Text};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;

use super::api::{self, Family, Impact};
use crate::auth::use_auth;
use crate::pages::boards;
use crate::pages::repositories;

/// The option of a picker that selects nothing.
const NO_CHOICE: &str = "";

/// The board that a document names, as the mirror of the capabilities
/// reads it: the slug, and the id of the team of the board.
type NamedBoard = (String, Option<String>);

/// The boards that a document can move TO: each live board where the user
/// holds `manage_documents`, but for the board that the document names
/// now. A board of each level can own a document. Returns `(slug, name)`
/// in the order of the list of boards. Pure, host-tested.
pub(crate) fn owner_board_targets(
    me: &crate::api::Whoami,
    boards: &[boards::data::Board],
    here_slug: Option<&str>,
) -> Vec<(String, String)> {
    boards
        .iter()
        .filter(|board| Some(board.slug.as_str()) != here_slug)
        .filter(|board| {
            boards::holds_capability(
                me,
                Some(&board.slug),
                board.team_id.as_deref(),
                "manage_documents",
            )
        })
        .map(|board| (board.slug.clone(), board.name.clone()))
        .collect()
}

/// The repositories that a picker offers for a new impacts link: each
/// live repository that the item does not impact now, as `(slug, label)`
/// in slug order. Pure, host-tested.
pub(crate) fn impact_options(
    directory: &[repositories::api::Repository],
    impacts: &[Impact],
) -> Vec<(String, String)> {
    repositories::picker_options(directory, None)
        .into_iter()
        .filter(|(slug, _)| {
            !impacts.iter().any(|impact| {
                impact.repository.archived_at.is_none() && impact.repository.slug == *slug
            })
        })
        .collect()
}

/// The owner board of a document: the board that the document names, and
/// the control that changes it.
#[component]
pub fn OwnerBoardPanel(
    #[prop(into)] code: String,
    /// The page's shared board read. `Ok(None)` = the document names no
    /// board.
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    /// The document is archived: each write resolves live items only.
    archived: bool,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let value = RwSignal::new(NO_CHOICE.to_string());
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let all_boards = LocalResource::new(move || {
        let _ = auth.token();
        boards::data::list_boards(auth)
    });
    // The board of now, as `(slug, team)`. `None` while the read is not
    // there, and `Some(None)` for a document that names no board.
    let here: Memo<Option<Option<NamedBoard>>> = Memo::new(move |_| {
        board
            .get()
            .and_then(Result::ok)
            .map(|board| board.map(|board| (board.slug.clone(), board.team_id.clone())))
    });
    // The half of the rule about the board of now. With no board, the
    // owner is the board of a parent, which the page does not know: the
    // mirror is then "on some board", and the server decides.
    let can_move = Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok);
        let (Some(me), Some(here)) = (me, here.get()) else {
            return false;
        };
        match here {
            Some((slug, team)) => {
                boards::holds_capability(&me, Some(&slug), team.as_deref(), "manage_documents")
            }
            None => boards::holds_capability(&me, None, None, "manage_documents"),
        }
    });
    // The half of the rule about the new board.
    let targets: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok);
        let (Some(me), Some(Ok(list)), Some(here)) = (me, all_boards.get(), here.get()) else {
            return Vec::new();
        };
        owner_board_targets(&me, &list, here.as_ref().map(|(slug, _)| slug.as_str()))
    });

    let change = move |target: Option<String>| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::set_document_board(auth, &code.get_value(), target.as_deref()).await {
                // Success reads the item again, and the new page has the
                // new board: no busy reset, this view is gone.
                Ok(_) => on_moved.run(match target {
                    Some(slug) => format!("The owner board is {slug}."),
                    None => "The document names no owner board.".to_string(),
                }),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    };
    let submit: Callback<()> = Callback::new(move |()| {
        let chosen = value.get_untracked();
        if chosen != NO_CHOICE {
            change(Some(chosen));
        }
    });
    let remove: Callback<()> = Callback::new(move |()| change(None));

    view! {
        <Panel title="Owner board" caption="who can edit">
            <Stack gap="sm" attr:data-testid="owner-board">
                {move || match board.get() {
                    None => view! { <Text size="xs" dimmed=true>"Loading board…"</Text> }.into_any(),
                    Some(Err(error)) => view! {
                        <Text size="sm" dimmed=true>{api::error_text(&error)}</Text>
                    }.into_any(),
                    Some(Ok(None)) => view! {
                        <Text size="sm" dimmed=true>
                            "This document names no board. Its owner is the board of the item \
                             that it supports."
                        </Text>
                    }.into_any(),
                    Some(Ok(Some(board))) => view! {
                        <Stack gap="xs">
                            <Anchor href=format!("/boards/{}", board.slug)>{board.name.clone()}</Anchor>
                            <Text size="xs" dimmed=true>
                                "This board gives the right to edit the document. The document \
                                 is not a card on the board."
                            </Text>
                        </Stack>
                    }.into_any(),
                }}
                {move || (!archived && can_move.get()).then(|| {
                    let named = matches!(here.get(), Some(Some(_)));
                    view! {
                        <Group gap="sm" attr:data-testid="owner-board-control">
                            // A closure: the list of boards can come after
                            // the check of the right, and the selector must
                            // then show.
                            {move || (!targets.with(Vec::is_empty)).then(|| view! {
                                <div class="cl-field">
                                    <label class="cl-field__label">"New owner board"</label>
                                    <select
                                        class="cl-input cl-select"
                                        prop:value=move || value.get()
                                        on:change=move |e| value.set(event_target_value(&e))
                                    >
                                        <option value=NO_CHOICE>"(select a board)"</option>
                                        {move || targets.get().into_iter().map(|(slug, name)| view! {
                                            <option value=slug>{name}</option>
                                        }).collect_view()}
                                    </select>
                                </div>
                                {move || {
                                    let disabled = busy.get() || value.get() == NO_CHOICE;
                                    view! {
                                        <Button size="xs" disabled=disabled on_click=submit>
                                            "Set owner board"
                                        </Button>
                                    }
                                }}
                            })}
                            {named.then(|| view! {
                                {move || {
                                    let disabled = busy.get();
                                    view! {
                                        <Button variant="default" size="xs" disabled=disabled
                                            on_click=remove>
                                            "Remove owner board"
                                        </Button>
                                    }
                                }}
                            })}
                        </Group>
                    }
                })}
                {move || error.get().map(|e| view! {
                    <Alert title="Kairos did not change the owner board" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </Stack>
        </Panel>
    }
}

/// The repositories that a document or an ADR impacts, with the add and
/// the remove of a link for a user who can edit the item.
#[component]
pub fn ImpactsPanel(
    family: Family,
    #[prop(into)] code: String,
    impacts: Vec<Impact>,
    /// The page's shared board read: the board of the edit rule.
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    /// The user id of the creator of the item.
    #[prop(into)]
    created_by: String,
    /// The item is archived: each write resolves live items only.
    archived: bool,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let created_by = StoredValue::new(created_by);
    let links = StoredValue::new(impacts);
    let value = RwSignal::new(NO_CHOICE.to_string());
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let required = match family {
        Family::Adr => "manage_adrs",
        _ => "manage_documents",
    };
    // The edit rule (COLLIERY-T-0228). No right on a repository is part of
    // it: the link gives no right, and it takes none.
    let can_edit = Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok);
        let (Some(me), Some(board)) = (me, board.get()) else {
            return false;
        };
        let board = board.ok().flatten();
        created_by.with_value(|created_by| {
            boards::may_edit_item(
                &me,
                created_by,
                board.as_ref().map(|board| board.slug.as_str()),
                board.as_ref().and_then(|board| board.team_id.as_deref()),
                required,
            )
        })
    });
    let directory = LocalResource::new(move || {
        let _ = auth.token();
        async move { repositories::api::list_repositories(auth, None).await }
    });
    let options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let Some(Ok(directory)) = directory.get() else {
            return Vec::new();
        };
        links.with_value(|links| impact_options(&directory, links))
    });

    let add: Callback<()> = Callback::new(move |()| {
        let chosen = value.get_untracked();
        if chosen == NO_CHOICE {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::add_impact(auth, family, &code.get_value(), &chosen).await {
                Ok(_) => on_moved.run(format!("This item impacts the repository {chosen}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });
    let remove: Callback<String> = Callback::new(move |slug: String| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match api::remove_impact(auth, family, &code.get_value(), &slug).await {
                Ok(()) => on_moved.run(format!("This item does not impact the repository {slug}.")),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    });

    view! {
        <Panel title="Impacts" caption="the repositories that this item is about">
            <Stack gap="sm" attr:data-testid="impacts">
                {links.with_value(|links| links.is_empty()).then(|| view! {
                    <Text size="sm" dimmed=true>"This item impacts no repository."</Text>
                })}
                {links.get_value().into_iter().map(|impact| {
                    let slug = impact.repository.slug.clone();
                    let row = slug.clone();
                    let label = if impact.repository.repo_full_name.is_empty() {
                        slug.clone()
                    } else {
                        format!("{slug} · {}", impact.repository.repo_full_name)
                    };
                    let retired = impact.repository.archived_at.is_some();
                    view! {
                        <Group gap="sm" justify="between" attr:data-impact=row>
                            <Group gap="sm">
                                <Text size="sm">{label}</Text>
                                {retired.then(|| view! {
                                    <Pill color=token::GOLD>"repository deleted"</Pill>
                                })}
                            </Group>
                            {move || (!archived && can_edit.get()).then(|| {
                                let slug = slug.clone();
                                let disabled = busy.get();
                                view! {
                                    <Button variant="default" size="xs" disabled=disabled
                                        on_click=Callback::new(move |_| remove.run(slug.clone()))>
                                        "Remove"
                                    </Button>
                                }
                            })}
                        </Group>
                    }
                }).collect_view()}
                {move || (!archived && can_edit.get() && !options.with(Vec::is_empty)).then(|| view! {
                    <Group gap="sm" attr:data-testid="impacts-add">
                        <div class="cl-field">
                            <label class="cl-field__label">"Repository"</label>
                            <select
                                class="cl-input cl-select"
                                prop:value=move || value.get()
                                on:change=move |e| value.set(event_target_value(&e))
                            >
                                <option value=NO_CHOICE>"(select a repository)"</option>
                                {move || options.get().into_iter().map(|(slug, label)| view! {
                                    <option value=slug>{label}</option>
                                }).collect_view()}
                            </select>
                        </div>
                        {move || {
                            let disabled = busy.get() || value.get() == NO_CHOICE;
                            view! {
                                <Button size="xs" disabled=disabled on_click=add>
                                    "Add impacts link"
                                </Button>
                            }
                        }}
                    </Group>
                })}
                <Text size="xs" dimmed=true>
                    "A link says what the item is about. It gives no right on the item, and no \
                     right on the repository."
                </Text>
                {move || error.get().map(|e| view! {
                    <Alert title="Kairos did not change the link" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </Stack>
        </Panel>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Whoami, WhoamiBoardCapabilities, WhoamiOrganization, WhoamiTeam, WhoamiUser};
    use crate::pages::boards::data::Board;
    use crate::pages::repositories::api::{Repository, RepositoryTeam};

    fn me(role: &str, teams: &[&str], grants: &[(&str, &str)]) -> Whoami {
        Whoami {
            user: WhoamiUser {
                id: "u-1".into(),
                email: "alice@kairos.test".into(),
                display_name: "Alice".into(),
            },
            organization: WhoamiOrganization {
                slug: "acme".into(),
                role: role.into(),
            },
            teams: teams
                .iter()
                .map(|id| WhoamiTeam {
                    id: id.to_string(),
                    slug: id.to_string(),
                    name: id.to_string(),
                })
                .collect(),
            capabilities: grants
                .iter()
                .map(|(board, grant)| WhoamiBoardCapabilities {
                    board_slug: board.to_string(),
                    grants: vec![grant.to_string()],
                })
                .collect(),
            implicit: Vec::new(),
        }
    }

    fn board(slug: &str, level: &str, team: Option<&str>) -> Board {
        Board {
            id: format!("b-{slug}"),
            name: slug.to_uppercase(),
            slug: slug.into(),
            board_level: level.into(),
            team_id: team.map(str::to_string),
        }
    }

    fn boards() -> Vec<Board> {
        vec![
            board("strategy", "strategy", None),
            board("initiatives", "initiative", None),
            board("web-delivery", "delivery", Some("t-web")),
            board("platform-delivery", "delivery", Some("t-platform")),
        ]
    }

    fn slugs(targets: Vec<(String, String)>) -> Vec<String> {
        targets.into_iter().map(|(slug, _)| slug).collect()
    }

    /// A member of a team can give a document to the board of that team,
    /// and to a board where the member has the grant. A board of each
    /// level can be the owner.
    #[test]
    fn the_targets_are_the_boards_where_the_user_manages_documents() {
        let alice = me("member", &["t-web"], &[("initiatives", "manage_documents")]);
        assert_eq!(
            slugs(owner_board_targets(&alice, &boards(), None)),
            ["initiatives", "web-delivery"]
        );
        // The board of now is not a target.
        assert_eq!(
            slugs(owner_board_targets(&alice, &boards(), Some("web-delivery"))),
            ["initiatives"]
        );
        // A grant for a different capability opens no board.
        let bob = me("member", &[], &[("strategy", "manage_strategies")]);
        assert!(owner_board_targets(&bob, &boards(), None).is_empty());
        // `manage_*` has `manage_documents` in it.
        let carol = me("member", &[], &[("strategy", "manage_*")]);
        assert_eq!(
            slugs(owner_board_targets(&carol, &boards(), None)),
            ["strategy"]
        );
    }

    #[test]
    fn an_organization_admin_has_each_board() {
        let admin = me("admin", &[], &[]);
        assert_eq!(
            slugs(owner_board_targets(&admin, &boards(), Some("strategy"))),
            ["initiatives", "web-delivery", "platform-delivery"]
        );
    }

    fn repository(slug: &str) -> Repository {
        Repository {
            id: format!("r-{slug}"),
            slug: slug.to_string(),
            forge: "github".to_string(),
            repo_full_name: format!("acme/{slug}"),
            repo_url: format!("https://github.com/acme/{slug}"),
            default_branch: "main".to_string(),
            description: String::new(),
            team: RepositoryTeam {
                id: "t-platform".to_string(),
                slug: "platform".to_string(),
                name: "Platform".to_string(),
            },
            delivery_board_id: None,
            open_tasks: 0,
            has_webhook: false,
        }
    }

    fn impact(slug: &str, archived_at: Option<&str>) -> Impact {
        Impact {
            repository: api::ImpactedRepository {
                slug: slug.into(),
                repo_full_name: format!("acme/{slug}"),
                archived_at: archived_at.map(str::to_string),
            },
        }
    }

    /// The picker offers each live repository that the item does not
    /// impact. A link to a deleted repository does not hide the live
    /// repository that has its slug now.
    #[test]
    fn the_picker_offers_the_repositories_that_the_item_does_not_impact() {
        let directory = vec![repository("fidius"), repository("hlin"), repository("weir")];
        assert_eq!(
            slugs(impact_options(&directory, &[])),
            ["fidius", "hlin", "weir"]
        );
        assert_eq!(
            slugs(impact_options(
                &directory,
                &[
                    impact("hlin", None),
                    impact("weir", Some("2026-09-01T00:00:00Z"))
                ]
            )),
            ["fidius", "weir"]
        );
        assert!(impact_options(&[], &[impact("hlin", None)]).is_empty());
    }

    /// The mirrors decode the body of the server.
    #[test]
    fn the_document_mirror_decodes_the_owner_board_and_the_impacts() {
        let document: api::ItemDetail = serde_json::from_value(serde_json::json!({
            "id": "d-1", "short_code": "ACME-D-0004", "title": "The vision of fidius",
            "content": "", "version": 1, "board_id": "b-1", "template_id": null,
            "lifecycle": "draft", "created_by": "u-1", "updated_by": "u-1",
            "created_at": "2026-09-29T00:00:00Z", "updated_at": "2026-09-29T00:00:00Z",
            "impacts": [{
                "relationship": "impacts", "target_kind": "repository",
                "repository": {"id": "r-1", "slug": "fidius", "forge": "github",
                               "repo_full_name": "acme/fidius"},
                "created_at": "2026-09-29T00:00:00Z"
            }]
        }))
        .expect("the mirror decodes");
        assert_eq!(document.board_id.as_deref(), Some("b-1"));
        assert_eq!(document.column_id, None);
        assert_eq!(document.impacts, [impact("fidius", None)]);
        // A body of a task has no impacts.
        let task: api::ItemDetail = serde_json::from_value(serde_json::json!({
            "id": "t-1", "short_code": "ACME-T-0001", "title": "x", "content": "",
            "version": 1, "board_id": "b-1", "column_id": "c-1",
            "updated_at": "2026-09-29T00:00:00Z"
        }))
        .expect("the mirror decodes");
        assert!(task.impacts.is_empty());
    }
}
