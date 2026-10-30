//! The Rust rules give the violations of `scripts/ste-check.py` (KAIROS-T-0727).
//!
//! `tests/fixtures/expected.json` is the output of the Python on
//! `tests/fixtures/inputs.json`; `scripts/ste-parity.py` writes it. Run that
//! script again after a change to either checker.

use std::path::{Path, PathBuf};

use kairos_plain::{Violation, check_code_text, check_text};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    text: String,
    expected: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct Cases {
    prose: Vec<Case>,
    code: Vec<Case>,
}

fn load(path: &Path) -> Cases {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&raw).expect("valid fixture JSON")
}

/// The differences between the Rust and the Python for one text. The rule
/// ids must be the same, in the same order; each Rust message starts with the
/// Python "what" (the first letter aside), then names the fix.
fn differences(kind: &str, case: &Case, found: &[Violation]) -> Option<String> {
    let rules: Vec<&str> = found.iter().map(|v| v.rule).collect();
    let expected: Vec<&str> = case.expected.iter().map(|(r, _)| r.as_str()).collect();
    let same_messages = found
        .iter()
        .zip(&case.expected)
        .all(|(v, (_, what))| v.message.to_lowercase().starts_with(&what.to_lowercase()));
    let spans_ok = found
        .iter()
        .all(|v| case.text.get(v.span.clone()).is_some());
    (rules != expected || !same_messages || !spans_ok).then(|| {
        format!(
            "{kind} {:?}\n  python: {:?}\n  rust:   {:?}",
            case.text,
            case.expected,
            found
                .iter()
                .map(|v| (v.rule, v.message.as_str()))
                .collect::<Vec<_>>()
        )
    })
}

fn check_all(cases: &Cases) -> Vec<String> {
    let prose = cases
        .prose
        .iter()
        .filter_map(|c| differences("prose", c, &check_text(&c.text)));
    let code = cases
        .code
        .iter()
        .filter_map(|c| differences("code", c, &check_code_text(&c.text)));
    prose.chain(code).collect()
}

#[test]
fn the_fixtures_give_the_violations_of_the_python() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/expected.json");
    let cases = load(&path);
    assert!(cases.prose.len() > 20 && cases.code.len() > 20);
    let diffs = check_all(&cases);
    assert!(
        diffs.is_empty(),
        "{} differences:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// The docs pages and the texts of the code of this repository, written by
/// `scripts/ste-parity.py --corpus <file>`. Run with
/// `KAIROS_PLAIN_CORPUS=<file> cargo test -p kairos-plain -- --ignored`.
#[test]
#[ignore = "needs a corpus file from scripts/ste-parity.py --corpus"]
fn the_corpus_gives_the_violations_of_the_python() {
    let path = std::env::var("KAIROS_PLAIN_CORPUS").expect("KAIROS_PLAIN_CORPUS is set");
    let cases = load(Path::new(&path));
    let diffs = check_all(&cases);
    eprintln!(
        "corpus: {} pages, {} code texts, {} differences",
        cases.prose.len(),
        cases.code.len(),
        diffs.len()
    );
    assert!(
        diffs.is_empty(),
        "{} differences:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}
