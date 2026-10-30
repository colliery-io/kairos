//! Typed sections of a template, and the sections of an item's content
//! (KAIROS-T-0729, COLLIERY-I-0664).
//!
//! A template names some sections of its content: a heading and a type. The
//! server checks the text under each typed section (KAIROS-T-0730). This
//! module is pure (KAIROS-A-0009): the type vocabulary and a parser that
//! finds a section by the text of its heading.
//!
//! The parser is apart from [`crate::chunk`] on purpose. The chunker splits
//! for embeddings and keys on no heading name; this parser keys on the name,
//! because a template says which name to find.

use std::fmt;
use std::str::FromStr;

/// The type of a typed section: which check the server runs on its text.
///
/// `ace` (a controlled language) was dropped for now (KAIROS-T-0732 n-2):
/// it is an unknown type, and it is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SectionType {
    /// Free text: the section must be there and have text; no language check.
    Free,
    /// Plain language: the text must pass the plain-language checker.
    Plain,
    /// Requirements: each requirement must fit one frame of EARS
    /// (KAIROS-T-0732).
    Ears,
}

impl SectionType {
    /// Each type, in the order of the docs and of the error texts.
    pub const ALL: [SectionType; 3] = [SectionType::Free, SectionType::Plain, SectionType::Ears];

    /// The stored and wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            SectionType::Free => "free",
            SectionType::Plain => "plain",
            SectionType::Ears => "ears",
        }
    }

    /// The names of each type, for an error that names the allowed values.
    pub fn names() -> Vec<&'static str> {
        Self::ALL.iter().map(|t| t.as_str()).collect()
    }
}

impl fmt::Display for SectionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The refusal of an unknown section type. It names the value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("The section type {0:?} is not known. Use one of: free, plain, ears.")]
pub struct UnknownSectionType(pub String);

impl FromStr for SectionType {
    type Err = UnknownSectionType;

    /// Exact match only: `Plain` and ` plain` are refused, so that the
    /// stored value is always one of the names.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str() == value)
            .ok_or_else(|| UnknownSectionType(value.to_string()))
    }
}

/// Why a template heading is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeadingError {
    /// No text after trimming.
    #[error("The heading is empty. Send the text of the heading.")]
    Empty,
    /// The text starts with `#`.
    #[error("The heading {0:?} starts with \"#\". Send the text of the heading without the \"#\".")]
    Hashes(String),
    /// The text has a line break.
    #[error("The heading {0:?} has a line break. A heading is one line.")]
    LineBreak(String),
}

/// Check the heading of a typed section and give its stored form (trimmed).
/// The heading is the text only, with no `#`: it matches a heading of each
/// level in the content.
pub fn normalize_heading(heading: &str) -> Result<String, HeadingError> {
    let trimmed = heading.trim();
    if trimmed.is_empty() {
        return Err(HeadingError::Empty);
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(HeadingError::LineBreak(heading.to_string()));
    }
    if trimmed.starts_with('#') {
        return Err(HeadingError::Hashes(heading.to_string()));
    }
    Ok(trimmed.to_string())
}

/// One section of an item's content: an ATX heading and the text under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The heading text: trimmed, with no `#` and no closing `#`s.
    pub heading: String,
    /// 1 to 6: the count of `#`.
    pub level: u8,
    /// The 0-based line of the heading.
    pub line: usize,
    /// The text under the heading, to the next heading of the same or a
    /// higher level. A nested heading (a higher count of `#`) and its text
    /// are part of the body. Not trimmed.
    pub body: String,
}

impl Section {
    /// True when the body has no text other than white space.
    pub fn is_empty(&self) -> bool {
        self.body.trim().is_empty()
    }
}

/// Each section of the content, in order. A heading in a fenced code block
/// (```` ``` ```` or `~~~`) is not a heading. Text before the first heading
/// is in no section.
pub fn parse_sections(content: &str) -> Vec<Section> {
    let lines: Vec<&str> = content.lines().collect();
    let mut headings: Vec<(usize, u8, String)> = Vec::new();
    let mut fence: Option<char> = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        match (fence, marker) {
            (None, Some(open)) => {
                fence = Some(open);
                continue;
            }
            (Some(open), Some(close)) if open == close => {
                fence = None;
                continue;
            }
            (Some(_), _) => continue,
            (None, None) => {}
        }
        if let Some((level, text)) = heading(line) {
            headings.push((index, level, text));
        }
    }
    headings
        .iter()
        .enumerate()
        .map(|(position, (line, level, text))| {
            let end = headings[position + 1..]
                .iter()
                .find(|(_, next_level, _)| next_level <= level)
                .map(|(next_line, _, _)| *next_line)
                .unwrap_or(lines.len());
            Section {
                heading: text.clone(),
                level: *level,
                line: *line,
                body: lines[line + 1..end].join("\n"),
            }
        })
        .collect()
}

/// Each section of the content whose heading is `heading` (exact text, after
/// the trim of [`parse_sections`]). None: the section is missing. More than
/// one: the heading is repeated.
pub fn sections_named(content: &str, heading: &str) -> Vec<Section> {
    parse_sections(content)
        .into_iter()
        .filter(|section| section.heading == heading)
        .collect()
}

/// The level and text of an ATX heading line (`#` to `######`, then a space
/// or the end of the line; up to 3 spaces of indent), or `None`.
fn heading(line: &str) -> Option<(u8, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let trimmed = line[indent..].trim_end_matches(['\n', '\r']);
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim().to_string();
    Some((hashes as u8, text))
}

/// One rule that the text of a `plain` or an `ears` section breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionViolation {
    /// The rule id, as `STE-S2` or `EARS-1`.
    pub rule: &'static str,
    /// The text that breaks the rule: a sentence, a paragraph or a word.
    pub text: String,
    /// What is wrong, then what to do.
    pub fix: String,
}

/// Why a typed section does not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionProblem {
    /// No heading of the content has the text of the section.
    Missing,
    /// The section is there, but it has no text.
    Empty,
    /// The text breaks the rules of the type of the section.
    Violations(Vec<SectionViolation>),
}

/// A typed section of a template that the content does not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionFailure {
    pub heading: String,
    pub section_type: SectionType,
    pub problem: SectionProblem,
}

/// Check `content` against the typed sections of its template
/// (KAIROS-T-0730, COLLIERY-I-0664 decision 1). Each section must be there
/// and have text. The text of a `plain` section must also pass the
/// plain-language checker and the vocabulary. Each requirement of an `ears`
/// section must fit one frame of EARS (KAIROS-T-0732). A `free` section gets
/// no language check. A heading that the content repeats: each occurrence is
/// checked. Gives the failures in the order of `sections`; empty when the
/// content passes.
pub fn check_sections(
    content: &str,
    sections: &[(String, SectionType)],
    vocabulary: &kairos_plain::Vocabulary,
) -> Vec<SectionFailure> {
    let parsed = parse_sections(content);
    let mut failures = Vec::new();
    for (heading, section_type) in sections {
        let found: Vec<&Section> = parsed.iter().filter(|s| &s.heading == heading).collect();
        let problem = if found.is_empty() {
            Some(SectionProblem::Missing)
        } else if found.iter().any(|s| s.is_empty()) {
            Some(SectionProblem::Empty)
        } else {
            match section_type {
                SectionType::Free => None,
                SectionType::Plain => {
                    let violations: Vec<SectionViolation> = found
                        .iter()
                        .flat_map(|s| plain_violations(&s.body, vocabulary))
                        .collect();
                    (!violations.is_empty()).then_some(SectionProblem::Violations(violations))
                }
                SectionType::Ears => {
                    let violations: Vec<SectionViolation> = found
                        .iter()
                        .flat_map(|s| {
                            as_section_violations(&s.body, kairos_plain::check_ears(&s.body))
                        })
                        .collect();
                    (!violations.is_empty()).then_some(SectionProblem::Violations(violations))
                }
            }
        };
        if let Some(problem) = problem {
            failures.push(SectionFailure {
                heading: heading.clone(),
                section_type: *section_type,
                problem,
            });
        }
    }
    failures
}

/// The typed sections that a write from `old` to `new` changes
/// (KAIROS-T-0730 n-4): a section whose text (each occurrence, trimmed) is
/// the same in the two is left out. A section that is missing in the two is
/// not a change. An edit checks only these; a create checks each section.
pub fn changed_sections(
    old: &str,
    new: &str,
    sections: &[(String, SectionType)],
) -> Vec<(String, SectionType)> {
    let (old, new) = (parse_sections(old), parse_sections(new));
    let bodies = |parsed: &[Section], heading: &str| -> Vec<String> {
        parsed
            .iter()
            .filter(|s| s.heading == heading)
            .map(|s| s.body.trim().to_string())
            .collect()
    };
    sections
        .iter()
        .filter(|(heading, _)| bodies(&old, heading) != bodies(&new, heading))
        .cloned()
        .collect()
}

/// The violations of one `plain` text, in the order of the text.
fn plain_violations(body: &str, vocabulary: &kairos_plain::Vocabulary) -> Vec<SectionViolation> {
    let mut found = kairos_plain::check_text(body);
    found.extend(vocabulary.check_text(body));
    found.sort_by_key(|v| (v.span.start, v.span.end));
    as_section_violations(body, found)
}

/// The violations of the checker as violations of a section: the text that
/// each one is about, and its message as the fix.
fn as_section_violations(body: &str, found: Vec<kairos_plain::Violation>) -> Vec<SectionViolation> {
    found
        .into_iter()
        .map(|v| SectionViolation {
            rule: v.rule,
            text: body
                .get(v.span.clone())
                .unwrap_or_default()
                .trim()
                .to_string(),
            fix: v.message,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK: &str = "Intro text.\n\
        \n\
        ## Goal\n\
        Make it work.\n\
        \n\
        ## Acceptance criteria\n\
        - One.\n\
        ### Edge cases\n\
        - Two.\n\
        \n\
        ## Notes\n";

    fn typed() -> Vec<(String, SectionType)> {
        vec![
            ("Goal".to_string(), SectionType::Plain),
            ("Notes".to_string(), SectionType::Free),
        ]
    }

    fn small_vocabulary() -> kairos_plain::Vocabulary {
        kairos_plain::Vocabulary::new(["the", "server", "checks", "text"], Vec::<&str>::new())
    }

    #[test]
    fn only_the_sections_that_a_write_changes_are_checked() {
        let old = "## Goal\nOld goal.\n\n## Notes\nA note.\n";
        // Goal changed, Notes the same (white space only).
        let new = "## Goal\nNew goal.\n\n## Notes\nA note.\n\n";
        assert_eq!(
            changed_sections(old, new, &typed()),
            vec![("Goal".to_string(), SectionType::Plain)]
        );
        // Missing in the two: not a change. Added: a change.
        let typed = vec![("Risks".to_string(), SectionType::Free)];
        assert_eq!(changed_sections(old, new, &typed), vec![]);
        let added = format!("{new}## Risks\nNone.\n");
        assert_eq!(changed_sections(old, &added, &typed), typed);
        // Removed: a change.
        assert_eq!(
            changed_sections(&added, new, &typed),
            vec![("Risks".to_string(), SectionType::Free)]
        );
    }

    #[test]
    fn content_that_passes_gives_no_failure() {
        let content = "## Goal\nThe server checks the text.\n\n## Notes\nAnything, utilize it.\n";
        assert_eq!(
            check_sections(content, &typed(), &small_vocabulary()),
            vec![]
        );
    }

    #[test]
    fn a_missing_and_an_empty_section_are_named() {
        let content = "## Notes\n\n## Other\ntext\n";
        let failures = check_sections(content, &typed(), &small_vocabulary());
        assert_eq!(
            failures,
            vec![
                SectionFailure {
                    heading: "Goal".into(),
                    section_type: SectionType::Plain,
                    problem: SectionProblem::Missing,
                },
                SectionFailure {
                    heading: "Notes".into(),
                    section_type: SectionType::Free,
                    problem: SectionProblem::Empty,
                },
            ]
        );
    }

    #[test]
    fn a_plain_section_names_each_violation() {
        let content = "## Goal\nThe text is checked by the server. The server utilize the text.\n\n## Notes\nfree text\n";
        let failures = check_sections(content, &typed(), &small_vocabulary());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].heading, "Goal");
        let SectionProblem::Violations(violations) = &failures[0].problem else {
            panic!("expected violations, got {:?}", failures[0].problem);
        };
        let rules: Vec<&str> = violations.iter().map(|v| v.rule).collect();
        let passive = violations
            .iter()
            .find(|v| v.fix.starts_with("Passive"))
            .unwrap();
        assert_eq!(passive.text, "is checked");
        assert!(rules.contains(&"STE-V1"), "{violations:?}");
        let v1 = violations.iter().find(|v| v.text == "utilize").unwrap();
        assert_eq!(v1.rule, "STE-V1");
        assert_eq!(v1.text, "utilize");
        assert!(v1.fix.contains("not in the vocabulary"), "{}", v1.fix);
    }

    #[test]
    fn a_free_section_gets_no_language_check() {
        let content =
            "## Goal\nThe server checks the text.\n## Notes\nWe utilize words nobody listed.\n";
        assert!(check_sections(content, &typed(), &small_vocabulary()).is_empty());
    }

    #[test]
    fn each_occurrence_of_a_repeated_heading_is_checked() {
        let content = "## Goal\nThe server checks the text.\n## Notes\nx\n## Goal\n\n";
        let failures = check_sections(content, &typed(), &small_vocabulary());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].problem, SectionProblem::Empty);
    }

    #[test]
    fn the_standard_vocabulary_knows_plain_words() {
        let vocabulary = kairos_plain::Vocabulary::standard(Vec::<&str>::new());
        let content = "## Goal\nWrite the item.\n## Notes\nx\n";
        assert_eq!(check_sections(content, &typed(), &vocabulary), vec![]);
    }

    #[test]
    fn an_ears_section_names_each_requirement_that_fits_no_frame() {
        let sections = vec![("Requirements".to_string(), SectionType::Ears)];
        let content = "## Requirements\n\
            - When a write fails, the server shall name the field.\n\
            - The server refuses the write.\n";
        let failures = check_sections(content, &sections, &small_vocabulary());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].section_type, SectionType::Ears);
        let SectionProblem::Violations(violations) = &failures[0].problem else {
            panic!("expected violations, got {:?}", failures[0].problem);
        };
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "EARS-1");
        assert_eq!(violations[0].text, "The server refuses the write.");
        assert!(violations[0].fix.contains("Closest frame: ubiquitous"));
        // Words inside the frame are free: no vocabulary check.
        let content = "## Requirements\nThe server shall utilize the cache.\n";
        assert!(check_sections(content, &sections, &small_vocabulary()).is_empty());
    }

    #[test]
    fn known_types_parse_and_round_trip() {
        for t in SectionType::ALL {
            assert_eq!(t.as_str().parse::<SectionType>(), Ok(t));
            assert_eq!(t.to_string(), t.as_str());
        }
        assert_eq!(SectionType::names(), vec!["free", "plain", "ears"]);
    }

    #[test]
    fn an_unknown_type_is_refused_and_named() {
        for value in ["ace", "Plain", " plain", "EARS", "", "markdown"] {
            let error = value.parse::<SectionType>().unwrap_err();
            assert_eq!(error, UnknownSectionType(value.to_string()));
            assert!(error.to_string().contains(&format!("{value:?}")));
        }
    }

    #[test]
    fn headings_are_checked_and_trimmed() {
        assert_eq!(normalize_heading("  Goal "), Ok("Goal".to_string()));
        assert_eq!(normalize_heading("   "), Err(HeadingError::Empty));
        assert!(matches!(
            normalize_heading("## Goal"),
            Err(HeadingError::Hashes(_))
        ));
        assert!(matches!(
            normalize_heading("Goal\nMore"),
            Err(HeadingError::LineBreak(_))
        ));
    }

    #[test]
    fn sections_are_found_by_heading() {
        let sections = parse_sections(TASK);
        let names: Vec<(&str, u8)> = sections
            .iter()
            .map(|s| (s.heading.as_str(), s.level))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Goal", 2),
                ("Acceptance criteria", 2),
                ("Edge cases", 3),
                ("Notes", 2)
            ]
        );
        assert_eq!(sections[0].body.trim(), "Make it work.");
        assert_eq!(sections[0].line, 2);
    }

    #[test]
    fn a_missing_section_gives_none() {
        assert!(sections_named(TASK, "Risks").is_empty());
        assert!(sections_named("", "Goal").is_empty());
        assert!(sections_named("No headings at all.", "Goal").is_empty());
        // The match is on the exact text.
        assert!(sections_named(TASK, "goal").is_empty());
    }

    #[test]
    fn an_empty_section_is_found_and_is_empty() {
        let notes = sections_named(TASK, "Notes");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].is_empty());
        let blank = sections_named("## Goal\n\n   \n## Next\ntext", "Goal");
        assert_eq!(blank.len(), 1);
        assert!(blank[0].is_empty());
        // A heading with no text is a section with an empty heading.
        assert_eq!(parse_sections("#\nbody")[0].heading, "");
    }

    #[test]
    fn a_repeated_heading_gives_each_section() {
        let content = "## Goal\nfirst\n## Other\nx\n### Goal\nsecond\n";
        let goals = sections_named(content, "Goal");
        assert_eq!(goals.len(), 2);
        assert_eq!(goals[0].body.trim(), "first");
        assert_eq!(goals[1].body.trim(), "second");
        assert_eq!(goals[1].level, 3);
    }

    #[test]
    fn a_nested_heading_is_in_the_body_of_its_parent() {
        let criteria = sections_named(TASK, "Acceptance criteria");
        assert_eq!(criteria.len(), 1);
        assert_eq!(criteria[0].body, "- One.\n### Edge cases\n- Two.\n");
        // The nested section is also a section of its own.
        let edge = sections_named(TASK, "Edge cases");
        assert_eq!(edge[0].body, "- Two.\n");
        // A heading of a higher level ends the section.
        let content = "### Deep\nd\n# Top\nt\n";
        assert_eq!(sections_named(content, "Deep")[0].body, "d");
    }

    #[test]
    fn headings_in_code_fences_are_not_headings() {
        let content = "## Goal\n```\n## Not a heading\n```\ntext\n~~~md\n# No\n~~~\n## End\n";
        let names: Vec<String> = parse_sections(content)
            .into_iter()
            .map(|s| s.heading)
            .collect();
        assert_eq!(names, vec!["Goal", "End"]);
        assert!(
            sections_named(content, "Goal")[0]
                .body
                .contains("## Not a heading")
        );
    }

    #[test]
    fn heading_forms() {
        assert_eq!(parse_sections("####### Seven").len(), 0);
        assert_eq!(parse_sections("#NoSpace").len(), 0);
        assert_eq!(parse_sections("    # Indented code").len(), 0);
        assert_eq!(parse_sections("   ## Indented 3").len(), 1);
        assert_eq!(parse_sections("## Closed ##")[0].heading, "Closed");
        assert_eq!(parse_sections("## Goal\r\nx\r\n")[0].heading, "Goal");
    }
}
