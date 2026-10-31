//! The EARS checker (KAIROS-T-0732, COLLIERY-I-0664).
//!
//! EARS (Easy Approach to Requirements Syntax, Mavin et al.) puts each
//! requirement in one of a few fixed frames. The words inside the frame are
//! free: this checker reads the frame only (the keywords, the commas, "then"
//! and "shall"), not the vocabulary. The plain-language check is a different
//! section type.
//!
//! - Ubiquitous: "The <system> shall <response>."
//! - Event-driven: "When <trigger>, the <system> shall <response>."
//! - State-driven: "While <state>, the <system> shall <response>."
//! - Unwanted behaviour: "If <condition>, then the <system> shall <response>."
//! - Optional feature: "Where <feature>, the <system> shall <response>."
//! - Complex: more than one of these clauses before the system.
//!
//! Only list items are requirements (Dylan, 2026-10-30, note n-7 on
//! KAIROS-T-0732): each bullet (`-`, `*`, `+`) and each numbered item
//! (`1.`) is one requirement, with the lines that continue it. A nested item
//! is a requirement of its own. Paragraphs are free text and are not
//! checked; code fences, headings, tables and quotes neither. Inline code is
//! one word, so a "shall" or a comma in code does not count.
//!
//! Each failing requirement gives one [`Violation`]: the first rule that it
//! breaks, the span of the requirement, and a message that names the fix and
//! the closest frame. A text with no list item gives one violation, EARS-7:
//! the section has no requirement.

use std::sync::LazyLock;

use regex::Regex;

use crate::Violation;
use crate::mapped::Mapped;
use crate::prose::list_items;

static SHALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bshall\b").expect("valid regex"));
static KEYWORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(when|while|where|if)\b").expect("valid regex"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z]+").expect("valid regex"));

/// The frames of EARS. The checker names the closest one in each message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Ubiquitous,
    Event,
    State,
    Unwanted,
    Option,
    Complex,
}

impl Frame {
    /// The name of the frame, as the messages give it.
    pub fn name(self) -> &'static str {
        match self {
            Frame::Ubiquitous => "ubiquitous",
            Frame::Event => "event-driven",
            Frame::State => "state-driven",
            Frame::Unwanted => "unwanted behaviour",
            Frame::Option => "optional feature",
            Frame::Complex => "complex",
        }
    }

    /// The template of the frame.
    pub fn template(self) -> &'static str {
        match self {
            Frame::Ubiquitous => "The <system> shall <response>.",
            Frame::Event => "When <trigger>, the <system> shall <response>.",
            Frame::State => "While <state>, the <system> shall <response>.",
            Frame::Unwanted => "If <condition>, then the <system> shall <response>.",
            Frame::Option => "Where <feature>, the <system> shall <response>.",
            Frame::Complex => "While <state>, when <trigger>, the <system> shall <response>.",
        }
    }

    fn of_keyword(keyword: &str) -> Option<Frame> {
        match keyword {
            "when" => Some(Frame::Event),
            "while" => Some(Frame::State),
            "if" => Some(Frame::Unwanted),
            "where" => Some(Frame::Option),
            _ => None,
        }
    }

    /// The frame of the keywords of the clauses before the system.
    fn of_keywords(keywords: &[String]) -> Frame {
        let mut frames: Vec<Frame> = keywords
            .iter()
            .filter_map(|k| Frame::of_keyword(k))
            .collect();
        frames.dedup();
        match frames.as_slice() {
            [] => Frame::Ubiquitous,
            [one] => *one,
            _ => Frame::Complex,
        }
    }
}

/// Check the text of an `ears` section: each list item is a requirement and
/// must fit one frame of EARS. A text with no list item gives EARS-7.
/// Gives the violations in the order of the text.
pub fn check_ears(text: &str) -> Vec<Violation> {
    let items = list_items(text);
    if items.is_empty() {
        let start = text.len() - text.trim_start().len();
        let end = text.trim_end().len().max(start);
        return vec![Violation::new(
            "EARS-7",
            start..end,
            "The section has no requirement: it has no list item. Write each requirement \
             as a list item (\"- \" or \"1. \"). Paragraphs are not checked."
                .to_string(),
        )];
    }
    items.iter().filter_map(check_requirement).collect()
}

/// The lower-case first word of `text`, if any.
fn first_word(text: &str) -> Option<String> {
    WORD.find(text).map(|m| m.as_str().to_ascii_lowercase())
}

fn is_keyword(word: &str) -> bool {
    Frame::of_keyword(word).is_some()
}

/// The keywords of the comma parts of `head` that start with one.
fn clause_keywords(parts: &[&str]) -> Vec<String> {
    parts
        .iter()
        .filter_map(|part| first_word(part))
        .filter(|word| is_keyword(word))
        .collect()
}

/// True when `word` is a word of `text` (any case).
fn has_word(text: &str, word: &str) -> bool {
    WORD.find_iter(text)
        .any(|m| m.as_str().eq_ignore_ascii_case(word))
}

/// The violation of one requirement, or `None` when it fits a frame.
fn check_requirement(requirement: &Mapped) -> Option<Violation> {
    let text = requirement.text.as_str();
    let fail = |rule: &'static str, problem: String, fix: &str, frame: Frame| {
        Some(Violation::new(
            rule,
            requirement.whole_span(),
            format!(
                "{problem} {fix} Closest frame: {}, \"{}\"",
                frame.name(),
                frame.template()
            ),
        ))
    };

    let shalls: Vec<_> = SHALL.find_iter(text).collect();
    let Some(shall) = shalls.first() else {
        return no_shall(text, fail);
    };
    let head = &text[..shall.start()];
    let tail = &text[shall.end()..];
    let parts: Vec<&str> = head.split(',').collect();
    let leading = first_word(head).filter(|w| is_keyword(w));
    let keywords = if leading.is_some() {
        clause_keywords(&parts[..parts.len() - 1])
    } else {
        Vec::new()
    };
    let frame = Frame::of_keywords(&keywords);

    if shalls.len() > 1 {
        return fail(
            "EARS-6",
            format!("The requirement has {} \"shall\".", shalls.len()),
            "Write one requirement in each list item.",
            frame,
        );
    }
    if !tail.chars().any(|c| c.is_alphanumeric()) {
        return fail(
            "EARS-4",
            "The requirement has no response after \"shall\".".to_string(),
            "Write what the system does after \"shall\".",
            frame,
        );
    }

    // The part after the last comma is the system; the parts before it are
    // the clauses.
    let subject = parts.last().copied().unwrap_or_default().trim();
    match &leading {
        Some(keyword) if parts.len() == 1 => {
            return fail(
                "EARS-2",
                format!("The \"{}\" clause has no comma after it.", capital(keyword)),
                "Put a comma between the clause and the system.",
                frame,
            );
        }
        None if parts.len() > 1 && !is_list(subject) => {
            let clause = parts[0].trim();
            return fail(
                "EARS-2",
                format!("The clause \"{clause}\" does not start with While, When, Where or If."),
                "Start the clause with one of these words, or remove the comma.",
                closest_for_clause(clause),
            );
        }
        _ => {}
    }

    let (then, system) = match WORD.find(subject) {
        Some(word) if word.as_str().eq_ignore_ascii_case("then") => {
            (true, subject[word.end()..].trim())
        }
        _ => (false, subject),
    };
    if first_word(system).is_none_or(|word| is_keyword(&word)) {
        return fail(
            "EARS-3",
            "The requirement names no system before \"shall\".".to_string(),
            "Write the system before \"shall\", as \"the server shall\".",
            frame,
        );
    }
    let has_if = keywords.iter().any(|k| k == "if");
    if has_if && !then {
        return fail(
            "EARS-5",
            "The \"If\" clause has no \"then\".".to_string(),
            "Write \"then\" before the system.",
            frame,
        );
    }
    if then && !has_if {
        return fail(
            "EARS-5",
            "The \"then\" has no \"If\" clause.".to_string(),
            "Start the clause with \"If\", or remove \"then\".",
            frame,
        );
    }
    None
}

/// EARS-1: the requirement has no "shall". The closest frame comes from a
/// keyword at the start, or from a keyword in the middle (then the fix also
/// says to put that clause first).
fn no_shall(
    text: &str,
    fail: impl Fn(&'static str, String, &str, Frame) -> Option<Violation>,
) -> Option<Violation> {
    let problem = "The requirement has no \"shall\".".to_string();
    if first_word(text).is_some_and(|w| is_keyword(&w)) {
        let parts: Vec<&str> = text.split(',').collect();
        let keywords = clause_keywords(&parts[..parts.len().max(2) - 1]);
        let frame = Frame::of_keywords(&keywords);
        return fail(
            "EARS-1",
            problem,
            "Write \"shall\" between the system and the response.",
            frame,
        );
    }
    if let Some(keyword) = KEYWORD.find(text) {
        let keyword = keyword.as_str().to_ascii_lowercase();
        let frame = Frame::of_keyword(&keyword).unwrap_or(Frame::Ubiquitous);
        let fix = format!(
            "Put the \"{}\" clause first, then a comma, then the system, \"shall\" and the response.",
            keyword
        );
        return fail("EARS-1", problem, &fix, frame);
    }
    fail(
        "EARS-1",
        problem,
        "Write \"shall\" between the system and the response.",
        Frame::Ubiquitous,
    )
}

/// True when the part after the last comma ends a list of systems
/// ("The server, the worker and the CLI shall ..."): it has "and" or "or".
fn is_list(subject: &str) -> bool {
    has_word(subject, "and") || has_word(subject, "or")
}

/// The closest frame of a clause that starts with a word that is not a
/// keyword: "On each fault, ..." is a trigger.
fn closest_for_clause(clause: &str) -> Frame {
    match first_word(clause).as_deref() {
        Some("during" | "until" | "in") => Frame::State,
        Some("for" | "with") => Frame::Option,
        Some("unless" | "otherwise") => Frame::Unwanted,
        _ => Frame::Event,
    }
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Vec<&'static str> {
        check_ears(text).iter().map(|v| v.rule).collect()
    }

    /// The one violation of a text, as a list item.
    fn one(item: &str) -> Violation {
        let found = check_ears(&format!("- {item}"));
        assert_eq!(found.len(), 1, "{found:?}");
        found.into_iter().next().unwrap()
    }

    fn item_rules(item: &str) -> Vec<&'static str> {
        rules(&format!("- {item}"))
    }

    #[test]
    fn each_frame_passes() {
        for text in [
            "The server shall refuse the write.",
            "When a write fails, the server shall name the field.",
            "While the worker runs, the server shall queue the job.",
            "If the disk is full, then the server shall stop the import.",
            "Where the team has a dictionary, the checker shall read its terms.",
            "While the item is in Active, when the claim ends, the server shall notify the owner.",
            "While the sidecar is down, if a write comes, then the server shall refuse it.",
            "Where search is on, while the index builds, when a query comes, the server shall wait.",
        ] {
            assert_eq!(item_rules(text), Vec::<&str>::new(), "{text}");
        }
    }

    #[test]
    fn words_inside_the_frame_are_free() {
        // Commas inside a clause, code, a list of systems, lower case.
        let text = "- When `create_item` gets a key, a body, and a board, the server shall utilize the cache.\n\
                    - The server, the worker and the CLI shall read `config.toml`.\n\
                    - when a run ends, kairos shall write the log";
        assert_eq!(check_ears(text), vec![]);
    }

    #[test]
    fn code_does_not_count() {
        // The "shall" in code is not the keyword.
        let v = one("The server sends `shall` to the client.");
        assert_eq!(v.rule, "EARS-1");
    }

    #[test]
    fn a_sentence_with_no_shall_names_the_closest_frame() {
        let v = one("The server refuses the write.");
        assert_eq!(v.rule, "EARS-1");
        assert_eq!(
            v.message,
            "The requirement has no \"shall\". Write \"shall\" between the system and the \
             response. Closest frame: ubiquitous, \"The <system> shall <response>.\""
        );
        let v = one("When the target can be reached, the refusal gives the shortest path.");
        assert!(
            v.message.contains("Closest frame: event-driven"),
            "{}",
            v.message
        );
    }

    #[test]
    fn a_near_miss_says_to_put_the_clause_first() {
        let item = "A move names `reopen_item` when the item is a task in a done column.";
        let text = format!("- {item}");
        let found = check_ears(&text);
        assert_eq!(found.len(), 1, "{found:?}");
        let v = &found[0];
        assert_eq!(v.rule, "EARS-1");
        assert_eq!(&text[v.span.clone()], item);
        assert_eq!(
            v.message,
            "The requirement has no \"shall\". Put the \"when\" clause first, then a comma, then \
             the system, \"shall\" and the response. Closest frame: event-driven, \"When \
             <trigger>, the <system> shall <response>.\""
        );
    }

    #[test]
    fn a_clause_that_is_not_a_frame_is_named() {
        let v = one("On each of these faults, the proxy shall serve no tools.");
        assert_eq!(v.rule, "EARS-2");
        assert!(
            v.message.starts_with(
                "The clause \"On each of these faults\" does not start with While, When, Where or If."
            ),
            "{}",
            v.message
        );
        assert!(v.message.contains("event-driven"), "{}", v.message);
        let v = one("When the job fails the server shall retry.");
        assert_eq!(v.rule, "EARS-2");
        assert!(
            v.message.starts_with("The \"When\" clause has no comma"),
            "{}",
            v.message
        );
    }

    #[test]
    fn no_system_and_no_response_are_named() {
        assert_eq!(
            item_rules("When the job fails, shall retry."),
            vec!["EARS-3"]
        );
        assert_eq!(item_rules("The server shall."), vec!["EARS-4"]);
    }

    #[test]
    fn if_needs_then_and_then_needs_if() {
        let v = one("If the disk is full, the server shall stop.");
        assert_eq!(v.rule, "EARS-5");
        assert!(v.message.contains("unwanted behaviour"), "{}", v.message);
        assert_eq!(
            item_rules("When the disk is full, then the server shall stop."),
            vec!["EARS-5"]
        );
        assert_eq!(item_rules("Then the server shall stop."), vec!["EARS-5"]);
    }

    #[test]
    fn two_shalls_are_two_requirements() {
        assert_eq!(
            item_rules("The server shall save the item and shall send a mail."),
            vec!["EARS-6"]
        );
    }

    #[test]
    fn only_list_items_are_requirements() {
        // Paragraphs are free text: a paragraph with no "shall" passes. Each
        // bullet and each numbered item is one requirement.
        let text = "The server saves each item. This is context, not a requirement.\n\n\
                    - The worker shall run.\n\
                    - The worker runs\n\
                    * When it stops, the worker shall log.\n\
                    + The worker logs.\n\
                    1. Tests cover each frame.\n\
                    2. The CLI shall print the code.\n\n\
                    A closing paragraph with no keyword at all.\n";
        let found = check_ears(text);
        let texts: Vec<&str> = found.iter().map(|v| &text[v.span.clone()]).collect();
        assert_eq!(
            texts,
            vec![
                "The worker runs",
                "The worker logs.",
                "Tests cover each frame."
            ]
        );
        assert!(found.iter().all(|v| v.rule == "EARS-1"));
    }

    #[test]
    fn a_multi_line_item_is_one_requirement() {
        let text = "- When a write fails,\n  the server shall name the field.\n\
                    - The server\n  refuses the write.\n";
        let found = check_ears(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rule, "EARS-1");
        assert_eq!(
            &text[found[0].span.clone()],
            "The server\n  refuses the write."
        );
    }

    #[test]
    fn a_nested_item_is_a_requirement_of_its_own() {
        let text = "- The server shall check each item:\n  \
                    - The checker shall read the frame.\n  \
                    - The checker reads the words.\n";
        let found = check_ears(text);
        let texts: Vec<&str> = found.iter().map(|v| &text[v.span.clone()]).collect();
        assert_eq!(texts, vec!["The checker reads the words."]);
    }

    #[test]
    fn headings_fences_and_tables_are_not_requirements() {
        let text = "### Edge cases\n```\n- not ears\n```\n| a | b |\n\n- The server shall log.\n";
        assert_eq!(check_ears(text), vec![]);
    }

    #[test]
    fn a_section_with_no_list_item_has_no_requirement() {
        let text = "The server saves each item.\n\n```\n- not an item\n```\n";
        let found = check_ears(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rule, "EARS-7");
        assert_eq!(found[0].span, 0..text.trim_end().len());
        assert!(
            found[0]
                .message
                .starts_with("The section has no requirement: it has no list item."),
            "{}",
            found[0].message
        );
        assert_eq!(rules(""), vec!["EARS-7"]);
    }

    #[test]
    fn a_complex_requirement_names_the_complex_frame() {
        let v = one("While the item is open, when the owner leaves, the server notifies.");
        assert!(
            v.message.contains("Closest frame: complex"),
            "{}",
            v.message
        );
    }
}
