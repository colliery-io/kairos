//! Does the Rust chunker agree with the one that produced the measurement?
//! (KAIROS-T-0190.)
//!
//! `kairos_core::chunk`'s module docs quote figures from 4,927 real Metis
//! documents — 63,270 chunks, 2.2% of sections needing the window fallback,
//! median 9 sections per document — and claim the Rust code is a faithful port of
//! `scripts/corpus/extract.py`, so that those numbers describe *this* code. That
//! is a checkable claim, and this checks it.
//!
//! It is **skipped unless `KAIROS_CORPUS_ROOT` is set**, because the corpus is a
//! directory of unrelated repositories on a developer's machine, not something
//! the repo can carry. Run it deliberately:
//!
//! ```sh
//! KAIROS_CORPUS_ROOT=~/Desktop cargo test -p kairos-core --test corpus_agreement -- --nocapture
//! ```
//!
//! A skipped test proves nothing, which is why the figures it guards are also
//! written down in KAIROS-T-0190's status updates with the date they were taken.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kairos_core::chunk::{ChunkConfig, chunk, chunk_with};

/// What `scripts/corpus/extract.py` reported on 2026-09-24, for provenance:
/// 4,927 documents, 60,116 sections, 63,270 chunks, 1,308 windowed, median 9
/// sections per document, and zero documents without headings.
///
/// **This test does not assert those counts exactly, and must not.** The corpus is
/// live: it is nineteen working repositories, and writing a single Metis document
/// changes it. Running this test the same afternoon the figures were taken already
/// gave 4,928 documents and 63,303 chunks — because the session had authored
/// KAIROS-T-0194 and appended status updates to several others in the meantime.
/// One document of drift is not a chunker disagreement, and a test that reported it
/// as one would be retired within a week.
///
/// So what is asserted is what the claims in `kairos_core::chunk`'s docs actually
/// rest on: that NO document lacks headings, that the window fallback stays a
/// small minority, and that documents have roughly this many sections. Those hold
/// across drift; exact counts do not.
const WINDOWED_FRACTION: f64 = 0.022;
const WINDOWED_TOLERANCE: f64 = 0.008;

fn corpus_root() -> Option<PathBuf> {
    let raw = std::env::var("KAIROS_CORPUS_ROOT").ok()?;
    let expanded = if let Some(rest) = raw.strip_prefix("~/") {
        PathBuf::from(std::env::var("HOME").ok()?).join(rest)
    } else {
        PathBuf::from(raw)
    };
    expanded.is_dir().then_some(expanded)
}

/// Every Metis document under `root`, matching the Python extractor's rules:
/// only each repository's own top-level `.metis`, and deduplicated by content —
/// git worktrees under `.claude/worktrees/` and vendored `prior-art` trees carry
/// full copies, which contributed 3,774 phantom documents to the first run.
///
/// The dedupe key is the concatenation of the document's SECTION TEXTS, not its
/// raw body, because that is what the Python extractor hashed. Raw bodies are a
/// stricter key — two documents differing only in a heading would be distinct —
/// and using one here made this test report 5,176 documents against Python's
/// 4,927 on the first run, which was a difference in the comparison rather than
/// in the chunker.
fn documents(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let Ok(repos) = std::fs::read_dir(root) else {
        return out;
    };
    let mut repos: Vec<PathBuf> = repos.flatten().map(|r| r.path()).collect();
    repos.sort();
    for repo in repos {
        let metis = repo.join(".metis");
        if !metis.is_dir() {
            continue;
        }
        let mut stack = vec![metis];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "md") {
                    let Ok(text) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    // Only documents with a `level:` in their frontmatter, as the
                    // extractor did — MEMORY.md, code-index.md and the like are
                    // not work items.
                    let Some((front, body)) = split_frontmatter(&text) else {
                        continue;
                    };
                    if !front.lines().any(|l| l.starts_with("level:")) {
                        continue;
                    }
                    let key: String = sections(body).into_iter().map(|s| s.text).collect();
                    if seen.insert(key) {
                        out.push(body.to_string());
                    }
                }
            }
        }
    }
    out
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    Some((&rest[..end], &rest[end + 4..]))
}

/// The document's sections before any windowing.
///
/// Obtained by running the real chunker with the ceiling raised out of the way,
/// so "section" means exactly what the chunker means by it rather than something
/// this test reconstructs by guesswork. The first version of this test tried to
/// infer sections from the windowed output and got the windowed COUNT wrong as a
/// result — it counted extra windows (a section split into three contributing
/// two) where Python counted sections split (contributing one).
fn sections(body: &str) -> Vec<kairos_core::chunk::Chunk> {
    chunk_with(
        body,
        &ChunkConfig {
            max_chars: usize::MAX,
            ..ChunkConfig::default()
        },
    )
}

#[test]
fn the_rust_chunker_agrees_with_the_measured_one() {
    let Some(root) = corpus_root() else {
        eprintln!(
            "SKIPPED: set KAIROS_CORPUS_ROOT to a directory of repositories with \
             .metis trees to check the chunker against the corpus the figures in \
             kairos_core::chunk's docs came from."
        );
        return;
    };

    let docs = documents(&root);
    assert!(
        !docs.is_empty(),
        "no Metis documents found under {}",
        root.display()
    );

    let ceiling = ChunkConfig::default().max_chars;
    let mut chunks = 0usize;
    let mut section_count = 0usize;
    let mut windowed = 0usize;
    let mut no_headings = 0usize;
    let mut sections_per_doc: Vec<usize> = Vec::new();

    for body in &docs {
        chunks += chunk(body).len();
        let secs = sections(body);
        section_count += secs.len();
        sections_per_doc.push(secs.len());
        // A section longer than the ceiling is one windowed section, however many
        // windows it becomes — which is what the Python extractor counted.
        windowed += secs.iter().filter(|s| s.len_chars() > ceiling).count();
        if !secs.is_empty() && secs.iter().all(|s| s.heading.is_none()) {
            no_headings += 1;
        }
    }

    sections_per_doc.sort_unstable();
    let median = sections_per_doc[sections_per_doc.len() / 2];

    eprintln!(
        "corpus under {}: {} documents, {} sections, {} chunks, {} windowed ({:.1}%), \
         median {} sections/doc, {} documents with no headings",
        root.display(),
        docs.len(),
        section_count,
        chunks,
        windowed,
        100.0 * windowed as f64 / section_count as f64,
        median,
        no_headings
    );

    // The claim that matters most, and the one that needs no arithmetic: rule 3
    // rests on heading-boundary chunking being universally applicable, and it is
    // only universal if this is zero.
    assert_eq!(
        no_headings,
        0,
        "the docs claim ZERO of {} documents lack headings; {no_headings} do",
        docs.len()
    );

    // Enough corpus to be talking about a distribution rather than a handful.
    assert!(
        docs.len() > 4_000,
        "expected the nineteen-repository corpus, found {} documents under {} \
         — is KAIROS_CORPUS_ROOT pointing at the right directory?",
        docs.len(),
        root.display()
    );

    // The window fallback is the exception, which is what makes heading
    // boundaries the mechanism rather than a hint. Measured at 2.2%.
    let fraction = windowed as f64 / section_count as f64;
    assert!(
        (fraction - WINDOWED_FRACTION).abs() < WINDOWED_TOLERANCE,
        "the docs claim about {:.1}% of sections need windowing; this run says \
         {:.1}% ({windowed} of {section_count})",
        100.0 * WINDOWED_FRACTION,
        100.0 * fraction
    );

    // Documents are made of several sections, so an append re-embeds one of
    // them rather than the document. The docs say the median is 9.
    assert!(
        (8..=10).contains(&median),
        "the docs claim a median of 9 sections per document; this run says {median}"
    );

    // And windowing adds few chunks overall: sections are the unit, windows are
    // the rare correction.
    let ratio = chunks as f64 / section_count as f64;
    assert!(
        (1.0..1.15).contains(&ratio),
        "chunks per section should be barely above 1; got {ratio:.3}"
    );
}
