//! STE-V1: the vocabulary hook (KAIROS-T-0727, COLLIERY-I-0664 decisions 5
//! and 6).
//!
//! A word is valid when it is in the base word list or is part of a term of
//! the dictionaries. The caller gives both lists: the vendored word list is
//! KAIROS-T-0728, the dictionaries are KAIROS-T-0733. A term can have more
//! than one word; at each word the longest term that matches wins, so
//! "production document" is read before "production".
//!
//! A match ignores case. The regular plural (`s`, `es`, `ies`) and the
//! possessive (`'s`, and `s'` for a plural) of a listed noun also pass:
//! "files", "boxes", "entries", "team's", "teams'" (Dylan, 2026-10-30). No
//! other form is made: a verb form or an irregular plural passes only when the
//! list has it. [`Vocabulary::new`] does not know the part of speech, so each
//! of its entries counts as a noun; [`Vocabulary::standard`] takes the nouns
//! of the vendored list (KAIROS-T-0728) and each dictionary term.
//!
//! A word that is not approved and that the vendored list gives a replacement
//! for gets a message that names it: write "keep", not "maintain".

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::Violation;
use crate::code::code_prose;
use crate::mapped::Mapped;
use crate::prose::prose_blocks;
use crate::standard::{approved_words, not_approved_words};

static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}'’/._-]+").expect("valid regex"));

/// The characters that a word does not start or end with.
const EDGE: &[char] = &['\'', '’', '.', '_', '-', '/'];

/// A base word list and the terms of the dictionaries, to check STE-V1.
#[derive(Clone, Debug, Default)]
pub struct Vocabulary {
    /// Each one-word entry, in lower case.
    words: HashSet<String>,
    /// The one-word entries whose regular plural and possessive also pass.
    nouns: HashSet<String>,
    /// The terms of more than one word, by their first word, longest first.
    phrases: HashMap<String, Vec<Vec<String>>>,
    /// The words to write instead of a word that is not approved.
    instead: HashMap<String, Vec<String>>,
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
    ///
    /// Each one-word entry counts as a noun: its regular plural and its
    /// possessive pass too.
    pub fn new<B, T>(base: B, terms: T) -> Self
    where
        B: IntoIterator,
        B::Item: AsRef<str>,
        T: IntoIterator,
        T::Item: AsRef<str>,
    {
        let mut vocabulary = Self::default();
        for entry in base {
            vocabulary.add(entry.as_ref(), true);
        }
        for term in terms {
            vocabulary.add(term.as_ref(), true);
        }
        vocabulary.sort_phrases();
        vocabulary
    }

    /// A vocabulary of the vendored word list ([`crate::approved_words`])
    /// and the dictionary `terms`. Only the nouns of the list, and each
    /// term, take a plural or a possessive. A word of
    /// [`crate::not_approved_words`] that is in no list gets the words to
    /// write instead in its message.
    pub fn standard<T>(terms: T) -> Self
    where
        T: IntoIterator,
        T::Item: AsRef<str>,
    {
        let mut vocabulary = Self::default();
        for approved in approved_words() {
            vocabulary.add(approved.word, approved.part_of_speech == "n");
        }
        for term in terms {
            vocabulary.add(term.as_ref(), true);
        }
        vocabulary.sort_phrases();
        for row in not_approved_words() {
            let word = normalize(row.word);
            if word.contains(char::is_whitespace) || vocabulary.words.contains(&word) {
                continue;
            }
            let instead = vocabulary.instead.entry(word).or_default();
            for replacement in row.replacements() {
                if !instead.iter().any(|known| known == replacement) {
                    instead.push(replacement.to_owned());
                }
            }
        }
        vocabulary.instead.retain(|_, instead| !instead.is_empty());
        vocabulary
    }

    fn add(&mut self, entry: &str, noun: bool) {
        let words: Vec<String> = entry.split_whitespace().map(normalize).collect();
        match words.len() {
            0 => {}
            1 => {
                if noun {
                    self.nouns.insert(words[0].clone());
                }
                self.words.extend(words);
            }
            _ => self
                .phrases
                .entry(words[0].clone())
                .or_default()
                .push(words),
        }
    }

    fn sort_phrases(&mut self) {
        for phrases in self.phrases.values_mut() {
            phrases.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
            phrases.dedup();
        }
    }

    /// True when `word` (in lower case) is listed, or is the regular plural
    /// or the possessive of a listed noun.
    fn knows(&self, word: &str) -> bool {
        self.words.contains(word)
            || noun_bases(word)
                .iter()
                .any(|base| self.nouns.contains(base))
    }

    /// The words to write instead of `word`, or of the word that it is the
    /// plural or the possessive of.
    fn instead_of(&self, word: &str) -> Option<&[String]> {
        std::iter::once(word.to_owned())
            .chain(noun_bases(word))
            .find_map(|base| self.instead.get(&base))
            .map(Vec::as_slice)
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
            if !self.knows(word) {
                let written = &block.text[tokens[i].at.clone()];
                let fix = match self.instead_of(word) {
                    Some(instead) => format!("Write {}, not \"{written}\".", quoted_list(instead)),
                    None => "Use a word of the word list or a term of the dictionary.".to_owned(),
                };
                found.push(Violation::new(
                    "STE-V1",
                    tokens[i].span.clone(),
                    format!("\"{written}\" is not in the vocabulary. {fix}"),
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

/// Which of `terms` the prose of `texts` uses (KAIROS-T-0735): the index of
/// each term that is used, in the order of `terms`.
///
/// The texts are cleaned as [`crate::check_text`] cleans them, so a word in
/// code is not used. A match ignores case. At each word the longest term
/// that matches wins and takes its words: in "the production document",
/// the term "production document" is used and "production" is not. The
/// last word of a term also matches in its regular plural or its
/// possessive ("production documents", "team's"), as
/// [`Vocabulary::check_text`] lets them pass.
pub fn terms_used<X, T>(texts: X, terms: &[T]) -> Vec<usize>
where
    X: IntoIterator,
    X::Item: AsRef<str>,
    T: AsRef<str>,
{
    // The terms by their first word, longest first.
    let mut by_first: HashMap<String, Vec<(Vec<String>, usize)>> = HashMap::new();
    for (index, term) in terms.iter().enumerate() {
        let words: Vec<String> = term.as_ref().split_whitespace().map(normalize).collect();
        if let Some(first) = words.first() {
            by_first
                .entry(first.clone())
                .or_default()
                .push((words, index));
        }
    }
    for candidates in by_first.values_mut() {
        candidates.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.1.cmp(&b.1)));
    }
    let mut used = vec![false; terms.len()];
    for text in texts {
        for block in prose_blocks(text.as_ref()) {
            let tokens = tokens(&block);
            let mut i = 0;
            while i < tokens.len() {
                let found = term_at(&block, &tokens, i, &by_first);
                match found {
                    Some((len, index)) => {
                        used[index] = true;
                        i += len;
                    }
                    None => i += 1,
                }
            }
        }
    }
    used.iter()
        .enumerate()
        .filter_map(|(index, used)| used.then_some(index))
        .collect()
}

/// The longest term that starts at token `i`, as (word count, index).
fn term_at(
    block: &Mapped,
    tokens: &[Token],
    i: usize,
    by_first: &HashMap<String, Vec<(Vec<String>, usize)>>,
) -> Option<(usize, usize)> {
    // The first word can be the last word too, in its plural.
    let first = tokens[i].word.as_ref()?;
    let keys = std::iter::once(first.clone()).chain(noun_bases(first));
    let mut best: Option<(usize, usize)> = None;
    for key in keys {
        for (words, index) in by_first.get(&key).into_iter().flatten() {
            if best.is_some_and(|(len, _)| len >= words.len()) {
                continue;
            }
            let Some(window) = tokens.get(i..i + words.len()) else {
                continue;
            };
            let last = words.len() - 1;
            let words_match = window.iter().zip(words).enumerate().all(|(n, (t, w))| {
                let Some(word) = t.word.as_ref() else {
                    return false;
                };
                word == w || (n == last && noun_bases(word).iter().any(|base| base == w))
            });
            let adjacent = window.windows(2).all(|pair| {
                block.text[pair[0].at.end..pair[1].at.start]
                    .chars()
                    .all(char::is_whitespace)
            });
            if words_match && adjacent {
                best = Some((words.len(), *index));
            }
        }
    }
    best
}

/// The nouns that `word` can be the regular plural or the possessive of:
/// "boxes" gives "box" and "boxe", "entries" gives "entry" and "entrie",
/// "team's" gives "team". The caller checks which one is a noun. A plural
/// possessive ("teams'") is a plural here: the tokens lose a closing `'`.
fn noun_bases(word: &str) -> Vec<String> {
    if let Some(base) = word.strip_suffix("'s") {
        return vec![base.to_owned()];
    }
    let mut bases = Vec::new();
    if let Some(stem) = word.strip_suffix("ies") {
        bases.push(format!("{stem}y"));
    }
    if let Some(stem) = word.strip_suffix("es")
        && takes_es(stem)
    {
        bases.push(stem.to_owned());
    }
    if let Some(stem) = word.strip_suffix('s')
        && !stem.is_empty()
        && !takes_es(stem)
        && !takes_ies(stem)
    {
        bases.push(stem.to_owned());
    }
    bases
}

/// A noun that ends in `s`, `x`, `z`, `ch` or `sh` makes its plural with `es`.
fn takes_es(stem: &str) -> bool {
    ["s", "x", "z", "ch", "sh"]
        .iter()
        .any(|end| stem.ends_with(end))
}

/// A noun that ends in a consonant and `y` makes its plural with `ies`.
fn takes_ies(stem: &str) -> bool {
    let mut end = stem.chars().rev();
    end.next() == Some('y') && end.next().is_some_and(|c| !"aeiou".contains(c))
}

/// `"a"`, `"a" or "b"`, `"a", "b" or "c"`.
fn quoted_list(words: &[String]) -> String {
    let quoted: Vec<String> = words.iter().map(|w| format!("\"{w}\"")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => quoted.concat(),
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
    fn terms_used_takes_the_longest_term_first() {
        let terms = ["production", "production document", "board"];
        let used = terms_used(["The Production Document is ready."], &terms);
        assert_eq!(used, vec![1]);
        let used = terms_used(["A production document.", "Production starts."], &terms);
        assert_eq!(used, vec![0, 1]);
    }

    #[test]
    fn terms_used_matches_a_plural_and_a_possessive() {
        let terms = ["production document", "team", "entry"];
        let used = terms_used(["Two production documents of the team's entries."], &terms);
        assert_eq!(used, vec![0, 1, 2]);
    }

    #[test]
    fn terms_used_skips_code_and_split_words() {
        let terms = ["production document", "board"];
        let used = terms_used(
            ["Run `board`.\n\n```\nboard\n```\n\nThe production, document."],
            &terms,
        );
        assert!(used.is_empty(), "{used:?}");
    }

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
