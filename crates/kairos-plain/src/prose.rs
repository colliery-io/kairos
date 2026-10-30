//! The rules for prose: `prose_blocks`, `sentences` and `violations_in_block`
//! of `scripts/ste-check.py`.

use std::sync::LazyLock;

use regex::Regex;

use crate::Violation;
use crate::mapped::{Mapped, Rep};

/// STE-S2: a procedural sentence is 20 words or fewer.
pub(crate) const MAX_SENTENCE_WORDS: usize = 20;
/// STE-S3: a descriptive sentence is 25 words or fewer (texts of the code).
pub(crate) const MAX_DESCRIPTION_WORDS: usize = 25;
/// STE-S4.
const MAX_PARAGRAPH_SENTENCES: usize = 6;

/// STE-G1. A form of "to be" followed by a past participle. Narrow on
/// purpose, as in the Python.
static PASSIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:is|are|was|were|be|been|being|get|gets)\s+(?:not\s+|also\s+|then\s+|only\s+|already\s+)?([a-z]+(?:ed|en))\b",
    )
    .expect("valid regex")
});

/// Past participles that are adjectives or nouns here.
const PASSIVE_ALLOW: &[&str] = &[
    "based",
    "advanced",
    "detailed",
    "limited",
    "related",
    "unauthenticated",
    "authenticated",
    "needed",
    "intended",
    "supported",
    "unchanged",
    "used",
    "shared",
    "fixed",
    "named",
    "expected",
    "required",
    "combined",
    "connected",
    "provided",
    "deleted",
    "archived",
    "enabled",
    "disabled",
    "empty",
];

/// STE-G4: two or more gerunds in one sentence. The Python matches the
/// lower-case sentence; `(?i)` does the same here and keeps the spans.
static GERUND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([a-z]{4,}ing)\b").expect("valid regex"));

const GERUND_ALLOW: &[&str] = &[
    "setting",
    "settings",
    "string",
    "strings",
    "during",
    "nothing",
    "something",
    "anything",
    "everything",
    "thing",
    "things",
    "ring",
    "bring",
    "spring",
    "morning",
    "warning",
    "warnings",
    "meaning",
    "engineering",
    "onboarding",
    "housekeeping",
    "planning",
    "reporting",
    "tooling",
    "logging",
    "tracing",
    "missing",
    "existing",
    "following",
    "remaining",
    "according",
    "including",
    "leading",
    "trailing",
    "working",
    "running",
];

/// STE-V2 / STE-V3: the "do not write" column of KAIROS-D-0009 section 4.1,
/// in the order of the Python dict.
pub(crate) const BANNED_TERMS: &[(&str, &str)] = &[
    ("org admin", "organization admin"),
    ("orgs", "organizations"),
    ("swimlane", "lane"),
    ("kanban", "board"),
    ("epic", "initiative"),
    ("ticket", "task"),
    ("squad", "team"),
    ("bot", "service account"),
    ("machine user", "service account"),
    ("idp", "issuer"),
    ("superuser", "deployment admin"),
    ("sysadmin", "deployment admin"),
    ("installation", "deployment"),
];

static BANNED: LazyLock<Vec<(Regex, &'static str, &'static str)>> = LazyLock::new(|| {
    BANNED_TERMS
        .iter()
        .map(|(banned, instead)| {
            let re =
                Regex::new(&format!(r"(?i)\b{}\b", regex::escape(banned))).expect("valid regex");
            (re, *banned, *instead)
        })
        .collect()
});

static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9'’/._-]+").expect("valid regex"));
static FRONT_MATTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\A---\n.*?\n---\n").expect("valid regex"));
static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```.*?```").expect("valid regex"));
static COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").expect("valid regex"));
static LIST_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[-*+]|\d+\.)\s+").expect("valid regex"));
pub(crate) static INLINE_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`]*`").expect("valid regex"));
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("valid regex"));
static EMPHASIS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*\*?([^*]*)\*\*?").expect("valid regex"));

/// The sentence limit of a block: fixed (the docs), or by the first word of
/// the sentence (the texts of the code).
#[derive(Clone, Copy)]
pub(crate) enum Limit {
    Fixed,
    ByFirstWord,
}

/// Check markdown prose with the rules of the docs: STE-S2, STE-S4,
/// STE-G1, STE-G4 and STE-V3.
///
/// Front matter, code fences, HTML comments, headings, tables, quotes and
/// indented code are not prose and are not checked. Each list item is its
/// own paragraph.
pub fn check_text(text: &str) -> Vec<Violation> {
    prose_blocks(text)
        .iter()
        .flat_map(|block| violations_in_block(block, Limit::Fixed))
        .collect()
}

/// The paragraphs of prose, cleaned as `prose_blocks` of the Python does.
pub(crate) fn prose_blocks(text: &str) -> Vec<Mapped> {
    let text = Mapped::new(text)
        .replace_all(&FRONT_MATTER, Rep::Delete)
        .replace_all(&FENCE, Rep::Delete)
        .replace_all(&COMMENT, Rep::Delete);
    let clean = |chunk: Mapped| {
        chunk
            .replace_all(&INLINE_CODE, Rep::Literal("CODE"))
            .replace_all(&LINK, Rep::Group(1))
            .replace_all(&EMPHASIS, Rep::Group(1))
    };

    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let mut paragraph: Vec<Mapped> = Vec::new();
        for line in block.split("\n") {
            let stripped = line.trim();
            let s = stripped.text.as_str();
            if s.is_empty()
                || ["#", "|", ">", "---", "    "]
                    .iter()
                    .any(|p| s.starts_with(p))
            {
                continue;
            }
            match LIST_MARKER.find(s) {
                Some(marker) => {
                    if !paragraph.is_empty() {
                        out.push(clean(Mapped::join(&paragraph, " ")));
                        paragraph.clear();
                    }
                    out.push(clean(stripped.slice(marker.end()..stripped.len())));
                }
                None => paragraph.push(stripped),
            }
        }
        if !paragraph.is_empty() {
            out.push(clean(Mapped::join(&paragraph, " ")));
        }
    }
    out.retain(|b| !b.text.is_empty());
    out
}

/// The sentences of a block, as `sentences` of the Python does: a split at
/// whitespace after `.`, `!` or `?` and before a capital or `(`.
pub(crate) fn sentences(block: &Mapped) -> Vec<Mapped> {
    let guarded = block.replace_str("e.g.", "eg").replace_str("i.e.", "ie");
    let chars: Vec<(usize, char)> = guarded.text.char_indices().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut k = 0;
    while k < chars.len() {
        let (at, c) = chars[k];
        if c.is_whitespace() && k > 0 && matches!(chars[k - 1].1, '.' | '!' | '?') {
            let mut m = k;
            while m < chars.len() && chars[m].1.is_whitespace() {
                m += 1;
            }
            if m < chars.len() && (chars[m].1.is_ascii_uppercase() || chars[m].1 == '(') {
                parts.push(guarded.slice(start..at));
                start = chars[m].0;
            }
            k = m;
            continue;
        }
        k += 1;
    }
    parts.push(guarded.slice(start..guarded.len()));
    parts
        .into_iter()
        .map(|p| p.trim())
        .filter(|p| !p.text.is_empty())
        .collect()
}

/// An instruction starts with a verb in the imperative (texts of the code).
const IMPERATIVE: &[&str] = &[
    "add", "ask", "change", "close", "delete", "do", "get", "give", "make", "move", "open", "put",
    "read", "remove", "restore", "run", "send", "set", "sign", "start", "stop", "try", "use",
    "wait", "write",
];

fn sentence_limit(sentence: &str, limit: Limit) -> usize {
    match limit {
        Limit::Fixed => MAX_SENTENCE_WORDS,
        Limit::ByFirstWord => {
            let first: String = sentence
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            if IMPERATIVE.contains(&first.to_ascii_lowercase().as_str()) {
                MAX_SENTENCE_WORDS
            } else {
                MAX_DESCRIPTION_WORDS
            }
        }
    }
}

/// Each violation in one block, as `violations_in_block` of the Python.
pub(crate) fn violations_in_block(block: &Mapped, limit: Limit) -> Vec<Violation> {
    let mut found = Vec::new();
    let sents = sentences(block);
    if sents.len() > MAX_PARAGRAPH_SENTENCES {
        found.push(Violation::new(
            "STE-S4",
            block.whole_span(),
            format!(
                "Paragraph of {} sentences. Write {MAX_PARAGRAPH_SENTENCES} sentences or fewer in a paragraph.",
                sents.len()
            ),
        ));
    }
    for sentence in &sents {
        let text = sentence.text.as_str();
        let words = WORD.find_iter(text).count();
        let max = sentence_limit(text, limit);
        if words > max {
            let rule = if max == MAX_SENTENCE_WORDS {
                "STE-S2"
            } else {
                "STE-S3"
            };
            found.push(Violation::new(
                rule,
                sentence.whole_span(),
                format!("{words} words. Write {max} words or fewer in a sentence."),
            ));
        }
        for caps in PASSIVE.captures_iter(text) {
            let participle = caps.get(1).expect("group 1 is set").as_str();
            if !PASSIVE_ALLOW.contains(&participle.to_lowercase().as_str()) {
                let m = caps.get(0).expect("group 0 is set");
                found.push(Violation::new(
                    "STE-G1",
                    sentence.span(m.range()),
                    format!(
                        "Passive: {}. Use the active voice, and name who does the action.",
                        m.as_str()
                    ),
                ));
            }
        }
        let gerunds: Vec<String> = GERUND
            .find_iter(text)
            .map(|m| m.as_str().to_lowercase())
            .filter(|g| !GERUND_ALLOW.contains(&g.as_str()))
            .collect();
        if gerunds.len() >= 2 {
            found.push(Violation::new(
                "STE-G4",
                sentence.whole_span(),
                format!(
                    "Gerunds: {}. Write one action in one sentence.",
                    gerunds[..gerunds.len().min(3)].join(", ")
                ),
            ));
        }
        for (re, banned, instead) in BANNED.iter() {
            if let Some(m) = re.find(text) {
                found.push(Violation::new(
                    "STE-V3",
                    sentence.span(m.range()),
                    format!("\"{banned}\": write \"{instead}\"."),
                ));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_points_at_the_input() {
        let input = "# Title\n\nThe file is created by the server.\n";
        let found = check_text(input);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "STE-G1");
        assert_eq!(&input[found[0].span.clone()], "is created");
    }

    #[test]
    fn a_banned_term_names_the_word_to_write() {
        let input = "Open a ticket on the board.";
        let found = check_text(input);
        assert_eq!(found[0].rule, "STE-V3");
        assert_eq!(found[0].message, "\"ticket\": write \"task\".");
        assert_eq!(&input[found[0].span.clone()], "ticket");
    }

    #[test]
    fn code_and_fences_are_not_prose() {
        let input = "```\nThe file is created by the server.\n```\n\nRun `it is created`.";
        assert!(check_text(input).is_empty());
    }
}
