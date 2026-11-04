//! The markdown of the app: the shared renderer
//! ([`kairos_markdown`]), with each short code in the text as a link to
//! its item page.
//!
//! The renderer of the server links only the codes that it finds in its
//! database. The app has no such read at render time, and it needs none:
//! `/items/:code` serves every family, follows a retired code, and shows
//! its own not-found state for a code that does not exist.

use std::collections::HashMap;

/// The path of the item page of `code`. The hover preview
/// ([`crate::pages::code_peek`]) reads the code back from it.
pub(crate) const ITEM_PATH: &str = "/items/";

/// Markdown to HTML, with each short code in text as a link to its page.
/// Codes in code spans, in code blocks and in links stay as they are.
pub(crate) fn to_html(source: &str) -> String {
    kairos_markdown::to_html_with_codes(source, |codes| {
        codes
            .iter()
            .map(|code| (code.clone(), format!("{ITEM_PATH}{code}")))
            .collect::<HashMap<_, _>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_in_text_links_to_its_page() {
        let html = to_html("Blocked by DEMO-T-0042 for now.");
        assert!(
            html.contains(r#"<a href="/items/DEMO-T-0042">DEMO-T-0042</a>"#),
            "{html}"
        );
    }

    #[test]
    fn every_family_links() {
        for code in [
            "DEMO-S-0001",
            "DEMO-I-0002",
            "DEMO-T-0003",
            "DEMO-D-0004",
            "DEMO-A-0005",
        ] {
            let html = to_html(&format!("see {code}"));
            assert!(html.contains(&format!(r#"href="/items/{code}""#)), "{html}");
        }
    }

    #[test]
    fn a_code_in_code_stays_text() {
        let html = to_html("`DEMO-T-0042`\n\n```\nDEMO-T-0043\n```\n");
        assert!(!html.contains("<a "), "{html}");
    }

    #[test]
    fn a_code_in_a_link_is_not_linked_twice() {
        let html = to_html("[DEMO-T-0042](https://example.com/x)");
        assert_eq!(html.matches("<a ").count(), 1, "{html}");
        assert!(html.contains("https://example.com/x"), "{html}");
    }

    #[test]
    fn text_with_no_code_is_the_shared_render() {
        let source = "# Title\n\nSome *text*.";
        assert_eq!(to_html(source), kairos_markdown::to_html(source));
    }
}
