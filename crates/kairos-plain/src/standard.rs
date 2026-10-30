//! The vendored word list (KAIROS-T-0728, COLLIERY-I-0664 decision 5).
//!
//! Two tab-separated files in `data/`, derived from ASD-STE100 Issue 9
//! (January 2025), Part 2, for non-commercial use:
//!
//! - `approved-words.tsv`: word, part of speech, meaning, and an optional
//!   example. A listed verb or adjective form (`absorbs`, `slower`) is a row
//!   of its own with the meaning "form of <word>".
//! - `not-approved-words.tsv`: word, part of speech, and the approved words to
//!   use instead (`; ` between them, `(none)` when no word replaces it).
//!
//! Lines that start with `#` are comments; the first other line is the header.

use std::sync::LazyLock;

const APPROVED: &str = include_str!("../data/approved-words.tsv");
const NOT_APPROVED: &str = include_str!("../data/not-approved-words.tsv");

/// The "use instead" cell of a word that has no replacement word.
const NO_REPLACEMENT: &str = "(none)";

/// One row of the approved word list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedWord {
    /// The word in lower case. A word of more than one part is a term
    /// ("make sure").
    pub word: &'static str,
    /// `n`, `v`, `adj`, `adv`, `prep`, `pron`, `art`, `conj` or `phrase`.
    pub part_of_speech: &'static str,
    /// The approved meaning, or "form of <word>".
    pub meaning: &'static str,
    /// A sentence that uses the word in its approved meaning.
    pub example: Option<&'static str>,
}

/// One row of the list of words that are not approved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotApprovedWord {
    /// The word in lower case.
    pub word: &'static str,
    pub part_of_speech: &'static str,
    /// The approved words to use instead, as the list gives them: each with
    /// its part of speech, `; ` between them, or `(none)`.
    pub use_instead: &'static str,
}

impl NotApprovedWord {
    /// The words to write instead, without their part of speech: "keep
    /// (v); hold (v)" gives `keep` and `hold`. Empty when no word replaces it.
    pub fn replacements(&self) -> Vec<&'static str> {
        if self.use_instead == NO_REPLACEMENT {
            return Vec::new();
        }
        self.use_instead
            .split("; ")
            .map(|alternative| match alternative.find(" (") {
                Some(at) => &alternative[..at],
                None => alternative,
            })
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .collect()
    }
}

/// The rows of a data file: comments and the header line skipped, each row
/// split at its tabs.
fn rows(data: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
    data.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .skip(1)
        .map(|line| line.split('\t').collect())
}

static APPROVED_WORDS: LazyLock<Vec<ApprovedWord>> = LazyLock::new(|| {
    rows(APPROVED)
        .map(|cells| ApprovedWord {
            word: cells.first().copied().unwrap_or_default(),
            part_of_speech: cells.get(1).copied().unwrap_or_default(),
            meaning: cells.get(2).copied().unwrap_or_default(),
            example: cells.get(3).copied().filter(|e| !e.is_empty()),
        })
        .collect()
});

static NOT_APPROVED_WORDS: LazyLock<Vec<NotApprovedWord>> = LazyLock::new(|| {
    rows(NOT_APPROVED)
        .map(|cells| NotApprovedWord {
            word: cells.first().copied().unwrap_or_default(),
            part_of_speech: cells.get(1).copied().unwrap_or_default(),
            use_instead: cells.get(2).copied().unwrap_or_default(),
        })
        .collect()
});

/// The approved words of the vendored list, in the order of the file.
pub fn approved_words() -> &'static [ApprovedWord] {
    &APPROVED_WORDS
}

/// The words that are not approved, in the order of the file.
pub fn not_approved_words() -> &'static [NotApprovedWord] {
    &NOT_APPROVED_WORDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacements_drop_the_part_of_speech() {
        let row = NotApprovedWord {
            word: "maintain",
            part_of_speech: "v",
            use_instead: "keep (v); hold (v); maintenance (n)",
        };
        assert_eq!(row.replacements(), ["keep", "hold", "maintenance"]);
        let none = NotApprovedWord {
            use_instead: NO_REPLACEMENT,
            ..row
        };
        assert!(none.replacements().is_empty());
    }
}
