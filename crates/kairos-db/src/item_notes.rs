//! The notes of an item as rows of their own (KAIROS-T-0630).
//!
//! A note is one row of `item_notes`: its number in the item (`seq`, the 3
//! of "note n-3 on ACME-T-0012"), its author, its time, its markdown text,
//! and the item that a promote made from it (`promoted_to`). A note is not
//! part of the content: [`append_note`] writes no version and no
//! `item_history` row, and an edit of the content does not change a note.
//!
//! # The number of a note
//!
//! [`append_note`] locks the row of the item (an update of `updated_at`)
//! before it reads the highest number, so two notes at the same time get two
//! numbers, one after the other. The unique key `(item_id, seq)` is the
//! guard behind the lock. A number is never given again: a note is not
//! removed, except by the purge of its item.
//!
//! # The author
//!
//! `author_id` is the user that wrote the note. The tenant migration
//! `2026-10-18-003000_item_notes` gave a moved note `author_name` when its
//! name resolved to no user. The name that each answer shows is the display
//! name of the user, else `author_name`, else the id ([`NOTE_SELECT`]).
//!
//! Every function runs in the CURRENT `search_path` tenant schema, as
//! [`crate::items`] does. Raw SQL, as in [`crate::search`]: the author name
//! is a join with `public.users`.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

use kairos_core::short_code::ItemType;

use crate::events::{self, EventKind};
use crate::items::ItemError;

/// One note of an item.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName, serde::Deserialize)]
pub struct ItemNote {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    pub item_id: Uuid,
    /// The number of the note in its item: 3 for `n-3`.
    #[diesel(sql_type = Integer)]
    pub seq: i32,
    /// The user that wrote the note. `None` for a moved note whose name
    /// resolved to no user.
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub author_id: Option<Uuid>,
    /// The name that the answers show (module docs).
    #[diesel(sql_type = Text)]
    pub author: String,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
    /// The markdown text. It can have headings.
    #[diesel(sql_type = Text)]
    pub text: String,
    /// The code of the item that a promote made from the note.
    #[diesel(sql_type = Nullable<Text>)]
    pub promoted_to: Option<String>,
}

impl ItemNote {
    /// The id of the note: `n-3`.
    pub fn note_id(&self) -> String {
        format!("n-{}", self.seq)
    }

    /// The UTC date of the note: `2026-10-15`.
    pub fn date(&self) -> String {
        self.created_at.format("%Y-%m-%d").to_string()
    }

    /// The note as a `## Note n-3 (date, name)` section
    /// ([`kairos_markdown::notes::render_note`]).
    pub fn markdown(&self) -> String {
        kairos_markdown::notes::render_note(
            self.seq,
            &self.date(),
            &self.author,
            &self.text,
            self.promoted_to.as_deref(),
        )
    }

    /// The first line of the text with words, without the fences; empty
    /// when the text has none (KAIROS-T-0504: the list of `get_item`).
    pub fn first_line(&self) -> &str {
        self.text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("```") && !line.starts_with("~~~"))
            .unwrap_or("")
    }
}

/// The notes as `## Note` sections after an empty line each
/// ([`kairos_markdown::notes::render_notes`]): the text that follows the
/// content in `get_item`. Empty for no notes.
pub fn render(notes: &[ItemNote]) -> String {
    let dates: Vec<String> = notes.iter().map(ItemNote::date).collect();
    kairos_markdown::notes::render_notes(notes.iter().zip(&dates).map(|(note, date)| {
        kairos_markdown::notes::NoteParts {
            seq: note.seq,
            date,
            author: &note.author,
            text: &note.text,
            promoted_to: note.promoted_to.as_deref(),
        }
    }))
}

/// The content and then its notes (KAIROS-T-0630): the content with no
/// white space at the end, then [`render`]. The content as it is when the
/// item has no notes. The SQL of the embedding sweep gives the same text.
pub fn content_with_notes(content: &str, notes: &[ItemNote]) -> String {
    if notes.is_empty() {
        return content.to_string();
    }
    format!("{}{}", content.trim_end(), render(notes))
}

/// The columns of an [`ItemNote`], from `item_notes n` and `public.users u`.
pub const NOTE_SELECT: &str = "SELECT n.id, n.item_id, n.seq, n.author_id, \
     coalesce(nullif(btrim(u.display_name), ''), n.author_name, n.author_id::text) AS author, \
     n.created_at, n.text, n.promoted_to \
     FROM item_notes n LEFT JOIN public.users u ON u.id = n.author_id";

/// The notes of the item `$1` as one JSON array, in the order of their
/// numbers: a column for a statement that reads more than the notes (the
/// one statement of the facts of `get_item`, KAIROS-T-0626). Read it with
/// [`parse_notes_json`].
pub const NOTES_JSON_OF_ITEM: &str = "(SELECT coalesce(json_agg(json_build_object( \
     'id', n.id, 'item_id', n.item_id, 'seq', n.seq, 'author_id', n.author_id, \
     'author', coalesce(nullif(btrim(u.display_name), ''), n.author_name, n.author_id::text), \
     'created_at', n.created_at, 'text', n.text, 'promoted_to', n.promoted_to) \
     ORDER BY n.seq), '[]'::json)::text \
     FROM item_notes n LEFT JOIN public.users u ON u.id = n.author_id WHERE n.item_id = $1)";

/// The notes of a [`NOTES_JSON_OF_ITEM`] column.
pub fn parse_notes_json(json: &str) -> Result<Vec<ItemNote>, serde_json::Error> {
    serde_json::from_str(json)
}

/// The notes of the item, in the order of their numbers.
pub fn notes_of(conn: &mut PgConnection, item_id: Uuid) -> Result<Vec<ItemNote>, DieselError> {
    sql_query(format!("{NOTE_SELECT} WHERE n.item_id = $1 ORDER BY n.seq"))
        .bind::<SqlUuid, _>(item_id)
        .load(conn)
}

/// The note `seq` of the item, with its row locked until the end of the
/// transaction of `conn` (`FOR UPDATE`): a promote reads it, and a second
/// promote waits and then reads the mark of the first.
pub fn lock_note(
    conn: &mut PgConnection,
    item_id: Uuid,
    seq: i32,
) -> Result<Option<ItemNote>, DieselError> {
    sql_query(format!(
        "{NOTE_SELECT} WHERE n.item_id = $1 AND n.seq = $2 FOR UPDATE OF n"
    ))
    .bind::<SqlUuid, _>(item_id)
    .bind::<Integer, _>(seq)
    .get_result(conn)
    .optional()
}

/// Why [`append_note`] wrote nothing (KAIROS-T-0367).
#[derive(Debug, thiserror::Error)]
pub enum NoteError {
    /// The text is empty or only white space.
    #[error("The text of the note is empty. Send the text to append.")]
    EmptyText,
    /// The item is not there, or the database failed.
    #[error(transparent)]
    Item(#[from] ItemError),
}

impl From<DieselError> for NoteError {
    fn from(e: DieselError) -> Self {
        NoteError::Item(ItemError::Database(e))
    }
}

/// What [`append_note`] wrote (KAIROS-T-0504, KAIROS-T-0630).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendedNote {
    /// The version of the content. A note does not change it.
    pub version: i32,
    /// The id of the note: `n-3`.
    pub note_id: String,
    /// The note.
    pub note: ItemNote,
}

#[derive(QueryableByName)]
struct Locked {
    #[diesel(sql_type = Integer)]
    version: i32,
}

/// Append a note to an item (KAIROS-T-0367, KAIROS-T-0630): one row of
/// `item_notes` with the next number, by `actor`, in one transaction. The
/// row of the item gets a new `updated_at` and keeps its version and its
/// content: no `item_history` row. Each `@user_name` of the text is a
/// mention (KAIROS-T-0381). The events are `item_updated` and
/// `note_appended`, with the id of the note.
///
/// The update of the item row is the lock (module docs): two notes at the
/// same time both stay, each with its own number.
pub fn append_note(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    text: &str,
    actor: Uuid,
) -> Result<AppendedNote, NoteError> {
    let text = kairos_core::items::note_text(text).ok_or(NoteError::EmptyText)?;
    conn.transaction::<_, NoteError, _>(|conn| {
        let locked: Option<Locked> = sql_query(format!(
            "UPDATE {} SET updated_at = now() WHERE id = $1 AND deleted_at IS NULL \
             RETURNING version",
            crate::items::table_of(item_type)
        ))
        .bind::<SqlUuid, _>(item_id)
        .get_result(conn)
        .optional()?;
        let Some(Locked { version }) = locked else {
            return Err(NoteError::Item(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            }));
        };
        let note: ItemNote = sql_query(format!(
            "WITH new AS ( \
                 INSERT INTO item_notes (item_id, seq, author_id, text) \
                 SELECT $1, coalesce(max(seq), 0) + 1, $2, $3 FROM item_notes WHERE item_id = $1 \
                 RETURNING * \
             ) {}",
            NOTE_SELECT.replace("FROM item_notes n", "FROM new n")
        ))
        .bind::<SqlUuid, _>(item_id)
        .bind::<SqlUuid, _>(actor)
        .bind::<Text, _>(text)
        .get_result(conn)?;
        // KAIROS-T-0381: each mention in the text is new.
        let names: Vec<String> = kairos_core::notifications::mentions(text)
            .into_keys()
            .collect();
        crate::notifications::on_mentions(conn, item_id, &names, actor)?;
        // KAIROS-T-0736: the watchers of the item.
        crate::watches::on_watched(
            conn,
            item_id,
            kairos_core::notifications::WatchEvent::NoteAdded,
            None,
            actor,
        )?;
        let note_id = note.note_id();
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemUpdated,
            item_type.entity_type(),
            item_id,
            actor,
        )?;
        events::emit_item_change_by_id(
            conn,
            EventKind::NoteAppended,
            item_type.entity_type(),
            item_id,
            actor,
            Some(&note_id),
        )?;
        Ok(AppendedNote {
            version,
            note_id,
            note,
        })
    })
}

/// Mark the note `note` as promoted to `code` (KAIROS-T-0505,
/// KAIROS-T-0630), and emit `item_updated` and `note_promoted` for its
/// item. The caller holds the lock of the note ([`lock_note`]) and has
/// checked that the note has no mark.
pub fn mark_promoted(
    conn: &mut PgConnection,
    item_type: ItemType,
    note: &ItemNote,
    code: &str,
    actor: Uuid,
) -> Result<(), DieselError> {
    sql_query("UPDATE item_notes SET promoted_to = $2 WHERE id = $1")
        .bind::<SqlUuid, _>(note.id)
        .bind::<Text, _>(code)
        .execute(conn)?;
    events::emit_item_event_by_id(
        conn,
        EventKind::ItemUpdated,
        item_type.entity_type(),
        note.item_id,
        actor,
    )?;
    events::emit_item_change_by_id(
        conn,
        EventKind::NotePromoted,
        item_type.entity_type(),
        note.item_id,
        actor,
        Some(&note.note_id()),
    )
}

/// Remove the notes of an item: the purge of the item only
/// ([`crate::purge`]).
pub fn delete_for_item(conn: &mut PgConnection, item_id: Uuid) -> Result<usize, DieselError> {
    sql_query("DELETE FROM item_notes WHERE item_id = $1")
        .bind::<SqlUuid, _>(item_id)
        .execute(conn)
}
