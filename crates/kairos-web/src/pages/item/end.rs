//! The two ends of a task that are not "done" (KAIROS-T-0362):
//!
//! - Cancel ([`CancelDialog`]): the team will not do the task. The dialog
//!   asks for the reason; the task moves to the done column of its board
//!   with the mark "cancelled". A move out of the done column removes the
//!   mark.
//! - Reopen ([`ReopenDialog`], KAIROS-T-0648): a done task comes back.
//!   The server moves it to the first column of its board that is not
//!   done, and removes the cancel mark (KAIROS-A-0048).
//! - Move back ([`BackDialog`], KAIROS-T-0774): a task goes back one
//!   column after a false start (Active → Todo, Todo → Backlog).
//! - Delete ([`PurgeDialog`]): the task is gone for good. The dialog names
//!   the task, and the person types its code to confirm. Only a person
//!   who manages the board of the task sees the button.
//!
//! Archive (the [`super::delete`] dialog) is the third way, and the only
//! one that a restore can undo.
//!
//! [`CancelledBanner`] shows the mark and the reason on the item page.

use aurora_dark::components::{Alert, Stack, Text, Textarea};
use aurora_dark::frame::{ConfirmDialog, use_toaster};
use aurora_dark::tokens::token;
use aurora_dark::widgets::Banner;
use leptos::prelude::*;

use super::api::{self, Cancellation};
use crate::auth::use_auth;

/// The text of the cancel mark: who cancelled the task, and when. Pure,
/// host-tested.
pub(crate) fn cancelled_heading(mark: &Cancellation) -> String {
    let when = super::put_away_when(&mark.cancelled_at);
    if mark.cancelled_by_name.is_empty() {
        format!("Cancelled on {when}")
    } else {
        format!("Cancelled on {when} by {}", mark.cancelled_by_name)
    }
}

/// The banner of a cancelled task: the mark and the reason.
#[component]
pub fn CancelledBanner(mark: Cancellation) -> impl IntoView {
    let heading = cancelled_heading(&mark);
    let reason = format!("Reason: {}", mark.reason);
    view! {
        <div class="kairos-item__cancelled" data-testid="cancelled-banner">
            <Banner color=token::MUTED icon="⊘">
                <Stack gap="xs">
                    <Text bright=true bold=true>{heading}</Text>
                    <Text size="sm">{reason}</Text>
                    <Text size="xs" dimmed=true>
                        "The team will not do this task. To work on it again, reopen it."
                    </Text>
                </Stack>
            </Banner>
        </div>
    }
}

/// The cancel dialog: it asks for the reason. `open` is owned by the page
/// header button; `on_cancelled` gets the message of the page.
#[component]
pub fn CancelDialog(
    #[prop(into)] code: String,
    #[prop(into)] title: String,
    open: RwSignal<bool>,
    on_cancelled: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let reason = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        if open.get() {
            error.set(None);
        }
    });
    let confirm = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        let text = reason.get_untracked();
        if text.trim().is_empty() {
            error.set(Some("Write the reason.".to_string()));
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::cancel_task(auth, &code.get_value(), text.trim()).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    open.set(false);
                    reason.set(String::new());
                    on_cancelled.run(format!(
                        "Kairos cancelled {}. It is in the done column now.",
                        code.get_value()
                    ));
                }
                Err(e) => error.set(Some(api::error_text(&e))),
            }
        });
    });
    view! {
        <ConfirmDialog
            open
            title=format!("Cancel {}?", code.get_value())
            message=title
            confirm_label="Cancel task"
            cancel_label="Keep"
            danger=false
            busy
            size="md"
            on_confirm=confirm
            notice=std::sync::Arc::new(|| view! {
                <Text size="sm" dimmed=true>
                    "The task moves to the done column with the mark \"cancelled\" and the \
                     reason. To work on it again, reopen it."
                </Text>
            }.into_any())
        >
            <Textarea label="Reason" value=reason rows=3 required=true/>
            {move || error.get().map(|message| view! {
                <Alert title="Cancel failed" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
        </ConfirmDialog>
    }
}

/// The message of the page after a reopen. Pure, host-tested.
pub(crate) fn reopened_message(code: &str) -> String {
    format!("Kairos reopened {code}. It is in the first open column of its board.")
}

/// The message of the page after a move back (KAIROS-T-0774). Pure,
/// host-tested.
pub(crate) fn moved_back_message(code: &str, to: &str) -> String {
    format!("Kairos moved {code} back to {to}.")
}

/// The move-back dialog of a task (KAIROS-T-0774): Active → Todo, Todo →
/// Backlog, after a false start. A move out of Active ends the claim. A
/// refusal shows the message of the server.
#[component]
pub fn BackDialog(
    #[prop(into)] code: String,
    #[prop(into)] title: String,
    #[prop(into)] to: Signal<Option<String>>,
    open: RwSignal<bool>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        if open.get() {
            error.set(None);
        }
    });
    let confirm = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::move_task_back(auth, &code.get_value()).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    open.set(false);
                    let to = to.get_untracked().unwrap_or_default();
                    on_moved.run(moved_back_message(&code.get_value(), &to));
                }
                Err(e) => error.set(Some(api::error_text(&e))),
            }
        });
    });
    view! {
        <ConfirmDialog
            open
            title=format!("Move {} back?", code.get_value())
            message=title
            confirm_label="Move back"
            cancel_label="Keep it here"
            danger=false
            busy
            size="md"
            on_confirm=confirm
            notice=std::sync::Arc::new(|| view! {
                <Text size="sm" dimmed=true>
                    "The task moves back one column. A move out of Active ends the claim."
                </Text>
            }.into_any())
        >
            {move || error.get().map(|message| view! {
                <Alert title="Move back failed" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
        </ConfirmDialog>
    }
}

/// The lines of the modal of a move back of an initiative
/// (KAIROS-T-0775): one for each task, with the claim that ends. Pure,
/// host-tested.
pub(crate) fn initiative_back_lines(plan: &api::InitiativeMoveBack) -> Vec<String> {
    plan.tasks
        .iter()
        .map(|t| {
            let claim = t
                .claimant
                .as_ref()
                .map(|name| format!(" — the claim of {name} ends"))
                .unwrap_or_default();
            format!(
                "{} {}: {} → {}{claim}",
                t.short_code, t.title, t.from_column, t.to_column
            )
        })
        .collect()
}

/// The modal of a move back of an initiative (KAIROS-T-0775): it loads the
/// plan from the server, lists what happens, and says that the move
/// happens at once. A refusal shows the message of the server.
#[component]
pub fn InitiativeBackDialog(
    #[prop(into)] code: String,
    open: RwSignal<bool>,
    on_moved: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let plan = RwSignal::new(None::<api::InitiativeMoveBack>);
    Effect::new(move |_| {
        if open.get() {
            error.set(None);
            plan.set(None);
            leptos::task::spawn_local(async move {
                match api::initiative_back_plan(auth, &code.get_value()).await {
                    Ok(p) => plan.set(Some(p)),
                    Err(e) => error.set(Some(api::error_text(&e))),
                }
            });
        }
    });
    let confirm = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::move_initiative_back(auth, &code.get_value()).await;
            busy.set(false);
            match result {
                Ok(done) => {
                    open.set(false);
                    on_moved.run(format!(
                        "Kairos moved {} back to {}. {} task(s) went to Backlog.",
                        done.short_code,
                        done.to_column,
                        done.tasks.len()
                    ));
                }
                Err(e) => error.set(Some(api::error_text(&e))),
            }
        });
    });
    view! {
        <ConfirmDialog
            open
            title=format!("Move {} back?", code.get_value())
            message="This happens at once. Make sure that each person knows.".to_string()
            confirm_label="Move back now"
            cancel_label="Keep it here"
            danger=true
            busy
            size="md"
            on_confirm=confirm
            notice=std::sync::Arc::new(move || view! {
                <Stack gap="xs">
                    {move || plan.get().map(|p| view! {
                        <Text size="sm">
                            {format!(
                                "The initiative moves from {} to {}. {} task(s) go to Backlog, \
                                 and their claims end. Blocked tasks stay. The claimants, the \
                                 owner and the watchers get a notification.",
                                p.from_column,
                                p.to_column,
                                p.tasks.len()
                            )}
                        </Text>
                        <Stack gap="xs">
                            {initiative_back_lines(&p)
                                .into_iter()
                                .map(|line| view! { <Text size="sm" dimmed=true>{line}</Text> })
                                .collect::<Vec<_>>()}
                        </Stack>
                    })}
                </Stack>
            }.into_any())
        >
            {move || error.get().map(|message| view! {
                <Alert title="Move back failed" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
        </ConfirmDialog>
    }
}

/// The reopen dialog of a done task (KAIROS-T-0648). The server picks the
/// column, so the dialog asks only to confirm. A refusal shows the code
/// and the message of the server. `on_reopened` gets the message of the
/// page.
#[component]
pub fn ReopenDialog(
    #[prop(into)] code: String,
    #[prop(into)] title: String,
    open: RwSignal<bool>,
    on_reopened: Callback<String>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        if open.get() {
            error.set(None);
        }
    });
    let confirm = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::reopen_task(auth, &code.get_value()).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    open.set(false);
                    on_reopened.run(reopened_message(&code.get_value()));
                }
                Err(e) => error.set(Some(api::error_text(&e))),
            }
        });
    });
    view! {
        <ConfirmDialog
            open
            title=format!("Reopen {}?", code.get_value())
            message=title
            confirm_label="Reopen"
            cancel_label="Keep done"
            danger=false
            busy
            size="md"
            on_confirm=confirm
            notice=std::sync::Arc::new(|| view! {
                <Text size="sm" dimmed=true>
                    "The task moves to the first column of its board that is not done. \
                     Kairos removes the cancel mark. The history keeps the reason."
                </Text>
            }.into_any())
        >
            {move || error.get().map(|message| view! {
                <Alert title="Reopen failed" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
        </ConfirmDialog>
    }
}

/// The delete dialog of an archived task (KAIROS-T-0570): the dialog names
/// the task, and the person types its code to confirm. After the delete the
/// page goes to the board list: the item page has nothing to show.
#[component]
pub fn PurgeDialog(
    #[prop(into)] code: String,
    #[prop(into)] title: String,
    open: RwSignal<bool>,
) -> impl IntoView {
    let auth = use_auth();
    let toaster = use_toaster();
    let navigate = leptos_router::hooks::use_navigate();
    let navigate = StoredValue::new(navigate);
    let code = StoredValue::new(code);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        if open.get() {
            error.set(None);
        }
    });
    let confirm = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::purge_task(auth, &code.get_value()).await;
            busy.set(false);
            match result {
                Ok(outcome) => {
                    open.set(false);
                    toaster.success(format!("Kairos deleted {} for good.", outcome.short_code));
                    navigate.with_value(|navigate| navigate("/boards", Default::default()));
                }
                Err(e) => error.set(Some(api::error_text(&e))),
            }
        });
    });
    let message = format!("{}: \"{}\"", code.get_value(), title);
    view! {
        <ConfirmDialog
            open
            title=format!("Delete {} for good?", code.get_value())
            message
            confirm_text=code.get_value()
            confirm_label="Delete for good"
            busy
            size="md"
            on_confirm=confirm
            notice=std::sync::Arc::new(|| view! {
                <Alert title="You cannot undo this" color=token::BAD>
                    <Text size="sm" dimmed=true>
                        "Kairos deletes the archived task, its history, its metadata, its \
                         links and its external references. No restore can bring it back. \
                         The issue of an intake task can be imported again."
                    </Text>
                </Alert>
            }.into_any())
        >
            {move || error.get().map(|message| view! {
                <Alert title="Delete failed" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
        </ConfirmDialog>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KAIROS-T-0362: the mark says who cancelled the task, and when.
    #[test]
    fn the_heading_names_who_and_when() {
        let mark = |name: &str| Cancellation {
            reason: "Out of scope.".into(),
            cancelled_by_name: name.into(),
            cancelled_at: "2026-10-09T10:30:00Z".into(),
        };
        assert_eq!(
            cancelled_heading(&mark("Robin")),
            "Cancelled on 2026-10-09 10:30 UTC by Robin"
        );
        assert_eq!(
            cancelled_heading(&mark("")),
            "Cancelled on 2026-10-09 10:30 UTC"
        );
    }

    /// KAIROS-T-0648: the message after a reopen names the task, and the
    /// reopen goes to the reopen route of the task with no body.
    #[test]
    fn the_reopen_names_the_task_and_its_route() {
        assert_eq!(
            reopened_message("ACME-T-0001"),
            "Kairos reopened ACME-T-0001. It is in the first open column of its board."
        );
        assert_eq!(
            api::reopen_path("ACME-T-0001"),
            "/api/tasks/ACME-T-0001/reopen"
        );
    }

    /// KAIROS-T-0648: a refusal of a reopen shows the code and the message
    /// of the server.
    #[test]
    fn a_reopen_refusal_shows_the_server_message() {
        let refusal = aurora_dark::tokens::ApiError::Http {
            status: 422,
            message: "The initiative is not active.".into(),
            code: Some("PARENT_NOT_ACTIVE".into()),
        };
        assert_eq!(
            api::error_text(&refusal),
            "PARENT_NOT_ACTIVE (422): The initiative is not active."
        );
    }

    /// The item mirror decodes the mark, and a body with no mark.
    #[test]
    fn the_item_mirror_decodes_the_mark() {
        let item: api::ItemDetail = serde_json::from_str(
            r#"{"short_code":"ACME-T-0001","title":"t","content":"","version":1,
                "id":"i","updated_at":"2026-10-09T10:00:00Z",
                "cancellation":{"reason":"No need","cancelled_by":"u",
                                "cancelled_by_name":"Robin",
                                "cancelled_at":"2026-10-09T10:00:00Z"}}"#,
        )
        .expect("decodes");
        assert_eq!(item.cancellation.expect("mark").reason, "No need");
        let old: api::ItemDetail = serde_json::from_str(
            r#"{"short_code":"ACME-T-0001","title":"t","content":"","version":1,
                "id":"i","updated_at":"2026-10-09T10:00:00Z"}"#,
        )
        .expect("decodes");
        assert_eq!(old.cancellation, None);
    }
}
