//! The plain-language checker (KAIROS-T-0727, COLLIERY-I-0664 decision 2).
//!
//! The mechanical rules of `scripts/ste-check.py` (KAIROS-S-0009, the STE
//! spec KAIROS-D-0009 section 6.1), in Rust, so that the server and the
//! worker can check a text. The rules and their word lists are a port: the
//! tests in `tests/parity.rs` hold this crate to the output of the Python on a
//! shared fixture set, so a change to one is a change to both.
//!
//! - [`check_text`]: markdown prose. STE-S2, STE-S4, STE-G1, STE-G4, STE-V3.
//! - [`check_code_text`]: the text of an error. STE-S2/S3, STE-G1, STE-G4,
//!   STE-V3 and STE-C1 to STE-C7.
//! - [`Vocabulary`]: STE-V1, a word that is in no list. The caller gives the
//!   lists, or takes the vendored word list with [`Vocabulary::standard`]
//!   ([`approved_words`], [`not_approved_words`], KAIROS-T-0728).
//! - [`terms_used`]: which terms of a dictionary a text uses, longest
//!   first (KAIROS-T-0735).
//! - [`parse_dictionary`]: the entries of a dictionary document
//!   (KAIROS-T-0733; one dictionary for the organization, KAIROS-A-0079).
//!
//! Each [`Violation`] names its rule, the byte range of the input that it is
//! about, and a message that names the fix.

mod code;
mod dictionary;
mod mapped;
mod prose;
mod standard;
mod vocabulary;

use std::ops::Range;

pub use code::check_code_text;
pub use dictionary::{DictionaryEntry, DictionaryProblem, ParsedDictionary, parse_dictionary};
pub use prose::check_text;
pub use standard::{ApprovedWord, NotApprovedWord, approved_words, not_approved_words};
pub use vocabulary::{Vocabulary, terms_used};

/// One place where a text breaks a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The rule id of KAIROS-D-0009, as `STE-S2`.
    pub rule: &'static str,
    /// The byte range of the checked text that the violation is about: a
    /// sentence, a paragraph, a word or one character.
    pub span: Range<usize>,
    /// What is wrong, then what to do.
    pub message: String,
}

impl Violation {
    pub(crate) fn new(rule: &'static str, span: Range<usize>, message: String) -> Self {
        Self {
            rule,
            span,
            message,
        }
    }
}
