//! The stand-up page at `/teams/:slug/standup` (KAIROS-T-0723,
//! COLLIERY-I-0663, KAIROS-A-0077): go around the people of a team, one at
//! a time.
//!
//! - The people come from `GET /api/teams/{id}/standup`: the members of
//!   the team, and each person who is not a member but holds work of the
//!   team (KAIROS-T-0725; the page marks them). The page shuffles them
//!   when it opens, and keeps the order. "Reshuffle" makes a new order and
//!   goes to the first person. A reload reads the people again.
//! - Each stop shows the two lanes of the "for me" page for one person in
//!   the team (`GET /api/teams/{id}/standup/people/{user_id}`). A person
//!   with no work is still a stop.
//! - The last stop is "Unclaimed": the Todo and Blocked tasks of the team
//!   that no one claims (`GET /api/teams/{id}/standup/unclaimed`).
//! - The URL holds the slug of the team and the stop: `?person=<user name>`
//!   or `?stop=unclaimed`, so a shared link opens the same stop. A link
//!   with the id of the team goes to the slug (KAIROS-T-0725). The order is
//!   not in the URL.
//! - The page reads the stop again on each item event of the organization,
//!   as the "for me" page does. It writes nothing.

use aurora_dark::components::{
    Alert, Button, ErrorState, Group, Loading, PageHeader, Pill, Stack, Text,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use serde::Deserialize;

use super::for_me::{DEBOUNCE_MS, Debounce, MyWork, WorkItem, WorkLane};
use crate::api::encode_component;
use crate::auth::use_auth;

/// mirror of: `kairos_client::types_work::StandupTeam`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StandupTeam {
    pub slug: String,
    pub name: String,
}

/// mirror of: `kairos_client::types_work::StandupPerson`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StandupPerson {
    pub user_id: String,
    pub user_name: String,
    pub display_name: String,
    /// A member of the team. `false`: a person who holds work of the team
    /// and is not a member.
    pub member: bool,
}

/// mirror of: `kairos_client::types_work::Standup`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Standup {
    pub team: StandupTeam,
    pub people: Vec<StandupPerson>,
}

/// mirror of: `kairos_client::types_work::UnclaimedWork`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct UnclaimedWork {
    pub tasks: Vec<WorkItem>,
}

/// The stand-up page of a team, by its slug or its id (the page goes to
/// the slug). The team page and the delivery board link here
/// (KAIROS-T-0724). Pure, host-tested.
pub(crate) fn standup_href(team: &str) -> String {
    format!("/teams/{}/standup", encode_component(team))
}

/// One stop of the round.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Stop {
    /// The person, by index in the people of the team.
    Person(usize),
    /// The last stop: the tasks that no one claims.
    Unclaimed,
}

/// The order of `n` people for `seed`: a Fisher-Yates shuffle with a
/// xorshift generator. The same seed gives the same order. Pure,
/// host-tested.
fn shuffled(n: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut state = seed | 1;
    for i in (1..n).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}

/// The stops of the round: each person in `order`, then Unclaimed. Pure,
/// host-tested.
fn stops(order: &[usize]) -> Vec<Stop> {
    order
        .iter()
        .map(|&i| Stop::Person(i))
        .chain(std::iter::once(Stop::Unclaimed))
        .collect()
}

/// Where the query puts the round: the position in `stops` of the stop
/// that it names, and whether it names a person who is not in the round.
/// `stop=unclaimed` is the last stop; `person` is a user name. An absent or
/// unknown stop is the first. Pure, host-tested.
fn position(
    stops: &[Stop],
    people: &[StandupPerson],
    person: Option<&str>,
    stop: Option<&str>,
) -> (usize, bool) {
    if stop == Some("unclaimed") {
        return (stops.len().saturating_sub(1), false);
    }
    let Some(name) = person else {
        return (0, false);
    };
    let found = stops.iter().position(|s| match s {
        Stop::Person(i) => people.get(*i).is_some_and(|p| p.user_name == name),
        Stop::Unclaimed => false,
    });
    match found {
        Some(at) => (at, false),
        None => (0, true),
    }
}

/// The URL of a stop. Pure, host-tested.
fn stop_href(slug: &str, stop: &Stop, people: &[StandupPerson]) -> String {
    let base = standup_href(slug);
    match stop {
        Stop::Person(i) => match people.get(*i) {
            Some(p) => format!("{base}?person={}", encode_component(&p.user_name)),
            None => base,
        },
        Stop::Unclaimed => format!("{base}?stop=unclaimed"),
    }
}

/// The label of a stop: a person who is not a member of the team is
/// marked. Pure, host-tested.
fn stop_label(stop: &Stop, people: &[StandupPerson]) -> String {
    match stop {
        Stop::Person(i) => match people.get(*i) {
            Some(p) if p.member => p.display_name.clone(),
            Some(p) => format!("{} (not in the team)", p.display_name),
            None => String::new(),
        },
        Stop::Unclaimed => "Unclaimed".to_string(),
    }
}

/// The query of the URL again, for the redirect from an id to the slug:
/// `person` and `stop` only. Pure, host-tested.
fn keep_query(person: Option<&str>, stop: Option<&str>) -> String {
    match (stop, person) {
        (Some(stop), _) => format!("?stop={}", encode_component(stop)),
        (None, Some(person)) => format!("?person={}", encode_component(person)),
        (None, None) => String::new(),
    }
}

/// A new seed for the shuffle.
fn new_seed() -> u64 {
    (js_sys::Math::random() * u64::MAX as f64) as u64 ^ (js_sys::Date::now() as u64)
}

/// The two lanes of one person.
#[component]
fn PersonStop(slug: String, person: StandupPerson, reload: RwSignal<u32>) -> impl IntoView {
    let auth = use_auth();
    let name = StoredValue::new(person.display_name.clone());
    let path = format!(
        "/api/teams/{}/standup/people/{}",
        encode_component(&slug),
        encode_component(&person.user_id)
    );
    let work = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        let path = path.clone();
        async move { crate::api::get_json::<MyWork>(auth, &path).await }
    });
    view! {
        <div data-testid="standup-person" data-person=person.user_name>
            {move || match work.get() {
                None => view! { <Loading/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                }.into_any(),
                Some(Ok(work)) => {
                    let empty = work.initiatives.is_empty() && work.tasks.is_empty();
                    view! {
                        <Stack gap="lg">
                            {empty.then(|| view! {
                                <span data-testid="standup-person-empty">
                                    <Text dimmed=true size="sm">
                                        {format!("{} has no work in this team now.", name.get_value())}
                                    </Text>
                                </span>
                            })}
                            <WorkLane title="Initiatives" testid="standup-initiatives"
                                items=work.initiatives/>
                            <WorkLane title="Tasks" testid="standup-tasks" items=work.tasks/>
                        </Stack>
                    }.into_any()
                }
            }}
        </div>
    }
}

/// The last stop: the tasks of the team that no one claims.
#[component]
fn UnclaimedStop(slug: String, reload: RwSignal<u32>) -> impl IntoView {
    let auth = use_auth();
    let path = format!("/api/teams/{}/standup/unclaimed", encode_component(&slug));
    let work = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        let path = path.clone();
        async move { crate::api::get_json::<UnclaimedWork>(auth, &path).await }
    });
    view! {
        <div data-testid="standup-unclaimed">
            {move || match work.get() {
                None => view! { <Loading/> }.into_any(),
                Some(Err(error)) => view! {
                    <ErrorState error on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                }.into_any(),
                Some(Ok(work)) => {
                    let empty = work.tasks.is_empty();
                    view! {
                        <Stack gap="lg">
                            {empty.then(|| view! {
                                <span data-testid="standup-unclaimed-empty">
                                    <Text dimmed=true size="sm">
                                        "Each Todo and Blocked task of this team has a claim. \
                                         A task that no one claims shows here."
                                    </Text>
                                </span>
                            })}
                            <WorkLane title="Tasks that no one claims"
                                testid="standup-unclaimed-tasks" items=work.tasks/>
                        </Stack>
                    }.into_any()
                }
            }}
        </div>
    }
}

/// The round: a chip for each stop, the current one marked.
#[component]
fn Round(slug: String, stops: Vec<Stop>, people: Vec<StandupPerson>, at: usize) -> impl IntoView {
    view! {
        <nav data-testid="standup-round">
            <Group gap="xs" wrap=true>
                {stops.iter().enumerate().map(|(index, stop)| {
                    let href = stop_href(&slug, stop, &people);
                    let label = stop_label(stop, &people);
                    let current = index == at;
                    let color = if current { token::OK } else { token::MUTED };
                    view! {
                        <a href=href data-testid="standup-stop"
                            data-current=current.to_string()
                            aria-current=current.then_some("step")>
                            <Pill color=color>{label}</Pill>
                        </a>
                    }
                }).collect_view()}
            </Group>
        </nav>
    }
}

/// A link button, or the same button turned off when there is no link.
#[component]
fn StepButton(
    href: Option<String>,
    #[prop(into)] variant: String,
    label: &'static str,
) -> impl IntoView {
    match href {
        Some(href) => view! { <Button variant size="xs" href>{label}</Button> }.into_any(),
        None => view! { <Button variant size="xs" disabled=true>{label}</Button> }.into_any(),
    }
}

/// `/teams/:slug/standup`: the stand-up of a team.
#[component]
pub fn StandupPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let query = use_query_map();
    let navigate = StoredValue::new(use_navigate());
    let slug = Memo::new(move |_| params.read().get("slug").unwrap_or_default());
    let reload = RwSignal::new(0u32);
    let reload_people = RwSignal::new(0u32);
    let seed = RwSignal::new(new_seed());

    let standup = LocalResource::new(move || {
        auth.track_session();
        reload_people.get();
        let slug = slug.get();
        async move {
            crate::api::get_json::<Standup>(
                auth,
                &format!("/api/teams/{}/standup", encode_component(&slug)),
            )
            .await
        }
    });

    // KAIROS-T-0725: a link with the id of the team (the delivery board
    // has only the id) goes to the slug, with the same stop, so that the
    // links of the stops do not change the path of the page.
    Effect::new(move |_| {
        if let Some(Ok(standup)) = standup.get()
            && standup.team.slug != slug.get()
        {
            let q = query.read_untracked();
            let href = format!(
                "{}{}",
                standup_href(&standup.team.slug),
                keep_query(q.get("person").as_deref(), q.get("stop").as_deref())
            );
            navigate.with_value(|go| {
                go(
                    &href,
                    NavigateOptions {
                        replace: true,
                        ..Default::default()
                    },
                )
            });
        }
    });

    // Live: each item event of the organization, and each reconnect, reads
    // the stop again, through a debounce (as the "for me" page).
    let debounce = StoredValue::new(Debounce::default());
    let guard = super::boards::live::subscribe_all_events(auth, move |_| {
        if debounce.try_update_value(Debounce::arm).unwrap_or(false) {
            set_timeout(
                move || {
                    debounce.try_update_value(Debounce::fire);
                    reload.try_update(|n| *n += 1);
                },
                std::time::Duration::from_millis(DEBOUNCE_MS),
            );
        }
    });
    let live_guard: StoredValue<Option<super::boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(Some(guard));
    on_cleanup(move || live_guard.set_value(None));

    view! {
        {move || match standup.get() {
            None => view! { <Loading/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| reload_people.update(|n| *n += 1))/>
            }.into_any(),
            Some(Ok(standup)) => {
                let Standup { team, people } = standup;
                let order = shuffled(people.len(), seed.get());
                let stops = stops(&order);
                let (at, unknown) = {
                    let q = query.read();
                    let asked = q.get("person");
                    let (at, unknown) =
                        position(&stops, &people, asked.as_deref(), q.get("stop").as_deref());
                    (at, unknown.then_some(asked).flatten())
                };
                let current = stops.get(at).cloned().unwrap_or(Stop::Unclaimed);
                let slug = team.slug.clone();
                let previous = at
                    .checked_sub(1)
                    .and_then(|i| stops.get(i))
                    .map(|s| stop_href(&slug, s, &people));
                let next = stops.get(at + 1).map(|s| stop_href(&slug, s, &people));
                let title = format!("Stand-up: {}", team.name);
                let heading = stop_label(&current, &people);
                let place = format!("{} of {}", at + 1, stops.len());
                let reshuffle = {
                    let people = people.clone();
                    let slug = slug.clone();
                    Callback::new(move |_| {
                        let fresh = new_seed();
                        seed.set(fresh);
                        let first = shuffled(people.len(), fresh)
                            .first()
                            .map_or(Stop::Unclaimed, |&i| Stop::Person(i));
                        let href = stop_href(&slug, &first, &people);
                        navigate.with_value(|go| go(&href, Default::default()));
                    })
                };
                let stop_view = match current {
                    Stop::Person(i) => match people.get(i).cloned() {
                        Some(person) => view! {
                            <PersonStop slug=slug.clone() person reload/>
                        }.into_any(),
                        None => view! { <Loading/> }.into_any(),
                    },
                    Stop::Unclaimed => view! {
                        <UnclaimedStop slug=slug.clone() reload/>
                    }.into_any(),
                };
                view! {
                    <PageHeader title=title sub="Go around the team, one person at a time. \
                        Each person shows the initiatives of the team that they own and the \
                        tasks of the team that they or their agents claim. The last stop is \
                        the tasks that no one claims."/>
                    <div data-testid="standup">
                        <Stack gap="md">
                            {unknown.map(|name| view! {
                                <div data-testid="standup-unknown-person">
                                    <Alert title="No such person in this stand-up" color=token::GOLD>
                                        {format!("The link names {name:?}, who has no turn in this \
                                                  stand-up. The stand-up starts at the first person.")}
                                    </Alert>
                                </div>
                            })}
                            <Round slug=slug.clone() stops=stops.clone() people=people.clone() at/>
                            <Group justify="between" wrap=true>
                                <Group gap="xs">
                                    <span data-testid="standup-current">
                                        <Text bright=true bold=true>{heading}</Text>
                                    </span>
                                    <Text dimmed=true size="xs">{place}</Text>
                                </Group>
                                <Group gap="xs">
                                    <StepButton href=previous variant="default" label="Previous"/>
                                    <StepButton href=next variant="filled" label="Next"/>
                                    <Button variant="subtle" size="xs" on_click=reshuffle>
                                        "Reshuffle"
                                    </Button>
                                </Group>
                            </Group>
                            {stop_view}
                        </Stack>
                    </div>
                }.into_any()
            }
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, member: bool) -> StandupPerson {
        StandupPerson {
            user_id: format!("id-{name}"),
            user_name: name.to_string(),
            display_name: name.to_uppercase(),
            member,
        }
    }

    #[test]
    fn the_mirrors_decode_the_answers_of_the_server() {
        let standup: Standup = serde_json::from_str(
            r#"{"team": {"id": "6f1a1f9e-0000-0000-0000-00000000000a", "slug": "web",
                          "name": "Web"},
                "people": [{"user_id": "u-1", "user_name": "robin", "display_name": "Robin",
                            "member": true},
                           {"user_id": "u-2", "user_name": "jamie", "display_name": "Jamie",
                            "member": false}]}"#,
        )
        .expect("the stand-up");
        assert_eq!(standup.team.slug, "web");
        assert_eq!(standup.people.len(), 2);
        assert!(!standup.people[1].member);
        let unclaimed: UnclaimedWork = serde_json::from_str(
            r#"{"tasks": [{"id": "x", "short_code": "WEB-T-0001", "title": "Open",
                           "entity_type": "task", "state": "todo", "reasons": ["unclaimed"],
                           "board": {"id": "b", "slug": "web", "name": "Web"},
                           "column_id": "c", "column_name": "Todo", "blockers": []}]}"#,
        )
        .expect("the unclaimed work");
        assert_eq!(unclaimed.tasks[0].short_code, "WEB-T-0001");
        assert!(unclaimed.tasks[0].claim.is_none());
    }

    #[test]
    fn a_seed_gives_one_order_of_each_person() {
        let order = shuffled(5, 42);
        assert_eq!(order, shuffled(5, 42), "the same seed, the same order");
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4], "each person one time");
        assert!(shuffled(0, 7).is_empty());
        assert_eq!(shuffled(1, 7), vec![0]);
        let orders: std::collections::HashSet<Vec<usize>> =
            (0..20).map(|seed| shuffled(5, seed)).collect();
        assert!(orders.len() > 1, "different seeds give different orders");
    }

    #[test]
    fn the_round_ends_with_unclaimed() {
        assert_eq!(
            stops(&[2, 0, 1]),
            vec![
                Stop::Person(2),
                Stop::Person(0),
                Stop::Person(1),
                Stop::Unclaimed
            ]
        );
        assert_eq!(stops(&[]), vec![Stop::Unclaimed]);
    }

    #[test]
    fn the_query_names_the_stop() {
        let people = vec![person("robin", true), person("sam", true)];
        let round = stops(&[1, 0]);
        assert_eq!(position(&round, &people, None, None), (0, false));
        assert_eq!(position(&round, &people, Some("robin"), None), (1, false));
        assert_eq!(position(&round, &people, Some("sam"), None), (0, false));
        assert_eq!(
            position(&round, &people, Some("jamie"), None),
            (0, true),
            "an unknown person: the first stop, with a notice"
        );
        assert_eq!(
            position(&round, &people, None, Some("unclaimed")),
            (2, false)
        );
    }

    #[test]
    fn a_stop_has_a_link_and_a_label() {
        let people = vec![person("robin", true), person("sam jo", false)];
        assert_eq!(standup_href("web"), "/teams/web/standup");
        assert_eq!(
            stop_href("web", &Stop::Person(0), &people),
            "/teams/web/standup?person=robin"
        );
        assert_eq!(
            stop_href("web", &Stop::Person(1), &people),
            "/teams/web/standup?person=sam%20jo"
        );
        assert_eq!(
            stop_href("web", &Stop::Unclaimed, &people),
            "/teams/web/standup?stop=unclaimed"
        );
        assert_eq!(
            stop_href("web", &Stop::Person(9), &people),
            "/teams/web/standup"
        );
        assert_eq!(stop_label(&Stop::Unclaimed, &people), "Unclaimed");
        assert_eq!(stop_label(&Stop::Person(0), &people), "ROBIN");
        assert_eq!(
            stop_label(&Stop::Person(1), &people),
            "SAM JO (not in the team)"
        );
    }

    #[test]
    fn the_redirect_to_the_slug_keeps_the_stop() {
        assert_eq!(keep_query(None, None), "");
        assert_eq!(keep_query(Some("robin"), None), "?person=robin");
        assert_eq!(keep_query(None, Some("unclaimed")), "?stop=unclaimed");
        assert_eq!(
            keep_query(Some("robin"), Some("unclaimed")),
            "?stop=unclaimed"
        );
    }
}
