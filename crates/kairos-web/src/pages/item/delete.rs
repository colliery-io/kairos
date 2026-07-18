//! Soft delete with cascade warning (KAIROS-T-0041, per KAIROS-A-0001):
//! deleting an item cascades to ALL of its live `parent`-edge descendants,
//! server-side. The confirm dialog warns with the AUTHORITATIVE transitive
//! descendant set from the cascade-preview endpoint (KAIROS-T-0051 —
//! `GET …/cascade-preview`, the same set the server would cascade to),
//! not merely the item's direct children; after the delete it shows the
//! server's cascade report (`DeleteResponse.cascaded_short_codes`), which
//! matches what the preview warned with.

use aurora_dark::components::{Alert, Anchor, Button, ErrorState, Group, Loading, Pill, Text};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api::{self, DeleteOutcome, Family};
use crate::auth::use_auth;

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
                    None => view! { <Loading label="Computing the cascade…"/> }.into_any(),
                    Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
                    Some(Ok(preview)) if preview.cascaded_short_codes.is_empty() => view! {
                        <Text size="sm" dimmed=true>"No descendants — only this item will be deleted."</Text>
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
