//! The vendored word list and `Vocabulary::standard` (KAIROS-T-0728).

use std::collections::HashSet;

use kairos_plain::{Vocabulary, approved_words, not_approved_words};

const NONE: [&str; 0] = [];

fn unknown(v: &Vocabulary, text: &str) -> Vec<String> {
    v.check_text(text)
        .into_iter()
        .map(|found| text[found.span].to_owned())
        .collect()
}

#[test]
fn the_lists_have_the_extracted_counts() {
    // ASD-STE100 Issue 9 Part 2: 879 approved entries plus 495 rows for
    // their listed verb and adjective forms, and 1319 entries that are not
    // approved. The introduction of the issue says 875 and 1274; the
    // extraction found more (note n-4 on KAIROS-T-0728).
    assert_eq!(approved_words().len(), 1374);
    let forms = approved_words()
        .iter()
        .filter(|w| w.meaning.starts_with("form of "))
        .count();
    assert_eq!(forms, 495);
    assert_eq!(not_approved_words().len(), 1319);
}

#[test]
fn each_row_has_three_cells_that_are_not_empty() {
    for w in approved_words() {
        assert!(
            !w.word.is_empty() && !w.part_of_speech.is_empty() && !w.meaning.is_empty(),
            "{w:?}"
        );
        assert_eq!(w.word, w.word.trim(), "{w:?}");
        assert_eq!(w.word, w.word.to_lowercase(), "{w:?}");
    }
    for w in not_approved_words() {
        assert!(
            !w.word.is_empty() && !w.part_of_speech.is_empty() && !w.use_instead.is_empty(),
            "{w:?}"
        );
        assert_eq!(w.word, w.word.to_lowercase(), "{w:?}");
    }
}

#[test]
fn the_part_of_speech_is_a_known_one() {
    let known = [
        "n", "v", "adj", "adv", "prep", "pron", "art", "conj", "phrase", "prefix",
    ];
    for w in approved_words() {
        assert!(known.contains(&w.part_of_speech), "{w:?}");
    }
    for w in not_approved_words() {
        assert!(known.contains(&w.part_of_speech), "{w:?}");
    }
}

#[test]
fn no_word_is_listed_twice_with_the_same_part_of_speech() {
    let mut seen = HashSet::new();
    for w in approved_words() {
        assert!(seen.insert((w.word, w.part_of_speech)), "approved: {w:?}");
    }
    let mut seen = HashSet::new();
    for w in not_approved_words() {
        assert!(
            seen.insert((w.word, w.part_of_speech)),
            "not approved: {w:?}"
        );
    }
}

#[test]
fn listed_forms_are_approved_words() {
    let v = Vocabulary::standard(NONE);
    // Verb forms and comparatives that the list gives.
    assert!(unknown(&v, "It was pushed. They were slower. It is better.").is_empty());
    // A verb form that the list does not give is not made.
    assert_eq!(unknown(&v, "Push it. It pushing."), ["pushing"]);
}

#[test]
fn every_example_uses_only_approved_words() {
    let v = Vocabulary::standard(NONE);
    let mut count = 0;
    for w in approved_words() {
        let Some(example) = w.example else { continue };
        count += 1;
        assert_eq!(unknown(&v, example), Vec::<String>::new(), "{w:?}");
    }
    assert_eq!(count, 41);
}

#[test]
fn a_text_of_approved_words_passes() {
    let v = Vocabulary::standard(NONE);
    let text = "Make sure that the unit is in the correct position. \
                Then do the test again and record the values. \
                If the result is not correct, stop the procedure.";
    assert!(v.check_text(text).is_empty(), "{:?}", v.check_text(text));
}

#[test]
fn regular_plurals_and_possessives_of_listed_nouns_pass() {
    let v = Vocabulary::new(["the", "of", "file", "team", "box", "entry", "read"], NONE);
    assert!(unknown(&v, "Read the files of the team's boxes.").is_empty());
    assert!(unknown(&v, "Read the teams' entries.").is_empty());
    assert!(unknown(&v, "Read the team’s files.").is_empty());
    // No other form is made.
    assert_eq!(
        unknown(&v, "Read the filed boxs entrys."),
        ["filed", "boxs", "entrys"]
    );

    // In the standard list only a noun takes a plural.
    let v = Vocabulary::standard(["team"]);
    assert!(unknown(&v, "The teams record the unit's values.").is_empty());
    // "usual" is an adjective, not a noun.
    assert_eq!(unknown(&v, "The usuals."), ["usuals"]);
}

#[test]
fn a_word_that_is_not_approved_names_the_word_to_write() {
    let v = Vocabulary::standard(NONE);
    let text = "Abandon the test.";
    let found = v.check_text(text);
    assert_eq!(found.len(), 1);
    assert_eq!(&text[found[0].span.clone()], "Abandon");
    assert_eq!(
        found[0].message,
        "\"Abandon\" is not in the vocabulary. Write \"go\" or \"stop\", not \"Abandon\"."
    );

    let found = v.check_text("Examine the abnormalities.");
    assert_eq!(found.len(), 1);
    assert!(
        found[0]
            .message
            .contains("Write \"defect\", not \"abnormalities\"."),
        "{}",
        found[0].message
    );

    // A word in no list keeps the general message.
    let found = v.check_text("Frobnicate the unit.");
    assert!(
        found[0]
            .message
            .ends_with("Use a word of the word list or a term of the dictionary."),
        "{}",
        found[0].message
    );
}

#[test]
fn a_dictionary_term_passes_in_the_standard_vocabulary() {
    let v = Vocabulary::standard(["production document"]);
    assert!(unknown(&v, "Do the production document.").is_empty());
}
