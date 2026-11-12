//! The AdHoc stand-up (KAIROS-T-0764, COLLIERY-I-0674, KAIROS-A-0084): a
//! saved, named group of people, and its round.
//!
//! - [`StandupGroupsSection`] is the section of the "for me" page: each
//!   group (`GET /api/standup-groups`), a form that makes a group, and a
//!   link that opens each group.
//! - [`StandupGroupPage`] at `/standup-groups/:id` is the same view as the
//!   stand-up of a team (`standup.rs`), with no Unclaimed stop. Each stop
//!   shows the work of one person by ownership on each board
//!   (`GET /api/standup-groups/{id}/people/{user_id}`). The URL holds the
//!   stop: `?person=<user name>`. The page also edits the group: its name
//!   and its people. Only the creator gets the Delete button.

use aurora_dark::components::{
    Alert, Button, Empty, ErrorState, Group, Loading, PageHeader, Panel, Pill, Select, Stack, Text,
    TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use serde::{Deserialize, Serialize};

use super::admin::{MutationNotice, MutationOutcome};
use super::for_me::{DEBOUNCE_MS, Debounce, MyWork, WorkLane};
use super::standup::{StepButton, new_seed, shuffled};
use crate::api::encode_component;
use crate::auth::use_auth;

/// The longest name of a group, as the server checks it.
const NAME_MAX: usize = 100;

const GROUPS: &str = "/api/standup-groups";

/// mirror of: `kairos_client::types_work::StandupGroupPerson`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct GroupPerson {
    pub user_id: String,
    pub user_name: String,
    pub display_name: String,
}

/// mirror of: `kairos_client::types_work::StandupGroup` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StandupGroup {
    pub id: String,
    pub name: String,
    pub created_by: GroupPerson,
    pub can_delete: bool,
    pub people: Vec<GroupPerson>,
}

/// mirror of: `kairos_client::types_work::StandupGroupList`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct StandupGroupList {
    pub items: Vec<StandupGroup>,
}

/// mirror of: `kairos_client::types_org::OrgMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Member {
    pub user_id: String,
    pub display_name: String,
}

/// mirror of: `kairos_client::types_work::CreateStandupGroupRequest`.
#[derive(Clone, Debug, PartialEq, Serialize)]
struct CreateGroup {
    name: String,
    people: Vec<String>,
}

/// mirror of: `kairos_client::types_work::UpdateStandupGroupRequest`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
struct UpdateGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    people: Option<Vec<String>>,
}

/// The page of a group. Pure, host-tested.
pub(crate) fn group_href(id: &str) -> String {
    format!("/standup-groups/{}", encode_component(id))
}

/// The URL of the stop of a person, or of the page when there is no
/// person. Pure, host-tested.
fn stop_href(id: &str, person: Option<&GroupPerson>) -> String {
    match person {
        Some(p) => format!(
            "{}?person={}",
            group_href(id),
            encode_component(&p.user_name)
        ),
        None => group_href(id),
    }
}

/// The position in `order` of the person that the query names, and
/// whether it names a person who is not in the group. An absent or unknown
/// person is the first stop. Pure, host-tested.
fn position(order: &[usize], people: &[GroupPerson], person: Option<&str>) -> (usize, bool) {
    let Some(name) = person else {
        return (0, false);
    };
    match order
        .iter()
        .position(|&i| people.get(i).is_some_and(|p| p.user_name == name))
    {
        Some(at) => (at, false),
        None => (0, true),
    }
}

/// What is wrong with a name, or `None`. Pure, host-tested.
fn name_problem(name: &str) -> Option<String> {
    let count = name.trim().chars().count();
    if count == 0 {
        Some("Type a name for the group.".to_string())
    } else if count > NAME_MAX {
        Some(format!("The name has more than {NAME_MAX} characters."))
    } else {
        None
    }
}

/// The members who are not in the group, by display name, as
/// `(user id, display name)`. Pure, host-tested.
fn addable(members: &[Member], people: &[GroupPerson]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = members
        .iter()
        .filter(|m| !people.iter().any(|p| p.user_id == m.user_id))
        .map(|m| (m.user_id.clone(), m.display_name.clone()))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    out
}

/// The user ids of the people after a person is added (`add`) or removed
/// (`remove`). Pure, host-tested.
fn people_after(people: &[GroupPerson], add: Option<&str>, remove: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = people
        .iter()
        .map(|p| p.user_id.clone())
        .filter(|id| Some(id.as_str()) != remove)
        .collect();
    if let Some(add) = add.filter(|a| !a.is_empty() && !out.iter().any(|id| id == a)) {
        out.push(add.to_string());
    }
    out
}

/// The line under a group in the list. Pure, host-tested.
fn group_caption(group: &StandupGroup) -> String {
    match group.people.len() {
        1 => "1 person".to_string(),
        n => format!("{n} people"),
    }
}

/// The section of the "for me" page: each group, and a form that makes one.
#[component]
pub fn StandupGroupsSection() -> impl IntoView {
    let auth = use_auth();
    let navigate = StoredValue::new(use_navigate());
    let reload = RwSignal::new(0u32);
    let name = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let problem = RwSignal::new(String::new());
    let groups = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        async move { crate::api::get_json::<StandupGroupList>(auth, GROUPS).await }
    });
    let on_create = move |_| {
        if busy.get_untracked() {
            return;
        }
        let typed = name.get_untracked();
        if let Some(text) = name_problem(&typed) {
            problem.set(text);
            return;
        }
        problem.set(String::new());
        busy.set(true);
        let body = CreateGroup {
            name: typed.trim().to_string(),
            people: Vec::new(),
        };
        leptos::task::spawn_local(async move {
            let result = crate::api::post_json::<_, StandupGroup>(auth, GROUPS, &body).await;
            busy.set(false);
            match result {
                Ok(group) => {
                    name.set(String::new());
                    outcome.set(None);
                    let href = group_href(&group.id);
                    navigate.with_value(|go| go(&href, Default::default()));
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
        });
    };
    view! {
        <div data-testid="standup-groups">
            <Panel title="AdHoc stand-ups"
                caption="A saved group of people from any team. Each person shows the \
                         initiatives that they own and the tasks that they or their agents \
                         claim, on each board. Each member can open and edit each group.">
                <Stack gap="sm">
                    <MutationNotice outcome/>
                    {move || match groups.get() {
                        None => view! { <Loading/> }.into_any(),
                        Some(Err(error)) => view! {
                            <ErrorState error
                                on_retry=Callback::new(move |_| reload.update(|n| *n += 1))/>
                        }.into_any(),
                        Some(Ok(list)) if list.items.is_empty() => view! {
                            <span data-testid="standup-groups-empty">
                                <Text dimmed=true size="sm">
                                    "There are no groups. Make one below."
                                </Text>
                            </span>
                        }.into_any(),
                        Some(Ok(list)) => view! {
                            <Group gap="xs" wrap=true>
                                {list.items.into_iter().map(|group| {
                                    let caption = group_caption(&group);
                                    view! {
                                        <a href=group_href(&group.id)
                                            data-testid="standup-group-link">
                                            <Pill color=token::MUTED>
                                                {format!("{} ({caption})", group.name)}
                                            </Pill>
                                        </a>
                                    }
                                }).collect_view()}
                            </Group>
                        }.into_any(),
                    }}
                    <Group align="end" gap="sm" wrap=true>
                        <TextInput label="Name of a new group" value=name
                            placeholder="Release crew" error=problem/>
                        <Button on_click=Callback::new(on_create) loading=busy>
                            "Make group"
                        </Button>
                    </Group>
                </Stack>
            </Panel>
        </div>
    }
}

/// The two lanes of one person of the group.
#[component]
fn PersonStop(group: String, person: GroupPerson, reload: RwSignal<u32>) -> impl IntoView {
    let auth = use_auth();
    let name = StoredValue::new(person.display_name.clone());
    let path = format!(
        "{GROUPS}/{}/people/{}",
        encode_component(&group),
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
                                        {format!("{} has no work now.", name.get_value())}
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

/// The edit of a group: its name, its people, and the delete for the
/// creator.
#[component]
fn EditGroup(group: StandupGroup, reload_group: RwSignal<u32>) -> impl IntoView {
    let auth = use_auth();
    let navigate = StoredValue::new(use_navigate());
    let id = StoredValue::new(group.id.clone());
    let people = StoredValue::new(group.people.clone());
    let name = RwSignal::new(group.name.clone());
    let problem = RwSignal::new(String::new());
    let picked = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let outcome: RwSignal<MutationOutcome> = RwSignal::new(None);
    let members = LocalResource::new(move || {
        auth.track_session();
        async move { crate::api::get_all::<Member>(auth, "/api/members").await }
    });
    let options = Signal::derive(move || match members.get() {
        Some(Ok(all)) => people.with_value(|p| addable(&all, p)),
        _ => Vec::new(),
    });
    let save = move |body: UpdateGroup, done: &'static str| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let path = format!("{GROUPS}/{}", encode_component(&id.get_value()));
        leptos::task::spawn_local(async move {
            let result = crate::api::patch_json::<_, StandupGroup>(auth, &path, &body).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    outcome.set(Some(Ok(done.to_string())));
                    picked.set(String::new());
                    reload_group.update(|n| *n += 1);
                }
                Err(error) => outcome.set(Some(Err(error))),
            }
        });
    };
    let on_rename = move |_| {
        let typed = name.get_untracked();
        if let Some(text) = name_problem(&typed) {
            problem.set(text);
            return;
        }
        problem.set(String::new());
        save(
            UpdateGroup {
                name: Some(typed.trim().to_string()),
                people: None,
            },
            "The name is changed.",
        );
    };
    let on_add = move |_| {
        let add = picked.get_untracked();
        if add.is_empty() {
            return;
        }
        let after = people.with_value(|p| people_after(p, Some(&add), None));
        save(
            UpdateGroup {
                name: None,
                people: Some(after),
            },
            "The person is in the group.",
        );
    };
    let on_delete = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let path = format!("{GROUPS}/{}", encode_component(&id.get_value()));
        leptos::task::spawn_local(async move {
            let result = crate::api::delete_json::<StandupGroup>(auth, &path).await;
            busy.set(false);
            match result {
                Ok(_) => navigate.with_value(|go| go("/", Default::default())),
                Err(error) => outcome.set(Some(Err(error))),
            }
        });
    };
    let can_delete = group.can_delete;
    let creator = group.created_by.display_name.clone();
    view! {
        <div data-testid="standup-group-edit">
            <Panel title="Edit the group"
                caption=format!("Each member can change the name and the people. Only \
                                 {creator} can delete the group.")>
                <Stack gap="sm">
                    <MutationNotice outcome/>
                    <Group align="end" gap="sm" wrap=true>
                        <TextInput label="Name" value=name error=problem/>
                        <Button variant="default" on_click=Callback::new(on_rename)
                            loading=busy>"Rename"</Button>
                    </Group>
                    <Group gap="xs" wrap=true>
                        {people.get_value().into_iter().map(|person| {
                            let remove = person.user_id.clone();
                            let on_remove = move |_| {
                                let after = people
                                    .with_value(|p| people_after(p, None, Some(&remove)));
                                save(
                                    UpdateGroup { name: None, people: Some(after) },
                                    "The person left the group.",
                                );
                            };
                            view! {
                                <span data-testid="standup-group-person">
                                    <Group gap="xs">
                                        <Pill color=token::MUTED>{person.display_name}</Pill>
                                        <Button variant="subtle" size="xs"
                                            on_click=Callback::new(on_remove)>"Remove"</Button>
                                    </Group>
                                </span>
                            }
                        }).collect_view()}
                    </Group>
                    <Group align="end" gap="sm" wrap=true>
                        <Select label="Add a person" option_pairs=options value=picked
                            placeholder="Pick a member"/>
                        <Button variant="default" on_click=Callback::new(on_add)
                            loading=busy>"Add"</Button>
                    </Group>
                    {can_delete.then(|| view! {
                        <Group justify="end">
                            <Button variant="default" size="xs" bad=true
                                on_click=Callback::new(on_delete) loading=busy>
                                "Delete the group"
                            </Button>
                        </Group>
                    })}
                </Stack>
            </Panel>
        </div>
    }
}

/// `/standup-groups/:id`: the round of a group.
#[component]
pub fn StandupGroupPage() -> impl IntoView {
    let auth = use_auth();
    let params = use_params_map();
    let query = use_query_map();
    let navigate = StoredValue::new(use_navigate());
    let id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let reload = RwSignal::new(0u32);
    let reload_group = RwSignal::new(0u32);
    let seed = RwSignal::new(new_seed());

    let group = LocalResource::new(move || {
        auth.track_session();
        reload_group.get();
        let id = id.get();
        async move {
            crate::api::get_json::<StandupGroup>(
                auth,
                &format!("{GROUPS}/{}", encode_component(&id)),
            )
            .await
        }
    });

    // Live: each item event of the organization reads the stop again,
    // through a debounce (as the stand-up of a team).
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
        {move || match group.get() {
            None => view! { <Loading/> }.into_any(),
            Some(Err(error)) => view! {
                <ErrorState error on_retry=Callback::new(move |_| reload_group.update(|n| *n += 1))/>
            }.into_any(),
            Some(Ok(group)) => {
                let people = group.people.clone();
                let order = shuffled(people.len(), seed.get());
                let (at, unknown) = {
                    let q = query.read();
                    let asked = q.get("person");
                    let (at, unknown) = position(&order, &people, asked.as_deref());
                    (at, unknown.then_some(asked).flatten())
                };
                let gid = group.id.clone();
                let person_at = |i: usize| order.get(i).and_then(|&p| people.get(p));
                let current = person_at(at).cloned();
                let previous = at
                    .checked_sub(1)
                    .and_then(person_at)
                    .map(|p| stop_href(&gid, Some(p)));
                let next = person_at(at + 1).map(|p| stop_href(&gid, Some(p)));
                let title = format!("Stand-up: {}", group.name);
                let place = if order.is_empty() {
                    String::new()
                } else {
                    format!("{} of {}", at + 1, order.len())
                };
                let reshuffle = {
                    let people = people.clone();
                    let gid = gid.clone();
                    Callback::new(move |_| {
                        let fresh = new_seed();
                        seed.set(fresh);
                        let first = shuffled(people.len(), fresh)
                            .first()
                            .and_then(|&i| people.get(i).cloned());
                        let href = stop_href(&gid, first.as_ref());
                        navigate.with_value(|go| go(&href, Default::default()));
                    })
                };
                let round = order.iter().enumerate().map(|(index, &i)| {
                    let person = people.get(i).cloned();
                    let href = stop_href(&gid, person.as_ref());
                    let label = person.map(|p| p.display_name).unwrap_or_default();
                    let is_current = index == at;
                    let color = if is_current { token::OK } else { token::MUTED };
                    view! {
                        <a href=href data-testid="standup-stop"
                            data-current=is_current.to_string()
                            aria-current=is_current.then_some("step")>
                            <Pill color=color>{label}</Pill>
                        </a>
                    }
                }).collect_view();
                let heading = current.as_ref().map(|p| p.display_name.clone()).unwrap_or_default();
                let stop_view = match current {
                    Some(person) => view! {
                        <PersonStop group=gid.clone() person reload/>
                    }.into_any(),
                    None => view! {
                        <div data-testid="standup-group-no-people">
                            <Empty message="No people in this group"
                                hint="Add a person below. Each person gets a turn."/>
                        </div>
                    }.into_any(),
                };
                view! {
                    <PageHeader title=title sub="Go around the group, one person at a time. \
                        Each person shows the initiatives that they own and the tasks that they \
                        or their agents claim, on each board."/>
                    <div data-testid="standup-group">
                        <Stack gap="md">
                            {unknown.map(|name| view! {
                                <div data-testid="standup-unknown-person">
                                    <Alert title="No such person in this group" color=token::GOLD>
                                        {format!("The link names {name:?}, who is not in this \
                                                  group. The stand-up starts at the first person.")}
                                    </Alert>
                                </div>
                            })}
                            <nav data-testid="standup-round">
                                <Group gap="xs" wrap=true>{round}</Group>
                            </nav>
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
                            <EditGroup group reload_group/>
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

    fn person(name: &str) -> GroupPerson {
        GroupPerson {
            user_id: format!("id-{name}"),
            user_name: name.to_string(),
            display_name: name.to_uppercase(),
        }
    }

    #[test]
    fn the_mirrors_decode_the_answers_of_the_server() {
        let list: StandupGroupList = serde_json::from_str(
            r#"{"items": [{"id": "g-1", "name": "Release crew",
                           "created_by": {"user_id": "u-1", "user_name": "robin",
                                          "display_name": "Robin"},
                           "can_delete": true,
                           "people": [{"user_id": "u-2", "user_name": "sam",
                                       "display_name": "Sam"}],
                           "created_at": "2026-11-12T00:00:00Z",
                           "updated_at": "2026-11-12T00:00:00Z"}]}"#,
        )
        .expect("the groups");
        assert_eq!(list.items[0].name, "Release crew");
        assert!(list.items[0].can_delete);
        assert_eq!(list.items[0].people[0].user_name, "sam");
        assert_eq!(group_caption(&list.items[0]), "1 person");
        let member: Member = serde_json::from_str(
            r#"{"user_id": "u-3", "external_id": "x", "email": "jamie@example.test",
                "display_name": "Jamie", "role": "member", "joined_at": "2026-01-01T00:00:00Z"}"#,
        )
        .expect("a member");
        assert_eq!(member.display_name, "Jamie");
    }

    #[test]
    fn the_bodies_send_the_known_fields_only() {
        let body = serde_json::to_value(UpdateGroup {
            name: Some("Launch".into()),
            people: None,
        })
        .expect("a body");
        assert_eq!(body, serde_json::json!({"name": "Launch"}));
        let body = serde_json::to_value(CreateGroup {
            name: "Launch".into(),
            people: vec![],
        })
        .expect("a body");
        assert_eq!(body, serde_json::json!({"name": "Launch", "people": []}));
    }

    #[test]
    fn a_stop_has_a_link() {
        assert_eq!(group_href("g-1"), "/standup-groups/g-1");
        assert_eq!(stop_href("g-1", None), "/standup-groups/g-1");
        assert_eq!(
            stop_href("g-1", Some(&person("sam jo"))),
            "/standup-groups/g-1?person=sam%20jo"
        );
    }

    #[test]
    fn the_round_has_the_people_only_with_no_unclaimed_stop() {
        let people = vec![person("robin"), person("sam")];
        let order = vec![1, 0];
        assert_eq!(position(&order, &people, None), (0, false));
        assert_eq!(position(&order, &people, Some("robin")), (1, false));
        assert_eq!(position(&order, &people, Some("sam")), (0, false));
        assert_eq!(
            position(&order, &people, Some("unclaimed")),
            (0, true),
            "no Unclaimed stop: the name is an unknown person"
        );
        assert_eq!(position(&[], &[], Some("robin")), (0, true));
    }

    #[test]
    fn a_name_has_1_to_100_characters() {
        assert!(name_problem("   ").is_some());
        assert!(name_problem(" Launch ").is_none());
        assert!(name_problem(&"x".repeat(100)).is_none());
        assert!(name_problem(&"x".repeat(101)).is_some());
    }

    #[test]
    fn the_edit_adds_and_removes_people() {
        let people = vec![person("robin"), person("sam")];
        let members = vec![
            Member {
                user_id: "id-sam".into(),
                display_name: "SAM".into(),
            },
            Member {
                user_id: "id-jamie".into(),
                display_name: "Jamie".into(),
            },
            Member {
                user_id: "id-casey".into(),
                display_name: "Casey".into(),
            },
        ];
        assert_eq!(
            addable(&members, &people),
            vec![
                ("id-casey".to_string(), "Casey".to_string()),
                ("id-jamie".to_string(), "Jamie".to_string())
            ],
            "the members who are not in the group, by name"
        );
        assert_eq!(
            people_after(&people, Some("id-jamie"), None),
            vec!["id-robin", "id-sam", "id-jamie"]
        );
        assert_eq!(
            people_after(&people, Some("id-sam"), None),
            vec!["id-robin", "id-sam"],
            "a person who is in the group is not added again"
        );
        assert_eq!(
            people_after(&people, None, Some("id-robin")),
            vec!["id-sam"]
        );
    }
}
