//! Composing the text behind an item's primary vector (KAIROS-A-0021 rule 4,
//! KAIROS-T-0190).
//!
//! Pure, per KAIROS-A-0009. This decides *what text* represents "what is this
//! item about"; `kairos-embed` turns it into a vector and KAIROS-T-0187's
//! `item_embeddings` stores it.
//!
//! # The system composes it; the template does not
//!
//! Rule 4 lists the inputs: title, entity type, repository, owning team, parent's
//! title, stamped metadata, opening prose. Every one of them is **structural** —
//! present for every item, and owned by the product rather than by a tenant's
//! template.
//!
//! That constraint came from being wrong earlier. An initial design keyed on
//! named sections — "take the Summary" — and the corpus killed it: across 4,927
//! real documents there were 11,553 distinct heading strings, **85% occurring
//! exactly once**, and the tenth most common heading was a template marker nobody
//! had deleted. Tenants own their templates and templates evolve, so anything
//! that reads a section by name is a design that works until someone edits a
//! template.
//!
//! Stamped metadata carries real weight here for the opposite reason: where
//! section names cannot be trusted, a metadata definition is the one place in the
//! product where meaning is **explicitly labelled**, by a tenant who chose the
//! label.
//!
//! # Opening prose is bounded, and that bound is not arbitrary
//!
//! Items are long — a median Metis task is about 3,900 characters and
//! specifications run past 13,000 — while an embedding model has a fixed context
//! and gives every token equal say. Feeding a whole document in dilutes the title
//! it is mostly about. So the prose is cut at [`PrimaryConfig::prose_chars`].
//!
//! The budget is **600 characters, uniform across entity types**, and that was
//! measured rather than guessed. Varying one budget over the 4,927-document
//! corpus, scored by recall@1 on known duplicate tickets against the number of
//! unlinked same-project pairs crossing 0.93 — the threshold where precision was
//! measured at roughly half, so lower is better:
//!
//! | prose chars | recall@1 | pairs ≥0.93 | of those, initiative-level |
//! |---|---|---|---|
//! | 0 | 94% | 324 | 102 |
//! | 150 | 80% | 279 | 33 |
//! | 300 | 86% | 296 | 38 |
//! | **600** | **86%** | **305** | **39** |
//! | 1200 | 86% | 340 | 41 |
//!
//! Two things came out of that, one of them a correction.
//!
//! **More prose past 600 is strictly worse**: 1,200 characters buys no recall and
//! adds 35 borderline pairs. So the budget is at the point where the curve stops
//! paying, not at an arbitrary round number. 300 is within noise of 600 and would
//! be equally defensible.
//!
//! **The reason for a per-type budget turned out not to hold.** KAIROS-T-0190
//! observed false positives clustering at initiative level and the hypothesis was
//! that their opening prose is boilerplate and should therefore be cut for long
//! types. The measurement says the opposite: initiative-level borderline pairs are
//! **102 with no prose and 33–41 with it**. Prose is what rescues initiatives; it
//! is their generic *titles* that make them look alike. A per-type table cutting
//! prose for initiatives would have made exactly the problem it was meant to fix
//! worse, so there is one budget and no table.
//!
//! One caveat on reading that recall column: the ground truth is pairs of tickets
//! with identical titles, so a title-only composition scores well on it almost by
//! construction. That is why 0 leads the recall column and why it cannot be
//! chosen on that basis — the pairs column is the counterweight, and it is
//! emphatic.
//!
use std::fmt::Write as _;

use crate::chunk::chunk;
use crate::short_code::ItemType;

/// The structural facts about an item that go into its primary vector.
///
/// Borrowed rather than owned: the caller has these on a database row already and
/// composing a vector's text should not cost a round of allocations.
#[derive(Debug, Clone, Default)]
pub struct PrimaryInputs<'a> {
    /// The item's title.
    pub title: &'a str,
    /// The repository the item is issued against, if any. A strong prior for
    /// genuine coupling (KAIROS-A-0019).
    pub repository: Option<&'a str>,
    /// The owning team's name or slug, if any.
    pub team: Option<&'a str>,
    /// The parent item's title, if it has a parent. Cheap context that says
    /// which piece of work this belongs to.
    pub parent_title: Option<&'a str>,
    /// Stamped metadata as `(label, value)` pairs, in the order they should be
    /// read. The **label** matters: it is tenant-chosen meaning, unlike a
    /// heading.
    pub metadata: &'a [(String, String)],
    /// The item's full content. Only the opening survives — see the module docs.
    pub content: &'a str,
}

/// How much of each input to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimaryConfig {
    /// How many characters of opening prose to include.
    ///
    /// 600 by default: enough for a paragraph or two of real description, short
    /// enough that a 13,000-character specification does not drown its own title.
    pub prose_chars: usize,
}

impl Default for PrimaryConfig {
    fn default() -> Self {
        Self { prose_chars: 600 }
    }
}

/// Compose the text whose embedding is the item's primary vector.
pub fn primary_text(item_type: ItemType, inputs: &PrimaryInputs<'_>) -> String {
    primary_text_with(item_type, inputs, &PrimaryConfig::default())
}

/// [`primary_text`] with an explicit budget.
///
/// The output is deliberately plain labelled lines rather than prose. It is read
/// by a model, not a person, and a stable shape means the same item composes the
/// same text — which is what makes KAIROS-T-0187's `content_hash` able to skip
/// unchanged work.
pub fn primary_text_with(
    item_type: ItemType,
    inputs: &PrimaryInputs<'_>,
    config: &PrimaryConfig,
) -> String {
    let mut out = String::new();

    // Type and title first: the most discriminating things an item has, and
    // first is where a truncating model pays the most attention.
    let _ = writeln!(out, "{}: {}", type_label(item_type), inputs.title.trim());

    if let Some(repository) = non_blank(inputs.repository) {
        let _ = writeln!(out, "repository: {repository}");
    }
    if let Some(team) = non_blank(inputs.team) {
        let _ = writeln!(out, "team: {team}");
    }
    if let Some(parent) = non_blank(inputs.parent_title) {
        let _ = writeln!(out, "part of: {parent}");
    }
    for (label, value) in inputs.metadata {
        let (label, value) = (label.trim(), value.trim());
        if label.is_empty() || value.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{label}: {value}");
    }

    if let Some(prose) = opening_prose(inputs.content, config.prose_chars) {
        let _ = writeln!(out, "{prose}");
    }
    out
}

/// The item's opening prose, up to `budget` characters.
///
/// Taken from the chunker's first chunk with real content, so "opening" means the
/// same thing here as it does in `item_chunks` — one definition of where a
/// document starts, not two that can disagree. Content that is all headings, or
/// empty, yields `None` rather than an empty line.
fn opening_prose(content: &str, budget: usize) -> Option<String> {
    if budget == 0 {
        return None;
    }
    let chunks = chunk(content);
    let first = chunks.iter().find(|c| !c.text.trim().is_empty())?;
    let text = first.text.trim();
    if text.is_empty() {
        return None;
    }
    let taken: String = text.chars().take(budget).collect();
    // Do not end mid-word when the cut lands inside one: a truncated token is
    // noise to a tokeniser, and losing a few characters costs nothing.
    if taken.chars().count() < text.chars().count() {
        if let Some(last_space) = taken.rfind(char::is_whitespace) {
            let trimmed = taken[..last_space].trim_end();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    Some(taken.trim_end().to_string())
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// How an entity type is named to the model.
///
/// Spelled out rather than derived from `Debug`, because this string is part of
/// the text that gets embedded: renaming a Rust variant must not silently change
/// every stored vector's meaning.
fn type_label(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Strategy => "strategy",
        ItemType::Initiative => "initiative",
        ItemType::Task => "task",
        ItemType::Document => "document",
        ItemType::Adr => "decision record",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_title_and_type_lead() {
        let inputs = PrimaryInputs {
            title: "Refund rounds the wrong way",
            ..Default::default()
        };
        let text = primary_text(ItemType::Task, &inputs);
        assert!(
            text.starts_with("task: Refund rounds the wrong way"),
            "{text}"
        );
    }

    #[test]
    fn every_structural_input_appears() {
        let metadata = meta(&[("priority", "high"), ("component", "billing")]);
        let inputs = PrimaryInputs {
            title: "Refund rounding",
            repository: Some("payments-api"),
            team: Some("platform"),
            parent_title: Some("Billing correctness"),
            metadata: &metadata,
            content: "The total is rounded before tax.",
        };
        let text = primary_text(ItemType::Task, &inputs);
        for expected in [
            "task: Refund rounding",
            "repository: payments-api",
            "team: platform",
            "part of: Billing correctness",
            "priority: high",
            "component: billing",
            "The total is rounded before tax.",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
    }

    /// Absent inputs must leave no trace — a dangling `repository:` line would
    /// put the same token into every item that has no repository, which is the
    /// opposite of discriminating.
    #[test]
    fn absent_and_blank_inputs_leave_no_line() {
        let metadata = meta(&[("", "ignored"), ("priority", "  "), ("real", "kept")]);
        let inputs = PrimaryInputs {
            title: "Bare item",
            repository: None,
            team: Some("   "),
            parent_title: Some(""),
            metadata: &metadata,
            content: "",
        };
        let text = primary_text(ItemType::Document, &inputs);
        assert_eq!(
            text, "document: Bare item\nreal: kept\n",
            "only the title and the one real metadata pair"
        );
    }

    /// Stability is what makes content hashing able to skip work: the same inputs
    /// must compose the same text, byte for byte.
    #[test]
    fn composition_is_stable() {
        let metadata = meta(&[("priority", "high")]);
        let inputs = PrimaryInputs {
            title: "Stable",
            repository: Some("repo"),
            metadata: &metadata,
            content: "Body text here.",
            ..Default::default()
        };
        let once = primary_text(ItemType::Task, &inputs);
        let again = primary_text(ItemType::Task, &inputs);
        assert_eq!(once, again);
    }

    #[test]
    fn prose_is_bounded_and_does_not_end_mid_word() {
        let content = "alpha beta gamma delta epsilon zeta eta theta";
        let config = PrimaryConfig { prose_chars: 20 };
        let inputs = PrimaryInputs {
            title: "T",
            content,
            ..Default::default()
        };
        let text = primary_text_with(ItemType::Task, &inputs, &config);
        let prose = text.lines().last().unwrap();
        assert!(content.starts_with(prose), "a prefix of the content: {prose:?}");
        assert!(prose.len() <= 20, "within budget: {prose:?}");
        assert!(
            !content[prose.len()..].starts_with(|c: char| c.is_alphanumeric()),
            "cut at a word boundary, not mid-word: {prose:?}"
        );
    }

    /// The opening comes from the chunker, so a document whose first section is a
    /// heading with no body skips to the first real prose — one definition of
    /// "opening", shared with `item_chunks`.
    #[test]
    fn the_opening_skips_empty_sections() {
        let inputs = PrimaryInputs {
            title: "T",
            content: "# Objective\n\n## Details\n\nThe actual prose.\n",
            ..Default::default()
        };
        let text = primary_text(ItemType::Task, &inputs);
        assert!(text.contains("The actual prose."), "{text}");
        assert!(!text.contains("Objective"), "headings are not prose: {text}");
    }

    #[test]
    fn content_that_is_only_headings_contributes_nothing() {
        let inputs = PrimaryInputs {
            title: "Empty",
            content: "# A\n\n## B\n\n### C\n",
            ..Default::default()
        };
        assert_eq!(primary_text(ItemType::Task, &inputs), "task: Empty\n");
    }

    #[test]
    fn a_zero_budget_omits_prose_entirely() {
        let inputs = PrimaryInputs {
            title: "T",
            content: "Plenty of prose here.",
            ..Default::default()
        };
        let text = primary_text_with(ItemType::Task, &inputs, &PrimaryConfig { prose_chars: 0 });
        assert_eq!(text, "task: T\n");
    }

    /// The labels are part of the embedded text, so they are asserted rather than
    /// left to a `Debug` impl that a rename could change under us.
    #[test]
    fn every_type_has_a_stable_label() {
        for (t, expected) in [
            (ItemType::Strategy, "strategy"),
            (ItemType::Initiative, "initiative"),
            (ItemType::Task, "task"),
            (ItemType::Document, "document"),
            (ItemType::Adr, "decision record"),
        ] {
            assert_eq!(type_label(t), expected);
        }
    }

    #[test]
    fn prose_budget_counts_characters_not_bytes() {
        let content = "café ".repeat(40);
        let config = PrimaryConfig { prose_chars: 10 };
        let inputs = PrimaryInputs {
            title: "T",
            content: &content,
            ..Default::default()
        };
        let text = primary_text_with(ItemType::Task, &inputs, &config);
        let prose = text.lines().last().unwrap();
        assert!(prose.chars().count() <= 10, "{prose:?}");
        assert!(prose.starts_with("café"), "{prose:?}");
    }
}
