//! The notes of an item (KAIROS-T-0504, KAIROS-T-0630).
//!
//! Since KAIROS-T-0630 a note is a row of its own (`item_notes`), not a
//! section of the content. Each answer shows the notes after the content as
//! `## Note n-3 (2026-10-15, Robin)` sections, with [`render_note`] and
//! [`render_notes`]. The SQL function `item_notes_markdown` gives the same
//! text for the embedding sweep.
//!
//! [`parse_note_title`] reads a heading of a note. The renderer of the web
//! app gives a note heading in a content its anchor.
//!
//! The parser is here, not in kairos-core, so that the web app (wasm32)
//! and the server share one reading of a heading.

/// The id of the note at `position` (one-based): `n-3`.
pub fn note_id(position: usize) -> String {
    format!("n-{position}")
}

/// One note as markdown (KAIROS-T-0630): the heading, an empty line and
/// the text, then `Promoted to CODE.` after an empty line when a promote
/// made an item from it. A note with no text is the heading only.
pub fn render_note(
    seq: i32,
    date: &str,
    author: &str,
    text: &str,
    promoted_to: Option<&str>,
) -> String {
    let mut out = format!("## Note n-{seq} ({date}, {})", author.trim());
    if !text.is_empty() {
        out.push_str("\n\n");
        out.push_str(text);
    }
    if let Some(code) = promoted_to {
        out.push_str(&format!("\n\nPromoted to {code}."));
    }
    out
}

/// One note for [`render_notes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteParts<'a> {
    pub seq: i32,
    /// `YYYY-MM-DD`, in UTC.
    pub date: &'a str,
    pub author: &'a str,
    pub text: &'a str,
    pub promoted_to: Option<&'a str>,
}

/// The notes as markdown (KAIROS-T-0630): each note after an empty line, in
/// the order given. Empty for no notes. Thus `content + render_notes(…)`
/// is the text that the content and its notes were before the move.
pub fn render_notes<'a>(notes: impl IntoIterator<Item = NoteParts<'a>>) -> String {
    let mut out = String::new();
    for note in notes {
        out.push_str("\n\n");
        out.push_str(&render_note(
            note.seq,
            note.date,
            note.author,
            note.text,
            note.promoted_to,
        ));
    }
    out
}

/// The number of a note id: `n-3` gives 3. `None` for any other text, and
/// for a number that is 0 or too large.
pub fn parse_note_id(id: &str) -> Option<i32> {
    if !is_note_id(id.trim()) {
        return None;
    }
    id.trim()[2..].parse::<i32>().ok().filter(|n| *n > 0)
}

/// The parts of the title of a note heading (the text after `## `).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteTitle<'a> {
    /// The id in the heading, when it has one.
    pub id: Option<&'a str>,
    /// The date, as written.
    pub date: &'a str,
    /// The author, as written.
    pub author: &'a str,
}

/// Parse the title of a heading: `Note (2026-10-15, Robin)` or
/// `Note n-3 (2026-10-15, Robin)`. `None` for any other heading.
pub fn parse_note_title(title: &str) -> Option<NoteTitle<'_>> {
    let rest = title.trim().strip_prefix("Note")?;
    let rest = rest.strip_suffix(')')?;
    let (head, inside) = rest.split_once('(')?;
    let id = match head.trim() {
        "" => None,
        id if is_note_id(id) => Some(id),
        _ => return None,
    };
    let (date, author) = inside.split_once(',')?;
    let (date, author) = (date.trim(), author.trim());
    if date.is_empty() || author.is_empty() {
        return None;
    }
    Some(NoteTitle { id, date, author })
}

/// Is `id` of the form `n-<digits>`?
pub fn is_note_id(id: &str) -> bool {
    id.strip_prefix("n-")
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // KAIROS-T-0630: the text of a note row, as each answer shows it.
    #[test]
    fn a_note_renders_with_its_heading_and_its_mark() {
        assert_eq!(
            render_note(
                3,
                "2026-10-15",
                " Robin ",
                "One.\n\n## Findings\n\nTwo.",
                None
            ),
            "## Note n-3 (2026-10-15, Robin)\n\nOne.\n\n## Findings\n\nTwo."
        );
        assert_eq!(
            render_note(1, "2026-10-15", "Sam", "", Some("ACME-A-0001")),
            "## Note n-1 (2026-10-15, Sam)\n\nPromoted to ACME-A-0001."
        );
        let parts = [
            NoteParts {
                seq: 1,
                date: "2026-10-01",
                author: "Robin",
                text: "One.",
                promoted_to: None,
            },
            NoteParts {
                seq: 2,
                date: "2026-10-02",
                author: "Sam",
                text: "Two.",
                promoted_to: Some("ACME-D-0002"),
            },
        ];
        assert_eq!(
            render_notes(parts),
            "\n\n## Note n-1 (2026-10-01, Robin)\n\nOne.\n\n## Note n-2 (2026-10-02, Sam)\n\nTwo.\n\nPromoted to ACME-D-0002."
        );
        assert_eq!(render_notes([]), "");
    }

    #[test]
    fn a_note_id_gives_its_number() {
        assert_eq!(parse_note_id("n-3"), Some(3));
        assert_eq!(parse_note_id(" n-12 "), Some(12));
        for other in ["n-0", "n-", "3", "x-3", "n-99999999999", "n-3a"] {
            assert_eq!(parse_note_id(other), None, "{other}");
        }
    }

    #[test]
    fn a_title_with_and_without_an_id_parses() {
        assert_eq!(
            parse_note_title("Note (2026-10-11, Robin)"),
            Some(NoteTitle {
                id: None,
                date: "2026-10-11",
                author: "Robin"
            })
        );
        assert_eq!(
            parse_note_title("Note n-12 (2026-10-11, Sam Lee)"),
            Some(NoteTitle {
                id: Some("n-12"),
                date: "2026-10-11",
                author: "Sam Lee"
            })
        );
        for other in [
            "Notes",
            "Note",
            "Note (2026-10-11)",
            "Note x-1 (2026-10-11, Robin)",
            "Note n- (2026-10-11, Robin)",
            "Note (, Robin)",
            "Objective",
        ] {
            assert_eq!(parse_note_title(other), None, "{other}");
        }
    }
}
