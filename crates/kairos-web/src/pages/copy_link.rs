//! Copy-link button (KAIROS-T-0076): the small button that sits next to a
//! short code and puts the item's ABSOLUTE detail URL on the clipboard.
//!
//! COLLIERY-T-1836: this is the Aurora `CopyButton` (icon, link). It
//! copies the path as an absolute URL of the current origin, it confirms
//! with "Copied" (title and an `aria-live` region), and it does not render
//! when the browser has no Clipboard API (a plain-HTTP deploy), as the
//! old local button did (decision recorded in KAIROS-T-0076). A click does
//! not go on to the draggable card around it.

use aurora_dark::components::CopyButton;
use leptos::prelude::*;

/// The detail path of an item; `CopyButton link=true` makes it absolute.
fn item_path(code: &str) -> String {
    format!("/items/{code}")
}

/// Copies `/items/{code}` (absolute) to the clipboard.
#[component]
pub(crate) fn CopyLinkButton(#[prop(into)] code: String) -> impl IntoView {
    view! {
        <CopyButton value=item_path(&code) icon=true link=true label="Copy link"/>
    }
}

#[cfg(test)]
mod tests {
    use super::item_path;

    #[test]
    fn the_copied_path_is_the_detail_route() {
        assert_eq!(item_path("DEMO-T-0002"), "/items/DEMO-T-0002");
    }
}
