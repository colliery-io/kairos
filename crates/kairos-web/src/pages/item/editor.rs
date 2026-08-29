//! Content editing for the item detail page (KAIROS-T-0041): a thin
//! wrapper over the generalized [`crate::pages::editor::MarkdownEditor`]
//! (extracted in KAIROS-T-0086) that binds the save to the item family's
//! versioned content PATCH. All edit/preview, toolbar, and 409 merge
//! behavior lives in the shared component.

use std::rc::Rc;

use leptos::prelude::*;

use super::api::{self, Family};
use crate::auth::use_auth;
use crate::pages::editor::{MarkdownEditor, Saver};

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
    let saver: Saver = Rc::new(move |title, content, version| {
        let code = code.clone();
        Box::pin(async move {
            api::update_content(auth, family, &code, &title, &content, version)
                .await
                .map(|updated| updated.version)
        })
    });
    view! {
        <MarkdownEditor initial_title initial_content initial_version on_saved saver/>
    }
}
