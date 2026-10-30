//! The rules for a text of the code: `code_prose` and
//! `violations_in_code_text` of `scripts/ste-check.py` (COLLIERY-T-0258).

use std::sync::LazyLock;

use regex::Regex;

use crate::Violation;
use crate::mapped::{Mapped, Rep};
use crate::prose::{INLINE_CODE, Limit, violations_in_block};

static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[^{}]*\}").expect("valid regex"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""[^"]*""#).expect("valid regex"));
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));
static CONTRACTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:\w+n't|\w+'(?:re|ll|ve|d)|(?:it|that|there|what|let|here|who)'s)\b")
        .expect("valid regex")
});
static DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[—–]|\s-{1,2}\s").expect("valid regex"));
static PLURAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w\(s\)").expect("valid regex"));

/// The text as prose: a placeholder or a quoted name is one technical noun,
/// `CODE`.
pub(crate) fn code_prose(text: &str) -> Mapped {
    Mapped::new(text)
        .replace_str("{{", "(")
        .replace_str("}}", ")")
        .replace_all(&PLACEHOLDER, Rep::Literal("CODE"))
        .replace_all(&INLINE_CODE, Rep::Literal("CODE"))
        .replace_all(&QUOTED, Rep::Literal("CODE"))
        .replace_all(&SPACE, Rep::Literal(" "))
        .trim()
}

/// Check the text of an error (a `#[error]` attribute, the text of an API
/// error): STE-S2 for an instruction (it starts with a verb in the
/// imperative), STE-S3 for a description, STE-G1, STE-G4, STE-V3, and the
/// rules of a text of the code, STE-C1 to STE-C7.
///
/// A `{placeholder}`, a `` `name` `` and a `"quoted name"` count as one
/// technical noun.
pub fn check_code_text(text: &str) -> Vec<Violation> {
    let prose = code_prose(text);
    let mut found = violations_in_block(&prose, Limit::ByFirstWord);
    let p = prose.text.as_str();

    let first_word = p.split(' ').next().unwrap_or("");
    let technical = first_word.starts_with("CODE") || first_word.contains('_');
    let first = p.chars().next();
    if !technical && !first.is_some_and(char::is_uppercase) {
        let end = first.map_or(0, char::len_utf8);
        found.push(Violation::new(
            "STE-C1",
            prose.span(0..end),
            "The text does not start with a capital. Start the text with a capital letter."
                .to_owned(),
        ));
    }
    // A placeholder after the end of a sentence is a sentence that the code made.
    if ![".", ".)", ". CODE", ".CODE"]
        .iter()
        .any(|end| p.ends_with(end))
    {
        let last = p.char_indices().last().map_or(0, |(at, _)| at);
        found.push(Violation::new(
            "STE-C2",
            prose.span(last..p.len()),
            "The text does not end with a period. End the text with a period.".to_owned(),
        ));
    }
    for m in CONTRACTION.find_iter(p) {
        found.push(Violation::new(
            "STE-C3",
            prose.span(m.range()),
            format!("Contraction: {}. Write the full words.", m.as_str()),
        ));
    }
    if let Some(at) = p.find(';') {
        found.push(Violation::new(
            "STE-C4",
            prose.span(at..at + 1),
            "Semicolon. Write two sentences.".to_owned(),
        ));
    }
    let dash = DASH
        .find(p)
        .map(|m| m.range())
        .or_else(|| p.find("->").map(|at| at..at + 2));
    if let Some(range) = dash {
        found.push(Violation::new(
            "STE-C5",
            prose.span(range),
            "Dash or arrow as punctuation. Write two sentences, or use a comma.".to_owned(),
        ));
    }
    for range in latin(p) {
        found.push(Violation::new(
            "STE-C6",
            prose.span(range.clone()),
            format!(
                "Latin abbreviation: {}. Write the English words in full.",
                &p[range]
            ),
        ));
    }
    if let Some(m) = PLURAL.find(p) {
        found.push(Violation::new(
            "STE-C7",
            prose.span(m.range()),
            "Plural in parentheses: write the plural.".to_owned(),
        ));
    }
    found
}

/// The Latin abbreviations of a text: the matches of the Python
/// `\b(?:e\.g\.|i\.e\.|etc\.?|viz\.|vs\.?|via)(?=\W|$)`, case-insensitive.
/// The `regex` crate has no look-ahead, so this tries each form in the order
/// of the Python alternation, at each word boundary, left to right.
fn latin(text: &str) -> Vec<std::ops::Range<usize>> {
    const FORMS: &[&str] = &["e.g.", "i.e.", "etc.", "etc", "viz.", "vs.", "vs", "via"];
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    let mut iter = text.char_indices().peekable();
    while let Some((at, c)) = iter.next() {
        let boundary = prev.is_some_and(is_word) != is_word(c);
        let mut hit = None;
        if boundary {
            for form in FORMS {
                let end = at + form.len();
                let matches = text
                    .get(at..end)
                    .is_some_and(|s| s.eq_ignore_ascii_case(form));
                // `matches` first: only then is `end` a char boundary.
                if matches && text[end..].chars().next().is_none_or(|n| !is_word(n)) {
                    hit = Some(at..end);
                    break;
                }
            }
        }
        match hit {
            Some(range) => {
                let end = range.end;
                out.push(range);
                prev = text[..end].chars().next_back();
                while iter.peek().is_some_and(|(i, _)| *i < end) {
                    iter.next();
                }
            }
            None => prev = Some(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Vec<&'static str> {
        check_code_text(text).into_iter().map(|v| v.rule).collect()
    }

    #[test]
    fn a_clean_text_has_no_violation() {
        assert!(rules("The board {board} does not exist.").is_empty());
    }

    #[test]
    fn each_code_rule_fires() {
        assert_eq!(rules("the board is gone."), ["STE-C1"]);
        assert_eq!(rules("The board is gone"), ["STE-C2"]);
        assert_eq!(rules("The board doesn't exist."), ["STE-C3"]);
        assert_eq!(rules("The board is gone; ask again."), ["STE-C4"]);
        assert_eq!(rules("The board is gone — ask again."), ["STE-C5"]);
        assert_eq!(rules("Send a field, e.g. the name."), ["STE-C6"]);
        assert_eq!(rules("Send the name(s)."), ["STE-C7"]);
    }

    #[test]
    fn latin_tries_the_forms_in_order() {
        assert_eq!(latin("etc.x"), vec![0..3]);
        assert_eq!(latin("see etc. now"), vec![4..8]);
        assert!(latin("vsx viaduct").is_empty());
    }
}
