//! The vocabulary hook, STE-V1 (KAIROS-T-0727).

use kairos_plain::Vocabulary;

const BASE: &[&str] = &["a", "the", "read", "is", "open", "of"];

fn unknown(v: &Vocabulary, text: &str) -> Vec<String> {
    v.check_text(text)
        .into_iter()
        .map(|found| {
            assert_eq!(found.rule, "STE-V1");
            text[found.span].to_owned()
        })
        .collect()
}

#[test]
fn an_unknown_word_is_a_violation_with_its_span() {
    let v = Vocabulary::new(BASE, ["board"]);
    let text = "Read the frobnicated board.";
    let found = v.check_text(text);
    assert_eq!(found.len(), 1);
    assert_eq!(&text[found[0].span.clone()], "frobnicated");
    assert!(
        found[0]
            .message
            .contains("\"frobnicated\" is not in the vocabulary")
    );
}

#[test]
fn a_known_word_ignores_case() {
    let v = Vocabulary::new(BASE, ["Board"]);
    assert!(unknown(&v, "READ THE BOARD.").is_empty());
}

#[test]
fn a_known_multi_word_term_passes() {
    // "short" and "code" are in no list: only the term makes them valid.
    let v = Vocabulary::new(BASE, ["short code"]);
    assert!(unknown(&v, "Read the short code.").is_empty());
    // Each word alone is not the term.
    assert_eq!(unknown(&v, "Read the code."), ["code"]);
    // A term does not cross punctuation.
    assert_eq!(
        unknown(&v, "Read the short. Code is open."),
        ["short", "Code"]
    );
}

#[test]
fn the_longest_term_matches_first() {
    // With "production" first, "document" would be unknown.
    let v = Vocabulary::new(BASE, ["production", "production document"]);
    assert!(unknown(&v, "Open the production document.").is_empty());
    assert!(unknown(&v, "Open the production.").is_empty());

    // Of two multi-word terms, the longer one is read: "release" is valid
    // only as a part of "production document release".
    let v = Vocabulary::new(BASE, ["production document", "production document release"]);
    assert!(unknown(&v, "Open the production document release.").is_empty());
    assert_eq!(unknown(&v, "Open the release."), ["release"]);
}

#[test]
fn a_base_entry_can_be_a_term() {
    let v = Vocabulary::new(["read", "the", "user name"], [] as [&str; 0]);
    assert!(unknown(&v, "Read the user name.").is_empty());
}

#[test]
fn the_code_text_is_cleaned_first() {
    let v = Vocabulary::new(["the", "board", "is", "not", "valid"], [] as [&str; 0]);
    assert!(
        v.check_code_text("The board {board} is not valid.")
            .is_empty()
    );
    let text = "The board is gone.";
    let found = v.check_code_text(text);
    assert_eq!(found.len(), 1);
    assert_eq!(&text[found[0].span.clone()], "gone");
}
