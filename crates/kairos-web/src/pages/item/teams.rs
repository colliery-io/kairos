//! The teams of an initiative or a strategy (KAIROS-T-0322,
//! COLLIERY-I-0602).
//!
//! An item gets its teams from its tasks (the team of the board of each
//! task, two levels down for a strategy), and a person can set a team on
//! it by hand before it has tasks. The panel lists each team with its
//! source. A team set by hand has a Remove button; a team from tasks has
//! none, because it goes only when its tasks go. A set or a clear is an
//! EDIT: the edit rule applies ([`boards::may_edit_item`]). The server is
//! the authority, and a refusal shows in the panel.

use aurora_dark::components::{Alert, Button, Group, Panel, Pill, Select, Stack, Text};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;

use super::api::{self, Family};
use crate::auth::use_auth;
use crate::pages::boards;
use crate::pages::boards::data::ItemTeam;

/// The option of a picker that selects nothing.
const NO_CHOICE: &str = "";

/// The words for the source of a team.
pub(crate) fn source_label(team: &ItemTeam) -> &'static str {
    match (team.from_tasks, team.set_by_hand) {
        (true, true) => "from tasks · set by hand",
        (true, false) => "from tasks",
        _ => "set by hand",
    }
}

/// The teams that the picker offers: each team of the organization that
/// is not set on the item by hand now, as `(slug, name)`. A team from
/// tasks can still be set by hand: it then stays when the tasks go.
pub(crate) fn team_options(
    directory: &[crate::pages::teams::api::Team],
    teams: &[ItemTeam],
) -> Vec<(String, String)> {
    directory
        .iter()
        .filter(|team| {
            !teams
                .iter()
                .any(|item_team| item_team.set_by_hand && item_team.slug == team.slug)
        })
        .map(|team| (team.slug.clone(), format!("{} · {}", team.name, team.slug)))
        .collect()
}

/// The teams of an initiative or a strategy, with the set and the clear
/// of a team by hand for a user who can edit the item.
#[component]
pub fn TeamsPanel(
    family: Family,
    #[prop(into)] code: String,
    /// The page's shared board read: the board of the edit rule.
    board: LocalResource<Result<Option<api::BoardInfo>, ApiError>>,
    /// The user id of the creator of the item.
    #[prop(into)]
    created_by: String,
    /// The item is archived: each write resolves live items only.
    archived: bool,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let created_by = StoredValue::new(created_by);
    let value = RwSignal::new(NO_CHOICE.to_string());
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    // A set or a clear reads the teams again.
    let version = RwSignal::new(0u32);
    let teams = LocalResource::new(move || {
        let _ = auth.token();
        let _ = version.get();
        async move { api::fetch_item_teams(auth, family, &code.get_value()).await }
    });
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let required = match family {
        Family::Strategy => "manage_strategies",
        _ => "manage_initiatives",
    };
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
        async move { crate::pages::teams::api::list_teams(auth).await }
    });
    let current = Memo::new(move |_| match teams.get() {
        Some(Ok(teams)) => teams.teams,
        _ => Vec::new(),
    });
    let options: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let Some(Ok(directory)) = directory.get() else {
            return Vec::new();
        };
        current.with(|teams| team_options(&directory, teams))
    });

    let done = move |result: Result<(), ApiError>| {
        match result {
            Ok(()) => {
                value.set(NO_CHOICE.to_string());
                version.update(|v| *v += 1);
            }
            Err(e) => error.set(Some(e)),
        }
        busy.set(false);
    };
    let add: Callback<()> = Callback::new(move |()| {
        let chosen = value.get_untracked();
        if chosen == NO_CHOICE {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::set_item_team(auth, family, &code.get_value(), &chosen)
                .await
                .map(|_| ());
            done(result);
        });
    });
    let remove: Callback<String> = Callback::new(move |slug: String| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            done(api::clear_item_team(auth, family, &code.get_value(), &slug).await);
        });
    });

    view! {
        <Panel title="Teams" caption="the teams that do this work">
            <Stack gap="sm" attr:data-testid="item-teams">
                {move || match teams.get() {
                    None => view! { <Text size="sm" dimmed=true>"Loading…"</Text> }.into_any(),
                    Some(Err(e)) => view! {
                        <Text size="sm" dimmed=true>{api::error_text(&e)}</Text>
                    }.into_any(),
                    Some(Ok(loaded)) if loaded.teams.is_empty() => view! {
                        <Text size="sm" dimmed=true>
                            "No team: no task on a team board, and no team set by hand."
                        </Text>
                    }.into_any(),
                    Some(Ok(loaded)) => loaded.teams.into_iter().map(|team| {
                        let slug = team.slug.clone();
                        let row = slug.clone();
                        let source = source_label(&team);
                        let by_hand = team.set_by_hand;
                        view! {
                            <Group gap="sm" justify="between" attr:data-team=row>
                                <Group gap="sm">
                                    <Pill color=token::TEAL>{team.slug.clone()}</Pill>
                                    <Text size="sm">{team.name.clone()}</Text>
                                    <Text size="xs" dimmed=true>{source}</Text>
                                </Group>
                                {move || (by_hand && !archived && can_edit.get()).then(|| {
                                    let slug = slug.clone();
                                    view! {
                                        <Button variant="default" size="xs" disabled=busy.get()
                                            on_click=Callback::new(move |_| remove.run(slug.clone()))>
                                            "Remove"
                                        </Button>
                                    }
                                })}
                            </Group>
                        }
                    }).collect_view().into_any(),
                }}
                {move || (!archived && can_edit.get() && !options.with(Vec::is_empty)).then(|| view! {
                    <Group align="end" gap="sm" attr:data-testid="item-teams-add">
                        <Select
                            label="Team"
                            option_pairs=std::iter::once((
                                NO_CHOICE.to_string(),
                                "(select a team)".to_string(),
                            ))
                            .chain(options.get())
                            .collect()
                            value=value
                        />
                        <Button
                            size="xs"
                            disabled=move || busy.get() || value.get() == NO_CHOICE
                            on_click=add
                        >
                            "Set team"
                        </Button>
                    </Group>
                })}
                <Text size="xs" dimmed=true>
                    "A team comes from the tasks of the item, or a person sets it by hand. \
                     A team gives no right on the item."
                </Text>
                {move || error.get().map(|e| view! {
                    <Alert title="Kairos did not change the team" color=token::BAD>
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
    use crate::pages::teams::api::Team;

    fn team(slug: &str) -> Team {
        Team {
            id: format!("id-{slug}"),
            name: slug.to_uppercase(),
            slug: slug.into(),
            team_type: "stream_aligned".into(),
            delivery_board_id: None,
        }
    }

    fn item_team(slug: &str, from_tasks: bool, set_by_hand: bool) -> ItemTeam {
        ItemTeam {
            slug: slug.into(),
            name: slug.to_uppercase(),
            from_tasks,
            set_by_hand,
        }
    }

    #[test]
    fn the_picker_offers_the_teams_that_are_not_set_by_hand() {
        let directory = [team("kairos"), team("skadi"), team("weir")];
        let offered: Vec<String> = team_options(
            &directory,
            &[
                item_team("skadi", false, true),
                item_team("weir", true, false),
            ],
        )
        .into_iter()
        .map(|(slug, _)| slug)
        .collect();
        // A team from tasks can still be set by hand; a hand-set one cannot.
        assert_eq!(offered, ["kairos", "weir"]);
    }

    #[test]
    fn the_source_of_a_team_is_named() {
        assert_eq!(source_label(&item_team("a", true, false)), "from tasks");
        assert_eq!(source_label(&item_team("a", false, true)), "set by hand");
        assert_eq!(
            source_label(&item_team("a", true, true)),
            "from tasks · set by hand"
        );
    }
}
