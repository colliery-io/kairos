//! STE-V1: the vocabulary hook (KAIROS-T-0727, COLLIERY-I-0664 decisions 5
//! and 6).
//!
//! A word is valid when it is in the base word list or is part of a term of
//! the dictionaries. The caller gives both lists: the vendored word list is
//! KAIROS-T-0728, the dictionaries are KAIROS-T-0733. A term can have more
//! than one word; at each word the longest term that matches wins, so
//! "production document" is read before "production".
//!
//! A match is exact and ignores case: this hook does not know inflections or
//! the possessive. A word list that wants "files" to pass lists "files".

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::Violation;
use crate::code::code_prose;
use crate::mapped::Mapped;
use crate::prose::prose_blocks;

static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}'’/._-]+").expect("valid regex"));

/// The characters that a word does not start or end with.
const EDGE: &[char] = &['\'', '’', '.', '_', '-', '/'];

/// A base word list and the terms of the dictionaries, to check STE-V1.
#[derive(Clone, Debug, Default)]
pub struct Vocabulary {
    /// Each one-word entry, in lower case.
    words: HashSet<String>,
    /// The terms of more than one word, by their first word, longest first.
    phrases: HashMap<String, Vec<Vec<String>>>,
}

/// One word of a text, with its input span.
struct Token {
    /// The word in lower case, or `None` when it is not checked (a number, a
    /// path, a name with `_`, a placeholder of the cleaning).
    word: Option<String>,
    span: Range<usize>,
    /// The byte range in the cleaned text.
    at: Range<usize>,
}

fn normalize(word: &str) -> String {
    word.to_lowercase().replace('’', "'")
}

impl Vocabulary {
    /// A vocabulary of the `base` words and the dictionary `terms`. An entry
    /// with whitespace in it is a term of more than one word, in either list.
    pub fn new<B, T>(base: B, terms: T) -> Self
    where
        B: IntoIterator,
        B::Item: AsRef<str>,
        T: IntoIterator,
        T::Item: AsRef<str>,
    {
        let mut vocabulary = Self::default();
        for entry in base
            .into_iter()
            .map(|b| b.as_ref().to_owned())
            .chain(terms.into_iter().map(|t| t.as_ref().to_owned()))
        {
            let words: Vec<String> = entry.split_whitespace().map(normalize).collect();
            match words.len() {
                0 => {}
                1 => {
                    vocabulary.words.extend(words);
                }
                _ => vocabulary
                    .phrases
                    .entry(words[0].clone())
                    .or_default()
                    .push(words),
            }
        }
        for phrases in vocabulary.phrases.values_mut() {
            phrases.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
            phrases.dedup();
        }
        vocabulary
    }

    /// STE-V1 for markdown prose: each word that is in no list. The text is
    /// cleaned as [`crate::check_text`] cleans it.
    pub fn check_text(&self, text: &str) -> Vec<Violation> {
        prose_blocks(text)
            .iter()
            .flat_map(|block| self.check_block(block))
            .collect()
    }

    /// STE-V1 for the text of an error, cleaned as
    /// [`crate::check_code_text`] cleans it.
    pub fn check_code_text(&self, text: &str) -> Vec<Violation> {
        self.check_block(&code_prose(text))
    }

    fn check_block(&self, block: &Mapped) -> Vec<Violation> {
        let tokens = tokens(block);
        let mut found = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let Some(word) = &tokens[i].word else {
                i += 1;
                continue;
            };
            if let Some(n) = self.phrase_at(block, &tokens, i) {
                i += n;
                continue;
            }
            if !self.words.contains(word) {
                found.push(Violation::new(
                    "STE-V1",
                    tokens[i].span.clone(),
                    format!(
                        "\"{}\" is not in the vocabulary. Use a word of the word list or a term of the dictionary.",
                        &block.text[tokens[i].at.clone()]
                    ),
                ));
            }
            i += 1;
        }
        found
    }

    /// The word count of the longest term that starts at token `i`. The
    /// words of a term are next to each other, with only whitespace between.
    fn phrase_at(&self, block: &Mapped, tokens: &[Token], i: usize) -> Option<usize> {
        let first = tokens[i].word.as_ref()?;
        let candidates = self.phrases.get(first)?;
        candidates.iter().find_map(|phrase| {
            let window = tokens.get(i..i + phrase.len())?;
            let words_match = window
                .iter()
                .zip(phrase)
                .all(|(t, w)| t.word.as_ref() == Some(w));
            let adjacent = window.windows(2).all(|pair| {
                block.text[pair[0].at.end..pair[1].at.start]
                    .chars()
                    .all(char::is_whitespace)
            });
            (words_match && adjacent).then_some(phrase.len())
        })
    }
}

fn tokens(block: &Mapped) -> Vec<Token> {
    TOKEN
        .find_iter(&block.text)
        .filter_map(|m| {
            let raw = m.as_str();
            let trimmed = raw.trim_matches(EDGE);
            if trimmed.is_empty() {
                return None;
            }
            let start = m.start() + raw.find(trimmed).unwrap_or(0);
            let at = start..start + trimmed.len();
            let technical = trimmed.chars().any(|c| c.is_numeric())
                || trimmed.contains(['/', '_', '.'])
                || block.is_synthetic(at.clone());
            Some(Token {
                word: (!technical).then(|| normalize(trimmed)),
                span: block.span(at.clone()),
                at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_placeholder_is_not_a_word() {
        let v = Vocabulary::new(["run", "the"], [] as [&str; 0]);
        assert!(v.check_text("Run `kairos login`.").is_empty());
        assert!(v.check_code_text("Run {command}.").is_empty());
    }

    #[test]
    fn numbers_and_paths_are_not_words() {
        let v = Vocabulary::new(["read"], [] as [&str; 0]);
        assert!(v.check_text("Read docs/a.md 12 times_x").is_empty());
    }
}
