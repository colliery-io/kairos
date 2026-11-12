//! The watch of the signed-in person on the item (KAIROS-T-0736).
//!
//! A person watches an item, with or without its children. A move, an
//! edit, a note or a done of the item (or of a child) then makes a
//! notification of the kind `watch` in the bell. [`WatchPanel`] reads
//! `GET /api/items/{short_code}/watch` and gives the controls:
//!
//! - Watch: `PUT` with `with_children: false`;
//! - Watch with children: `PUT` with `with_children: true`;
//! - Stop watching: `DELETE`.
//!
//! An archived item takes no new watch: only Stop watching shows.
//!
//! KAIROS-T-0762: the panel also lists the watchers of the item (`GET
//! /api/items/{short_code}/watchers`), each with who added the watch. A
//! member of the organization can be added (`PUT .../watchers/{user}`);
//! Remove shows for a watch that the signed-in person may remove: their
//! own, or one that they added (`can_remove`; `DELETE
//! .../watchers/{user}`).

use aurora_dark::components::{Alert, Button, Group, Panel, Select, Stack, Text};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;

use super::api;
use crate::auth::use_auth;

/// mirror of: `kairos_client::types_notifications::WatchState`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct WatchState {
    pub watching: bool,
    pub with_children: bool,
}

/// mirror of: `kairos_client::types_notifications::ItemWatcher` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct Watcher {
    pub user_id: String,
    pub user_name: String,
    pub display_name: String,
    pub added_by: String,
    pub added_by_name: Option<String>,
    pub can_remove: bool,
}

/// mirror of: `kairos_client::types_notifications::ItemWatchers` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct Watchers {
    pub watchers: Vec<Watcher>,
}

/// mirror of: `kairos_client::types_org::OrgMember` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct Member {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
}

/// The line of one watcher: the name, and who added the watch when it is
/// not the watcher. Pure, host-tested.
pub(crate) fn watcher_line(w: &Watcher) -> String {
    let name = crate::pages::mention::with_handle(&w.display_name, &w.user_name);
    if w.added_by == w.user_id {
        return name;
    }
    match w.added_by_name.as_deref() {
        Some(by) => format!("{name}, added by {by}"),
        None => format!("{name}, added by another person"),
    }
}

/// The members that can be added: each member who does not watch the
/// item, as `(user id, label)`. Pure, host-tested.
pub(crate) fn add_options(members: &[Member], watchers: &[Watcher]) -> Vec<(String, String)> {
    members
        .iter()
        .filter(|m| !watchers.iter().any(|w| w.user_id == m.user_id))
        .map(|m| {
            (
                m.user_id.clone(),
                format!("{} ({})", m.display_name, m.email),
            )
        })
        .collect()
}

/// The text of the watch. Pure, host-tested.
pub(crate) fn watch_text(state: &WatchState) -> &'static str {
    match (state.watching, state.with_children) {
        (false, _) => "You do not watch this item.",
        (true, false) => "You watch this item. A move, an edit, a note or a done tells you.",
        (true, true) => {
            "You watch this item and its children. A move, an edit, a note or a done of \
             each tells you."
        }
    }
}

/// The controls that show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WatchActions {
    pub watch: bool,
    pub watch_children: bool,
    pub stop: bool,
}

/// The controls for the state. An archived item takes no new watch. Pure,
/// host-tested.
pub(crate) fn watch_actions(state: &WatchState, archived: bool) -> WatchActions {
    let with_children = state.watching && state.with_children;
    WatchActions {
        watch: !archived && (!state.watching || state.with_children),
        watch_children: !archived && !with_children,
        stop: state.watching,
    }
}

fn path(code: &str) -> String {
    format!("/api/items/{code}/watch")
}

fn watchers_path(code: &str) -> String {
    format!("/api/items/{code}/watchers")
}

/// The watch panel of an item.
#[component]
pub fn WatchPanel(
    #[prop(into)] code: String,
    #[prop(into)] archived: Signal<bool>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reload = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);
    let state = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        let path = path(&code.get_value());
        async move { crate::api::get_json::<WatchState>(auth, &path).await }
    });
    let change = move |with_children: Option<bool>| {
        busy.set(true);
        error.set(None);
        let path = path(&code.get_value());
        leptos::task::spawn_local(async move {
            let result = match with_children {
                Some(with_children) => {
                    crate::api::put_json::<_, WatchState>(
                        auth,
                        &path,
                        &json!({ "with_children": with_children }),
                    )
                    .await
                }
                None => crate::api::delete_json::<WatchState>(auth, &path).await,
            };
            if let Err(e) = result {
                error.set(Some(e));
            }
            busy.set(false);
            reload.update(|n| *n += 1);
        });
    };
    view! {
        <Panel title="Watch" caption="notifications of changes">
            <div class="kairos-item__watch" data-testid="watch-panel">
                <Stack gap="sm">
                    {move || match state.get() {
                        None => view! {
                            <Text size="sm" dimmed=true>"Loading the watch…"</Text>
                        }.into_any(),
                        Some(Err(e)) => view! {
                            <Text size="sm" dimmed=true>{api::error_text(&e)}</Text>
                        }.into_any(),
                        Some(Ok(current)) => {
                            let actions = watch_actions(&current, archived.get());
                            view! {
                                <Text size="sm" bright=current.watching
                                    dimmed=!current.watching>
                                    <span data-testid="watch-state">{watch_text(&current)}</span>
                                </Text>
                                <Group gap="sm" wrap=true>
                                    {actions.watch.then(|| view! {
                                        <span data-testid="watch-button">
                                            <Button size="xs" disabled=busy
                                                on_click=Callback::new(move |_| change(Some(false)))>
                                                "Watch"
                                            </Button>
                                        </span>
                                    })}
                                    {actions.watch_children.then(|| view! {
                                        <span data-testid="watch-children-button">
                                            <Button size="xs" variant="default" disabled=busy
                                                on_click=Callback::new(move |_| change(Some(true)))>
                                                "Watch with children"
                                            </Button>
                                        </span>
                                    })}
                                    {actions.stop.then(|| view! {
                                        <span data-testid="unwatch-button">
                                            <Button size="xs" variant="default" disabled=busy
                                                on_click=Callback::new(move |_| change(None))>
                                                "Stop watching"
                                            </Button>
                                        </span>
                                    })}
                                </Group>
                            }.into_any()
                        }
                    }}
                    {move || error.get().map(|e| view! {
                        <Alert title="Could not change the watch" color=token::BAD>
                            <Text size="sm">{api::error_text(&e)}</Text>
                        </Alert>
                    })}
                    <WatcherList code=code.get_value() archived reload/>
                </Stack>
            </div>
        </Panel>
    }
}

/// The watchers of the item (KAIROS-T-0762), with Add and Remove.
#[component]
fn WatcherList(code: String, archived: Signal<bool>, reload: RwSignal<u32>) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let chosen = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<ApiError>);
    let watchers = LocalResource::new(move || {
        auth.track_session();
        reload.get();
        let path = watchers_path(&code.get_value());
        async move { crate::api::get_json::<Watchers>(auth, &path).await }
    });
    let members = LocalResource::new(move || {
        auth.track_session();
        async move { crate::api::get_all::<Member>(auth, "/api/members").await }
    });
    // `add`: add the person `user` as a watcher; else remove the watch.
    let change = move |user: String, add: bool| {
        busy.set(true);
        error.set(None);
        let path = format!("{}/{user}", watchers_path(&code.get_value()));
        leptos::task::spawn_local(async move {
            let result = if add {
                crate::api::put_json::<_, Watchers>(auth, &path, &json!({})).await
            } else {
                crate::api::delete_json::<Watchers>(auth, &path).await
            };
            match result {
                Ok(_) => chosen.set(String::new()),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
            reload.update(|n| *n += 1);
        });
    };
    view! {
        <div data-testid="watchers">
            <Stack gap="xs">
                {move || match watchers.get() {
                    None => view! {
                        <Text size="sm" dimmed=true>"Loading the watchers…"</Text>
                    }.into_any(),
                    Some(Err(e)) => view! {
                        <Text size="sm" dimmed=true>{api::error_text(&e)}</Text>
                    }.into_any(),
                    Some(Ok(list)) if list.watchers.is_empty() => view! {
                        <Text size="sm" dimmed=true>
                            <span data-testid="watchers-none">"Nobody watches this item."</span>
                        </Text>
                    }.into_any(),
                    Some(Ok(list)) => view! {
                        <Text size="sm">"Watchers:"</Text>
                        {list.watchers.into_iter().map(|w| {
                            let line = watcher_line(&w);
                            let user = w.user_id.clone();
                            view! {
                                <Group gap="sm" wrap=true>
                                    <Text size="sm">
                                        <span data-testid="watcher" style="overflow-wrap:anywhere">{line}</span>
                                    </Text>
                                    {w.can_remove.then(|| view! {
                                        <span data-testid="watcher-remove">
                                            <Button size="xs" variant="default" disabled=busy
                                                on_click=Callback::new(move |_| {
                                                    change(user.clone(), false)
                                                })>
                                                "Remove"
                                            </Button>
                                        </span>
                                    })}
                                </Group>
                            }
                        }).collect_view()}
                    }.into_any(),
                }}
                {move || (!archived.get()).then(|| {
                    match (members.get(), watchers.get()) {
                        (Some(Ok(members)), Some(Ok(list))) => {
                            let options = add_options(&members, &list.watchers);
                            view! {
                                <Group align="end" gap="sm" wrap=true>
                                    <span data-testid="watcher-add-select">
                                        <Select label="Add a watcher" placeholder="Choose a person"
                                            option_pairs=options value=chosen/>
                                    </span>
                                    <span data-testid="watcher-add">
                                        <Button size="xs"
                                            disabled=move || busy.get() || chosen.with(String::is_empty)
                                            on_click=Callback::new(move |_| {
                                                change(chosen.get(), true)
                                            })>
                                            "Add"
                                        </Button>
                                    </span>
                                </Group>
                            }.into_any()
                        }
                        _ => ().into_any(),
                    }
                })}
                {move || error.get().map(|e| view! {
                    <Alert title="Could not change the watchers" color=token::BAD>
                        <Text size="sm">{api::error_text(&e)}</Text>
                    </Alert>
                })}
            </Stack>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Member, WatchActions, WatchState, Watcher, add_options, watch_actions, watch_text,
        watcher_line,
    };

    fn watcher(id: &str, name: &str, added_by: &str, by_name: Option<&str>) -> Watcher {
        Watcher {
            user_id: id.into(),
            user_name: name.to_lowercase(),
            display_name: name.into(),
            added_by: added_by.into(),
            added_by_name: by_name.map(Into::into),
            can_remove: false,
        }
    }

    #[test]
    fn a_watcher_line_names_who_added_the_watch() {
        assert_eq!(
            watcher_line(&watcher("1", "Sam", "1", Some("Sam"))),
            "Sam @sam"
        );
        assert_eq!(
            watcher_line(&watcher("1", "Sam", "2", Some("Robin"))),
            "Sam @sam, added by Robin"
        );
        assert_eq!(
            watcher_line(&watcher("1", "Sam", "2", None)),
            "Sam @sam, added by another person"
        );
    }

    #[test]
    fn a_watcher_is_not_offered_again() {
        let member = |id: &str, name: &str| Member {
            user_id: id.into(),
            email: format!("{}@acme.test", name.to_lowercase()),
            display_name: name.into(),
        };
        let members = [member("1", "Sam"), member("2", "Robin")];
        assert_eq!(
            add_options(&members, &[watcher("1", "Sam", "1", None)]),
            vec![("2".to_string(), "Robin (robin@acme.test)".to_string())]
        );
    }

    fn state(watching: bool, with_children: bool) -> WatchState {
        WatchState {
            watching,
            with_children,
        }
    }

    #[test]
    fn the_text_says_what_you_watch() {
        assert_eq!(
            watch_text(&state(false, false)),
            "You do not watch this item."
        );
        assert!(watch_text(&state(true, false)).starts_with("You watch this item."));
        assert!(
            watch_text(&state(true, true)).starts_with("You watch this item and its children.")
        );
    }

    #[test]
    fn the_controls_follow_the_watch() {
        let all = |watch, watch_children, stop| WatchActions {
            watch,
            watch_children,
            stop,
        };
        assert_eq!(
            watch_actions(&state(false, false), false),
            all(true, true, false)
        );
        assert_eq!(
            watch_actions(&state(true, false), false),
            all(false, true, true)
        );
        assert_eq!(
            watch_actions(&state(true, true), false),
            all(true, false, true)
        );
        // An archived item: only Stop watching.
        assert_eq!(
            watch_actions(&state(true, true), true),
            all(false, false, true)
        );
        assert_eq!(
            watch_actions(&state(false, false), true),
            WatchActions::default()
        );
    }
}
