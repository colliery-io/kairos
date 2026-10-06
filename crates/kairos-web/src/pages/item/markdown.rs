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
//!
//! Two fences render specially (KAIROS-T-0324):
//!
//! - ```` ```gherkin ```` (or ```` ```feature ````): highlighted here, in
//!   Rust, as spans over ESCAPED text ([`gherkin_html`]);
//! - ```` ```mermaid ````: a `<pre class="kairos-mermaid">` that holds the
//!   ESCAPED source. The page script (`index.html`) finds each one, loads
//!   the vendored Mermaid only then, and draws it with
//!   `securityLevel: 'strict'`.
//!
//! The HTML of the two fences is built only from escaped text, so the rule
//! above holds: no markup that a user wrote reaches the page.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html};

/// A fence that renders specially.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fence {
    Gherkin,
    Mermaid,
}

fn fence_of(info: &str) -> Option<Fence> {
    match info
        .split_whitespace()
        .next()?
        .to_ascii_lowercase()
        .as_str()
    {
        "gherkin" | "feature" | "cucumber" => Some(Fence::Gherkin),
        "mermaid" => Some(Fence::Mermaid),
        _ => None,
    }
}

/// Render markdown to HTML, escaping raw HTML events (XSS-safe for
/// `inner_html` under the same-origin API's user-authored content).
pub fn to_html(source: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut events: Vec<Event> = Vec::new();
    // The open special fence and its text.
    let mut open: Option<(Fence, String)> = None;
    for event in Parser::new_ext(source, options) {
        match (&mut open, event) {
            (None, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))) => {
                match fence_of(&info) {
                    Some(fence) => open = Some((fence, String::new())),
                    None => events.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))),
                }
            }
            (Some((_, text)), Event::Text(chunk)) => text.push_str(&chunk),
            (Some(_), Event::End(TagEnd::CodeBlock)) => {
                let (fence, text) = open.take().expect("an open fence");
                let markup = match fence {
                    Fence::Gherkin => gherkin_html(&text),
                    Fence::Mermaid => mermaid_html(&text),
                };
                // Built from escaped text only: see the module docs.
                events.push(Event::Html(CowStr::from(markup)));
            }
            (Some(_), _) => {}
            // Text events are HTML-escaped by push_html; Html events are not.
            (None, Event::Html(raw)) => events.push(Event::Text(raw)),
            (None, Event::InlineHtml(raw)) => events.push(Event::Text(raw)),
            (None, other) => events.push(other),
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

/// Escape text for HTML content and attributes.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn span(class: &str, text: &str) -> String {
    format!(
        "<span class=\"kairos-gherkin__{class}\">{}</span>",
        escape(text)
    )
}

/// The keywords that open a line, longest first, each with its colon when
/// it has one.
const GHERKIN_BLOCK_KEYWORDS: &[&str] = &[
    "Scenario Outline:",
    "Scenario Template:",
    "Background:",
    "Examples:",
    "Scenarios:",
    "Feature:",
    "Scenario:",
    "Example:",
    "Rule:",
];
const GHERKIN_STEP_KEYWORDS: &[&str] = &["Given ", "When ", "Then ", "And ", "But ", "* "];

/// A gherkin fence as highlighted, escaped HTML. Each line keeps its text;
/// the spans mark keywords, tags, comments, table cells, strings,
/// `<placeholders>` and docstrings.
pub(crate) fn gherkin_html(source: &str) -> String {
    let mut out = String::from("<pre class=\"kairos-gherkin\"><code>");
    let mut in_docstring = false;
    for (i, line) in source.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        out.push_str(&escape(indent));
        if trimmed.starts_with("\"\"\"") || trimmed.starts_with("```") {
            in_docstring = !in_docstring;
            out.push_str(&span("docstring", trimmed));
            continue;
        }
        if in_docstring {
            out.push_str(&span("docstring", trimmed));
            continue;
        }
        if trimmed.starts_with('#') {
            out.push_str(&span("comment", trimmed));
        } else if trimmed.starts_with('@') {
            out.push_str(&span("tag", trimmed));
        } else if trimmed.starts_with('|') {
            out.push_str(&table_row(trimmed));
        } else if let Some(keyword) = GHERKIN_BLOCK_KEYWORDS
            .iter()
            .find(|keyword| trimmed.starts_with(**keyword))
        {
            out.push_str(&span("keyword", keyword));
            out.push_str(&span("title", &trimmed[keyword.len()..]));
        } else if let Some(keyword) = GHERKIN_STEP_KEYWORDS
            .iter()
            .find(|keyword| trimmed.starts_with(**keyword))
        {
            out.push_str(&span("step", keyword.trim_end()));
            out.push(' ');
            out.push_str(&step_text(&trimmed[keyword.len()..]));
        } else {
            out.push_str(&escape(trimmed));
        }
    }
    out.push_str("</code></pre>");
    out
}

/// A table row: the pipes and the cells.
fn table_row(row: &str) -> String {
    let mut out = String::new();
    for (i, cell) in row.split('|').enumerate() {
        if i > 0 {
            out.push_str(&span("pipe", "|"));
        }
        if !cell.is_empty() {
            out.push_str(&span("cell", cell));
        }
    }
    out
}

/// The text of a step: `"strings"` and `<placeholders>` are marked.
fn step_text(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(['"', '<']) {
        out.push_str(&escape(&rest[..start]));
        let close = if rest[start..].starts_with('"') {
            '"'
        } else {
            '>'
        };
        match rest[start + 1..].find(close) {
            Some(len) => {
                let end = start + 1 + len + 1;
                let class = if close == '"' {
                    "string"
                } else {
                    "placeholder"
                };
                out.push_str(&span(class, &rest[start..end]));
                rest = &rest[end..];
            }
            None => {
                out.push_str(&escape(&rest[start..]));
                rest = "";
            }
        }
    }
    out.push_str(&escape(rest));
    out
}

/// A mermaid fence: a container that holds the escaped source. The page
/// script draws it (see the module docs).
pub(crate) fn mermaid_html(source: &str) -> String {
    format!(
        "<pre class=\"kairos-mermaid\">{}</pre>",
        escape(source.trim_end())
    )
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

    /// KAIROS-T-0324: a gherkin fence is highlighted, and stays escaped.
    #[test]
    fn highlights_a_gherkin_fence() {
        let rendered = to_html(
            "```gherkin\n@smoke\nFeature: Teams\n  # a comment\n  Scenario: A pill\n    Given an initiative \"<b>x</b>\" with <count> tasks\n    Then the card shows a pill\n    | team | count |\n```\n",
        );
        assert!(
            rendered.contains("<pre class=\"kairos-gherkin\">"),
            "{rendered}"
        );
        assert!(rendered.contains("<span class=\"kairos-gherkin__tag\">@smoke</span>"));
        assert!(rendered.contains("<span class=\"kairos-gherkin__keyword\">Feature:</span>"));
        assert!(rendered.contains("<span class=\"kairos-gherkin__keyword\">Scenario:</span>"));
        assert!(rendered.contains("<span class=\"kairos-gherkin__comment\"># a comment</span>"));
        assert!(rendered.contains("<span class=\"kairos-gherkin__step\">Given</span>"));
        assert!(rendered.contains("<span class=\"kairos-gherkin__step\">Then</span>"));
        assert!(rendered.contains(
            "<span class=\"kairos-gherkin__string\">&quot;&lt;b&gt;x&lt;/b&gt;&quot;</span>"
        ));
        assert!(
            rendered.contains("<span class=\"kairos-gherkin__placeholder\">&lt;count&gt;</span>")
        );
        assert!(rendered.contains("<span class=\"kairos-gherkin__cell\"> team </span>"));
        assert!(
            !rendered.contains("<b>"),
            "user markup must stay escaped: {rendered}"
        );
    }

    /// A docstring keeps its lines as text.
    #[test]
    fn a_gherkin_docstring_is_text() {
        let rendered =
            to_html("```feature\nGiven a page\n  \"\"\"\n  Then <script>\n  \"\"\"\n```\n");
        assert!(
            rendered
                .contains("<span class=\"kairos-gherkin__docstring\">Then &lt;script&gt;</span>")
        );
        assert!(!rendered.contains("<script>"));
    }

    /// KAIROS-T-0324: a mermaid fence is a container with the escaped
    /// source; the page script draws it.
    #[test]
    fn a_mermaid_fence_is_a_diagram_container() {
        let rendered = to_html("```mermaid\ngraph TD\n  A[\"<img src=x>\"] --> B\n```\n");
        assert!(rendered.contains("<pre class=\"kairos-mermaid\">graph TD\n  A[&quot;&lt;img src=x&gt;&quot;] --&gt; B</pre>"), "{rendered}");
        assert!(!rendered.contains("<img"));
    }

    /// Other fences render as before.
    #[test]
    fn other_fences_are_code() {
        let rendered = to_html("```rust\nfn main() {}\n```\n");
        assert!(
            rendered.contains("<pre><code class=\"language-rust\">fn main() {}"),
            "{rendered}"
        );
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
