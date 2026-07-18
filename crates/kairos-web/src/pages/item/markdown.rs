//! Markdown → HTML for the preview pane (KAIROS-T-0041).
//!
//! Renderer decision (recorded in the task): **pulldown-cmark** — pure
//! Rust, CommonMark-conformant, compiles cleanly to `wasm32-unknown-
//! unknown` (no `getrandom`/`tokio` in its dependency tree; default
//! features disabled — `getopts` only feeds its CLI examples). The one
//! serious alternative, `comrak`, pulls a substantially heavier tree for
//! extensions this page doesn't need.
//!
//! Raw HTML embedded in the markdown is **escaped, not passed through**:
//! item content is multi-user data rendered via `inner_html`, so
//! `Event::Html`/`Event::InlineHtml` become text (pulldown-cmark's
//! `push_html` escapes text events). Everything else — headings, lists,
//! tables, code fences, links — renders normally.

use pulldown_cmark::{Event, Options, Parser, html};

/// Render markdown to HTML, escaping raw HTML events (XSS-safe for
/// `inner_html` under the same-origin API's user-authored content).
pub fn to_html(source: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let events = Parser::new_ext(source, options).map(|event| match event {
        // Text events are HTML-escaped by push_html; Html events are not.
        Event::Html(raw) => Event::Text(raw),
        Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

#[cfg(test)]
mod tests {
    use super::to_html;

    #[test]
    fn renders_commonmark_structure() {
        let rendered = to_html("## Why\n\n- one\n- two\n\n`code` and **bold**\n");
        assert!(rendered.contains("<h2>Why</h2>"));
        assert!(rendered.contains("<li>one</li>"));
        assert!(rendered.contains("<code>code</code>"));
        assert!(rendered.contains("<strong>bold</strong>"));
    }

    #[test]
    fn renders_tables_and_task_lists() {
        let rendered = to_html("| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n");
        assert!(rendered.contains("<table>"));
        assert!(rendered.contains("checkbox"));
    }

    /// Raw HTML (block and inline) is escaped, never emitted as markup.
    #[test]
    fn escapes_raw_html() {
        let rendered = to_html("<script>alert(1)</script>\n\ninline <img src=x onerror=y> here\n");
        assert!(!rendered.contains("<script>"));
        assert!(!rendered.contains("<img"));
        assert!(rendered.contains("&lt;script&gt;"));
        assert!(rendered.contains("&lt;img"));
    }
}
