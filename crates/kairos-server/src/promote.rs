//! Promote a note into a draft ADR or document (KAIROS-T-0505,
//! COLLIERY-I-0629): the ONE implementation of
//! `POST /api/{family}/{short_code}/notes/{note_id}/promote` and of the MCP
//! tool `promote_note`.
//!
//! The new item gets the text of the note and a line that names the note,
//! its author and its date. The new item `supports` the source item when
//! the source is a strategy, an initiative or a task (the type rule of a
//! `supports` edge, [`kairos_core::graph::check_link`]); a note on a
//! document or an ADR gets no edge. The note gets `promoted_to`, the code
//! of the new item (KAIROS-T-0630: a column of the note row, not a line of
//! the content). Each answer shows it as the line
//! `Promoted to KAIROS-A-0029.` after the note.
//!
//! A note is promoted once: a second promote is refused with `CONFLICT`,
//! and the refusal names the item that the note became. The read of the
//! note locks its row, so two promotes at the same time make one item.
//!
//! Who may: the edit rule of the source item (the mark is an edit), and the
//! right to create on the target board (`manage_adrs`, or
//! `manage_documents` through [`crate::api::documents::create`]). Each
//! check runs before the first write; the caller runs this in one
//! transaction ([`crate::api::atomically`]).

use diesel::pg::PgConnection;
use kairos_core::short_code::ItemType;
use kairos_db::items;
use kairos_db::models::RelationshipType;
use uuid::Uuid;

use crate::api::documents::{self, CreateWording, NewDocument};
use crate::api::{
    board_id_by_ref, map_graph_error, map_item_error, require_capability, require_edge_write,
    require_item_edit,
};
use crate::error::ApiError;

/// The kinds that a note can become.
pub const KINDS: [&str; 2] = ["adr", "document"];

/// The inputs of a promote, from each surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PromoteInput<'a> {
    /// The source item: its id, type and short code.
    pub source: (Uuid, ItemType, &'a str),
    /// The id of the note: `n-3`.
    pub note_id: &'a str,
    /// `adr` or `document`.
    pub kind: &'a str,
    /// The target board, by slug or UUID.
    pub board: &'a str,
    /// The title of the new item; the first line of the note when absent.
    pub title: Option<&'a str>,
}

/// What a promote made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Promoted {
    pub source_code: String,
    pub note_id: String,
    pub kind: ItemType,
    pub created_code: String,
    pub created_id: Uuid,
    /// True when the new item `supports` the source item.
    pub supports: bool,
}

/// The kind as an item type, or the refusal that names the kinds.
pub(crate) fn parse_kind(kind: &str, wording: CreateWording) -> Result<ItemType, ApiError> {
    match kind.trim() {
        "adr" => Ok(ItemType::Adr),
        "document" => Ok(ItemType::Document),
        other => Err(ApiError::validation(format!(
            "The {} has the kind {other:?}. A note becomes an adr or a document. Send kind: \
             adr or document.",
            wording.request
        ))
        .with_details(serde_json::json!({ "field": "kind", "allowed": KINDS }))),
    }
}

/// Promote the note: the new item, its edge, and the line on the note.
pub(crate) fn promote_note(
    conn: &mut PgConnection,
    slug: &str,
    role: kairos_db::models::OrgRole,
    user: Uuid,
    input: PromoteInput<'_>,
    wording: CreateWording,
) -> Result<Promoted, ApiError> {
    let (source_id, source_type, source_code) = input.source;
    let kind = parse_kind(input.kind, wording)?;
    if input.board.trim().is_empty() {
        return Err(ApiError::validation(format!(
            "The {} has no {}. Send the slug or the id of the board that gets the new item.",
            wording.request, wording.board
        ))
        .with_details(serde_json::json!({ "field": "board" })));
    }
    // The mark on the note is an edit of the source item.
    require_item_edit(conn, role, user, source_id, source_type)?;
    let found = match kairos_markdown::notes::parse_note_id(input.note_id) {
        Some(seq) => {
            kairos_db::item_notes::lock_note(conn, source_id, seq).map_err(ApiError::internal)?
        }
        None => None,
    };
    let Some(note) = found else {
        let ids: Vec<String> = kairos_db::item_notes::notes_of(conn, source_id)
            .map_err(ApiError::internal)?
            .iter()
            .map(|note| note.note_id())
            .collect();
        return Err(ApiError::not_found(if ids.is_empty() {
            format!("The item {source_code} has no note.")
        } else {
            format!(
                "The item {source_code} has no note {:?}. Its notes: {}.",
                input.note_id,
                ids.join(", ")
            )
        })
        .with_details(serde_json::json!({ "field": "note_id" })));
    };
    let note_id = note.note_id();
    // KAIROS-T-0551, KAIROS-T-0630: a second promote would make a second
    // item.
    if let Some(promoted) = &note.promoted_to {
        return Err(ApiError::conflict(format!(
            "The note {note_id} on {source_code} became {promoted} already. Kairos promotes a \
             note one time only. Read {promoted}, or promote a different note."
        ))
        .with_details(serde_json::json!({ "note_id": note_id, "promoted_to": promoted })));
    }
    let title = match input.title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => title.to_string(),
        None if !note.first_line().is_empty() => clip(note.first_line(), 120),
        None => format!("Note {note_id} of {source_code}"),
    };
    let from = format!(
        "From note {note_id} on {source_code} ({}, {}).",
        note.author,
        note.date()
    );
    let new_content = if note.text.is_empty() {
        format!("{from}\n")
    } else {
        format!("{}\n\n{from}\n", note.text)
    };
    let supports = matches!(
        source_type,
        ItemType::Strategy | ItemType::Initiative | ItemType::Task
    );

    let (created_code, created_id) = match kind {
        ItemType::Adr => {
            let board_id = board_id_by_ref(conn, input.board)?;
            require_capability(conn, slug, Some(board_id), user, "manage_adrs")?;
            let created = items::create_adr(
                conn,
                items::CreateAdr {
                    board_id: Some(board_id),
                    column_id: None,
                    title: &title,
                    content: &new_content,
                    decision_maker: None,
                    decision_date: None,
                },
                user,
            )
            .map_err(map_item_error)?;
            if supports {
                require_edge_write(
                    conn,
                    role,
                    user,
                    RelationshipType::Supports.as_str(),
                    (source_id, source_type),
                    (created.id, ItemType::Adr),
                )?;
                kairos_db::graph::link_items(
                    conn,
                    source_id,
                    created.id,
                    RelationshipType::Supports,
                    user,
                )
                .map_err(map_graph_error)?;
            }
            (created.short_code, created.id)
        }
        ItemType::Document => {
            let created = documents::create(
                conn,
                slug,
                role,
                user,
                NewDocument {
                    title: &title,
                    content: Some(&new_content),
                    template_id: None,
                    board: Some(input.board),
                    parent: supports.then_some(source_code),
                    before_write: None,
                },
                wording,
            )?;
            (created.document.short_code, created.document.id)
        }
        _ => unreachable!("parse_kind gives an adr or a document"),
    };

    kairos_db::item_notes::mark_promoted(conn, source_type, &note, &created_code, user)
        .map_err(ApiError::internal)?;

    Ok(Promoted {
        source_code: source_code.to_string(),
        note_id,
        kind,
        created_code,
        created_id,
        supports,
    })
}

/// `text` cut to `max` characters, with `…` when it was longer.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}
