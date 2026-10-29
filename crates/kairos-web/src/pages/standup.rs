//! The stand-up page at `/teams/:slug/standup` (KAIROS-T-0723,
//! COLLIERY-I-0663): go around the people of a team, one at a time.
//!
//! - The people come from `GET /api/teams/{id}/standup`. The page shuffles
//!   them when it opens, and keeps the order. "Reshuffle" makes a new
//!   order and goes to the first person.
//! - Each stop shows the two lanes of the "for me" page for one person in
//!   the team (`GET /api/teams/{id}/standup/people/{user_id}`). A person
//!   with no work is still a stop.
//! - The last stop is "Unclaimed": the Todo and Blocked tasks of the team
//!   that no one claims (`GET /api/teams/{id}/standup/unclaimed`).
//! - The URL holds the stop: `?person=<user name>` or `?stop=unclaimed`,
//!   so a shared link opens the same stop. The order is not in the URL.
//! - The page reads again on each item event of the organization, as the
//!   "for me" page does. It writes nothing.

use aurora_dark::components::{Button, ErrorState, Group, Loading, PageHeader, Pill, Stack, Text};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use serde::Deserialize;

use super::for_me::{DEBOUNCE_MS, Debounce, MyWork, WorkItem, WorkLane};
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

/// The position in `stops` of the stop that the query names: `stop` is
/// `unclaimed`, or `person` is the user name of a person. An unknown or
/// absent stop is the first. Pure, host-tested.
fn position(
    stops: &[Stop],
    people: &[StandupPerson],
    person: Option<&str>,
    stop: Option<&str>,
) -> usize {
    if stop == Some("unclaimed") {
        return stops.len().saturating_sub(1);
    }
    person
        .and_then(|name| {
            stops.iter().position(|s| match s {
                Stop::Person(i) => people.get(*i).is_some_and(|p| p.user_name == name),
                Stop::Unclaimed => false,
            })
        })
        .unwrap_or(0)
}

/// The URL of a stop. Pure, host-tested.
fn stop_href(slug: &str, stop: &Stop, people: &[StandupPerson]) -> String {
    match stop {
        Stop::Person(i) => match people.get(*i) {
            Some(p) => format!(
                "/teams/{slug}/standup?person={}",
                encode_query(&p.user_name)
            ),
            None => format!("/teams/{slug}/standup"),
        },
        Stop::Unclaimed => format!("/teams/{slug}/standup?stop=unclaimed"),
    }
}

/// Percent-encode a query value: each byte that is not an unreserved
/// character of RFC 3986. Pure, host-tested.
fn encode_query(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The label of a stop.
fn stop_label(stop: &Stop, people: &[StandupPerson]) -> String {
    match stop {
        Stop::Person(i) => people
            .get(*i)
            .map(|p| p.display_name.clone())
            .unwrap_or_default(),
        Stop::Unclaimed => "Unclaimed".to_string(),
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
    let path = format!("/api/teams/{slug}/standup/people/{}", person.user_id);
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
    let path = format!("/api/teams/{slug}/standup/unclaimed");
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
                Some(Ok(work)) => view! {
                    <WorkLane title="Tasks that no one claims" testid="standup-unclaimed-tasks"
                        items=work.tasks/>
                }.into_any(),
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

/// `/teams/:slug/standup`: the stand-up of a team.
#[component]
pub fn StandupPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let query = use_query_map();
    let navigate = use_navigate();
    let slug = Memo::new(move |_| params.read().get("slug").unwrap_or_default());
    let reload = RwSignal::new(0u32);
    let seed = RwSignal::new(new_seed());

    let standup = LocalResource::new(move || {
        auth.track_session();
        let slug = slug.get();
        async move { crate::api::get_json::<Standup>(auth, &format!("/api/teams/{slug}/standup")).await }
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

    let navigate = StoredValue::new(navigate);
    view! {
        {move || match standup.get() {
            None => view! { <Loading/> }.into_any(),
            Some(Err(error)) => {
                let error: ApiError = error;
                view! { <ErrorState error/> }.into_any()
            }
            Some(Ok(standup)) => {
                let Standup { team, people } = standup;
                let order = shuffled(people.len(), seed.get());
                let stops = stops(&order);
                let q = query.read();
                let at = position(&stops, &people, q.get("person").as_deref(),
                    q.get("stop").as_deref());
                drop(q);
                let current = stops[at].clone();
                let slug = team.slug.clone();
                let previous = (at > 0).then(|| stop_href(&slug, &stops[at - 1], &people));
                let next = stops.get(at + 1).map(|s| stop_href(&slug, s, &people));
                let first = stop_href(&slug, &Stop::Person(0), &people);
                let title = format!("Stand-up: {}", team.name);
                let heading = stop_label(&current, &people);
                let place = format!("{} of {}", at + 1, stops.len());
                let reshuffle = {
                    let people = people.clone();
                    let slug = slug.clone();
                    Callback::new(move |_| {
                        let fresh = new_seed();
                        seed.set(fresh);
                        let order = shuffled(people.len(), fresh);
                        let href = order
                            .first()
                            .map(|&i| stop_href(&slug, &Stop::Person(i), &people))
                            .unwrap_or_else(|| first.clone());
                        navigate.with_value(|go| go(&href, Default::default()));
                    })
                };
                let stop_view = match current {
                    Stop::Person(i) => view! {
                        <PersonStop slug=slug.clone() person=people[i].clone() reload/>
                    }.into_any(),
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
                            <Round slug=slug.clone() stops=stops.clone() people=people.clone() at/>
                            <Group justify="between" wrap=true>
                                <Group gap="xs">
                                    <span data-testid="standup-current">
                                        <Text bright=true bold=true>{heading}</Text>
                                    </span>
                                    <Text dimmed=true size="xs">{place}</Text>
                                </Group>
                                <Group gap="xs">
                                    {match previous {
                                        Some(href) => view! {
                                            <Button variant="default" size="xs" href>"Previous"</Button>
                                        }.into_any(),
                                        None => view! {
                                            <Button variant="default" size="xs" disabled=true>"Previous"</Button>
                                        }.into_any(),
                                    }}
                                    {match next {
                                        Some(href) => view! {
                                            <Button variant="filled" size="xs" href>"Next"</Button>
                                        }.into_any(),
                                        None => view! {
                                            <Button variant="filled" size="xs" disabled=true>"Next"</Button>
                                        }.into_any(),
                                    }}
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

    fn person(name: &str) -> StandupPerson {
        StandupPerson {
            user_id: format!("id-{name}"),
            user_name: name.to_string(),
            display_name: name.to_uppercase(),
        }
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
        let people = vec![person("robin"), person("sam")];
        let round = stops(&[1, 0]);
        assert_eq!(position(&round, &people, None, None), 0);
        assert_eq!(position(&round, &people, Some("robin"), None), 1);
        assert_eq!(position(&round, &people, Some("sam"), None), 0);
        assert_eq!(position(&round, &people, Some("jamie"), None), 0);
        assert_eq!(position(&round, &people, None, Some("unclaimed")), 2);
    }

    #[test]
    fn a_stop_has_a_link() {
        let people = vec![person("robin"), person("sam jo")];
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
        assert_eq!(stop_label(&Stop::Unclaimed, &people), "Unclaimed");
        assert_eq!(stop_label(&Stop::Person(0), &people), "ROBIN");
    }
}
