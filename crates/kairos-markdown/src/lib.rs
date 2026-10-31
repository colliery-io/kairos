//! Markdown → HTML for the web app and the docs sites (KAIROS-T-0041; moved
//! out of kairos-web into this crate by KAIROS-T-0402, KAIROS-D-0013).
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
//!
//! Link and image URLs are filtered (KAIROS-T-0402): only `http`, `https`,
//! `mailto`, relative URLs and `#` anchors make a link or an image. Any other
//! URL (`javascript:`, `data:`, ...) is dropped and its text stays as text.
//!
//! [`to_html_with_codes`] also turns short codes in text into links. The
//! caller resolves the codes (one call for the whole document); a code it
//! does not resolve stays text. Code spans, code blocks and the text of links
//! are left alone.
//!
//! [`to_html_with_terms`] and [`to_html_with_links`] also link the terms of a
//! dictionary (KAIROS-T-0734, [`terms`]): the longest term first, case
//! ignored, with the meaning in the `title` of the link. Code, the text of
//! links and headings are left alone.

use std::collections::HashMap;

use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html,
};

pub mod notes;
pub mod terms;

pub use terms::TermLink;
use terms::Terms;

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
    render(source, None, &Terms::new(&[]))
}

/// [`to_html`], and each term of `terms` in text becomes a link with its
/// meaning on hover (KAIROS-T-0734).
pub fn to_html_with_terms(source: &str, terms: &[TermLink]) -> String {
    render(source, None, &Terms::new(terms))
}

/// [`to_html`], and each short code in text that `resolve` gives an href
/// becomes a link. `resolve` gets each distinct code once, in one call.
pub fn to_html_with_codes(
    source: &str,
    resolve: impl FnOnce(&[String]) -> HashMap<String, String>,
) -> String {
    to_html_with_links(source, resolve, &[])
}

/// [`to_html_with_codes`] and [`to_html_with_terms`] in one pass: a short
/// code is a link to its item, and a dictionary term in the rest of the
/// text is a link with its meaning on hover.
pub fn to_html_with_links(
    source: &str,
    resolve: impl FnOnce(&[String]) -> HashMap<String, String>,
    terms: &[TermLink],
) -> String {
    let terms = Terms::new(terms);
    let codes = {
        let mut codes: Vec<String> = Vec::new();
        let mut in_link = 0usize;
        let mut in_code = false;
        for event in Parser::new_ext(source, options()) {
            match event {
                Event::Start(Tag::Link { .. }) => in_link += 1,
                Event::End(TagEnd::Link) => in_link = in_link.saturating_sub(1),
                Event::Start(Tag::CodeBlock(_)) => in_code = true,
                Event::End(TagEnd::CodeBlock) => in_code = false,
                Event::Text(text) if in_link == 0 && !in_code => {
                    for (start, end) in short_codes(&text) {
                        let code = &text[start..end];
                        if !codes.iter().any(|known| known == code) {
                            codes.push(code.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        codes
    };
    if codes.is_empty() {
        return render(source, None, &terms);
    }
    let links = resolve(&codes);
    render(source, Some(&links), &terms)
}

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

fn render(source: &str, links: Option<&HashMap<String, String>>, terms: &Terms) -> String {
    let mut events: Vec<Event> = Vec::new();
    // The open special fence and its text.
    let mut open: Option<(Fence, String)> = None;
    // For each open link or image: was its start dropped (an unsafe URL)?
    let mut dropped: Vec<bool> = Vec::new();
    // In a code block that renders as code: its text is not linked.
    let mut in_code = false;
    // KAIROS-T-0504: the note headings seen, for the id of a note without one.
    let mut note_count = 0usize;
    // KAIROS-T-0734: no dictionary term is linked in a heading.
    let mut in_heading = false;
    for event in Parser::new_ext(source, options()) {
        match &event {
            Event::Start(Tag::Heading { .. }) => in_heading = true,
            Event::End(TagEnd::Heading(_)) => in_heading = false,
            _ => {}
        }
        match (&mut open, event) {
            // A note heading gets its id as the anchor, and a link to it.
            (None, Event::Text(text))
                if matches!(
                    events.last(),
                    Some(Event::Start(Tag::Heading {
                        level: HeadingLevel::H2,
                        ..
                    }))
                ) && notes::parse_note_title(&text).is_some() =>
            {
                note_count += 1;
                let id = notes::parse_note_title(&text)
                    .and_then(|title| title.id)
                    .map_or_else(|| notes::note_id(note_count), str::to_string);
                if let Some(Event::Start(Tag::Heading { id: anchor, .. })) = events.last_mut() {
                    *anchor = Some(CowStr::from(id.clone()));
                }
                events.push(Event::Text(text));
                // `id` is `n-<digits>` (checked by the parser): safe markup.
                events.push(Event::Html(CowStr::from(format!(
                    " <a class=\"kairos-note-link\" href=\"#{id}\" title=\"Link to this note\">#</a>"
                ))));
            }
            (None, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))) => {
                match fence_of(&info) {
                    Some(fence) => open = Some((fence, String::new())),
                    None => {
                        in_code = true;
                        events.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))));
                    }
                }
            }
            (None, Event::Start(Tag::CodeBlock(kind))) => {
                in_code = true;
                events.push(Event::Start(Tag::CodeBlock(kind)));
            }
            (None, Event::End(TagEnd::CodeBlock)) => {
                in_code = false;
                events.push(Event::End(TagEnd::CodeBlock));
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
            (None, Event::Start(tag @ (Tag::Link { .. } | Tag::Image { .. }))) => {
                let url = match &tag {
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => dest_url,
                    _ => unreachable!(),
                };
                let safe = is_safe_url(url);
                dropped.push(!safe);
                if safe {
                    events.push(Event::Start(tag));
                }
            }
            (None, Event::End(end @ (TagEnd::Link | TagEnd::Image))) => {
                if !dropped.pop().unwrap_or(false) {
                    events.push(Event::End(end));
                }
            }
            // No link inside a link: the text of a kept link stays as it is.
            (None, Event::Text(text))
                if (links.is_some() || !terms.is_empty())
                    && !in_code
                    && !dropped.iter().any(|dropped| !dropped) =>
            {
                let terms = (!in_heading).then_some(terms);
                link_text(&text, links, terms, &mut events);
            }
            (None, other) => events.push(other),
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

/// Is `url` a safe target for a link or an image: `http`, `https`, `mailto`,
/// or a URL with no scheme (relative, `#anchor`)? A browser drops ASCII tab
/// and newline from a URL and ignores leading spaces and controls, so they
/// are dropped here before the scheme is read.
fn is_safe_url(url: &str) -> bool {
    let cleaned: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let scheme_end = cleaned.find(':');
    let path_start = cleaned.find(['/', '?', '#']);
    match (scheme_end, path_start) {
        (None, _) => true,
        (Some(colon), Some(path)) if path < colon => true,
        (Some(colon), _) => matches!(
            cleaned[..colon].to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto"
        ),
    }
}

/// The byte ranges of the short codes in `text`: `<PREFIX>-<S|I|T|D|A>-<n>`,
/// with an upper-case prefix, at least four digits, and no letter, digit or
/// `-` just before or after.
fn short_codes(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_uppercase() || (i > 0 && word(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && (bytes[j].is_ascii_uppercase() || bytes[j].is_ascii_digit()) {
            j += 1;
        }
        let kind_ok = j + 2 < bytes.len()
            && bytes[j] == b'-'
            && matches!(bytes[j + 1], b'S' | b'I' | b'T' | b'D' | b'A')
            && bytes[j + 2] == b'-';
        if kind_ok {
            let digits_start = j + 3;
            let mut k = digits_start;
            while k < bytes.len() && bytes[k].is_ascii_digit() {
                k += 1;
            }
            if k - digits_start >= 4 && (k == bytes.len() || !word(bytes[k])) {
                found.push((i, k));
                i = k;
                continue;
            }
        }
        i = j.max(i + 1);
    }
    found
}

/// Push `text` as events, with each resolved short code as a link, and
/// each dictionary term of `terms` in the rest as a link (KAIROS-T-0734).
fn link_text<'a>(
    text: &CowStr<'a>,
    links: Option<&HashMap<String, String>>,
    terms: Option<&Terms>,
    events: &mut Vec<Event<'a>>,
) {
    let mut out: Vec<Event<'a>> = Vec::new();
    let mut last = 0;
    let codes = links.map(|links| {
        short_codes(text)
            .into_iter()
            .filter_map(|(start, end)| links.get(&text[start..end]).map(|href| (start, end, href)))
            .collect::<Vec<_>>()
    });
    for (start, end, href) in codes.unwrap_or_default() {
        link_terms(&text[last..start], terms, &mut out);
        out.push(Event::Html(CowStr::from(format!(
            "<a href=\"{}\">{}</a>",
            escape(href),
            escape(&text[start..end])
        ))));
        last = end;
    }
    link_terms(&text[last..], terms, &mut out);
    // Nothing linked: the text as it was.
    if out.iter().all(|event| matches!(event, Event::Text(_))) {
        events.push(Event::Text(text.clone()));
    } else {
        events.extend(out);
    }
}

/// Push `text` as events, with each term of `terms` as a link.
fn link_terms<'a>(text: &str, terms: Option<&Terms>, events: &mut Vec<Event<'a>>) {
    let mut last = 0;
    if let Some(terms) = terms.filter(|terms| !terms.is_empty()) {
        for (start, end, term) in terms::find(text, terms) {
            if start > last {
                events.push(Event::Text(CowStr::from(text[last..start].to_string())));
            }
            events.push(Event::Html(CowStr::from(terms::term_html(
                term,
                &text[start..end],
            ))));
            last = end;
        }
    }
    if last < text.len() {
        events.push(Event::Text(CowStr::from(text[last..].to_string())));
    }
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
fn gherkin_html(source: &str) -> String {
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
fn mermaid_html(source: &str) -> String {
    format!(
        "<pre class=\"kairos-mermaid\">{}</pre>",
        escape(source.trim_end())
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{to_html, to_html_with_codes};

    /// KAIROS-T-0402: a link or an image with an unsafe URL is text.
    #[test]
    fn unsafe_links_and_images_are_text() {
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "java\tscript:alert(1)",
            "data:text/html,x",
            "vbscript:x",
        ] {
            let rendered = to_html(&format!("[click]({url}) and ![pic]({url})\n"));
            assert!(!rendered.contains("<a "), "{url}: {rendered}");
            assert!(!rendered.contains("<img"), "{url}: {rendered}");
            assert!(rendered.contains("click"), "{url}: {rendered}");
        }
        let rendered = to_html("<javascript:alert(1)>\n");
        assert!(!rendered.contains("<a "), "{rendered}");
    }

    /// Safe URLs still make links and images.
    #[test]
    fn safe_links_and_images_render() {
        for url in [
            "https://example.com/x",
            "http://example.com",
            "mailto:robin@example.com",
            "../other/page",
            "/pages/x/",
            "#section",
            "page?a=b:c",
        ] {
            let rendered = to_html(&format!("[go]({url})\n"));
            assert!(rendered.contains("<a href="), "{url}: {rendered}");
        }
        let rendered = to_html("![pic](https://example.com/a.png)\n");
        assert!(
            rendered.contains("<img src=\"https://example.com/a.png\""),
            "{rendered}"
        );
    }

    /// An unsafe image inside a safe link keeps the link.
    #[test]
    fn an_unsafe_image_in_a_link_keeps_the_link() {
        let rendered = to_html("[![pic](javascript:x)](https://example.com)\n");
        assert!(
            rendered.contains("<a href=\"https://example.com\">"),
            "{rendered}"
        );
        assert!(!rendered.contains("<img"), "{rendered}");
    }

    fn resolver(known: &[(&str, &str)]) -> impl FnOnce(&[String]) -> HashMap<String, String> {
        let known: HashMap<String, String> = known
            .iter()
            .map(|(code, href)| (code.to_string(), href.to_string()))
            .collect();
        move |codes: &[String]| {
            codes
                .iter()
                .filter_map(|code| known.get(code).map(|href| (code.clone(), href.clone())))
                .collect()
        }
    }

    /// KAIROS-T-0402: a resolved code is a link, an unresolved code is text,
    /// and a code in a code span, a code block or a link is left alone.
    // KAIROS-T-0504: a note heading gets its id as the anchor and a link.
    #[test]
    fn note_headings_get_anchors() {
        let rendered = to_html(
            "## Note (2026-10-01, Robin)\n\nOne.\n\n## Objective\n\nx\n\n## Note n-2 (2026-10-02, Sam)\n\nTwo.\n",
        );
        assert!(
            rendered.contains(
                "<h2 id=\"n-1\">Note (2026-10-01, Robin) <a class=\"kairos-note-link\" href=\"#n-1\" title=\"Link to this note\">#</a></h2>"
            ),
            "{rendered}"
        );
        assert!(rendered.contains("<h2>Objective</h2>"), "{rendered}");
        assert!(
            rendered.contains("<h2 id=\"n-2\">Note n-2 (2026-10-02, Sam) <a class=\"kairos-note-link\" href=\"#n-2\""),
            "{rendered}"
        );
    }

    #[test]
    fn short_codes_link_when_resolved() {
        let source = "See KAIROS-D-0012, KAIROS-T-0001 and `KAIROS-D-0012`.\n\n```\nKAIROS-D-0012\n```\n\n[KAIROS-D-0012](https://example.com)\n";
        let rendered = to_html_with_codes(
            source,
            resolver(&[("KAIROS-D-0012", "/pages/c/k/docs/KAIROS-D-0012/")]),
        );
        assert!(
            rendered.contains("See <a href=\"/pages/c/k/docs/KAIROS-D-0012/\">KAIROS-D-0012</a>, KAIROS-T-0001 and"),
            "{rendered}"
        );
        assert!(
            rendered.contains("<code>KAIROS-D-0012</code>"),
            "{rendered}"
        );
        assert!(
            rendered.contains("<pre><code>KAIROS-D-0012\n</code></pre>"),
            "{rendered}"
        );
        assert!(
            rendered.contains("<a href=\"https://example.com\">KAIROS-D-0012</a>"),
            "{rendered}"
        );
        assert_eq!(rendered.matches("<a ").count(), 2, "{rendered}");
    }

    /// The resolver gets each distinct code once, and only codes.
    #[test]
    fn the_resolver_gets_each_code_once() {
        let mut asked = Vec::new();
        to_html_with_codes(
            "KAIROS-T-0001 KAIROS-T-0001 COLLIERY-I-0619 X-T-12 kairos-T-0001 AKAIROS-T-0001x KAIROS-Q-0001\n",
            |codes| {
                asked = codes.to_vec();
                HashMap::new()
            },
        );
        assert_eq!(
            asked,
            vec!["KAIROS-T-0001".to_string(), "COLLIERY-I-0619".to_string()]
        );
    }

    /// No code: the resolver is not called.
    #[test]
    fn no_code_no_resolver_call() {
        let rendered = to_html_with_codes("plain text\n", |_| panic!("not called"));
        assert!(rendered.contains("plain text"));
    }

    /// A resolved href is escaped.
    #[test]
    fn a_resolved_href_is_escaped() {
        let rendered =
            to_html_with_codes("KAIROS-T-0001\n", resolver(&[("KAIROS-T-0001", "/a\"b")]));
        assert!(rendered.contains("href=\"/a&quot;b\""), "{rendered}");
    }

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
