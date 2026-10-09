//! The two ends of a task that are not "done" (KAIROS-T-0362):
//!
//! - Cancel ([`CancelDialog`]): the team will not do the task. The dialog
//!   asks for the reason; the task moves to the done column of its board
//!   with the mark "cancelled". A move out of the done column removes the
//!   mark.
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
                        "The team will not do this task. To work on it again, move it out of \
                         the done column."
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
                     reason. To work on it again, move it out of the done column."
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

/// The delete dialog of a task: the dialog names the task, and the person
/// types its code to confirm. After the delete the page goes to the board
/// list: the item page has nothing to show.
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
                        "Kairos deletes the task, its history, its metadata and its links. \
                         No restore can bring it back. To keep a copy that you can restore, \
                         archive the task."
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
