//! A rename of an item on a move to another board (COLLIERY-T-3101,
//! COLLIERY-I-0407).
//!
//! A move keeps the short code of the item unless the caller asks for a
//! rename. [`rename_item`] does the rename, in the transaction of the move:
//!
//! 1. The item gets the next code of its new board
//!    ([`crate::items::next_short_code`]).
//! 2. The old code goes into `retired_codes`
//!    ([`crate::retired_codes::retire_code`]), so it is never issued again,
//!    and a read with it finds the item (COLLIERY-T-3100).
//! 3. Each reference to the old code in the title and the content of each
//!    item of the tenant changes to the new code, ONE time
//!    ([`kairos_core::short_code::rewrite_code_references`]: a code in a URL
//!    or a path does not change). An item that changes gets a new version
//!    and an `item_history` row by the actor, as an edit does. Archived
//!    items change too, so that a restore does not bring back the old code.
//! 4. The activity log gets a `rename` row (`code:{old}->{new}`), which is
//!    the record of who renamed the item and when.
//!
//! A rename to a board whose prefix the code has already is refused
//! ([`RenameError::NotNeeded`]): it would use up a code and change nothing.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Bool, Integer, Text, Uuid as SqlUuid};
use kairos_core::short_code::{ItemType, parse_short_code, rewrite_code_references};
use uuid::Uuid;

use crate::events::{self, EventKind};
use crate::items::{self, ItemError};
use crate::models::enums::ActivityAction;
use crate::models::graph::{NewActivityLogEntry, NewItemHistory};
use crate::retired_codes::{self, RetireError};

/// What [`rename_item`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRename {
    /// The code that the item had. It is retired now.
    pub old_code: String,
    /// The code that the item has now.
    pub new_code: String,
    /// The current codes of the items whose title or content changed, in
    /// order. The renamed item is in the list when its own text named its
    /// old code.
    pub references: Vec<String>,
}

/// A refusal of [`rename_item`].
#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    /// The code of the item has the prefix of the target board already.
    #[error(
        "The code {code} has the prefix {prefix} of the target board already. A rename \
         gives the item a code with the prefix of the target board. Do the move with no \
         rename."
    )]
    NotNeeded { code: String, prefix: String },
    /// The item or the board is not there, or the code cannot be issued.
    #[error(transparent)]
    Item(#[from] ItemError),
    /// The old code cannot be retired.
    #[error(transparent)]
    Retire(#[from] RetireError),
    /// A database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

fn table_of(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Strategy => "strategies",
        ItemType::Initiative => "initiatives",
        ItemType::Task => "tasks",
        ItemType::Document => "documents",
        ItemType::Adr => "adrs",
    }
}

#[derive(QueryableByName)]
struct CodeRow {
    #[diesel(sql_type = Text)]
    short_code: String,
}

#[derive(QueryableByName)]
struct TextRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = Bool)]
    archived: bool,
}

#[derive(QueryableByName)]
struct VersionRow {
    #[diesel(sql_type = Integer)]
    version: i32,
}

/// Give the item `item_id` of `item_type` the next code of `board_id` (the
/// board that the item is on after the move; `None` for no board), retire
/// its old code for `reason`, and change the references to the old code in
/// the text of each item of the tenant. See the module docs.
///
/// Call it in the transaction of the move, after the item is on its new
/// board. A refusal leaves nothing changed.
pub fn rename_item(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    board_id: Option<Uuid>,
    actor: Uuid,
    reason: &str,
) -> Result<CodeRename, RenameError> {
    conn.transaction::<_, RenameError, _>(|conn| {
        let table = table_of(item_type);
        let old: Option<CodeRow> = sql_query(format!(
            "SELECT short_code FROM {table} WHERE id = $1 FOR UPDATE"
        ))
        .bind::<SqlUuid, _>(item_id)
        .get_result(conn)
        .optional()?;
        let old_code = old
            .ok_or(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            })?
            .short_code;
        let prefix = items::code_prefix_for(conn, board_id)?;
        if parse_short_code(&old_code).is_some_and(|(p, _, _)| p == prefix) {
            return Err(RenameError::NotNeeded {
                code: old_code,
                prefix,
            });
        }
        let new_code = items::next_short_code(conn, item_type, board_id)?;
        sql_query(format!(
            "UPDATE {table} SET short_code = $2, updated_by = $3, updated_at = now() WHERE id = $1"
        ))
        .bind::<SqlUuid, _>(item_id)
        .bind::<Text, _>(&new_code)
        .bind::<SqlUuid, _>(actor)
        .execute(conn)?;
        retired_codes::retire_code(conn, &old_code, item_id, reason)?;
        let references = rewrite_references(conn, &old_code, &new_code, actor)?;
        diesel::insert_into(crate::schema::activity_log::table)
            .values(NewActivityLogEntry {
                actor_id: actor,
                action: ActivityAction::Rename,
                entity_id: Some(item_id),
                entity_type: Some(item_type.entity_type().to_string()),
                details: format!("code:{old_code}->{new_code}"),
            })
            .execute(conn)?;
        Ok(CodeRename {
            old_code,
            new_code,
            references,
        })
    })
}

/// Change each reference to `old` in the title and the content of each item
/// of the tenant, live or archived, to `new`. Each item that changes gets
/// a new version and an `item_history` row by `actor`. Returns the codes
/// of the items that changed.
fn rewrite_references(
    conn: &mut PgConnection,
    old: &str,
    new: &str,
    actor: Uuid,
) -> Result<Vec<String>, DieselError> {
    let mut changed = Vec::new();
    for item_type in ItemType::ALL {
        let table = table_of(*item_type);
        let rows: Vec<TextRow> = sql_query(format!(
            "SELECT id, short_code, title, content, deleted_at IS NOT NULL AS archived \
               FROM {table} \
              WHERE strpos(title, $1) > 0 OR strpos(content, $1) > 0 \
              ORDER BY short_code \
                FOR UPDATE"
        ))
        .bind::<Text, _>(old)
        .load(conn)?;
        for row in rows {
            let title = rewrite_code_references(&row.title, old, new);
            let content = rewrite_code_references(&row.content, old, new);
            if title.is_none() && content.is_none() {
                continue;
            }
            let title = title.unwrap_or(row.title);
            let content = content.unwrap_or(row.content);
            let version: VersionRow = sql_query(format!(
                "UPDATE {table} SET title = $2, content = $3, version = version + 1, \
                        updated_by = $4, updated_at = now() \
                  WHERE id = $1 RETURNING version"
            ))
            .bind::<SqlUuid, _>(row.id)
            .bind::<Text, _>(&title)
            .bind::<Text, _>(&content)
            .bind::<SqlUuid, _>(actor)
            .get_result(conn)?;
            diesel::insert_into(crate::schema::item_history::table)
                .values(NewItemHistory {
                    item_id: row.id,
                    version: version.version,
                    title,
                    content,
                    edited_by: actor,
                })
                .execute(conn)?;
            if !row.archived {
                events::emit_item_event_by_id(
                    conn,
                    EventKind::ItemUpdated,
                    item_type.entity_type(),
                    row.id,
                    actor,
                )?;
            }
            changed.push(row.short_code);
        }
    }
    Ok(changed)
}
