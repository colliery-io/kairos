//! KAIROS-T-0325: the text of the GUI names no ADR and no ticket. A reader
//! of a page does not know codes such as `A-0003` or `KAIROS-T-0042`;
//! they belong in comments, commits and the docs.
//!
//! The test reads each source file of the crate, skips comments and test
//! modules, and fails on a string literal that cites such a code. Example
//! short codes that show a FORMAT (`PLATFORM-T-0001`, `DEMO-S-0001`) are
//! not references, and they pass.

use std::path::{Path, PathBuf};

/// Is `token` a reference to an ADR or a ticket: `KAIROS-T-0042`,
/// `COLLIERY-A-0023`, a bare `A-0003`, `T-0080`, `S-0005`, `I-0011`, or
/// `ADR-20`?
fn is_reference(token: &str) -> bool {
    let digits = |s: &str, min: usize| s.len() >= min && s.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<&str> = token.split('-').collect();
    match parts.as_slice() {
        [prefix, letter, number] if ["KAIROS", "COLLIERY"].contains(prefix) => {
            letter.len() == 1 && letter.bytes().all(|b| b.is_ascii_uppercase()) && digits(number, 3)
        }
        [letter, number] if ["A", "T", "S", "I"].contains(letter) => digits(number, 4),
        ["ADR", number] => digits(number, 1),
        _ => false,
    }
}

/// The references in one string literal.
fn references(literal: &str) -> Vec<String> {
    literal
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .map(|token| token.trim_matches('-'))
        .filter(|token| is_reference(token))
        .map(str::to_string)
        .collect()
}

/// The string literals of Rust source, outside comments, as
/// `(line, text)`. A small lexer: line and block comments, strings with
/// escapes, and char literals (so `'"'` does not open a string).
fn string_literals(source: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '\n' => line += 1,
            '/' if next == Some('/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if next == Some('*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 2;
                continue;
            }
            '\'' if next == Some('\\') || chars.get(i + 2) == Some(&'\'') => {
                // A char literal: skip to its closing quote.
                i += 2;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => {
                let start = line;
                let mut text = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    if chars.get(i) == Some(&'\n') {
                        line += 1;
                    }
                    if let Some(&ch) = chars.get(i) {
                        text.push(ch);
                    }
                    i += 1;
                }
                out.push((start, text));
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read the source directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_text_of_the_gui_names_no_adr_and_no_ticket() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();
    let mut found = Vec::new();
    for file in files {
        if file.ends_with("ui_text_guard.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&file).expect("read a source file");
        // The test module of a file is at its end; its strings are fixtures.
        let source = source.split("#[cfg(test)]").next().unwrap_or_default();
        for (line, text) in string_literals(source) {
            let refs = references(&text);
            if !refs.is_empty() {
                found.push(format!(
                    "{}:{line}: {}",
                    file.strip_prefix(&root).unwrap_or(&file).display(),
                    refs.join(", ")
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "GUI text cites an ADR or a ticket. Say what it means, and keep the code in a comment:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn a_reference_is_told_from_an_example_code() {
    for reference in [
        "KAIROS-T-0042",
        "COLLIERY-A-0023",
        "A-0003",
        "T-0080",
        "S-0005",
        "ADR-20",
    ] {
        assert!(is_reference(reference), "{reference}");
    }
    for other in [
        "PLATFORM-T-0001",
        "DEMO-S-0001",
        "v1",
        "A-1",
        "e-mail",
        "WEB-T-0004",
    ] {
        assert!(!is_reference(other), "{other}");
    }
    assert_eq!(references("typed fields (A-0003)"), ["A-0003"]);
    assert!(references("A short code has the form PLATFORM-T-0001.").is_empty());
}

#[test]
fn the_lexer_skips_comments_and_char_literals() {
    let source =
        "// \"A-0001\" in a comment\nlet q = '\"';\n/* \"T-0001\" */ let s = \"real (A-0003)\";\n";
    let literals = string_literals(source);
    assert_eq!(literals, vec![(3, "real (A-0003)".to_string())]);
}
