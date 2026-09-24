//! Splitting an item's content for embedding (KAIROS-A-0021 rule 3,
//! KAIROS-T-0190).
//!
//! Pure, per KAIROS-A-0009: text in, chunks out, no I/O and no model. The
//! provider ([`kairos-embed`](https://docs.rs/kairos-embed)) turns these into
//! vectors; the store (KAIROS-T-0187's `item_chunks`) keeps them.
//!
//! # Headings are anchors, not labels
//!
//! Rule 3, and the measurement behind it is the whole argument. Across **4,927
//! real Metis documents from nineteen repositories**:
//!
//! - **Zero** documents have no headings at all. Not few — none.
//! - Median 9 sections per document; section text p50 205 characters, p90 1,140.
//! - **2.2%** of sections exceed the size ceiling and need the sliding-window
//!   fallback, so heading boundaries handle 97.8% cleanly on their own.
//!
//! And the other half of the measurement is why nothing may key on a heading's
//! *name*: there were **11,553 distinct heading strings across 63,270 headings,
//! 85% of them occurring exactly once**, and the tenth most common heading was
//! `Parent Initiative **[CONDITIONAL: Assigned Task]**` — 1,439 documents that
//! never deleted the template marker. So [`Chunk::heading`] exists so a citation
//! can echo *where* a match was, and for no other purpose. There is deliberately
//! no function here that recognises a section.
//!
//! This is a faithful port of the chunker used for that measurement
//! (`scripts/corpus/extract.py`), so the numbers above describe this code rather
//! than something resembling it.
//!
//! # Offsets are character offsets
//!
//! [`Chunk::char_start`] and [`Chunk::char_end`] count **Unicode scalar values**
//! from the start of the content, not bytes. Bytes would be meaningless to any
//! reader of a citation, and byte offsets into UTF-8 cannot be handed to a
//! front end without translation. The caveat worth knowing: JavaScript string
//! indices are UTF-16 code units, so a GUI highlighting a range containing
//! astral-plane characters needs to convert rather than index directly.

/// One chunk of an item's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Position within the item, 0-based and contiguous.
    pub ordinal: usize,
    /// The literal heading text this chunk sits under, or `None` when it sits
    /// under none — content before the first heading, or a document with no
    /// headings at all.
    ///
    /// **Its value carries no meaning to any code here.** See the module docs.
    pub heading: Option<String>,
    /// Character offset of the chunk's first character within the content.
    pub char_start: usize,
    /// Character offset one past the chunk's last character.
    pub char_end: usize,
    /// The chunk's text.
    pub text: String,
}

impl Chunk {
    /// Length in characters.
    pub fn len_chars(&self) -> usize {
        self.char_end - self.char_start
    }
}

/// Chunking sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkConfig {
    /// A section longer than this is split by sliding window.
    ///
    /// 2,000 characters: measured, 94% of real sections are already under it, so
    /// the fallback is the exception rather than the rule.
    pub max_chars: usize,
    /// Window size when falling back.
    pub window: usize,
    /// How much consecutive windows overlap, so a sentence spanning a boundary
    /// is whole in at least one of them.
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_chars: 2_000,
            window: 1_200,
            overlap: 200,
        }
    }
}

impl ChunkConfig {
    /// The distance between window starts.
    fn stride(&self) -> usize {
        // A window that does not advance would loop forever, and an overlap at
        // or beyond the window size is a configuration mistake rather than a
        // request for infinite chunks.
        self.window.saturating_sub(self.overlap).max(1)
    }
}

/// Split `content` on heading boundaries, with a sliding-window fallback for
/// sections that are too long and for content with no headings.
pub fn chunk(content: &str) -> Vec<Chunk> {
    chunk_with(content, &ChunkConfig::default())
}

/// [`chunk`] with explicit sizes.
pub fn chunk_with(content: &str, config: &ChunkConfig) -> Vec<Chunk> {
    let mut sections = split_sections(content);
    // Drop sections whose body is only whitespace: a heading immediately
    // followed by another heading contributes nothing to embed, and an empty
    // vector is worse than no vector — it is a row that matches everything
    // equally badly.
    sections.retain(|s| !s.text.trim().is_empty());

    let mut out = Vec::new();
    for section in sections {
        let chars: Vec<char> = section.text.chars().collect();
        if chars.len() <= config.max_chars {
            push(&mut out, section.heading.clone(), section.start, section.text);
            continue;
        }
        let stride = config.stride();
        let mut i = 0;
        while i < chars.len() {
            let end = (i + config.window).min(chars.len());
            let window: String = chars[i..end].iter().collect();
            if !window.trim().is_empty() {
                push(&mut out, section.heading.clone(), section.start + i, window);
            }
            if end == chars.len() {
                break;
            }
            i += stride;
        }
    }
    out
}

fn push(out: &mut Vec<Chunk>, heading: Option<String>, start: usize, text: String) {
    let len = text.chars().count();
    out.push(Chunk {
        ordinal: out.len(),
        heading,
        char_start: start,
        char_end: start + len,
        text,
    });
}

struct Section {
    heading: Option<String>,
    start: usize,
    text: String,
}

/// Split into sections at ATX heading lines, keeping each section's start offset.
fn split_sections(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut heading: Option<String> = None;
    let mut body = String::new();
    let mut body_start = 0usize;
    let mut offset = 0usize;

    for line in split_keeping_newlines(content) {
        let line_chars = line.chars().count();
        if let Some(text) = heading_text(line) {
            sections.push(Section {
                heading: heading.take(),
                start: body_start,
                text: std::mem::take(&mut body),
            });
            heading = Some(text);
            // The section's text begins after the heading line, so a citation's
            // range covers the prose rather than the heading that names it.
            body_start = offset + line_chars;
        } else {
            body.push_str(line);
        }
        offset += line_chars;
    }
    sections.push(Section {
        heading,
        start: body_start,
        text: body,
    });
    sections
}

/// Lines with their terminators kept, so character offsets stay exact.
fn split_keeping_newlines(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, c) in content.char_indices() {
        if c == '\n' {
            lines.push(&content[start..i + 1]);
            start = i + 1;
        }
    }
    if start < content.len() {
        lines.push(&content[start..]);
    }
    lines
}

/// The text of an ATX heading line, or `None` if the line is not one.
///
/// ATX only (`#` through `######`). Setext headings (a line underlined with
/// `===` or `---`) are not recognised, deliberately: `---` is also a thematic
/// break and a YAML front-matter fence, and guessing wrong would split a
/// document at a horizontal rule. The measured corpus is entirely ATX.
fn heading_text(line: &str) -> Option<String> {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    // `#foo` is not a heading — ATX requires a space after the hashes. An empty
    // `#` line is a heading with no text.
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    // Trailing hashes are a closing sequence, not content.
    Some(rest.trim().trim_end_matches('#').trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headings(chunks: &[Chunk]) -> Vec<Option<&str>> {
        chunks.iter().map(|c| c.heading.as_deref()).collect()
    }

    /// Offsets must point back into the original content exactly, or a citation
    /// sends a reader to the wrong place.
    fn assert_offsets_are_exact(content: &str, chunks: &[Chunk]) {
        let chars: Vec<char> = content.chars().collect();
        for c in chunks {
            let slice: String = chars[c.char_start..c.char_end].iter().collect();
            assert_eq!(
                slice, c.text,
                "chunk {} at {}..{} does not match the content it points at",
                c.ordinal, c.char_start, c.char_end
            );
        }
    }

    #[test]
    fn ordinals_are_contiguous_from_zero() {
        let chunks = chunk("# A\nalpha\n\n# B\nbeta\n\n# C\ngamma\n");
        assert_eq!(
            chunks.iter().map(|c| c.ordinal).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn sections_split_on_headings_and_keep_the_literal_text() {
        let content = "# Objective\nDo the thing.\n\n## Acceptance Criteria\nIt works.\n";
        let chunks = chunk(content);
        assert_eq!(
            headings(&chunks),
            vec![Some("Objective"), Some("Acceptance Criteria")]
        );
        assert_eq!(chunks[0].text.trim(), "Do the thing.");
        assert_eq!(chunks[1].text.trim(), "It works.");
        assert_offsets_are_exact(content, &chunks);
    }

    /// The template markers the corpus is full of must survive verbatim — 1,439
    /// real documents have them, and a citation that silently tidied them would
    /// not match what the reader sees.
    #[test]
    fn template_markers_in_headings_are_preserved_verbatim() {
        let content = "## Parent Initiative **[CONDITIONAL: Assigned Task]**\nKAIROS-I-0017\n";
        let chunks = chunk(content);
        assert_eq!(
            chunks[0].heading.as_deref(),
            Some("Parent Initiative **[CONDITIONAL: Assigned Task]**")
        );
    }

    #[test]
    fn content_before_the_first_heading_has_no_heading() {
        let content = "A preamble sentence.\n\n# First\nbody\n";
        let chunks = chunk(content);
        assert_eq!(headings(&chunks), vec![None, Some("First")]);
        assert_eq!(chunks[0].text.trim(), "A preamble sentence.");
        assert_eq!(chunks[0].char_start, 0);
        assert_offsets_are_exact(content, &chunks);
    }

    /// The rule-3 fallback. Measured at 2.2% of real sections.
    #[test]
    fn content_with_no_headings_at_all_is_windowed() {
        let content = "x".repeat(3_000);
        let chunks = chunk(&content);
        assert!(chunks.len() > 1, "a long headingless document is split");
        assert!(chunks.iter().all(|c| c.heading.is_none()));
        assert_offsets_are_exact(&content, &chunks);
    }

    #[test]
    fn a_short_headingless_document_is_one_chunk() {
        let chunks = chunk("Just a sentence.");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, None);
        assert_eq!(chunks[0].char_start, 0);
        assert_eq!(chunks[0].char_end, 16);
    }

    #[test]
    fn an_oversized_section_is_windowed_but_keeps_its_heading() {
        let content = format!("# Long\n{}\n", "y".repeat(3_000));
        let chunks = chunk(&content);
        assert!(chunks.len() > 1);
        assert!(
            chunks.iter().all(|c| c.heading.as_deref() == Some("Long")),
            "every window cites the heading it came from"
        );
        assert_offsets_are_exact(&content, &chunks);
    }

    /// Windows overlap so a sentence crossing a boundary is intact somewhere.
    #[test]
    fn windows_overlap_by_the_configured_amount() {
        let config = ChunkConfig {
            max_chars: 10,
            window: 10,
            overlap: 4,
        };
        let content: String = ('a'..='z').collect();
        let chunks = chunk_with(&content, &config);
        assert!(chunks.len() > 1);
        for pair in chunks.windows(2) {
            let overlap = pair[0].char_end.saturating_sub(pair[1].char_start);
            assert_eq!(overlap, 4, "consecutive windows overlap");
        }
        assert_offsets_are_exact(&content, &chunks);
    }

    /// The last window must not be dropped, and must not run past the end.
    #[test]
    fn the_final_window_ends_exactly_at_the_end() {
        let config = ChunkConfig {
            max_chars: 10,
            window: 10,
            overlap: 3,
        };
        let content: String = "z".repeat(25);
        let chunks = chunk_with(&content, &config);
        let last = chunks.last().unwrap();
        assert_eq!(last.char_end, 25);
        assert_offsets_are_exact(&content, &chunks);
    }

    /// A pathological configuration must terminate rather than allocate forever.
    #[test]
    fn an_overlap_at_least_as_big_as_the_window_still_terminates() {
        let config = ChunkConfig {
            max_chars: 4,
            window: 4,
            overlap: 99,
        };
        let chunks = chunk_with(&"q".repeat(40), &config);
        assert!(!chunks.is_empty());
        assert!(chunks.len() <= 40, "it advances: {} chunks", chunks.len());
    }

    #[test]
    fn empty_sections_are_dropped() {
        // A heading immediately followed by another heading has nothing to embed.
        let chunks = chunk("# A\n\n# B\n\n# C\nonly C has a body\n");
        assert_eq!(headings(&chunks), vec![Some("C")]);
    }

    #[test]
    fn empty_and_whitespace_content_produce_no_chunks() {
        assert!(chunk("").is_empty());
        assert!(chunk("   \n\n\t\n").is_empty());
    }

    #[test]
    fn all_six_heading_levels_are_recognised() {
        for level in 1..=6 {
            let hashes = "#".repeat(level);
            let chunks = chunk(&format!("{hashes} Title\nbody\n"));
            assert_eq!(
                chunks[0].heading.as_deref(),
                Some("Title"),
                "level {level}"
            );
        }
    }

    #[test]
    fn seven_hashes_is_not_a_heading() {
        let chunks = chunk("####### Not a heading\nbody\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, None);
    }

    /// `#hashtag` is not a heading. Without the space rule, a line mentioning an
    /// issue number would split the document.
    #[test]
    fn hashes_without_a_space_are_not_a_heading() {
        let chunks = chunk("#1234 is the issue\nbody\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, None);
    }

    #[test]
    fn a_closing_hash_sequence_is_not_part_of_the_heading() {
        let chunks = chunk("## Objective ##\nbody\n");
        assert_eq!(chunks[0].heading.as_deref(), Some("Objective"));
    }

    /// `---` is a thematic break and a front-matter fence as often as it is a
    /// setext underline, so it must not split anything.
    #[test]
    fn setext_underlines_and_thematic_breaks_do_not_split() {
        let content = "Title\n=====\nbody\n\n---\n\nmore body\n";
        let chunks = chunk(content);
        assert_eq!(chunks.len(), 1, "no ATX heading means one chunk");
        assert_eq!(chunks[0].heading, None);
    }

    /// Offsets are characters, not bytes: a citation into a document with
    /// accents or emoji has to land in the right place.
    #[test]
    fn offsets_count_characters_not_bytes() {
        let content = "# Héading ✅\ncafé — naïve\n";
        let chunks = chunk(content);
        assert_eq!(chunks[0].heading.as_deref(), Some("Héading ✅"));
        assert_offsets_are_exact(content, &chunks);
        // The body is "café — naïve\n": 13 characters, 17 bytes.
        assert_eq!(chunks[0].len_chars(), 13, "characters, not the 17 bytes");
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        let content = "# A\r\nalpha\r\n# B\r\nbeta\r\n";
        let chunks = chunk(content);
        assert_eq!(headings(&chunks), vec![Some("A"), Some("B")]);
        assert_offsets_are_exact(content, &chunks);
    }

    /// The shape the measurement found: every real document has headings, most
    /// sections are small, and the whole thing round-trips.
    #[test]
    fn a_realistic_document_chunks_the_way_the_corpus_did() {
        let content = "\
## Parent Initiative

[[KAIROS-I-0017]]

## Objective

Somewhere to put vectors.

## Implementation Notes

### Technical Approach

Two tables rather than one column.

### Dependencies

None.

## Status Updates

*To be added during implementation*
";
        let chunks = chunk(content);
        assert_eq!(
            headings(&chunks),
            vec![
                Some("Parent Initiative"),
                Some("Objective"),
                Some("Technical Approach"),
                Some("Dependencies"),
                Some("Status Updates"),
            ],
            "`## Implementation Notes` has no body of its own, so it is dropped"
        );
        assert!(chunks.iter().all(|c| c.len_chars() <= 2_000));
        assert_offsets_are_exact(content, &chunks);
    }
}
