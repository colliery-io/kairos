//! Retired short codes (COLLIERY-T-3100, COLLIERY-I-0407).
//!
//! A short code is retired when its item gets a new code: a move with a
//! rename (COLLIERY-T-3101), or a re-import (COLLIERY-T-3103). The table
//! `retired_codes` keeps (code, item id, retired at, reason).
//!
//! Two rules follow from it:
//!
//! - **A retired code is never issued again.** [`crate::items::next_short_code`]
//!   skips it, and a trigger on each item table refuses it, so a code that
//!   is written directly cannot take it either.
//! - **A read with a retired code finds the item.** [`current_code`] gives the
//!   item and its current code. The read paths (MCP `get_item`, REST
//!   `GET /api/{family}/{code}`, search, the GUI item page) follow it and
//!   name the current code. A write with a retired code is refused, and the
//!   refusal names the current code: a write must name the item it changes.
//!
//! A code is held by an item or retired, never the two: [`retire_code`]
//! refuses a code that an item has, and the trigger refuses an item a code
//! that is retired.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Nullable, Text, Timestamptz, Uuid as SqlUuid};
use kairos_core::short_code::{ItemType, parse_short_code};
use uuid::Uuid;

use crate::schema::retired_codes;

/// A refusal of [`retire_code`].
#[derive(Debug, thiserror::Error)]
pub enum RetireError {
    /// The code does not have the form `{PREFIX}-{TYPE_LETTER}-{NNNN}`.
    #[error("{0:?} is not a short code. A short code has the form PREFIX-T-0001.")]
    NotAShortCode(String),
    /// The reason is empty.
    #[error("A retired code must have a reason.")]
    NoReason,
    /// An item has the code now, so it cannot be retired.
    #[error("The short code {0:?} is the code of an item. Change the code of the item first.")]
    InUse(String),
    /// The code is retired already.
    #[error("The list of retired codes has the short code {0:?} already.")]
    AlreadyRetired(String),
    /// A database error.
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// The item that a retired code names, as [`current_code`] gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredCode {
    /// The retired code that was asked for.
    pub code: String,
    /// The item that had the code.
    pub item_id: Uuid,
    /// When the code was retired.
    pub retired_at: DateTime<Utc>,
    /// Why the code was retired.
    pub reason: String,
    /// The current code of the item and its type. `None` when the item is
    /// not there now (it was removed for real); the code stays retired.
    pub current: Option<(String, ItemType)>,
}

/// Retire `code`, which was the code of `item_id`, for `reason`. Call it in
/// the transaction that gives the item its new code, after that change: the
/// code must not be the code of an item (archived items too).
pub fn retire_code(
    conn: &mut PgConnection,
    code: &str,
    item_id: Uuid,
    reason: &str,
) -> Result<(), RetireError> {
    if parse_short_code(code).is_none() {
        return Err(RetireError::NotAShortCode(code.to_string()));
    }
    if reason.trim().is_empty() {
        return Err(RetireError::NoReason);
    }
    #[derive(QueryableByName)]
    struct Held {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        held: bool,
    }
    let held: Held =
        sql_query("SELECT EXISTS (SELECT 1 FROM entity_directory WHERE short_code = $1) AS held")
            .bind::<Text, _>(code)
            .get_result(conn)?;
    if held.held {
        return Err(RetireError::InUse(code.to_string()));
    }
    let inserted = diesel::insert_into(retired_codes::table)
        .values((
            retired_codes::code.eq(code),
            retired_codes::item_id.eq(item_id),
            retired_codes::reason.eq(reason.trim()),
        ))
        .on_conflict_do_nothing()
        .execute(conn)?;
    if inserted == 0 {
        return Err(RetireError::AlreadyRetired(code.to_string()));
    }
    Ok(())
}

/// True when `code` is retired.
pub fn is_retired(conn: &mut PgConnection, code: &str) -> Result<bool, diesel::result::Error> {
    diesel::select(diesel::dsl::exists(
        retired_codes::table.filter(retired_codes::code.eq(code)),
    ))
    .get_result(conn)
}

#[derive(QueryableByName)]
struct RetiredRow {
    #[diesel(sql_type = Text)]
    code: String,
    #[diesel(sql_type = SqlUuid)]
    item_id: Uuid,
    #[diesel(sql_type = Timestamptz)]
    retired_at: DateTime<Utc>,
    #[diesel(sql_type = Text)]
    reason: String,
    #[diesel(sql_type = Nullable<Text>)]
    current_code: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    entity_type: Option<String>,
}

/// The item that the retired `code` names, with its current code. `None`
/// when `code` is not retired (it can be a live code, or no code). The
/// item can be archived: the caller decides if it serves archived work.
pub fn current_code(
    conn: &mut PgConnection,
    code: &str,
) -> Result<Option<RetiredCode>, diesel::result::Error> {
    let row: Option<RetiredRow> = sql_query(
        "SELECT r.code, r.item_id, r.retired_at, r.reason, \
                d.short_code AS current_code, d.entity_type \
           FROM retired_codes r \
           LEFT JOIN entity_directory d ON d.id = r.item_id \
          WHERE r.code = $1",
    )
    .bind::<Text, _>(code)
    .get_result(conn)
    .optional()?;
    Ok(row.map(|row| {
        let current = match (row.current_code, row.entity_type) {
            (Some(code), Some(entity_type)) => ItemType::ALL
                .iter()
                .copied()
                .find(|t| t.entity_type() == entity_type)
                .map(|t| (code, t)),
            _ => None,
        };
        RetiredCode {
            code: row.code,
            item_id: row.item_id,
            retired_at: row.retired_at,
            reason: row.reason,
            current,
        }
    }))
}

/// The retired codes of `item_id`, the oldest first.
pub fn codes_of_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Vec<String>, diesel::result::Error> {
    retired_codes::table
        .filter(retired_codes::item_id.eq(item_id))
        .order((retired_codes::retired_at.asc(), retired_codes::code.asc()))
        .select(retired_codes::code)
        .load(conn)
}
