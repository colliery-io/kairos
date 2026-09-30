//! What text is embedded, beyond the item's own words (COLLIERY-T-1840).
//!
//! Pure, per KAIROS-A-0009. [`crate::chunk`] splits content and
//! [`crate::primary`] composes the probe; this module holds the three rules
//! that both of them, and the retrieval query, share.
//!
//! # Why these rules exist
//!
//! `related_work COLLIERY-T-1757` proposed eight unrelated items and missed
//! the clear prior art in two other repositories. The investigation on
//! 2026-09-30 found the vector half of the answer was decided by text that says
//! nothing about the item:
//!
//! - **The importer's provenance footer.** Each of the 2,270 imported items
//!   ends with "This item came from the Metis record of the repository X. Its
//!   Metis code was …" (or, for the Kairos record, "*Migrated from Metis `X`
//!   …*"). The footer is in the last chunk of the item, and for 551 chunks it was
//!   half the text or more. Footer-dominated chunks of one repository sat at a
//!   median cosine of 0.957 with each other. [`mask_import_footer`] hides it
//!   from chunking and embedding; the stored content keeps it, because a text
//!   search for an old Metis code must still find the item.
//! - **Sections with no real words.** For 35% of items the first section was
//!   "Parent Initiative" with a line of short codes, and that line was the
//!   whole of the probe's prose. [`has_real_words`] is how the probe skips it.
//! - **Short chunks.** 21% of chunks had fewer than 80 characters ("None.", a
//!   line of codes, a template marker). Such a chunk is close to anything short,
//!   and an item with many of them had many chances to be the nearest. A chunk
//!   shorter than [`MIN_MATCH_CHARS`] cannot be the best match of an item.
//!
//! And a chunk alone is ambiguous — "Status Updates: tests pass" is every
//! task — so [`chunk_text`] embeds a chunk with the title of its item.

use std::borrow::Cow;

/// A chunk whose body is shorter than this cannot be the best match of its
/// item, and its heading is never quoted as the reason for a proposal.
///
/// 150 characters: the investigation measured 31.7% of chunks below it, and
/// those are the template bodies, the lines of codes and the one-line status
/// notes. A real section of prose is longer.
pub const MIN_MATCH_CHARS: usize = 150;

/// How many real words a section needs before the probe takes it as the
/// item's opening.
///
/// Eight: a line of short codes has one or two, a "Type" or "Priority" section
/// has two to six, and the sentence that says what an item is about has more.
pub const MIN_REAL_WORDS: usize = 8;

/// Where each importer footer starts, after its `---` rule.
///
/// Both are written by tools of this repository: the first by
/// `scripts/metis_import/remap.py` (`footer`), the second by the migration of
/// the Kairos Metis record on 2026-09-26.
const FOOTER_LEADS: [&str; 2] = [
    "This item came from the Metis record of the repository ",
    "*Migrated from Metis `",
];

/// `content` with each importer footer replaced by spaces.
///
/// The footer is the `---` rule, the blank lines after it and the paragraph
/// that starts with one of the footer leads. Each of its characters becomes a
/// space, and each line break stays, so the character offsets of every chunk
/// still point into the stored content exactly — a citation stays correct.
///
/// Content with no footer is returned as it is, without a copy.
pub fn mask_import_footer(content: &str) -> Cow<'_, str> {
    let lines = lines_keeping_newlines(content);
    let mut masked: Vec<bool> = vec![false; lines.len()];
    let mut any = false;
    let mut i = 0;
    while i < lines.len() {
        if !is_rule(lines[i]) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < lines.len() && lines[j].trim().is_empty() {
            j += 1;
        }
        if j < lines.len() && FOOTER_LEADS.iter().any(|lead| lines[j].starts_with(lead)) {
            // The footer paragraph: this line and the lines after it, up to
            // a blank line or a heading.
            let mut end = j + 1;
            while end < lines.len() && !lines[end].trim().is_empty() && !lines[end].starts_with('#')
            {
                end += 1;
            }
            for flag in &mut masked[i..end] {
                *flag = true;
            }
            any = true;
            i = end;
        } else {
            i += 1;
        }
    }
    if !any {
        return Cow::Borrowed(content);
    }
    let mut out = String::with_capacity(content.len());
    for (line, hide) in lines.iter().zip(masked) {
        if hide {
            out.extend(
                line.chars()
                    .map(|c| if c == '\n' || c == '\r' { c } else { ' ' }),
            );
        } else {
            out.push_str(line);
        }
    }
    Cow::Owned(out)
}

/// How many real words `text` has: tokens with at least two letters and no
/// digit, after their surrounding punctuation is removed.
///
/// So `COLLIERY-I-0262`, `v1`, `[x]` and `—` are not words, and `decision`,
/// `rotate_admin` and `SPA` are.
pub fn real_word_count(text: &str) -> usize {
    text.split_whitespace()
        .map(|token| token.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| {
            !word.chars().any(|c| c.is_ascii_digit())
                && word.chars().filter(|c| c.is_alphabetic()).count() >= 2
        })
        .count()
}

/// Whether `text` says something in words, and is not only codes, a
/// checkbox or a template marker. See [`MIN_REAL_WORDS`].
pub fn has_real_words(text: &str) -> bool {
    real_word_count(text) >= MIN_REAL_WORDS
}

/// The text that is embedded for one chunk: the title of its item, its
/// heading, and its body.
///
/// The title makes the chunk say which item it is part of; without it, a
/// section such as "Status Updates: the tests pass" is the same in every task.
/// The heading is echoed as a label and weighs little against a body of
/// [`MIN_MATCH_CHARS`] or more.
pub fn chunk_text(title: &str, heading: Option<&str>, body: &str) -> String {
    let mut out = String::with_capacity(title.len() + body.len() + 64);
    out.push_str(title.trim());
    out.push('\n');
    if let Some(heading) = heading.map(str::trim).filter(|h| !h.is_empty()) {
        out.push_str(heading);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(body.trim());
    out
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && t.chars().all(|c| c == '-')
}

fn lines_keeping_newlines(content: &str) -> Vec<&str> {
    content.split_inclusive('\n').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMPORTED: &str = "## Objective\n\nServe the UI.\n\n## Status Updates\n\nDone.\n\n---\n\n\
        This item came from the Metis record of the repository brokkr. Its Metis code was \
        BROKKR-T-0253. Metis created it on 2026-06-28. Its Metis phase was completed.\n";

    #[test]
    fn the_importer_footer_is_masked_and_offsets_stay() {
        let masked = mask_import_footer(IMPORTED);
        assert_eq!(
            masked.chars().count(),
            IMPORTED.chars().count(),
            "one space for each hidden character"
        );
        assert!(!masked.contains("Metis"), "{masked:?}");
        assert!(!masked.contains("---"), "the rule goes too: {masked:?}");
        assert!(
            masked.starts_with("## Objective\n\nServe the UI.\n\n## Status Updates\n\nDone.\n")
        );
        assert_eq!(
            masked.lines().count(),
            IMPORTED.lines().count(),
            "each line break stays"
        );
    }

    #[test]
    fn the_kairos_migration_footer_is_masked() {
        let content = "## What\n\nA thing.\n\n---\n\n*Migrated from Metis `KAIROS-T-0012` on \
            2026-09-26. On 2026-09-30 the Metis codes in this text changed to the new codes.*\n";
        let masked = mask_import_footer(content);
        assert!(!masked.contains("Migrated"), "{masked:?}");
        assert!(masked.contains("A thing."));
    }

    #[test]
    fn text_after_the_footer_is_kept() {
        let content = format!("{IMPORTED}\n## Status, 2026-09-30\n\nA later note.\n");
        let masked = mask_import_footer(&content);
        assert!(!masked.contains("BROKKR-T-0253"));
        assert!(masked.contains("## Status, 2026-09-30\n\nA later note.\n"));
    }

    #[test]
    fn content_without_a_footer_is_not_copied() {
        let content = "## A\n\nText.\n\n---\n\nA rule that ends a part, and no footer.\n";
        assert!(matches!(mask_import_footer(content), Cow::Borrowed(c) if c == content));
    }

    #[test]
    fn codes_and_checkboxes_are_not_real_words() {
        assert_eq!(
            real_word_count("COLLIERY-I-0262 · decision COLLIERY-A-0109"),
            1
        );
        assert_eq!(real_word_count("- [x] Bug - Production issue"), 3);
        assert_eq!(real_word_count("`rotate_admin` serves the SPA, v1"), 4);
        assert!(!has_real_words(
            "COLLIERY-I-0262 · decision COLLIERY-A-0109"
        ));
        assert!(!has_real_words("Tech Debt"));
        assert!(has_real_words(
            "Make the broker serve the built wasm bundle so the console is reachable."
        ));
    }

    #[test]
    fn a_chunk_is_embedded_with_its_title_and_heading() {
        assert_eq!(
            chunk_text("Serve the UI", Some("Objective"), "\nThe body.\n\n"),
            "Serve the UI\nObjective\n\nThe body."
        );
        assert_eq!(chunk_text(" T ", None, "B"), "T\n\nB");
        assert_eq!(chunk_text("T", Some("  "), "B"), "T\n\nB");
    }
}
