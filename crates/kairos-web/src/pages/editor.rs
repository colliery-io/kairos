//! The generalized markdown editor (KAIROS-T-0086): title + markdown
//! edit/preview with a syntax toolbar, the A-0004 versioned save, and the
//! 409 merge UI — extracted from the item detail's `ContentEditor`
//! (KAIROS-T-0041) behind a `Saver` callback so team pages (and any
//! future versioned content) reuse the exact same machinery instead of
//! forking the merge dialog.
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

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use aurora_dark::components::{
    ActionIcon, Alert, Button, Group, Panel, Pill, SegmentedControl, SimpleGrid, Text, TextInput,
};
use aurora_dark::tokens::token;
use leptos::prelude::*;

use super::item::api::{CurrentVersion, SaveError};
use super::item::markdown;

/// One save attempt: `(title, content, based-on version)` → the new
/// version, or the typed failure the merge UI understands.
pub type SaveFuture = Pin<Box<dyn Future<Output = Result<i32, SaveError>>>>;
/// The domain-specific save call (item content PATCH, team-page PATCH…).
pub type Saver = Rc<dyn Fn(String, String, i32) -> SaveFuture>;

/// The toolbar's insertions: `(label, tooltip, prefix, suffix,
/// placeholder-when-nothing-selected)`.
const TOOLBAR: &[(&str, &str, &str, &str, &str)] = &[
    ("H", "Heading", "## ", "", "Heading"),
    ("B", "Bold", "**", "**", "bold"),
    ("I", "Italic", "*", "*", "italic"),
    ("•", "List item", "- ", "", "item"),
    ("🔗", "Link", "[", "](url)", "text"),
    ("`", "Code", "`", "`", "code"),
];

/// The editable content panel. Recreated (with fresh server state) when
/// the page refetches after a successful save — the parent owns that
/// refetch via `on_saved`.
#[component]
pub fn MarkdownEditor(
    #[prop(into)] initial_title: String,
    #[prop(into)] initial_content: String,
    initial_version: i32,
    /// Fired after a successful save; the parent refetches (server state
    /// is the source of truth) and posts the page-level notice.
    on_saved: Callback<i32>,
    /// The domain's versioned save call.
    saver: Saver,
) -> impl IntoView {
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

    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();

    let dirty = move || title.get() != orig_title.get() || content.get() != orig_content.get();

    // One save routine; the merge dialog re-enters it with the rebased
    // version ("keep mine"). `new_local`: the saver is an `Rc` (wasm is
    // single-threaded; futures here are !Send by design).
    let saver = StoredValue::new_local(saver);
    let save_as = move |version: i32| {
        if saving.get_untracked() {
            return;
        }
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let future = saver.with_value(|s| {
                s(title.get_untracked(), content.get_untracked(), version)
            });
            let result = future.await;
            saving.set(false);
            match result {
                Ok(new_version) => {
                    conflict.set(None);
                    reference.set(None);
                    base_version.set(new_version);
                    on_saved.run(new_version);
                }
                Err(SaveError::Conflict(current)) => conflict.set(Some(current)),
                Err(SaveError::Api(e)) => {
                    error.set(Some(super::item::api::error_text(&e)));
                }
            }
        });
    };
    let save = move |_| save_as(base_version.get_untracked());

    // Toolbar insertion at the cursor (KAIROS-T-0086). Selection indexes
    // are UTF-16 code units, so splicing happens in UTF-16 space — byte
    // indexes would panic mid-codepoint on non-ASCII content.
    let apply_syntax = move |prefix: &str, suffix: &str, placeholder: &str| {
        let Some(el) = textarea_ref.get_untracked() else {
            return;
        };
        let value = el.value();
        let units: Vec<u16> = value.encode_utf16().collect();
        let start = (el.selection_start().ok().flatten().unwrap_or(0) as usize).min(units.len());
        let end = (el.selection_end().ok().flatten().unwrap_or(0) as usize).clamp(start, units.len());
        let selected = String::from_utf16_lossy(&units[start..end]);
        let inserted = if selected.is_empty() {
            format!("{prefix}{placeholder}{suffix}")
        } else {
            format!("{prefix}{selected}{suffix}")
        };
        let inserted_units: Vec<u16> = inserted.encode_utf16().collect();
        let mut new_units = Vec::with_capacity(units.len() + inserted_units.len());
        new_units.extend_from_slice(&units[..start]);
        new_units.extend_from_slice(&inserted_units);
        new_units.extend_from_slice(&units[end..]);
        let new_value = String::from_utf16_lossy(&new_units);
        // Element first (cursor placement), then the signal — same string,
        // so the reactive re-render leaves the cursor where we put it.
        el.set_value(&new_value);
        let cursor = (start + inserted_units.len()) as u32;
        let _ = el.set_selection_range(cursor, cursor);
        let _ = el.focus();
        content.set(new_value);
    };

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
                        <Group justify="between">
                            <label class="cl-field__label">"Content"</label>
                            <Group gap="xs">
                                {TOOLBAR.iter().map(|(label, tooltip, prefix, suffix, placeholder)| {
                                    view! {
                                        <ActionIcon
                                            title=*tooltip
                                            on_click=Callback::new(move |_| apply_syntax(prefix, suffix, placeholder))
                                        >
                                            {*label}
                                        </ActionIcon>
                                    }
                                }).collect_view()}
                            </Group>
                        </Group>
                        <textarea
                            class="kairos-editor__textarea"
                            node_ref=textarea_ref
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
                            "Someone saved this content while you were editing (KAIROS-A-0004). Nothing was overwritten — pick how to resolve; every path retries against the server's current version."
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
