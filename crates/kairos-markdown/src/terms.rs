//! Dictionary terms in text (KAIROS-T-0734, COLLIERY-I-0664 decisions 6
//! and 7).
//!
//! The caller gives the terms of a dictionary ([`TermLink`]). Each term in
//! the text becomes a link with the meaning in its `title`, so that the
//! browser shows the meaning on hover. The longest term wins
//! ("production document" before "production"), and case is ignored. A
//! term matches at word bounds only: "board" is not found in "boards" or
//! in "keyboard".
//!
//! [`crate::to_html_with_links`] calls [`find`] only for text outside
//! links, code and headings.

/// One term of a dictionary, as the page links it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermLink {
    /// The term: one word or more, as the dictionary writes it.
    pub word: String,
    pub part_of_speech: String,
    pub meaning: String,
    /// The target of the link: the dictionary that defines the term.
    /// `None`, or a URL that is not safe: the term is a `<span>` that can
    /// get the focus, with the same `title`.
    pub href: Option<String>,
}

/// The terms, ready to match: the longest first.
pub(crate) struct Terms<'t> {
    /// Each term with its words as chars, sorted by length (chars),
    /// longest first. A term with no word is left out.
    list: Vec<(&'t TermLink, Vec<Vec<char>>)>,
}

impl<'t> Terms<'t> {
    pub(crate) fn new(terms: &'t [TermLink]) -> Self {
        let mut list: Vec<(&TermLink, Vec<Vec<char>>)> = terms
            .iter()
            .map(|term| {
                let words: Vec<Vec<char>> = term
                    .word
                    .split_whitespace()
                    .map(|word| word.chars().collect())
                    .collect();
                (term, words)
            })
            .filter(|(_, words)| !words.is_empty())
            .collect();
        let length = |words: &[Vec<char>]| words.iter().map(Vec::len).sum::<usize>() + words.len();
        list.sort_by_key(|(_, words)| std::cmp::Reverse(length(words)));
        Terms { list }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// A letter, a digit or `_`: a term does not start or end next to one.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn same(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// The end (a byte offset) of `words` in `text` from `start`, or `None`.
/// One or more white space chars in the text match the space between two
/// words.
fn match_at(text: &str, start: usize, words: &[Vec<char>]) -> Option<usize> {
    let mut chars = text[start..].char_indices().peekable();
    for (n, word) in words.iter().enumerate() {
        if n > 0 {
            let mut spaces = 0;
            while chars.next_if(|(_, c)| c.is_whitespace()).is_some() {
                spaces += 1;
            }
            if spaces == 0 {
                return None;
            }
        }
        for &expected in word {
            match chars.next() {
                Some((_, c)) if same(c, expected) => {}
                _ => return None,
            }
        }
    }
    let end = chars
        .peek()
        .map_or(text.len(), |(offset, _)| start + offset);
    match chars.peek() {
        Some((_, c)) if word_char(*c) => None,
        _ => Some(end),
    }
}

/// Each term in `text`, as `(start, end, term)` byte ranges, from the left.
/// At one place the longest term wins; terms do not overlap.
pub(crate) fn find<'t>(text: &str, terms: &Terms<'t>) -> Vec<(usize, usize, &'t TermLink)> {
    let mut found = Vec::new();
    let mut previous: Option<char> = None;
    let mut skip_to = 0;
    for (start, c) in text.char_indices() {
        let at_bound = !previous.is_some_and(word_char);
        previous = Some(c);
        if start < skip_to || !at_bound {
            continue;
        }
        for (term, words) in &terms.list {
            if let Some(end) = match_at(text, start, words) {
                found.push((start, end, *term));
                skip_to = end;
                break;
            }
        }
    }
    found
}

/// The HTML of one term as it is in the text (`shown`). Built from escaped
/// text only.
pub(crate) fn term_html(term: &TermLink, shown: &str) -> String {
    let title = if term.part_of_speech.trim().is_empty() {
        term.meaning.clone()
    } else {
        format!("{}: {}", term.part_of_speech.trim(), term.meaning)
    };
    let attributes = format!(
        "class=\"kairos-term\" title=\"{}\" data-testid=\"dictionary-term\" data-term=\"{}\"",
        crate::escape(&title),
        crate::escape(&term.word),
    );
    match term.href.as_deref().filter(|href| crate::is_safe_url(href)) {
        Some(href) => format!(
            "<a href=\"{}\" {attributes}>{}</a>",
            crate::escape(href),
            crate::escape(shown)
        ),
        None => format!(
            "<span tabindex=\"0\" {attributes}>{}</span>",
            crate::escape(shown)
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{to_html, to_html_with_links, to_html_with_terms};

    fn term(word: &str, meaning: &str) -> TermLink {
        TermLink {
            word: word.to_string(),
            part_of_speech: "noun".to_string(),
            meaning: meaning.to_string(),
            href: Some("/items/ACME-D-0001".to_string()),
        }
    }

    fn dictionary() -> Vec<TermLink> {
        vec![
            term("production", "Code that runs for the users."),
            term("production document", "A document that the team gives out."),
            term("board", "A set of columns."),
        ]
    }

    /// The `data-term` of each linked term, in order.
    fn linked(html: &str) -> Vec<String> {
        html.split("data-term=\"")
            .skip(1)
            .map(|rest| rest[..rest.find('"').expect("closing quote")].to_string())
            .collect()
    }

    #[test]
    fn a_term_links_to_its_dictionary_with_the_meaning_as_title() {
        let html = to_html_with_terms("Move it on the board.", &dictionary());
        assert!(
            html.contains(
                "<a href=\"/items/ACME-D-0001\" class=\"kairos-term\" \
                 title=\"noun: A set of columns.\" data-testid=\"dictionary-term\" \
                 data-term=\"board\">board</a>"
            ),
            "{html}"
        );
    }

    #[test]
    fn the_longest_term_wins() {
        let html = to_html_with_terms("A production document is not production.", &dictionary());
        assert_eq!(linked(&html), ["production document", "production"]);
        assert!(html.contains(">production document</a>"), "{html}");
    }

    #[test]
    fn case_is_ignored_and_the_text_keeps_its_case() {
        let html = to_html_with_terms("Production Document and BOARD.", &dictionary());
        assert_eq!(linked(&html), ["production document", "board"]);
        assert!(html.contains(">Production Document</a>"), "{html}");
        assert!(html.contains(">BOARD</a>"), "{html}");
    }

    #[test]
    fn a_term_matches_at_word_bounds_only() {
        let html = to_html_with_terms("Boards, a keyboard, board_id, board.", &dictionary());
        assert_eq!(linked(&html), ["board"]);
        assert!(html.contains(", <a "), "{html}");
    }

    #[test]
    fn white_space_between_the_words_of_a_term_matches() {
        let html = to_html_with_terms("A production   document.", &dictionary());
        assert_eq!(linked(&html), ["production document"]);
    }

    #[test]
    fn each_occurrence_is_linked() {
        let html = to_html_with_terms("board and board", &dictionary());
        assert_eq!(linked(&html), ["board", "board"]);
    }

    #[test]
    fn links_code_and_headings_are_left_alone() {
        let source = "# The board\n\n\
                      See [the board](https://example.com/) and `board`.\n\n\
                      ```\nboard\n```\n\n    board\n\n\
                      ```mermaid\ngraph TD; board-->x\n```\n";
        let html = to_html_with_terms(source, &dictionary());
        assert!(linked(&html).is_empty(), "{html}");
        assert_eq!(html, to_html(source));
    }

    #[test]
    fn terms_link_in_lists_tables_and_emphasis() {
        let source = "- a *board*\n\n| Name |\n|---|\n| board |\n";
        let html = to_html_with_terms(source, &dictionary());
        assert_eq!(linked(&html), ["board", "board"]);
    }

    #[test]
    fn the_meaning_and_the_word_are_escaped() {
        let terms = [TermLink {
            word: "a&b".to_string(),
            part_of_speech: String::new(),
            meaning: "\"<script>\"".to_string(),
            href: Some("/items/X\"y".to_string()),
        }];
        let html = to_html_with_terms("see a&b now", &terms);
        assert!(
            html.contains("title=\"&quot;&lt;script&gt;&quot;\""),
            "{html}"
        );
        assert!(html.contains("href=\"/items/X&quot;y\""), "{html}");
        assert!(html.contains(">a&amp;b</a>"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
    }

    #[test]
    fn a_term_with_no_safe_href_is_a_span_that_takes_the_focus() {
        let terms = [
            TermLink {
                href: None,
                ..term("board", "A set of columns.")
            },
            TermLink {
                href: Some("javascript:alert(1)".to_string()),
                ..term("lane", "A kind of work.")
            },
        ];
        let html = to_html_with_terms("board lane", &terms);
        assert!(
            html.contains("<span tabindex=\"0\" class=\"kairos-term\""),
            "{html}"
        );
        assert!(!html.contains("javascript"), "{html}");
        assert_eq!(linked(&html), ["board", "lane"]);
    }

    #[test]
    fn codes_and_terms_link_in_one_pass() {
        let html = to_html_with_links(
            "The board of ACME-T-0001 is a board.",
            |_| HashMap::from([("ACME-T-0001".to_string(), "/items/ACME-T-0001".to_string())]),
            &dictionary(),
        );
        assert!(
            html.contains("<a href=\"/items/ACME-T-0001\">ACME-T-0001</a>"),
            "{html}"
        );
        assert_eq!(linked(&html), ["board", "board"]);
    }

    #[test]
    fn no_term_gives_the_plain_render() {
        let source = "A board.";
        assert_eq!(to_html_with_terms(source, &[]), to_html(source));
        assert_eq!(
            to_html_with_terms(source, &[term("  ", "x")]),
            to_html(source)
        );
    }
}
