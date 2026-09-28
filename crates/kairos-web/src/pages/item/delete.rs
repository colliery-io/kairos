//! Soft delete with cascade warning (KAIROS-T-0041, per KAIROS-A-0001):
//! deleting an item cascades to ALL of its live `parent`-edge descendants,
//! server-side. The confirm dialog warns with the AUTHORITATIVE transitive
//! descendant set from the cascade-preview endpoint (KAIROS-T-0051 —
//! `GET …/cascade-preview`, the same set the server would cascade to),
//! not merely the item's direct children; after the delete it shows the
//! server's cascade report (`DeleteResponse.cascaded_short_codes`), which
//! matches what the preview warned with.
//!
//! The archive stops at a descendant that the user cannot edit, and takes
//! nothing below it (COLLIERY-T-0234). So the dialog shows two lists,
//! before and after: what the archive takes, and what it leaves and why.
//! The server decides the two lists; the words are [`left_heading`] and
//! [`left_reason`].

use aurora_dark::components::{Alert, Anchor, Button, ErrorState, Group, Loading, Pill, Text};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api::{self, DeleteOutcome, Family, NotReached};
use crate::auth::use_auth;

/// The heading of the list of items that stay (COLLIERY-T-0234).
/// `done: false` is the preview, `done: true` is the report.
pub(crate) fn left_heading(count: usize, done: bool) -> String {
    let items = if count == 1 { "item" } else { "items" };
    if done {
        format!("Left {count} {items} live:")
    } else {
        format!("Will leave {count} {items} live:")
    }
}

/// Why one item stays (COLLIERY-T-0234): the capability that the user
/// does not hold, or the item above it where the archive stops. The board
/// is "its board" and not an id: the user can open the item to see it.
pub(crate) fn left_reason(item: &NotReached) -> String {
    match (&item.below, &item.required_capability, &item.board_id) {
        (Some(stop), _, _) => format!("It is below {stop}."),
        (None, Some(capability), Some(_)) => format!("You need {capability} on its board."),
        (None, Some(_), None) => "It has no board. Ask an organization admin.".to_string(),
        (None, None, _) => "You cannot edit it.".to_string(),
    }
}

/// The list of items that stay, with the reason of each. Nothing when the
/// archive takes each descendant.
#[component]
fn LeftLive(items: Vec<NotReached>, done: bool) -> impl IntoView {
    (!items.is_empty()).then(|| {
        let heading = left_heading(items.len(), done);
        view! {
            <Text size="sm" dimmed=true>{heading}</Text>
            {items.into_iter().map(|item| {
                let reason = left_reason(&item);
                view! {
                    <Group>
                        <Pill color=token::MUTED>{item.short_code}</Pill>
                        <Text size="sm" dimmed=true>{reason}</Text>
                    </Group>
                }
            }).collect_view()}
        }
    })
}

/// The delete confirm dialog. `open` is owned by the page header button.
#[component]
pub fn DeleteDialog(
    family: Family,
    #[prop(into)] code: String,
    #[prop(into)] title: String,
    open: RwSignal<bool>,
) -> impl IntoView {
    let code = StoredValue::new(code);
    let title = StoredValue::new(title);

    view! {
        {move || open.get().then(|| view! {
            <div class="kairos-dialog__backdrop"></div>
            <div class="kairos-dialog" role="dialog" aria-modal="true">
                <div class="kairos-dialog__box kairos-dialog__box--narrow">
                    <DeleteFlow
                        family
                        code=code.get_value()
                        title=title.get_value()
                        open
                    />
                </div>
            </div>
        })}
    }
}

/// Confirm → delete → cascade report (own component so the children
/// lookup only runs while the dialog is open).
#[component]
fn DeleteFlow(
    family: Family,
    #[prop(into)] code: String,
    #[prop(into)] title: String,
    open: RwSignal<bool>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);
    let title = StoredValue::new(title);
    let deleting = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let outcome = RwSignal::new(None::<DeleteOutcome>);

    // The AUTHORITATIVE pre-delete warning (KAIROS-T-0051): the full
    // transitive descendant set the server would cascade to, computed by
    // the same BFS as the delete — not just the item's direct children.
    let preview = LocalResource::new(move || {
        let _ = auth.token();
        async move { api::fetch_cascade_preview(auth, family, code.get_value()).await }
    });

    let confirm = move |_| {
        if deleting.get_untracked() {
            return;
        }
        deleting.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::delete_item(auth, family, &code.get_value()).await;
            deleting.set(false);
            match result {
                Ok(report) => outcome.set(Some(report)),
                Err(e) => error.set(Some(api::error_text(&e))),
            }
        });
    };

    view! {
        {move || match outcome.get() {
            // ---- after: the server's authoritative cascade report --------
            Some(report) => view! {
                <Group justify="between">
                    <Text bright=true bold=true>"Deleted"</Text>
                    <Pill color=token::BAD>{report.short_code.clone()}</Pill>
                </Group>
                <Text size="sm" dimmed=true>
                    {if report.cascade_count == 0 {
                        "No descendants were affected.".to_string()
                    } else {
                        format!(
                            "The delete cascaded to {} live descendant(s) (KAIROS-A-0001):",
                            report.cascade_count,
                        )
                    }}
                </Text>
                {(!report.cascaded_short_codes.is_empty()).then(|| {
                    let cascaded = report.cascaded_short_codes.clone();
                    view! {
                        <div class="kairos-delete__cascade">
                            {cascaded.into_iter().map(|code| view! {
                                <Pill color=token::GOLD>{code}</Pill>
                            }).collect_view()}
                        </div>
                    }
                })}
                <LeftLive items=report.not_reached.clone() done=true/>
                <Group justify="end">
                    <Anchor href="/boards">"Back to boards"</Anchor>
                </Group>
            }.into_any(),
            // ---- before: warn + confirm ----------------------------------
            None => view! {
                <Group justify="between">
                    <Text bright=true bold=true>{format!("Delete {}?", code.get_value())}</Text>
                    <Button variant="default" size="xs" on_click=Callback::new(move |_| open.set(false))>
                        "Cancel"
                    </Button>
                </Group>
                <Text size="sm">{title.get_value()}</Text>
                <Alert title="This cascades" color=token::GOLD>
                    <Text size="sm" dimmed=true>
                        "Soft-deletes this item and every live descendant under it (parent edges, computed server-side — KAIROS-A-0001). The full set is shown below and confirmed after deletion."
                    </Text>
                </Alert>
                {move || match preview.get() {
                    None => view! { <Loading label="Kairos calculates the cascade."/> }.into_any(),
                    Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                    Some(Ok(preview))
                        if preview.cascaded_short_codes.is_empty()
                            && preview.not_reached.is_empty() => view! {
                        <Text size="sm" dimmed=true>"No descendants — only this item will be deleted."</Text>
                    }.into_any(),
                    Some(Ok(preview)) if preview.cascaded_short_codes.is_empty() => view! {
                        <Text size="sm" dimmed=true>"Only this item will be deleted."</Text>
                        <LeftLive items=preview.not_reached done=false/>
                    }.into_any(),
                    Some(Ok(preview)) => {
                        let count = preview.cascade_count;
                        view! {
                            <Text size="sm" dimmed=true>
                                {format!("Will also delete {count} live descendant(s):")}
                            </Text>
                            <div class="kairos-delete__cascade">
                                {preview.cascaded_short_codes.into_iter().map(|code| view! {
                                    <Pill color=token::GOLD>{code}</Pill>
                                }).collect_view()}
                            </div>
                            <LeftLive items=preview.not_reached done=false/>
                        }.into_any()
                    }
                }}
                {move || error.get().map(|message| view! {
                    <Alert title="Delete failed" color=token::BAD>
                        <Text size="sm" dimmed=true>{message}</Text>
                    </Alert>
                })}
                <Group justify="end">
                    <button
                        class="cl-btn cl-btn--filled cl-btn--bad"
                        disabled=move || deleting.get()
                        on:click=confirm
                    >
                        {move || if deleting.get() { "Deleting…" } else { "Delete (cascades)" }}
                    </button>
                </Group>
            }.into_any(),
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(capability: Option<&str>, board: Option<&str>, below: Option<&str>) -> NotReached {
        NotReached {
            short_code: "ACME-T-0007".into(),
            required_capability: capability.map(str::to_string),
            board_id: board.map(str::to_string),
            below: below.map(str::to_string),
        }
    }

    /// COLLIERY-T-0234: each item that stays has one short reason.
    #[test]
    fn each_item_that_stays_has_a_reason() {
        assert_eq!(
            left_reason(&item(Some("manage_tasks"), Some("b-1"), None)),
            "You need manage_tasks on its board."
        );
        assert_eq!(
            left_reason(&item(None, None, Some("ACME-I-0002"))),
            "It is below ACME-I-0002."
        );
        assert_eq!(
            left_reason(&item(Some("manage_adrs"), None, None)),
            "It has no board. Ask an organization admin."
        );
        assert_eq!(left_reason(&item(None, None, None)), "You cannot edit it.");
    }

    /// COLLIERY-T-0234: the heading counts, and says before or after.
    #[test]
    fn the_heading_says_before_or_after() {
        assert_eq!(left_heading(1, false), "Will leave 1 item live:");
        assert_eq!(left_heading(3, false), "Will leave 3 items live:");
        assert_eq!(left_heading(1, true), "Left 1 item live:");
        assert_eq!(left_heading(2, true), "Left 2 items live:");
    }

    /// COLLIERY-T-0234: the two responses decode with and with no
    /// `not_reached`, so the dialog reads a server of either version.
    #[test]
    fn the_responses_decode_with_and_with_no_list() {
        let old: api::CascadePreview = serde_json::from_str(
            r#"{"short_code":"ACME-I-0001","cascade_count":0,"cascaded_short_codes":[]}"#,
        )
        .expect("decodes");
        assert!(old.not_reached.is_empty());
        let new: DeleteOutcome = serde_json::from_str(
            r#"{"short_code":"ACME-I-0001","cascade_count":0,"cascaded_short_codes":[],
                "not_reached":[{"short_code":"ACME-T-0007","required_capability":"manage_tasks",
                                "board_id":"b-1"},
                               {"short_code":"ACME-T-0008","below":"ACME-T-0007"}]}"#,
        )
        .expect("decodes");
        assert_eq!(new.not_reached.len(), 2);
        assert_eq!(new.not_reached[1].below.as_deref(), Some("ACME-T-0007"));
    }
}
