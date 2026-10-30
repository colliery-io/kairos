//! Dictionaries (KAIROS-T-0733, COLLIERY-I-0664 decision 6, KAIROS-A-0079).
//!
//! A dictionary is a document. Its entries are the rows of a markdown table
//! with the columns `Word | Part of speech | Meaning`: the shape of the
//! vendored word list. [`parse_dictionary`] reads the table, and gives each
//! bad row as a [`DictionaryProblem`] with its line. An organization has one
//! dictionary (KAIROS-A-0079): there is no team layer to put on top of it.
//!
//! The terms go to [`crate::Vocabulary::new`] as its `terms`. A term of more
//! than one word is matched longest first there.

use std::collections::BTreeMap;

/// The header of the table, in lower case.
const COLUMNS: [&str; 3] = ["word", "part of speech", "meaning"];

/// One row of a dictionary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DictionaryEntry {
    /// The term: one word or more, as written.
    pub word: String,
    pub part_of_speech: String,
    pub meaning: String,
    /// The 1-based line of the row in the document.
    pub line: usize,
}

/// A row that is not an entry, or a document with no table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DictionaryProblem {
    /// The 1-based line in the document.
    pub line: usize,
    pub message: String,
}

/// What [`parse_dictionary`] read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedDictionary {
    pub entries: Vec<DictionaryEntry>,
    pub problems: Vec<DictionaryProblem>,
}

/// Read the entries of a dictionary document.
///
/// The first table with the header `Word | Part of speech | Meaning` (case
/// ignored) holds the entries; the text around it is free. Each row of that
/// table must have three cells, and a word, a part of speech and a meaning.
/// A word that is in the table two times (case ignored) is a problem at the
/// second row, and the first row stays. A `\|` in a cell is a `|`.
pub fn parse_dictionary(markdown: &str) -> ParsedDictionary {
    let mut parsed = ParsedDictionary::default();
    let lines: Vec<&str> = markdown.lines().collect();
    let Some(header) = lines.iter().position(|line| {
        is_table_row(line)
            && cells(line)
                .iter()
                .map(|c| c.to_lowercase())
                .eq(COLUMNS.iter().map(|c| c.to_string()))
    }) else {
        parsed.problems.push(DictionaryProblem {
            line: 1,
            message: "The dictionary has no table with the columns Word | Part of speech | \
                      Meaning. Add the table."
                .to_string(),
        });
        return parsed;
    };

    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut rows = lines.iter().enumerate().skip(header + 1);
    match rows.next() {
        Some((_, line)) if is_separator(line) => {}
        _ => {
            parsed.problems.push(DictionaryProblem {
                line: header + 2,
                message: "The line after the header of the table is not a separator row \
                          (|---|---|---|). Add the separator row."
                    .to_string(),
            });
            return parsed;
        }
    }
    for (index, line) in rows {
        if !is_table_row(line) {
            break;
        }
        let number = index + 1;
        let row = cells(line);
        if row.len() != COLUMNS.len() {
            parsed.problems.push(DictionaryProblem {
                line: number,
                message: format!(
                    "The row has {} cells. Give each row 3 cells: word, part of speech and \
                     meaning.",
                    row.len()
                ),
            });
            continue;
        }
        let missing: Vec<&str> = row
            .iter()
            .zip(COLUMNS)
            .filter(|(cell, _)| cell.is_empty())
            .map(|(_, column)| column)
            .collect();
        if !missing.is_empty() {
            parsed.problems.push(DictionaryProblem {
                line: number,
                message: format!("The row has no {}. Fill each cell.", missing.join(" and ")),
            });
            continue;
        }
        let key = key(&row[0]);
        if let Some(first) = seen.get(&key) {
            parsed.problems.push(DictionaryProblem {
                line: number,
                message: format!(
                    "The word \"{}\" is also on the line {first}. Keep one row for each word.",
                    row[0]
                ),
            });
            continue;
        }
        seen.insert(key, number);
        let mut row = row.into_iter();
        parsed.entries.push(DictionaryEntry {
            word: row.next().unwrap_or_default(),
            part_of_speech: row.next().unwrap_or_default(),
            meaning: row.next().unwrap_or_default(),
            line: number,
        });
    }
    parsed
}

/// The key of a word: lower case, with one space between its words.
fn key(word: &str) -> String {
    word.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_table_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn is_separator(line: &str) -> bool {
    let row = cells(line);
    !row.is_empty()
        && row.iter().all(|cell| {
            let dashes = cell.trim_matches(':');
            !dashes.is_empty() && dashes.chars().all(|c| c == '-')
        })
}

/// The cells of a table row, trimmed. The outer pipes are optional at the
/// end; `\|` is a pipe in a cell.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    if !cell.trim().is_empty() {
        cells.push(cell.trim().to_string());
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# Dictionary\n\
        \n\
        Text before the table.\n\
        \n\
        | Word | Part of speech | Meaning |\n\
        |---|---|---|\n\
        | production document | noun | A document that a team uses for its work. |\n\
        | Board | noun | A set of columns \\| cards. |\n\
        \n\
        Text after.\n";

    #[test]
    fn reads_the_rows_of_the_table() {
        let parsed = parse_dictionary(TABLE);
        assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
        assert_eq!(
            parsed.entries,
            [
                DictionaryEntry {
                    word: "production document".into(),
                    part_of_speech: "noun".into(),
                    meaning: "A document that a team uses for its work.".into(),
                    line: 7,
                },
                DictionaryEntry {
                    word: "Board".into(),
                    part_of_speech: "noun".into(),
                    meaning: "A set of columns | cards.".into(),
                    line: 8,
                },
            ]
        );
    }

    #[test]
    fn the_header_ignores_case_and_an_empty_table_is_no_problem() {
        let parsed = parse_dictionary("| word | PART OF SPEECH | Meaning |\n| :-- | --- | --: |\n");
        assert_eq!(parsed, ParsedDictionary::default());
    }

    #[test]
    fn a_bad_row_is_reported_with_its_line() {
        let text = "| Word | Part of speech | Meaning |\n\
            |---|---|---|\n\
            | card | noun | A task on a board. |\n\
            | column | noun |\n\
            | lane | | The kind of work. |\n\
            | Card | noun | Again. |\n\
            | row | noun | A line of a table. |\n";
        let parsed = parse_dictionary(text);
        let words: Vec<&str> = parsed.entries.iter().map(|e| e.word.as_str()).collect();
        assert_eq!(words, ["card", "row"]);
        let lines: Vec<usize> = parsed.problems.iter().map(|p| p.line).collect();
        assert_eq!(lines, [4, 5, 6], "{:?}", parsed.problems);
        assert!(parsed.problems[0].message.contains("2 cells"));
        assert!(parsed.problems[1].message.contains("no part of speech"));
        assert!(parsed.problems[2].message.contains("line 3"));
    }

    #[test]
    fn a_document_with_no_table_is_a_problem() {
        let parsed = parse_dictionary("# Dictionary\n\nNo table.\n");
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.problems.len(), 1);
        assert!(parsed.problems[0].message.contains("no table"));
    }

    #[test]
    fn a_header_with_no_separator_is_a_problem() {
        let parsed = parse_dictionary("| Word | Part of speech | Meaning |\n| a | b | c |\n");
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.problems[0].line, 2);
    }
}
