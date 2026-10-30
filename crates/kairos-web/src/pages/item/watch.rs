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

use aurora_dark::components::{Alert, Button, Group, Panel, Stack, Text};
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
                </Stack>
            </div>
        </Panel>
    }
}

#[cfg(test)]
mod tests {
    use super::{WatchActions, WatchState, watch_actions, watch_text};

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
