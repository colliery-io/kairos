//! Content editing for the item detail page (KAIROS-T-0041): title +
//! markdown edit/preview, the A-0004 versioned save, and the 409 merge UI.
//!
//! Save carries the version the edit was based on. A 409 opens the merge
//! dialog — server-current vs. yours side by side — with three ways out,
//! all of which carry the *new* version forward:
//!
//! - **Keep mine**: retry the save immediately, based on the server's
//!   current version (overwrites their text with yours).
//! - **Take theirs**: load the server-current title/content into the
//!   editor and drop your draft.
//! - **Merge manually**: keep your draft, rebase onto the server version,
//!   and pin the server copy below the editor for reference until the
//!   next successful save.

use aurora_dark::components::{
    ActionIcon, Alert, Button, Group, Panel, Pill, SegmentedControl, SimpleGrid, Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::api::{self, CurrentVersion, Family, SaveError};
use super::markdown;
use crate::auth::use_auth;

/// The editable content panel. Recreated (with fresh server state) when
/// the page refetches after a successful save — the parent owns that
/// refetch via `on_saved`.
#[component]
pub fn ContentEditor(
    family: Family,
    #[prop(into)] code: String,
    #[prop(into)] initial_title: String,
    #[prop(into)] initial_content: String,
    initial_version: i32,
    /// Fired after a successful save; the parent refetches (server state
    /// is the source of truth) and posts the page-level notice.
    on_saved: Callback<i32>,
) -> impl IntoView {
    let auth = use_auth();
    let code = StoredValue::new(code);

    // Draft + baseline (baseline moves on take-theirs so the dirty check
    // stays honest).
    let title = RwSignal::new(initial_title.clone());
    let content = RwSignal::new(initial_content.clone());
    let orig_title = RwSignal::new(initial_title);
    let orig_content = RwSignal::new(initial_content);
    // The version the current draft is based on (A-0004: submitted with
    // every save; rebased by the merge dialog's outcomes).
    let base_version = RwSignal::new(initial_version);

    let mode = RwSignal::new("Edit".to_string());
    let saving = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // Some = the merge dialog is open on this server-current snapshot.
    let conflict = RwSignal::new(None::<CurrentVersion>);
    // Some = manual merge in progress; the server copy stays visible.
    let reference = RwSignal::new(None::<CurrentVersion>);

    let dirty = move || title.get() != orig_title.get() || content.get() != orig_content.get();

    // One save routine; the merge dialog re-enters it with the rebased
    // version ("keep mine").
    let save_as = move |version: i32| {
        if saving.get_untracked() {
            return;
        }
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let result = api::update_content(
                auth,
                family,
                &code.get_value(),
                &title.get_untracked(),
                &content.get_untracked(),
                version,
            )
            .await;
            saving.set(false);
            match result {
                Ok(updated) => {
                    conflict.set(None);
                    reference.set(None);
                    base_version.set(updated.version);
                    on_saved.run(updated.version);
                }
                Err(SaveError::Conflict(current)) => conflict.set(Some(current)),
                Err(SaveError::Api(e)) => error.set(Some(api::error_text(&e))),
            }
        });
    };
    let save = move |_| save_as(base_version.get_untracked());

    view! {
        <Panel title="Content" caption="markdown">
            <div class="kairos-editor">
                <Group justify="between">
                    <SegmentedControl
                        options=vec!["Edit".to_string(), "Preview".to_string()]
                        value=mode
                    />
                    <Group gap="sm">
                        <Pill color=token::ICE>{move || format!("editing v{}", base_version.get())}</Pill>
                        // Raw cl-btn: aurora Button's `disabled` prop is a
                        // plain bool (not reactive) — LoginPage precedent.
                        <button
                            class="cl-btn cl-btn--filled"
                            disabled=move || saving.get() || !dirty()
                            on:click=move |_| save(())
                        >
                            {move || if saving.get() { "Saving…" } else { "Save" }}
                        </button>
                    </Group>
                </Group>

                {move || error.get().map(|message| view! {
                    <Alert title="Save failed" color=token::BAD>
                        <Text size="sm" dimmed=true>{message}</Text>
                    </Alert>
                })}

                {move || if mode.get() == "Preview" {
                    view! {
                        <div class="kairos-markdown" inner_html=markdown::to_html(&content.get())></div>
                    }.into_any()
                } else {
                    view! {
                        <TextInput label="Title" value=title/>
                        <label class="cl-field__label">"Content"</label>
                        <textarea
                            class="kairos-editor__textarea"
                            prop:value=move || content.get()
                            on:input=move |e| content.set(event_target_value(&e))
                        ></textarea>
                    }.into_any()
                }}

                // Manual merge: the server copy stays pinned for reference.
                {move || reference.get().map(|server| view! {
                    <Alert title=format!("Merging by hand onto v{}", server.version) color=token::GOLD>
                        <div class="kairos-editor__reference">
                            <Group justify="between">
                                <Text size="sm" dimmed=true>
                                    "The server copy is below for reference; edit your draft above and save when merged."
                                </Text>
                                <ActionIcon title="Dismiss the server copy" on_click=Callback::new(move |_| reference.set(None))>
                                    "×"
                                </ActionIcon>
                            </Group>
                            <Text size="sm" bright=true>{server.title.clone()}</Text>
                            <textarea
                                class="kairos-editor__textarea kairos-editor__textarea--pane"
                                readonly=true
                                prop:value=server.content.clone()
                            ></textarea>
                        </div>
                    </Alert>
                })}
            </div>
        </Panel>

        // ---- the 409 merge dialog ------------------------------------------
        {move || conflict.get().map(|current| {
            let server = StoredValue::new(current);
            let keep_mine = Callback::new(move |_| {
                save_as(server.with_value(|s| s.version));
            });
            let take_theirs = Callback::new(move |_| {
                server.with_value(|s| {
                    title.set(s.title.clone());
                    content.set(s.content.clone());
                    orig_title.set(s.title.clone());
                    orig_content.set(s.content.clone());
                    base_version.set(s.version);
                });
                conflict.set(None);
                reference.set(None);
            });
            let merge_manually = Callback::new(move |_| {
                server.with_value(|s| {
                    base_version.set(s.version);
                    reference.set(Some(s.clone()));
                });
                conflict.set(None);
            });
            view! {
                <div class="kairos-dialog__backdrop"></div>
                <div class="kairos-dialog" role="dialog" aria-modal="true">
                    <div class="kairos-dialog__box">
                        <Group justify="between">
                            <Text bright=true bold=true>"Edit conflict"</Text>
                            <Pill color=token::BAD>
                                {server.with_value(|s| format!(
                                    "server is at v{} — you edited v{}",
                                    s.version,
                                    base_version.get_untracked(),
                                ))}
                            </Pill>
                        </Group>
                        <Text size="sm" dimmed=true>
                            "Someone saved this item while you were editing (KAIROS-A-0004). Nothing was overwritten — pick how to resolve; every path retries against the server's current version."
                        </Text>
                        <SimpleGrid cols=2>
                            <Panel
                                title=server.with_value(|s| format!("On the server — v{}", s.version))
                                caption="theirs"
                            >
                                <Text size="sm" bright=true>{server.with_value(|s| s.title.clone())}</Text>
                                <textarea
                                    class="kairos-editor__textarea kairos-editor__textarea--pane"
                                    readonly=true
                                    prop:value=server.with_value(|s| s.content.clone())
                                ></textarea>
                            </Panel>
                            <Panel
                                title=format!("Your edit — based on v{}", base_version.get_untracked())
                                caption="yours"
                            >
                                <Text size="sm" bright=true>{title.get_untracked()}</Text>
                                <textarea
                                    class="kairos-editor__textarea kairos-editor__textarea--pane"
                                    readonly=true
                                    prop:value=content.get_untracked()
                                ></textarea>
                            </Panel>
                        </SimpleGrid>
                        <Group gap="sm" justify="between">
                            <Button variant="default" on_click=Callback::new(move |_| conflict.set(None))>
                                "Cancel"
                            </Button>
                            <Group gap="sm">
                                <Button variant="default" on_click=take_theirs>"Take theirs"</Button>
                                <Button variant="default" on_click=merge_manually>"Merge manually"</Button>
                                <button
                                    class="cl-btn cl-btn--filled"
                                    disabled=move || saving.get()
                                    on:click=move |_| keep_mine.run(())
                                >
                                    {move || if saving.get() { "Saving…" } else { "Keep mine (overwrite)" }}
                                </button>
                            </Group>
                        </Group>
                    </div>
                </div>
            }
        })}
    }
}
