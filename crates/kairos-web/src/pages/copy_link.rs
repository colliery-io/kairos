//! Copy-link button (KAIROS-T-0076): the small button that sits next to a
//! short code and puts the item's ABSOLUTE detail URL on the clipboard.
//!
//! Uses the async Clipboard API, which browsers expose only in secure
//! contexts (localhost qualifies). When the API is absent the button does
//! not render at all — a plain-HTTP deploy degrades to no affordance
//! rather than a dead control (decision recorded in KAIROS-T-0076).

use leptos::prelude::*;

/// The absolute detail URL for an item, from the current origin.
fn item_url(code: &str) -> Option<String> {
    let origin = web_sys::window()?.location().origin().ok()?;
    Some(format!("{origin}/items/{code}"))
}

/// The Clipboard handle — `None` outside secure contexts, where the
/// `navigator.clipboard` property is undefined.
fn clipboard() -> Option<web_sys::Clipboard> {
    let clipboard = web_sys::window()?.navigator().clipboard();
    (!clipboard.is_undefined()).then_some(clipboard)
}

/// Copies `/items/{code}` (absolute) to the clipboard; flashes ✓ on
/// success. Keyboard-accessible: a real `<button>` with an aria-label.
#[component]
pub(crate) fn CopyLinkButton(#[prop(into)] code: String) -> impl IntoView {
    if clipboard().is_none() {
        return ().into_any();
    }
    let code = StoredValue::new(code);
    let copied = RwSignal::new(false);
    let on_click = move |ev: web_sys::MouseEvent| {
        // Inside a link group on a draggable card: this click is the
        // button's alone.
        ev.prevent_default();
        ev.stop_propagation();
        let Some((clipboard, url)) = clipboard().zip(code.with_value(|c| item_url(c))) else {
            return;
        };
        leptos::task::spawn_local(async move {
            if wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&url))
                .await
                .is_ok()
            {
                copied.set(true);
                set_timeout(
                    move || copied.set(false),
                    std::time::Duration::from_millis(1500),
                );
            }
        });
    };
    view! {
        <button
            class="kairos-copy-link"
            type="button"
            aria-label="Copy link"
            title="Copy link"
            on:click=on_click
        >
            {move || if copied.get() { "✓" } else { "⧉" }}
        </button>
    }
    .into_any()
}
