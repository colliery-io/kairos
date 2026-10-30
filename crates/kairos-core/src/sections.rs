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
/// `ace` (a controlled language for checkable sections) comes later
/// (KAIROS-T-0732). Until then it is an unknown type, and it is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SectionType {
    /// Free text: the section must be there and have text; no language check.
    Free,
    /// Plain language: the text must pass the plain-language checker.
    Plain,
}

impl SectionType {
    /// Each type, in the order of the docs and of the error texts.
    pub const ALL: [SectionType; 2] = [SectionType::Free, SectionType::Plain];

    /// The stored and wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            SectionType::Free => "free",
            SectionType::Plain => "plain",
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
#[error("The section type {0:?} is not known. Use one of: free, plain.")]
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

    #[test]
    fn known_types_parse_and_round_trip() {
        for t in SectionType::ALL {
            assert_eq!(t.as_str().parse::<SectionType>(), Ok(t));
            assert_eq!(t.to_string(), t.as_str());
        }
        assert_eq!(SectionType::names(), vec!["free", "plain"]);
    }

    #[test]
    fn an_unknown_type_is_refused_and_named() {
        for value in ["ace", "Plain", " plain", "", "markdown"] {
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
