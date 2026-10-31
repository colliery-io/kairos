//! Content editing for the item detail page (KAIROS-T-0041): a thin
//! wrapper over the generalized [`crate::pages::editor::MarkdownEditor`]
//! (extracted in KAIROS-T-0086) that binds the save to the item family's
//! versioned content PATCH. All edit/preview, toolbar, and 409 merge
//! behavior lives in the shared component.

use std::rc::Rc;

use leptos::prelude::*;

use super::api::{self, Family, ItemDetail};
use crate::auth::use_auth;
use crate::pages::editor::{MarkdownEditor, Saver, ServerCopy, draft_key};

/// The editable content panel. It stays mounted while the page shows the
/// item (KAIROS-T-0618): a save gives the item that the PATCH returns to
/// the page, and a new copy from the page comes in as `server`.
#[component]
pub fn ContentEditor(
    family: Family,
    #[prop(into)] code: String,
    #[prop(into)] initial_title: String,
    #[prop(into)] initial_content: String,
    initial_version: i32,
    /// The editor renders the content instead of offering an edit that
    /// the server refuses: the item is archived (KAIROS-T-0164, ADR-20),
    /// or the person may not edit it (KAIROS-T-0581, [`super::access`]).
    #[prop(into)]
    read_only: Signal<bool>,
    /// Why the content is read-only; empty while the page does not know.
    #[prop(into)]
    read_only_reason: Signal<String>,
    /// The content of the item as the page knows it (KAIROS-T-0618).
    #[prop(into)]
    server: Signal<ServerCopy>,
    /// Fired after a successful save with the item that the PATCH
    /// returned: the page shows it and posts the notice. No read follows.
    on_saved: Callback<ItemDetail>,
    /// The terms of the dictionary of the organization (KAIROS-T-0734).
    #[prop(into)]
    terms: Signal<Vec<kairos_markdown::TermLink>>,
) -> impl IntoView {
    let auth = use_auth();
    // KAIROS-T-0581: the draft is kept outside this subtree, so a reload
    // of the tab does not lose it.
    let key = draft_key(&code);
    // The item of the last save, for `on_saved` (the editor only takes
    // the version).
    let saved: StoredValue<Option<ItemDetail>, LocalStorage> = StoredValue::new_local(None);
    let saver: Saver = Rc::new(move |title, content, version| {
        let code = code.clone();
        Box::pin(async move {
            api::update_content(auth, family, &code, &title, &content, version)
                .await
                .map(|updated| {
                    let version = updated.version;
                    saved.set_value(Some(updated));
                    version
                })
        })
    });
    let on_version = Callback::new(move |_version: i32| {
        if let Some(item) = saved.try_update_value(Option::take).flatten() {
            on_saved.run(item);
        }
    });
    view! {
        <MarkdownEditor
            initial_title
            initial_content
            initial_version
            read_only
            read_only_reason
            draft_key=key
            server
            terms
            on_saved=on_version
            saver
            // KAIROS-T-0323: a ticket is read more than it is changed.
            start_in_preview=true
        />
    }
}
