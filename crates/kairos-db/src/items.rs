//! Item write-path services (KAIROS-T-0012): short-code generation, create
//! services, optimistic-concurrency content updates with `item_history`
//! snapshots (KAIROS-A-0004), rollback, and the soft-delete cascade
//! (KAIROS-A-0001).
//!
//! KAIROS-A-0009 layering, same shape as [`crate::boards`]: this module
//! loads rows, delegates the pure decisions to [`kairos_core::short_code`]
//! (code format, default prefix) and [`kairos_core::items`] (version-check
//! contract, cascade set over loaded `parent` edges), persists, and writes
//! `activity_log`. Every public function operates in the CURRENT
//! `search_path` tenant schema and runs in its own transaction.
//!
//! # Short codes
//!
//! A code is `{PREFIX}-{TYPE_LETTER}-{NNNN}` (COLLIERY-T-3099). The PREFIX
//! is the `code_prefix` of the board of the item: the board of a strategy,
//! an initiative, a task or an ADR, and the owner board of a document. Each
//! document has an owner board (COLLIERY-T-3109). An ADR with no board
//! takes the prefix of the tenant
//! ([`kairos_core::short_code::tenant_prefix`] of the slug in
//! `current_schema()`, which is `org_{slug}` on each tenant connection).
//!
//! The NUMBER comes from the row (prefix, type) of `short_code_sequences`
//! ([`next_short_code`]). The update of the row locks it until the
//! transaction of the create ends, so two creates cannot get the same
//! number. A create that fails rolls the row back, so a sequence has no
//! gaps from failed creates. Boards that share a prefix for one type (a
//! document of a board of a different level) share the row, so their codes
//! are unique.
//!
//! A create takes the number late (KAIROS-T-0633): it inserts the row under
//! a pending code and writes the history row first, then takes the number
//! and writes the code ([`finish_create`]), so the lock does not cover the
//! insert and the history row. The numbers stay without gaps.
//!
//! # Versioning contract (KAIROS-A-0004)
//!
//! - Create inserts at `version = 1` AND writes the v1 baseline snapshot to
//!   `item_history`, so history is complete from birth and rollback to ANY
//!   version (including v1) is coherent. A-0004 specifies a snapshot on
//!   every edit; the create-time baseline is the KAIROS-T-0012
//!   interpretation that makes "rollback by copying a historical snapshot"
//!   total.
//! - [`update_item_content`] enforces the version check atomically:
//!   `UPDATE … SET version = version + 1 … WHERE id = ? AND version =
//!   expected` — zero rows means conflict, and the current row is returned
//!   in the typed [`ItemError::VersionConflict`]. The pure mirror of this
//!   decision is [`kairos_core::items::check_version`].
//! - [`rollback_item`] copies a historical snapshot forward as a NEW
//!   version through the same concurrency-checked path (history is
//!   append-only; nothing is rewritten).
//! - A version is a full row or a diff row (KAIROS-T-0651): see
//!   [`crate::history`]. Read the text of a version with
//!   [`crate::history::version_row`], not from `item_history.content`.
//!
//! # Soft delete (KAIROS-A-0001)
//!
//! [`soft_delete_item`] loads the `parent` edges below the root in one
//! query, a recursive CTE from the root ([`subtree_parent_edges`],
//! KAIROS-T-0629), computes the descendant set with the pure
//! [`kairos_core::items::cascade_descendants`] (so the traversal decision
//! lives in core and is unit-testable), and stamps `deleted_at` on the
//! root plus every live descendant across all five entity tables. One
//! `activity_log` row (action `delete`) on the root records the cascade
//! count and the cascaded short codes.
//!
//! [`soft_delete_item`] applies no permission rule: it is the service, and
//! it takes each descendant. An archive FOR A PRINCIPAL is
//! [`soft_delete_item_as`] (COLLIERY-T-0234), which applies the edit rule
//! to each descendant. Each REST handler and each MCP tool calls that one.

use chrono::NaiveDate;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use uuid::Uuid;

use kairos_core::items as rules;
use kairos_core::short_code::{self, ItemType};

use crate::abac;
use crate::events::{self, EventKind};

use crate::models::enums::{
    ActivityAction, BucketType, Complexity, DocumentLifecycle, RelationshipType, TaskType,
    WorkClass,
};
use crate::models::graph::NewActivityLogEntry;
use crate::models::items::{
    Adr, Document, Initiative, NewAdr, NewDocument, NewInitiative, NewStrategy, NewTask, Strategy,
    Task,
};
use crate::models::templates::{NewItemMetadata, Template};

/// Errors from the item write path.
#[derive(Debug, thiserror::Error)]
pub enum ItemError {
    /// No live item of this type with this id exists (soft-deleted rows are
    /// not found).
    #[error("The {entity_type} {id} does not exist.")]
    ItemNotFound { entity_type: &'static str, id: Uuid },
    /// KAIROS-A-0004 optimistic-concurrency conflict: the submitted base
    /// version is stale. Carries the item's current version, title, and
    /// content so the client can reconcile (maps to HTTP 409).
    #[error(
        "The request has the version {expected_version} of the item {item_id}, and the \
         current version is {current_version}. Make the edit on the current version."
    )]
    VersionConflict {
        item_id: Uuid,
        expected_version: i32,
        current_version: i32,
        current_title: String,
        current_content: String,
    },
    /// No `item_history` snapshot exists for this item/version (rollback
    /// target does not exist).
    #[error("The item {item_id} has no version {version} in its history.")]
    HistoryNotFound { item_id: Uuid, version: i32 },
    /// The history has the version, but its text cannot be read: a diff
    /// row without its base, or a diff that does not fit (KAIROS-T-0651).
    #[error("The server cannot read the version {version} of the item {item_id}. {reason}")]
    HistoryCorrupt {
        item_id: Uuid,
        version: i32,
        reason: String,
    },
    /// No live board with this id exists.
    #[error("The board {0} does not exist.")]
    BoardNotFound(Uuid),
    /// The board exists but has no columns to default-place an item into.
    #[error("The board {0} has no columns. Add a column to the board.")]
    BoardHasNoColumns(Uuid),
    /// An explicit `column_id` does not belong to the given board.
    #[error("The column {column_id} is not a column of the board {board_id}.")]
    ColumnNotOnBoard { board_id: Uuid, column_id: Uuid },
    /// No template with this id exists.
    #[error("The template {0} does not exist.")]
    TemplateNotFound(Uuid),
    /// No live repository with this id (KAIROS-T-0103).
    #[error("The repository {0} does not exist.")]
    RepositoryNotFound(Uuid),
    /// The board `strategy` owns a live dictionary already, and the write
    /// would give it a second one (KAIROS-T-0733, KAIROS-A-0079). `board`
    /// is the slug of the board, `existing` the code of its dictionary.
    #[error(
        "The board {board} has the dictionary {existing} already. An organization has one \
         dictionary. Edit {existing}."
    )]
    DictionaryExists { board: String, existing: String },
    /// A dictionary on a board that is not the board `strategy`
    /// (KAIROS-A-0079): an organization has one dictionary, and the board
    /// `strategy` owns it. `board` is the slug of the board of the write.
    #[error(
        "A dictionary can be only on the board strategy. The board {board} is not the board \
         strategy. Put the dictionary on the board strategy."
    )]
    DictionaryNotOnStrategy { board: String },
    /// Any other database error.
    #[error("The database gave an error: {0}.")]
    Database(#[from] DieselError),
}

impl From<crate::history::HistoryError> for ItemError {
    fn from(e: crate::history::HistoryError) -> Self {
        match e {
            crate::history::HistoryError::Corrupt {
                item_id,
                version,
                reason,
            } => ItemError::HistoryCorrupt {
                item_id,
                version,
                reason,
            },
            crate::history::HistoryError::Database(e) => ItemError::Database(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Short codes (S-0004 sequences + core formatting)
// ---------------------------------------------------------------------------

#[derive(QueryableByName)]
struct SeqValue {
    #[diesel(sql_type = BigInt)]
    value: i64,
}

#[derive(QueryableByName)]
struct SchemaName {
    #[diesel(sql_type = Text)]
    name: String,
}

/// The prefix of the tenant of the current schema (see the module docs).
pub fn tenant_code_prefix(conn: &mut PgConnection) -> Result<String, DieselError> {
    let schema: SchemaName =
        sql_query("SELECT COALESCE(current_schema()::text, 'public') AS name").get_result(conn)?;
    let slug = schema
        .name
        .strip_prefix("org_")
        .unwrap_or(schema.name.as_str());
    Ok(short_code::tenant_prefix(slug))
}

/// The prefix that an item on `board_id` gets (COLLIERY-T-3099): the
/// `code_prefix` of the board, or the prefix of the tenant for an item with
/// no board. A board that does not exist gives
/// [`ItemError::BoardNotFound`].
pub fn code_prefix_for(
    conn: &mut PgConnection,
    board_id: Option<Uuid>,
) -> Result<String, ItemError> {
    match board_id {
        Some(board_id) => {
            use crate::schema::boards::dsl;
            dsl::boards
                .filter(dsl::id.eq(board_id))
                .select(dsl::code_prefix)
                .first::<String>(conn)
                .optional()?
                .ok_or(ItemError::BoardNotFound(board_id))
        }
        None => Ok(tenant_code_prefix(conn)?),
    }
}

/// Allocate the next short code for an item of `item_type` on `board_id`
/// (`None` for an item with no board) in the current tenant schema
/// (COLLIERY-T-3099). The prefix comes from [`code_prefix_for`]. The number
/// is the next value of the row (prefix, type) of `short_code_sequences`.
/// A pair with no row gets a row that starts after the highest number of
/// the existing codes of the pair.
///
/// A retired code is never issued again (COLLIERY-T-3100): the sequence
/// moves past it to the next number.
///
/// Call it in the transaction of the create: the row stays locked until
/// that transaction ends.
pub fn next_short_code(
    conn: &mut PgConnection,
    item_type: ItemType,
    board_id: Option<Uuid>,
) -> Result<String, ItemError> {
    let prefix = code_prefix_for(conn, board_id)?;
    ensure_code_sequence(conn, &prefix, item_type)?;
    loop {
        let seq: SeqValue = sql_query(
            "UPDATE short_code_sequences SET last_number = last_number + 1 \
              WHERE code_prefix = $1 AND item_type = $2 RETURNING last_number AS value",
        )
        .bind::<Text, _>(&prefix)
        .bind::<Text, _>(item_type.letter().to_string())
        .get_result(conn)?;
        let code = short_code::format_short_code(&prefix, item_type, seq.value);
        if !crate::retired_codes::is_retired(conn, &code)? {
            return Ok(code);
        }
    }
}

/// The table of the items of `item_type`.
pub(crate) fn table_of(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Strategy => "strategies",
        ItemType::Initiative => "initiatives",
        ItemType::Task => "tasks",
        ItemType::Document => "documents",
        ItemType::Adr => "adrs",
    }
}

/// The highest number of the codes `{prefix}-{letter}-{N}`: the codes of
/// the table of `item_type`, live or deleted, and the retired codes
/// (COLLIERY-T-3100). 0 when there is none.
fn highest_code_number(
    conn: &mut PgConnection,
    prefix: &str,
    item_type: ItemType,
) -> Result<i64, DieselError> {
    let table = table_of(item_type);
    let pattern = format!("{prefix}-{}-", item_type.letter());
    let row: SeqValue = sql_query(format!(
        "SELECT COALESCE(max(substring(short_code FROM length($1) + 1)::bigint), 0) AS value \
           FROM (SELECT short_code FROM {table} \
                 UNION ALL SELECT code FROM retired_codes) AS codes \
          WHERE left(short_code, length($1)) = $1 \
            AND substring(short_code FROM length($1) + 1) ~ '^[0-9]{{1,18}}$'"
    ))
    .bind::<Text, _>(pattern)
    .get_result(conn)?;
    Ok(row.value)
}

/// Make the row (prefix, type) of `short_code_sequences` if it is not
/// there. A new row starts after the highest number of the existing codes
/// of the pair. A row that is there does not change.
pub fn ensure_code_sequence(
    conn: &mut PgConnection,
    prefix: &str,
    item_type: ItemType,
) -> Result<(), DieselError> {
    use crate::schema::short_code_sequences::dsl;
    let letter = item_type.letter().to_string();
    let exists: Option<i64> = dsl::short_code_sequences
        .filter(dsl::code_prefix.eq(prefix))
        .filter(dsl::item_type.eq(&letter))
        .select(dsl::last_number)
        .first(conn)
        .optional()?;
    if exists.is_some() {
        return Ok(());
    }
    let highest = highest_code_number(conn, prefix, item_type)?;
    diesel::insert_into(dsl::short_code_sequences)
        .values((
            dsl::code_prefix.eq(prefix),
            dsl::item_type.eq(&letter),
            dsl::last_number.eq(highest),
        ))
        .on_conflict((dsl::code_prefix, dsl::item_type))
        .do_nothing()
        .execute(conn)?;
    Ok(())
}

/// Move the row (prefix, type) of `short_code_sequences` past the highest
/// number of the existing codes of the pair, and return its last number.
/// The row does not move back. For a change that writes codes directly,
/// for example a re-key (COLLIERY-T-3103), so that the next create does not
/// give a code that an item has.
pub fn sync_code_sequence(
    conn: &mut PgConnection,
    prefix: &str,
    item_type: ItemType,
) -> Result<i64, DieselError> {
    conn.transaction(|conn| {
        ensure_code_sequence(conn, prefix, item_type)?;
        let highest = highest_code_number(conn, prefix, item_type)?;
        let seq: SeqValue = sql_query(
            "UPDATE short_code_sequences SET last_number = GREATEST(last_number, $3) \
              WHERE code_prefix = $1 AND item_type = $2 RETURNING last_number AS value",
        )
        .bind::<Text, _>(prefix)
        .bind::<Text, _>(item_type.letter().to_string())
        .bind::<BigInt, _>(highest)
        .get_result(conn)?;
        Ok(seq.value)
    })
}

/// A refusal of [`set_next_code_number`] (COLLIERY-T-3104).
#[derive(Debug, thiserror::Error)]
pub enum CodeSequenceError {
    /// The number is 0 or less.
    #[error("next_number must be 1 or more. The request had {0}.")]
    NotPositive(i64),
    /// An item (live or archived) has the code.
    #[error("An item has the code {code}. Kairos does not give a code 2 times.")]
    CodeInUse { code: String },
    /// The code is retired (COLLIERY-T-3100).
    #[error(
        "The code {code} is retired{}. Kairos does not give a retired code again.",
        current_code.as_deref().map(|c| format!(" (the item now has the code {c})")).unwrap_or_default()
    )]
    CodeRetired {
        code: String,
        current_code: Option<String>,
    },
    /// The sequence is at or past the number. A sequence does not go back.
    #[error(
        "The sequence of {prefix}-{letter} is at {last_number}, so the next code is {next_code}. \
         A sequence does not go back. Send a next_number above {last_number}."
    )]
    Behind {
        prefix: String,
        letter: char,
        last_number: i64,
        next_code: String,
    },
    #[error(transparent)]
    Db(#[from] DieselError),
}

/// The sequence (prefix, type) after [`set_next_code_number`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSequence {
    pub code_prefix: String,
    pub item_type: ItemType,
    /// The last number that the sequence gave.
    pub last_number: i64,
    /// The code that the next create gets.
    pub next_code: String,
}

/// Move the row (prefix, type) of `short_code_sequences` forward, so that
/// the next create of `item_type` with `prefix` gets the number
/// `next_number` (COLLIERY-T-3104). The Metis import uses it to keep the
/// Metis numbers: it sets the number, creates the item, and checks the code.
///
/// The row never goes back. The floor is the larger of the last number of
/// the row and the highest number of the existing and retired codes of the
/// pair (so a number above the floor is never retired and never in use).
/// A `next_number` at or below the floor is refused: with
/// [`CodeSequenceError::CodeInUse`] when an item has that code, with
/// [`CodeSequenceError::CodeRetired`] when the code is retired, and with
/// [`CodeSequenceError::Behind`] if not. `next_number` = floor + 1 changes
/// nothing, so a second call with the same number passes.
pub fn set_next_code_number(
    conn: &mut PgConnection,
    prefix: &str,
    item_type: ItemType,
    next_number: i64,
) -> Result<CodeSequence, CodeSequenceError> {
    if next_number < 1 {
        return Err(CodeSequenceError::NotPositive(next_number));
    }
    conn.transaction(|conn| {
        ensure_code_sequence(conn, prefix, item_type)?;
        let letter = item_type.letter().to_string();
        let last: SeqValue = sql_query(
            "SELECT last_number AS value FROM short_code_sequences \
              WHERE code_prefix = $1 AND item_type = $2 FOR UPDATE",
        )
        .bind::<Text, _>(prefix)
        .bind::<Text, _>(&letter)
        .get_result(conn)?;
        let floor = last
            .value
            .max(highest_code_number(conn, prefix, item_type)?);
        let code = short_code::format_short_code(prefix, item_type, next_number);
        if next_number <= floor {
            if code_in_use(conn, item_type, &code)? {
                return Err(CodeSequenceError::CodeInUse { code });
            }
            if let Some(retired) = crate::retired_codes::current_code(conn, &code)? {
                return Err(CodeSequenceError::CodeRetired {
                    code,
                    current_code: retired.current.map(|(current, _)| current),
                });
            }
            return Err(CodeSequenceError::Behind {
                prefix: prefix.to_string(),
                letter: item_type.letter(),
                last_number: floor,
                next_code: short_code::format_short_code(prefix, item_type, floor + 1),
            });
        }
        sql_query(
            "UPDATE short_code_sequences SET last_number = $3 \
              WHERE code_prefix = $1 AND item_type = $2",
        )
        .bind::<Text, _>(prefix)
        .bind::<Text, _>(&letter)
        .bind::<BigInt, _>(next_number - 1)
        .execute(conn)?;
        Ok(CodeSequence {
            code_prefix: prefix.to_string(),
            item_type,
            last_number: next_number - 1,
            next_code: code,
        })
    })
}

/// True when an item of `item_type` (live or archived) has `code`.
fn code_in_use(
    conn: &mut PgConnection,
    item_type: ItemType,
    code: &str,
) -> Result<bool, DieselError> {
    let table = table_of(item_type);
    let row: SeqValue = sql_query(format!(
        "SELECT count(*) AS value FROM {table} WHERE short_code = $1"
    ))
    .bind::<Text, _>(code)
    .get_result(conn)?;
    Ok(row.value > 0)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Insert one `activity_log` row (current tenant schema).
fn log_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    entity_id: Uuid,
    entity_type: &str,
    details: String,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(entity_id),
            entity_type: Some(entity_type.to_string()),
            details,
        })
        .execute(conn)?;
    Ok(())
}

/// Append the v1 baseline of `item_history` (append-only, KAIROS-A-0004).
/// Version 1 has no base: it is a full row (KAIROS-T-0651), and the write
/// reads nothing.
fn insert_history(
    conn: &mut PgConnection,
    item_id: Uuid,
    version: i32,
    title: &str,
    content: &str,
    actor: Uuid,
) -> Result<(), DieselError> {
    crate::history::insert_planned(conn, item_id, version, title, content, actor, None)
}

/// The short code that a new row has until [`finish_create`] gives it its
/// code (KAIROS-T-0633). Unique, and it matches no code of the rule, so no
/// trigger and no reader takes it for a code. No other transaction sees it:
/// the row is not committed until it has its code.
fn pending_code() -> String {
    format!("pending-{}", Uuid::new_v4())
}

/// Create-time bookkeeping shared by all five create services, after the
/// insert of the row under [`pending_code`]: the v1 baseline `item_history`
/// snapshot (see module docs), then the short code ([`next_short_code`],
/// written to the row), then the mentions, the `activity_log` create row,
/// and the `item_created` thin event (KAIROS-T-0022 — emitted inside the
/// transaction, delivered by PostgreSQL on commit). Gives the code.
///
/// KAIROS-T-0633: the number comes after the insert and the history row,
/// so the lock of the row (prefix, type) of `short_code_sequences` does
/// not cover them. The update of the code fires the retired-code trigger
/// with the final code, and each write after it reads the final code.
fn finish_create(
    conn: &mut PgConnection,
    actor: Uuid,
    item_type: ItemType,
    item_id: Uuid,
    board_id: Option<Uuid>,
    title: &str,
    content: &str,
) -> Result<String, ItemError> {
    insert_history(conn, item_id, 1, title, content, actor)?;
    let code = next_short_code(conn, item_type, board_id)?;
    let table = table_of(item_type);
    sql_query(format!("UPDATE {table} SET short_code = $1 WHERE id = $2"))
        .bind::<Text, _>(&code)
        .bind::<diesel::sql_types::Uuid, _>(item_id)
        .execute(conn)?;
    // KAIROS-T-0381: each mention in the content of a new item.
    let names: Vec<String> = kairos_core::notifications::mentions(content)
        .into_keys()
        .collect();
    crate::notifications::on_mentions(conn, item_id, &names, actor)?;
    log_activity(
        conn,
        actor,
        ActivityAction::Create,
        item_id,
        item_type.entity_type(),
        format!("short_code:{code}"),
    )?;
    events::emit_item_event_by_id(
        conn,
        EventKind::ItemCreated,
        item_type.entity_type(),
        item_id,
        actor,
    )?;
    Ok(code)
}

/// Resolve an item's column placement on `board_id`: an explicit
/// `column_id` is validated to belong to the board and still be live;
/// `None` defaults to the board's first LIVE column (position order).
fn resolve_column(
    conn: &mut PgConnection,
    board_id: Uuid,
    column_id: Option<Uuid>,
) -> Result<Uuid, ItemError> {
    use crate::schema::{board_columns, boards};

    let board_exists: Option<Uuid> = boards::table
        .filter(boards::id.eq(board_id))
        .filter(boards::deleted_at.is_null())
        .select(boards::id)
        .first(conn)
        .optional()?;
    if board_exists.is_none() {
        return Err(ItemError::BoardNotFound(board_id));
    }

    // Live columns only: a removed column (KAIROS-T-0161) is not a column
    // of the board any more, so nothing new may land in it — naming one
    // reads as "not on this board", which is what it is.
    match column_id {
        Some(column_id) => board_columns::table
            .filter(board_columns::id.eq(column_id))
            .filter(board_columns::board_id.eq(board_id))
            .filter(board_columns::deleted_at.is_null())
            .select(board_columns::id)
            .first(conn)
            .optional()?
            .ok_or(ItemError::ColumnNotOnBoard {
                board_id,
                column_id,
            }),
        None => board_columns::table
            .filter(board_columns::board_id.eq(board_id))
            .filter(board_columns::deleted_at.is_null())
            .order(board_columns::position.asc())
            .select(board_columns::id)
            .first(conn)
            .optional()?
            .ok_or(ItemError::BoardHasNoColumns(board_id)),
    }
}

// ---------------------------------------------------------------------------
// Per-table content operations (macro over the five entity tables)
// ---------------------------------------------------------------------------

/// A content edit for [`update_item_content`]: `new_title = None` keeps the
/// existing title; `expected_version` is the version the client based its
/// edit on (KAIROS-A-0004 step 2: "I'm editing version N").
#[derive(Debug, Clone)]
pub struct ContentUpdate<'a> {
    pub new_title: Option<&'a str>,
    pub new_content: &'a str,
    pub expected_version: i32,
}

/// Generate the per-table primitives the generic write path dispatches
/// over: the live-row load, and the bulk soft-delete. The atomic content
/// UPDATE is [`apply_content_update`], one SQL text for the five tables.
macro_rules! content_table_ops {
    ($table:ident, $load_fn:ident, $delete_fn:ident, $preview_fn:ident) => {
        /// `(version, title, content, short_code)` of the live row, or
        /// `None` if missing/soft-deleted.
        fn $load_fn(
            conn: &mut PgConnection,
            item_id: Uuid,
        ) -> Result<Option<(i32, String, String, String)>, DieselError> {
            use crate::schema::$table::dsl;
            dsl::$table
                .filter(dsl::id.eq(item_id))
                .filter(dsl::deleted_at.is_null())
                .select((dsl::version, dsl::title, dsl::content, dsl::short_code))
                .first(conn)
                .optional()
        }

        /// Soft-delete every LIVE row of this table whose id is in `ids`
        /// (already-deleted rows are left untouched); returns the short
        /// codes actually deleted.
        ///
        /// KAIROS-T-0634: first lock the rows in the order of the ids. A
        /// blocker that closes locks its dependants in this order too
        /// ([`crate::dispatch::on_dependants_freed`]), so the two wait in
        /// turn and do not lock each other out. An `UPDATE` alone locks
        /// its rows in the order of the scan.
        fn $delete_fn(
            conn: &mut PgConnection,
            ids: &[Uuid],
            actor: Uuid,
        ) -> Result<Vec<String>, DieselError> {
            use crate::schema::$table::dsl;
            dsl::$table
                .filter(dsl::id.eq_any(ids))
                .filter(dsl::deleted_at.is_null())
                .order(dsl::id)
                .select(dsl::id)
                .for_update()
                .load::<Uuid>(conn)?;
            diesel::update(
                dsl::$table
                    .filter(dsl::id.eq_any(ids))
                    .filter(dsl::deleted_at.is_null()),
            )
            .set((
                dsl::deleted_at.eq(Some(chrono::Utc::now())),
                dsl::updated_by.eq(actor),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .returning(dsl::short_code)
            .get_results(conn)
        }

        /// The short codes of the LIVE rows of this table whose id is in
        /// `ids` — the read-only mirror of `$delete_fn` (same
        /// `deleted_at IS NULL` filter, no write). Backs
        /// [`preview_cascade`]: it reports exactly what `$delete_fn` WOULD
        /// return, so preview and actual cascade cannot disagree.
        fn $preview_fn(conn: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<String>, DieselError> {
            use crate::schema::$table::dsl;
            dsl::$table
                .filter(dsl::id.eq_any(ids))
                .filter(dsl::deleted_at.is_null())
                .select(dsl::short_code)
                .load(conn)
        }
    };
}

content_table_ops!(
    strategies,
    load_live_strategy,
    soft_delete_strategies,
    live_short_codes_strategies
);
content_table_ops!(
    initiatives,
    load_live_initiative,
    soft_delete_initiatives,
    live_short_codes_initiatives
);
content_table_ops!(
    tasks,
    load_live_task,
    soft_delete_tasks,
    live_short_codes_tasks
);
content_table_ops!(
    documents,
    load_live_document,
    soft_delete_documents,
    live_short_codes_documents
);
content_table_ops!(adrs, load_live_adr, soft_delete_adrs, live_short_codes_adrs);

/// The row that [`apply_content_update`] gives back: the new version, the
/// title, and the newest full row of the history before it
/// (KAIROS-T-0651).
#[derive(QueryableByName)]
struct AppliedContent {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    version: i32,
    #[diesel(sql_type = diesel::sql_types::Text)]
    title: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Integer>)]
    base_version: Option<i32>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    base_content: Option<String>,
}

/// The newest full row of the history before a new version, as the
/// UPDATE gives it: `(version, text)`. The text is `None` when the new
/// version must be a full row ([`kairos_core::history_diff`]).
type HistoryBase = Option<(i32, Option<String>)>;

/// Atomic KAIROS-A-0004 write: `UPDATE … SET version = version + 1 …
/// WHERE id = ? AND version = expected AND deleted_at IS NULL`.
/// `Ok(None)` = no row matched (stale version, missing, or soft-deleted);
/// `Ok(Some((new_version, title, base)))` on success. The content is not
/// given back: the caller has it (KAIROS-T-0635).
///
/// KAIROS-T-0651: the same statement gives the newest full row of the
/// history, the base of the diff row of the new version. Its text comes
/// back only when the new version can be a diff row: fewer than
/// [`kairos_core::history_diff::SNAPSHOT_EVERY`] versions after it. So the
/// write reads the base and sends a small diff, where it sent a second
/// copy of the content before.
fn apply_content_update(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    update: &ContentUpdate<'_>,
    actor: Uuid,
) -> Result<Option<(i32, String, HistoryBase)>, DieselError> {
    let row: Option<AppliedContent> = sql_query(format!(
        "UPDATE {table} AS t \
            SET version = t.version + 1, title = coalesce($3, t.title), content = $4, \
                updated_by = $5, updated_at = now() \
          WHERE t.id = $1 AND t.version = $2 AND t.deleted_at IS NULL \
         RETURNING t.version, t.title, \
           (SELECT h.version FROM item_history h \
             WHERE h.item_id = t.id AND h.base_version IS NULL \
             ORDER BY h.version DESC LIMIT 1) AS base_version, \
           (SELECT h.content FROM item_history h \
             WHERE h.item_id = t.id AND h.base_version IS NULL \
               AND h.version > t.version - $6 \
             ORDER BY h.version DESC LIMIT 1) AS base_content",
        table = table_of(item_type)
    ))
    .bind::<diesel::sql_types::Uuid, _>(item_id)
    .bind::<diesel::sql_types::Integer, _>(update.expected_version)
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(update.new_title)
    .bind::<diesel::sql_types::Text, _>(update.new_content)
    .bind::<diesel::sql_types::Uuid, _>(actor)
    .bind::<diesel::sql_types::Integer, _>(kairos_core::history_diff::SNAPSHOT_EVERY)
    .get_result(conn)
    .optional()?;
    Ok(row.map(|r| {
        let base = r.base_version.map(|v| (v, r.base_content));
        (r.version, r.title, base)
    }))
}

/// Dispatch the live-row load to the item's table.
fn load_live_content(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<Option<(i32, String, String, String)>, DieselError> {
    match item_type {
        ItemType::Strategy => load_live_strategy(conn, item_id),
        ItemType::Initiative => load_live_initiative(conn, item_id),
        ItemType::Task => load_live_task(conn, item_id),
        ItemType::Document => load_live_document(conn, item_id),
        ItemType::Adr => load_live_adr(conn, item_id),
    }
}

// ---------------------------------------------------------------------------
// Create services (short code + version 1 + v1 history baseline + audit)
// ---------------------------------------------------------------------------

/// Input for [`create_strategy`].
#[derive(Debug, Clone)]
pub struct CreateStrategy<'a> {
    pub board_id: Uuid,
    /// `None` defaults to the board's first column (position 0).
    pub column_id: Option<Uuid>,
    pub title: &'a str,
    pub content: &'a str,
    pub hypothesis: Option<&'a str>,
}

/// Create a strategy: assign the next `{PREFIX}-S-{NNNN}` short code,
/// insert at version 1, write the v1 history baseline, and log the
/// `create` activity row — one transaction.
pub fn create_strategy(
    conn: &mut PgConnection,
    input: CreateStrategy<'_>,
    actor: Uuid,
) -> Result<Strategy, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        let column_id = resolve_column(conn, input.board_id, input.column_id)?;
        let mut created: Strategy = diesel::insert_into(crate::schema::strategies::table)
            .values(NewStrategy {
                short_code: pending_code(),
                title: input.title.to_string(),
                content: input.content.to_string(),
                board_id: input.board_id,
                column_id,
                hypothesis: input.hypothesis.map(str::to_string),
                created_by: actor,
                updated_by: actor,
            })
            .returning(Strategy::as_returning())
            .get_result(conn)?;
        created.short_code = finish_create(
            conn,
            actor,
            ItemType::Strategy,
            created.id,
            Some(input.board_id),
            &created.title,
            &created.content,
        )?;
        // KAIROS-T-0719: a create puts the item in a column, as a move does;
        // the dispatch rules of that column fire, in this transaction.
        crate::dispatch::on_item_entered_column(conn, created.id, column_id, actor)?;
        Ok(created)
    })
}

/// Input for [`create_initiative`]. `bucket_type: Some(_)` marks the
/// initiative as a bucket (`is_bucket` is derived — the DDL enforces
/// `is_bucket = true <=> bucket_type IS NOT NULL`).
#[derive(Debug, Clone)]
pub struct CreateInitiative<'a> {
    pub board_id: Uuid,
    /// `None` defaults to the board's first column (position 0).
    pub column_id: Option<Uuid>,
    pub title: &'a str,
    pub content: &'a str,
    pub complexity: Option<Complexity>,
    pub bucket_type: Option<BucketType>,
}

/// Create an initiative (see [`create_strategy`] for the shared contract).
pub fn create_initiative(
    conn: &mut PgConnection,
    input: CreateInitiative<'_>,
    actor: Uuid,
) -> Result<Initiative, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        let column_id = resolve_column(conn, input.board_id, input.column_id)?;
        let mut created: Initiative = diesel::insert_into(crate::schema::initiatives::table)
            .values(NewInitiative {
                short_code: pending_code(),
                title: input.title.to_string(),
                content: input.content.to_string(),
                board_id: input.board_id,
                column_id,
                complexity: input.complexity,
                is_bucket: input.bucket_type.is_some(),
                bucket_type: input.bucket_type,
                created_by: actor,
                updated_by: actor,
            })
            .returning(Initiative::as_returning())
            .get_result(conn)?;
        created.short_code = finish_create(
            conn,
            actor,
            ItemType::Initiative,
            created.id,
            Some(input.board_id),
            &created.title,
            &created.content,
        )?;
        // KAIROS-T-0719: a create puts the item in a column, as a move does;
        // the dispatch rules of that column fire, in this transaction.
        crate::dispatch::on_item_entered_column(conn, created.id, column_id, actor)?;
        Ok(created)
    })
}

/// Input for [`create_task`].
#[derive(Debug, Clone)]
pub struct CreateTask<'a> {
    pub board_id: Uuid,
    /// `None` defaults to the board's first column (position 0).
    pub column_id: Option<Uuid>,
    pub title: &'a str,
    pub content: &'a str,
    pub task_type: TaskType,
    /// The Planned/Support lane (KAIROS-T-0077).
    pub work_class: WorkClass,
    /// The repository the task is issued against (KAIROS-A-0019). Plumbed
    /// here; the repo -> team -> board routing rule is enforced by the
    /// service layer above (KAIROS-T-0104).
    ///
    /// There is deliberately no `team_id` beside it (COLLIERY-T-0216,
    /// COLLIERY-A-0023 rule 2): the team of a task is the team of its board,
    /// so [`create_task`] reads it from the board rather than being told.
    /// While a caller could send one, it could send the wrong one, and four
    /// reads - a team's work documents, its link rollup, the search team
    /// filter and the embedding text - would then file the task under a team
    /// whose board it is not on.
    pub repository_id: Option<Uuid>,
}

/// The team a board belongs to, or `None` for a board with no team
/// (COLLIERY-T-0216). THE source of a task's team: create reads it here and
/// [`crate::boards::move_task`] reads the target board's the same way, so
/// the two cannot disagree about whose work a task is.
///
/// `None` is for old data only. Since COLLIERY-T-0230 a delivery board is
/// not created with no team, but no migration gives a team to a board that
/// was created before, so that row can exist and must be read.
pub fn board_team(conn: &mut PgConnection, board_id: Uuid) -> Result<Option<Uuid>, ItemError> {
    use crate::schema::boards;
    Ok(boards::table
        .filter(boards::id.eq(board_id))
        .select(boards::team_id)
        .first::<Option<Uuid>>(conn)
        .optional()?
        .flatten())
}

/// Create a task/bug/tech-debt item (see [`create_strategy`] for the
/// shared contract).
pub fn create_task(
    conn: &mut PgConnection,
    input: CreateTask<'_>,
    actor: Uuid,
) -> Result<Task, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        let column_id = resolve_column(conn, input.board_id, input.column_id)?;
        // COLLIERY-T-0216: the board decides the team. `resolve_column` has
        // already refused a board that does not exist, so a missing row here
        // is a teamless board (`None`), which is a valid answer.
        let team_id = board_team(conn, input.board_id)?;
        let mut created: Task = diesel::insert_into(crate::schema::tasks::table)
            .values(NewTask {
                short_code: pending_code(),
                title: input.title.to_string(),
                content: input.content.to_string(),
                board_id: input.board_id,
                column_id,
                task_type: input.task_type,
                work_class: input.work_class,
                team_id,
                repository_id: input.repository_id,
                created_by: actor,
                updated_by: actor,
            })
            .returning(Task::as_returning())
            .get_result(conn)?;
        created.short_code = finish_create(
            conn,
            actor,
            ItemType::Task,
            created.id,
            Some(input.board_id),
            &created.title,
            &created.content,
        )?;
        // KAIROS-T-0566: a create puts the task in a column, as a move
        // does. The dispatch rules of that column fire, in this
        // transaction. The column is the entry column for each caller but
        // the demo seed (the server refuses any other).
        crate::dispatch::on_item_entered_column(conn, created.id, column_id, actor)?;
        Ok(created)
    })
}

/// Set a task's Planned/Support lane (KAIROS-T-0077). The lane is
/// orthogonal to the board rules engine — it never affects transition
/// legality — so this is an item write, not a board transition: update +
/// `activity_log` (`work_class` action) + `item_updated` thin event, one
/// transaction. Setting the value the task already has is a no-op (no
/// log row, no event).
pub fn set_task_work_class(
    conn: &mut PgConnection,
    task_id: Uuid,
    work_class: WorkClass,
    actor: Uuid,
) -> Result<Task, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::tasks::dsl;
        let current: Task = dsl::tasks
            .filter(dsl::id.eq(task_id))
            .filter(dsl::deleted_at.is_null())
            .select(Task::as_select())
            .first(conn)
            .optional()?
            .ok_or(ItemError::ItemNotFound {
                entity_type: "task",
                id: task_id,
            })?;
        if current.work_class == work_class {
            return Ok(current);
        }
        let updated: Task = diesel::update(dsl::tasks.filter(dsl::id.eq(task_id)))
            .set((
                dsl::work_class.eq(work_class),
                dsl::updated_by.eq(actor),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .returning(Task::as_returning())
            .get_result(conn)?;
        log_activity(
            conn,
            actor,
            ActivityAction::WorkClass,
            task_id,
            "task",
            format!(
                "work_class:{}->{}",
                current.work_class.as_str(),
                work_class.as_str()
            ),
        )?;
        events::emit_item_event_by_id(conn, EventKind::ItemUpdated, "task", task_id, actor)?;
        Ok(updated)
    })
}

/// Bind a task to a repository, or clear it (KAIROS-T-0103, A-0019).
/// Like [`set_task_work_class`] this is not content: update +
/// `activity_log` (`repository` action) + `item_updated` event, no
/// `item_history` version bump. The repo -> team -> board consistency
/// rule is the caller's (KAIROS-T-0104); this only checks the repository
/// is live, and it never touches `team_id` (COLLIERY-T-0216). Setting the
/// value the task already has is a no-op.
pub fn set_task_repository(
    conn: &mut PgConnection,
    task_id: Uuid,
    repository_id: Option<Uuid>,
    actor: Uuid,
) -> Result<Task, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::tasks::dsl;
        let current: Task = dsl::tasks
            .filter(dsl::id.eq(task_id))
            .filter(dsl::deleted_at.is_null())
            .select(Task::as_select())
            .first(conn)
            .optional()?
            .ok_or(ItemError::ItemNotFound {
                entity_type: "task",
                id: task_id,
            })?;
        if current.repository_id == repository_id {
            return Ok(current);
        }
        // The repository must be live. Its owner is looked up only to prove
        // that, and is then dropped: until COLLIERY-T-0216 a bind also
        // rewrote `team_id` to the repository's owner (KAIROS-T-0112), which
        // moved the task into that team's rollups while it sat on another
        // team's board. The board decides the team now, and a link to a
        // repository changes nothing else about the task.
        if let Some(repo) = repository_id {
            use crate::schema::repositories;
            let live: Option<Uuid> = repositories::table
                .filter(repositories::id.eq(repo))
                .filter(repositories::deleted_at.is_null())
                .select(repositories::id)
                .first(conn)
                .optional()?;
            if live.is_none() {
                return Err(ItemError::RepositoryNotFound(repo));
            }
        }
        let updated: Task = diesel::update(dsl::tasks.filter(dsl::id.eq(task_id)))
            .set((
                dsl::repository_id.eq(repository_id),
                dsl::updated_by.eq(actor),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .returning(Task::as_returning())
            .get_result(conn)?;
        log_activity(
            conn,
            actor,
            ActivityAction::Repository,
            task_id,
            "task",
            format!(
                "repository:{}->{}",
                current
                    .repository_id
                    .map_or_else(|| "none".to_string(), |id| id.to_string()),
                repository_id.map_or_else(|| "none".to_string(), |id| id.to_string())
            ),
        )?;
        events::emit_item_event_by_id(conn, EventKind::ItemUpdated, "task", task_id, actor)?;
        Ok(updated)
    })
}

/// Whether a metadata definition applies to `entity_type`
/// (KAIROS-T-0078): no scope rows = applies to every type; otherwise the
/// type must be listed. THE enforcement primitive — both write paths
/// (the metadata PATCH and template stamping) go through it, so the
/// scoping rule lives in the data layer, not in a server-side map.
pub fn definition_applies_to(
    conn: &mut PgConnection,
    definition_id: Uuid,
    entity_type: &str,
) -> Result<bool, DieselError> {
    use crate::schema::metadata_definition_scopes::dsl;
    let scopes: Vec<String> = dsl::metadata_definition_scopes
        .filter(dsl::metadata_definition_id.eq(definition_id))
        .select(dsl::entity_type)
        .load(conn)?;
    Ok(scopes.is_empty() || scopes.iter().any(|scope| scope == entity_type))
}

/// Set a document's editorial lifecycle (KAIROS-T-0078). A label change,
/// not a content edit: activity-logged (`lifecycle:{from}->{to}`) and
/// announced via the existing `item_updated` thin event, with NO version
/// bump and NO `item_history` row — the A-0004 versioning contract covers
/// title/content only, exactly like `work_class` (KAIROS-T-0077).
/// Transitions are free (any state to any state). Setting the current
/// value is a no-op.
pub fn set_document_lifecycle(
    conn: &mut PgConnection,
    document_id: Uuid,
    lifecycle: DocumentLifecycle,
    actor: Uuid,
) -> Result<Document, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::documents::dsl;
        let current: Document = dsl::documents
            .filter(dsl::id.eq(document_id))
            .filter(dsl::deleted_at.is_null())
            .select(Document::as_select())
            .first(conn)
            .optional()?
            .ok_or(ItemError::ItemNotFound {
                entity_type: "document",
                id: document_id,
            })?;
        if current.lifecycle == lifecycle {
            return Ok(current);
        }
        let updated: Document = diesel::update(dsl::documents.filter(dsl::id.eq(document_id)))
            .set((
                dsl::lifecycle.eq(lifecycle),
                dsl::updated_by.eq(actor),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .returning(Document::as_returning())
            .get_result(conn)?;
        log_activity(
            conn,
            actor,
            ActivityAction::Lifecycle,
            document_id,
            "document",
            format!(
                "lifecycle:{}->{}",
                current.lifecycle.as_str(),
                lifecycle.as_str()
            ),
        )?;
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemUpdated,
            "document",
            document_id,
            actor,
        )?;
        Ok(updated)
    })
}

/// Set the complexity of an initiative, or clear it with `None`. Like
/// [`set_document_lifecycle`] this is not content: activity-logged
/// (`complexity:{from}->{to}`, `none` for no complexity) and announced via
/// `item_updated`, with NO version bump and NO `item_history` row. Setting
/// the current value is a no-op.
pub fn set_initiative_complexity(
    conn: &mut PgConnection,
    initiative_id: Uuid,
    complexity: Option<Complexity>,
    actor: Uuid,
) -> Result<Initiative, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::initiatives::dsl;
        let current: Initiative = dsl::initiatives
            .filter(dsl::id.eq(initiative_id))
            .filter(dsl::deleted_at.is_null())
            .select(Initiative::as_select())
            .first(conn)
            .optional()?
            .ok_or(ItemError::ItemNotFound {
                entity_type: "initiative",
                id: initiative_id,
            })?;
        if current.complexity == complexity {
            return Ok(current);
        }
        let updated: Initiative =
            diesel::update(dsl::initiatives.filter(dsl::id.eq(initiative_id)))
                .set((
                    dsl::complexity.eq(complexity),
                    dsl::updated_by.eq(actor),
                    dsl::updated_at.eq(diesel::dsl::now),
                ))
                .returning(Initiative::as_returning())
                .get_result(conn)?;
        let name = |c: Option<Complexity>| c.map_or("none", |c| c.as_str());
        log_activity(
            conn,
            actor,
            ActivityAction::Complexity,
            initiative_id,
            "initiative",
            format!(
                "complexity:{}->{}",
                name(current.complexity),
                name(complexity)
            ),
        )?;
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemUpdated,
            "initiative",
            initiative_id,
            actor,
        )?;
        Ok(updated)
    })
}

/// A live board with this id exists, or [`ItemError::BoardNotFound`].
fn require_live_board(conn: &mut PgConnection, board_id: Uuid) -> Result<(), ItemError> {
    use crate::schema::boards::dsl;
    let live: Option<Uuid> = dsl::boards
        .filter(dsl::id.eq(board_id))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .optional()?;
    live.map(|_| ()).ok_or(ItemError::BoardNotFound(board_id))
}

/// What [`set_document_board`] did (COLLIERY-T-0269).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentBoardChange {
    /// The document after the call.
    pub document: Document,
    /// The board that the document named before the call.
    pub from: Uuid,
    /// The call changed the row. `false` = the document named that board
    /// already, and the call wrote nothing.
    pub changed: bool,
}

/// Set or change the OWNER board of a document (COLLIERY-T-0269). The
/// board cannot be removed: each document has one (COLLIERY-T-3109).
/// As the lifecycle, this is not a content edit: no version bump and no
/// `item_history` row. It writes one `activity_log` row with the action
/// `update` (`owner_board:{from}->{to}`, each end the slug of a board) and
/// the `relationship_changed` event, which makes an open page
/// of the document read it again. Setting the board that the document has
/// is a no-op: it writes nothing.
///
/// A new board must be live ([`ItemError::BoardNotFound`]).
///
/// NO RULE is applied here. Who may change the owner is the caller's: the
/// server asks it in the transaction of this call.
pub fn set_document_board(
    conn: &mut PgConnection,
    document_id: Uuid,
    board_id: Uuid,
    actor: Uuid,
) -> Result<DocumentBoardChange, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::documents::dsl;
        let current: Document = dsl::documents
            .filter(dsl::id.eq(document_id))
            .filter(dsl::deleted_at.is_null())
            .select(Document::as_select())
            .first(conn)
            .optional()?
            .ok_or(ItemError::ItemNotFound {
                entity_type: "document",
                id: document_id,
            })?;
        let from = current.board_id;
        if from == board_id {
            return Ok(DocumentBoardChange {
                document: current,
                from,
                changed: false,
            });
        }
        require_live_board(conn, board_id)?;
        // KAIROS-A-0079: a dictionary stays on the board `strategy`, so a
        // move of a dictionary is refused (the board is not `strategy`).
        if crate::dictionaries::is_dictionary_document(conn, document_id)? {
            refuse_dictionary_board(conn, board_id)?;
        }
        let updated: Document = diesel::update(dsl::documents.filter(dsl::id.eq(document_id)))
            .set((
                dsl::board_id.eq(board_id),
                dsl::updated_by.eq(actor),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .returning(Document::as_returning())
            .get_result(conn)?;
        // KAIROS-T-0731: the check of the document, as for each write.
        crate::section_checks::on_document_write(
            conn,
            document_id,
            updated.version,
            &updated.content,
        )?;
        let details = format!(
            "owner_board:{}->{}",
            board_slug(conn, from)?,
            board_slug(conn, board_id)?
        );
        log_activity(
            conn,
            actor,
            ActivityAction::Update,
            document_id,
            "document",
            details,
        )?;
        events::emit_item_event_by_id(
            conn,
            EventKind::RelationshipChanged,
            "document",
            document_id,
            actor,
        )?;
        Ok(DocumentBoardChange {
            document: updated,
            from,
            changed: true,
        })
    })
}

/// The short codes of the LIVE documents that name `board_id` as their
/// owner, sorted (COLLIERY-T-0269). A board with one or more cannot be
/// deleted: a document always has an owner, and a deleted board is none.
/// An archived document does not count, as an archived card does not
/// (KAIROS-I-0012).
pub fn live_documents_of_board(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<Vec<String>, DieselError> {
    use crate::schema::documents::dsl;
    dsl::documents
        .filter(dsl::board_id.eq(board_id))
        .filter(dsl::deleted_at.is_null())
        .order(dsl::short_code.asc())
        .select(dsl::short_code)
        .load(conn)
}

/// The slug of a board, deleted or not, for the text of a record. An id
/// that names no board gives the id.
fn board_slug(conn: &mut PgConnection, board_id: Uuid) -> Result<String, DieselError> {
    use crate::schema::boards::dsl;
    let slug: Option<String> = dsl::boards
        .filter(dsl::id.eq(board_id))
        .select(dsl::slug)
        .first(conn)
        .optional()?;
    Ok(slug.unwrap_or_else(|| board_id.to_string()))
}

/// The checks of a dictionary on `board_id`, a live board:
/// [`ItemError::DictionaryNotOnStrategy`] when it is not the board
/// `strategy` (KAIROS-A-0079), and [`ItemError::DictionaryExists`] when it
/// owns a live dictionary already (KAIROS-T-0733). Locks the row of the
/// board to the end of the transaction.
fn refuse_dictionary_board(conn: &mut PgConnection, board_id: Uuid) -> Result<(), ItemError> {
    let board = board_slug(conn, board_id)?;
    if board != crate::dictionaries::ORGANIZATION_BOARD_SLUG {
        return Err(ItemError::DictionaryNotOnStrategy { board });
    }
    match crate::dictionaries::live_dictionary_of_board(conn, board_id)? {
        Some((_, existing)) => Err(ItemError::DictionaryExists { board, existing }),
        None => Ok(()),
    }
}

/// Input for [`create_document`].
#[derive(Debug, Clone)]
pub struct CreateDocument<'a> {
    /// The OWNER board (COLLIERY-T-0269), required (COLLIERY-T-3109). It
    /// must be live ([`ItemError::BoardNotFound`]). It can have each level:
    /// the owner of a document is a board, and not a column of it. The
    /// code of the document gets the prefix of this board.
    pub board_id: Uuid,
    pub title: &'a str,
    /// Explicit content wins; `None` copies the template's content when
    /// `template_id` is given, else empty.
    pub content: Option<&'a str>,
    /// KAIROS-A-0003: when given, the template's content seeds the document
    /// and its `template_metadata` defaults are stamped as `item_metadata`
    /// rows.
    pub template_id: Option<Uuid>,
}

/// Create a document on its owner board (`input.board_id`,
/// COLLIERY-T-3109). With a `template_id`, the template's content is
/// copied (unless `content` overrides it) and every `template_metadata`
/// row carrying a `default_value` is stamped as an `item_metadata` row
/// (KAIROS-A-0003).
///
/// The document is not a card of the board. It has no column, and the
/// event of the create has no board.
pub fn create_document(
    conn: &mut PgConnection,
    input: CreateDocument<'_>,
    actor: Uuid,
) -> Result<Document, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::{item_metadata, template_metadata, templates};

        let board_id = input.board_id;
        require_live_board(conn, board_id)?;

        let template: Option<Template> = match input.template_id {
            Some(template_id) => Some(
                templates::table
                    .filter(templates::id.eq(template_id))
                    .first(conn)
                    .optional()?
                    .ok_or(ItemError::TemplateNotFound(template_id))?,
            ),
            None => None,
        };
        if template
            .as_ref()
            .is_some_and(|t| t.slug == crate::dictionaries::DICTIONARY_TEMPLATE_SLUG)
        {
            refuse_dictionary_board(conn, board_id)?;
        }
        let content = input
            .content
            .unwrap_or_else(|| template.as_ref().map_or("", |t| t.content.as_str()));

        let mut created: Document = diesel::insert_into(crate::schema::documents::table)
            .values(NewDocument {
                short_code: pending_code(),
                title: input.title.to_string(),
                content: content.to_string(),
                template_id: input.template_id,
                board_id,
                created_by: actor,
                updated_by: actor,
            })
            .returning(Document::as_returning())
            .get_result(conn)?;

        if let Some(template) = &template {
            let defaults: Vec<(Uuid, Option<String>)> = template_metadata::table
                .filter(template_metadata::template_id.eq(template.id))
                .select((
                    template_metadata::metadata_definition_id,
                    template_metadata::default_value,
                ))
                .load(conn)?;
            let mut stamps: Vec<NewItemMetadata> = Vec::with_capacity(defaults.len());
            for (metadata_definition_id, default_value) in defaults {
                let Some(value) = default_value else { continue };
                // KAIROS-T-0078: stamping is a metadata write path — a
                // definition scoped away from documents never stamps,
                // whatever a template association claims.
                if !definition_applies_to(conn, metadata_definition_id, "document")? {
                    continue;
                }
                stamps.push(NewItemMetadata {
                    item_id: created.id,
                    metadata_definition_id,
                    value,
                });
            }
            if !stamps.is_empty() {
                diesel::insert_into(item_metadata::table)
                    .values(&stamps)
                    .execute(conn)?;
            }
        }

        created.short_code = finish_create(
            conn,
            actor,
            ItemType::Document,
            created.id,
            Some(board_id),
            &created.title,
            &created.content,
        )?;
        // KAIROS-T-0731: the check of the typed sections of the template.
        crate::section_checks::on_document_write(
            conn,
            created.id,
            created.version,
            &created.content,
        )?;
        Ok(created)
    })
}

/// Input for [`create_adr`]. ADR board placement is optional (the DDL
/// enforces `board_id`/`column_id` both NULL or both set): with
/// `board_id: Some(_)`, `column_id` resolves like the other board items
/// (default first column); with `board_id: None` the ADR is off-board and
/// `column_id` is ignored.
#[derive(Debug, Clone)]
pub struct CreateAdr<'a> {
    pub board_id: Option<Uuid>,
    pub column_id: Option<Uuid>,
    pub title: &'a str,
    pub content: &'a str,
    pub decision_maker: Option<&'a str>,
    pub decision_date: Option<NaiveDate>,
}

/// Create an ADR (see [`create_strategy`] for the shared contract and
/// [`CreateAdr`] for placement semantics).
pub fn create_adr(
    conn: &mut PgConnection,
    input: CreateAdr<'_>,
    actor: Uuid,
) -> Result<Adr, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        let placement = match input.board_id {
            Some(board_id) => Some((board_id, resolve_column(conn, board_id, input.column_id)?)),
            None => None,
        };
        let mut created: Adr = diesel::insert_into(crate::schema::adrs::table)
            .values(NewAdr {
                short_code: pending_code(),
                title: input.title.to_string(),
                content: input.content.to_string(),
                board_id: placement.map(|(board_id, _)| board_id),
                column_id: placement.map(|(_, column_id)| column_id),
                decision_maker: input.decision_maker.map(str::to_string),
                decision_date: input.decision_date,
                created_by: actor,
                updated_by: actor,
            })
            .returning(Adr::as_returning())
            .get_result(conn)?;
        created.short_code = finish_create(
            conn,
            actor,
            ItemType::Adr,
            created.id,
            placement.map(|(board_id, _)| board_id),
            &created.title,
            &created.content,
        )?;
        Ok(created)
    })
}

// ---------------------------------------------------------------------------
// Content update / rollback (KAIROS-A-0004)
// ---------------------------------------------------------------------------

/// Apply a content edit with optimistic concurrency (KAIROS-A-0004), in ONE
/// transaction: the version check and write are a single atomic `UPDATE …
/// WHERE version = expected_version`, and success appends the new version's
/// `item_history` snapshot. Returns the new version number.
///
/// A stale `expected_version` returns the typed
/// [`ItemError::VersionConflict`] carrying the item's current version,
/// title, and content — conflict resolution is client-side (A-0004 step 4).
///
/// KAIROS-T-0635: the UPDATE does not give the content back, and the
/// history row and the mention scan use `update.new_content`. When the new
/// text has a mention, one read under the lock of the row
/// (`FOR UPDATE`) gives the content before and, for a stale version, the
/// conflict answer. With no mention there is no read.
pub fn update_item_content(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    update: ContentUpdate<'_>,
    actor: Uuid,
) -> Result<i32, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        write_content(conn, item_type, item_id, &update, Before::Read, actor)
    })
}

/// [`update_item_content`] for a caller that read the content at
/// `update.expected_version`, as `edit_item` does (KAIROS-T-0635): `before`
/// is that content. The write reads no content. A content does not change
/// without a new version, so when the UPDATE matches, `before` is the
/// content that it replaces. A stale version is the same
/// [`ItemError::VersionConflict`].
pub fn edit_item_content(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    update: ContentUpdate<'_>,
    before: &str,
    actor: Uuid,
) -> Result<i32, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        write_content(
            conn,
            item_type,
            item_id,
            &update,
            Before::Known(before),
            actor,
        )
    })
}

/// The content before a write, for the new mentions (KAIROS-T-0381).
enum Before<'a> {
    /// Read it when the new text has a mention.
    Read,
    /// The caller has it: the content at `expected_version`.
    Known(&'a str),
}

/// `(version, title, content)` of the live row, locked until the end of
/// the transaction (`FOR UPDATE`).
#[derive(QueryableByName)]
struct LockedContent {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    version: i32,
    #[diesel(sql_type = diesel::sql_types::Text)]
    title: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    content: String,
}

/// The body of [`update_item_content`], [`edit_item_content`] and
/// [`rollback_item`], in the transaction of the caller: no savepoint.
fn write_content(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    update: &ContentUpdate<'_>,
    before: Before<'_>,
    actor: Uuid,
) -> Result<i32, ItemError> {
    // KAIROS-T-0381: the content before the update, to find the new
    // mentions. Needed only when the new content has a mention.
    let has_mention = !kairos_core::notifications::mentions(update.new_content).is_empty();
    let before: Option<std::borrow::Cow<'_, str>> = match before {
        _ if !has_mention => None,
        Before::Known(text) => Some(std::borrow::Cow::Borrowed(text)),
        Before::Read => {
            let locked: LockedContent = sql_query(format!(
                "SELECT version, title, content FROM {} \
                 WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
                table_of(item_type)
            ))
            .bind::<diesel::sql_types::Uuid, _>(item_id)
            .get_result(conn)
            .optional()?
            .ok_or(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            })?;
            if locked.version != update.expected_version {
                return Err(ItemError::VersionConflict {
                    item_id,
                    expected_version: update.expected_version,
                    current_version: locked.version,
                    current_title: locked.title,
                    current_content: locked.content,
                });
            }
            Some(std::borrow::Cow::Owned(locked.content))
        }
    };
    match apply_content_update(conn, item_type, item_id, update, actor)? {
        Some((new_version, title, base)) => {
            // KAIROS-T-0651: the base of the diff came with the UPDATE.
            crate::history::insert_planned(
                conn,
                item_id,
                new_version,
                &title,
                update.new_content,
                actor,
                base.as_ref()
                    .and_then(|(v, text)| text.as_deref().map(|text| (*v, text))),
            )?;
            // KAIROS-T-0636: the title of a parent is in the text of each
            // child. The event below marks the item itself.
            if update.new_title.is_some() {
                crate::embeddings::mark_children_dirty(conn, item_id)?;
            }
            if let Some(before) = before {
                let names = kairos_core::notifications::new_mentions(&before, update.new_content);
                crate::notifications::on_mentions(conn, item_id, &names, actor)?;
            }
            // KAIROS-T-0731: the check of the typed sections of a
            // document, on each write (REST and MCP). A task or another
            // type has no template: no statement.
            if item_type == ItemType::Document {
                crate::section_checks::on_document_write(
                    conn,
                    item_id,
                    new_version,
                    update.new_content,
                )?;
            }
            // KAIROS-T-0736: the watchers of the item.
            crate::watches::on_watched(
                conn,
                item_id,
                kairos_core::notifications::WatchEvent::Edited,
                None,
                actor,
            )?;
            // KAIROS-T-0022: thin event, delivered on commit.
            events::emit_item_event_by_id(
                conn,
                EventKind::ItemUpdated,
                item_type.entity_type(),
                item_id,
                actor,
            )?;
            Ok(new_version)
        }
        None => {
            let (current_version, current_title, current_content, _) =
                load_live_content(conn, item_type, item_id)?.ok_or(ItemError::ItemNotFound {
                    entity_type: item_type.entity_type(),
                    id: item_id,
                })?;
            // The row exists but the atomic UPDATE matched nothing: its
            // version moved past `expected_version`. The pure mirror of
            // this decision lives in kairos-core (one contract, two
            // layers).
            debug_assert!(matches!(
                rules::check_version(current_version, update.expected_version),
                rules::VersionCheck::Conflict { .. }
            ));
            Err(ItemError::VersionConflict {
                item_id,
                expected_version: update.expected_version,
                current_version,
                current_title,
                current_content,
            })
        }
    }
}

/// Roll an item back to a historical snapshot (KAIROS-A-0004 "rollback by
/// copying a historical snapshot back to the entity table — creates a new
/// version"): the `to_version` snapshot's title/content are written forward
/// as a NEW version through the same concurrency-checked path as
/// [`update_item_content`], so history stays append-only and a concurrent
/// writer still surfaces as [`ItemError::VersionConflict`]. Returns the new
/// version number.
pub fn rollback_item(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    to_version: i32,
    actor: Uuid,
) -> Result<i32, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        // KAIROS-T-0651: the text of a diff row comes from its base row.
        let snapshot = crate::history::version_row(conn, item_id, to_version)?;
        let (title, content) =
            snapshot
                .map(|row| (row.title, row.content))
                .ok_or(ItemError::HistoryNotFound {
                    item_id,
                    version: to_version,
                })?;

        let (current_version, _, current_content, _) = load_live_content(conn, item_type, item_id)?
            .ok_or(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            })?;

        // KAIROS-T-0635: in this transaction (no savepoint), with the
        // content before in hand.
        write_content(
            conn,
            item_type,
            item_id,
            &ContentUpdate {
                new_title: Some(&title),
                new_content: &content,
                expected_version: current_version,
            },
            Before::Known(&current_content),
            actor,
        )
    })
}

// ---------------------------------------------------------------------------
// The version list (KAIROS-T-0629)
// ---------------------------------------------------------------------------

/// A version of an item without its title and content: the row of the
/// version list.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = crate::schema::item_history)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct HistoryVersionMeta {
    pub version: i32,
    pub edited_by: Uuid,
    pub edited_at: chrono::DateTime<chrono::Utc>,
    pub agent_key_id: Option<Uuid>,
}

/// One page of the versions of an item, newest first, and the number of
/// versions. The rows have no title and no content: the read of one
/// version gives them.
pub fn history_versions(
    conn: &mut PgConnection,
    item_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(i64, Vec<HistoryVersionMeta>), DieselError> {
    use crate::schema::item_history as history;
    let total: i64 = history::table
        .filter(history::item_id.eq(item_id))
        .count()
        .get_result(conn)?;
    let rows = history::table
        .filter(history::item_id.eq(item_id))
        .order(history::version.desc())
        .limit(limit)
        .offset(offset)
        .select(HistoryVersionMeta::as_select())
        .load(conn)?;
    Ok((total, rows))
}

// ---------------------------------------------------------------------------
// Soft-delete cascade (KAIROS-A-0001)
// ---------------------------------------------------------------------------

/// The principal that an archive is for (COLLIERY-T-0234): the
/// organization, by the slug of the tenant, and the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Principal<'a> {
    /// The slug of the tenant, which identifies the organization
    /// ([`abac::is_org_admin`]).
    pub org_slug: &'a str,
    /// The person or the service account.
    pub user_id: Uuid,
}

/// Why an archive did not reach a descendant (COLLIERY-T-0234).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReachedReason {
    /// The principal cannot edit the descendant: it did not create it,
    /// does not hold `capability` on `board_id`, and is not an admin.
    /// `board_id: None` = the descendant has no authorization board.
    CannotEdit {
        /// The `manage_<type>` capability of the descendant.
        capability: &'static str,
        /// The authorization board of the descendant.
        board_id: Option<Uuid>,
    },
    /// The descendant is below an item that the archive did not reach.
    Below {
        /// The short code of that item.
        short_code: String,
    },
}

/// One LIVE descendant that an archive did not reach (COLLIERY-T-0234).
/// It stays live, and it keeps its `parent` edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotReached {
    /// The short code of the descendant.
    pub short_code: String,
    /// Why the archive did not reach it.
    pub reason: NotReachedReason,
}

/// What [`soft_delete_item`] deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftDeleteOutcome {
    /// The root item's short code.
    pub root_short_code: String,
    /// Short codes of the descendants cascaded to (root excluded; only rows
    /// that were live), sorted.
    pub cascaded_short_codes: Vec<String>,
    /// The live descendants that the archive did not reach, sorted by
    /// short code (COLLIERY-T-0234). Always empty from
    /// [`soft_delete_item`], which applies no rule.
    pub not_reached: Vec<NotReached>,
}

/// The AUTHORITATIVE pre-delete cascade set (KAIROS-T-0051): what a
/// [`soft_delete_item`] of the root WOULD cascade to, computed WITHOUT
/// deleting. Mirrors [`SoftDeleteOutcome`] so a client can render the same
/// warning before and after the fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadePreview {
    /// The root item's short code.
    pub root_short_code: String,
    /// Short codes of the live descendants a delete would cascade to (root
    /// excluded), sorted — identical to the
    /// [`SoftDeleteOutcome::cascaded_short_codes`] the delete would produce.
    pub cascaded_short_codes: Vec<String>,
    /// The live descendants that the archive would not reach, sorted by
    /// short code (COLLIERY-T-0234). Always empty from
    /// [`preview_cascade`], which applies no rule.
    pub not_reached: Vec<NotReached>,
}

/// What an archive of one root takes and what it leaves: the shared read
/// of the preview and of the delete, so the two cannot disagree.
struct CascadePlan {
    root_short_code: String,
    /// The descendants to archive (root excluded). Rows that are archived
    /// already are in the list; the write filters them out.
    reached: Vec<Uuid>,
    /// The LIVE descendants that the archive leaves, sorted by short code.
    not_reached: Vec<NotReached>,
}

/// The `parent` edges below `root`: each edge whose parent is `root` or a
/// descendant of it (KAIROS-T-0629). One recursive CTE from the root on
/// the source index, not a read of each `parent` edge of the tenant. The
/// walk of [`kairos_core::items::cascade_descendants`] and
/// [`kairos_core::items::cascade_reach`] reads no other edge. `UNION`
/// drops a node that the walk has, so a cycle ends the walk.
pub fn subtree_parent_edges(
    conn: &mut PgConnection,
    root: Uuid,
) -> Result<Vec<rules::ParentEdge>, DieselError> {
    #[derive(QueryableByName)]
    struct Edge {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        source_id: Uuid,
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        target_id: Uuid,
    }
    let rows: Vec<Edge> = sql_query(
        "WITH RECURSIVE below(id) AS ( \
             SELECT $1::uuid \
             UNION \
             SELECT r.target_id FROM item_relationships r \
             JOIN below b ON r.source_id = b.id \
             WHERE r.relationship = $2 \
         ) \
         SELECT r.source_id, r.target_id FROM item_relationships r \
         JOIN below b ON r.source_id = b.id \
         WHERE r.relationship = $2",
    )
    .bind::<diesel::sql_types::Uuid, _>(root)
    .bind::<Text, _>(RelationshipType::Parent.as_str())
    .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|edge| rules::ParentEdge {
            parent_id: edge.source_id,
            child_id: edge.target_id,
        })
        .collect())
}

/// Plan the cascade of an archive. Call it inside the transaction of the
/// caller.
///
/// `principal: None` = no rule: the plan takes each descendant
/// (KAIROS-A-0001, the service as it was).
///
/// `principal: Some` = the archive is for that principal
/// (COLLIERY-T-0234), and the edit rule applies to each descendant.
///
/// THE ATTACK that the check stops. A principal creates an initiative, so
/// it can edit it. The link rule lets it write a `parent` edge from its
/// initiative to a task of a different team. It archives its initiative.
/// Until COLLIERY-T-0234 the cascade took the task of the other team.
///
/// The check is for the SET, not for each item:
/// [`abac::edit_facts_of_items`] loads the facts of each descendant in a
/// number of reads that does not grow with the number of descendants. The
/// decision for each item is [`kairos_core::abac::may_edit_item`], and the
/// walk is [`kairos_core::items::cascade_reach`]. A descendant with no
/// facts (its row is in no table) is not editable: the check fails closed.
fn plan_cascade(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    principal: Option<Principal<'_>>,
) -> Result<CascadePlan, ItemError> {
    let (.., root_short_code) =
        load_live_content(conn, item_type, item_id)?.ok_or(ItemError::ItemNotFound {
            entity_type: item_type.entity_type(),
            id: item_id,
        })?;

    let edges = subtree_parent_edges(conn, item_id)?;

    let Some(principal) = principal else {
        return Ok(CascadePlan {
            root_short_code,
            reached: rules::cascade_descendants(item_id, &edges),
            not_reached: Vec::new(),
        });
    };

    let descendants = rules::cascade_descendants(item_id, &edges);
    let facts_of =
        abac::edit_facts_of_items(conn, principal.org_slug, principal.user_id, &descendants)
            .map_err(abac_error)?;
    let reach = rules::cascade_reach(item_id, &edges, |id| {
        facts_of
            .get(&id)
            .is_some_and(|item| kairos_core::abac::may_edit_item(item.facts))
    });

    // Name the LIVE items that stay. An item that is archived already has
    // nothing to leave; it can still be the stop above a live item.
    let mut not_reached: Vec<NotReached> = Vec::new();
    for stop in &reach.stopped {
        if let Some(item) = facts_of.get(stop).filter(|item| item.live) {
            not_reached.push(NotReached {
                short_code: item.short_code.clone(),
                reason: NotReachedReason::CannotEdit {
                    capability: item.manage_capability,
                    board_id: item.board_id,
                },
            });
        }
    }
    for (id, stop) in &reach.below {
        let (Some(item), Some(stop)) = (facts_of.get(id), facts_of.get(stop)) else {
            continue;
        };
        if item.live {
            not_reached.push(NotReached {
                short_code: item.short_code.clone(),
                reason: NotReachedReason::Below {
                    short_code: stop.short_code.clone(),
                },
            });
        }
    }
    not_reached.sort_by(|a, b| a.short_code.cmp(&b.short_code));

    Ok(CascadePlan {
        root_short_code,
        reached: reach.reached,
        not_reached,
    })
}

/// The facts of the edit rule are reads: the one error they can give is an
/// error of the database.
fn abac_error(e: abac::AbacError) -> ItemError {
    match e {
        abac::AbacError::Database(e) => ItemError::Database(e),
        other => ItemError::Database(DieselError::QueryBuilderError(Box::new(other))),
    }
}

fn preview_planned(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    principal: Option<Principal<'_>>,
) -> Result<CascadePreview, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        let plan = plan_cascade(conn, item_type, item_id, principal)?;

        // `plan.reached` excludes the root, so these ids are the
        // descendants a delete would cascade to — exactly the ids
        // `soft_delete_item` collects short codes for (minus the root it
        // then filters out).
        let mut cascaded_short_codes: Vec<String> = Vec::new();
        for preview_in_table in [
            live_short_codes_strategies,
            live_short_codes_initiatives,
            live_short_codes_tasks,
            live_short_codes_documents,
            live_short_codes_adrs,
        ] {
            cascaded_short_codes.extend(preview_in_table(conn, &plan.reached)?);
        }
        cascaded_short_codes.sort();

        Ok(CascadePreview {
            root_short_code: plan.root_short_code,
            cascaded_short_codes,
            not_reached: plan.not_reached,
        })
    })
}

/// Preview the KAIROS-A-0001 soft-delete cascade WITHOUT mutating anything
/// (KAIROS-T-0051): the read half of [`soft_delete_item`], sharing its
/// exact machinery so the two can never disagree. Loads the `parent`
/// edges below the root ([`subtree_parent_edges`]), computes the descendant set with the SAME pure
/// [`kairos_core::items::cascade_descendants`] BFS, and reads the LIVE
/// short codes of those descendants across all five entity tables (the
/// read-only mirror of the delete's per-table filter). Runs in one
/// read transaction for a consistent edge/row snapshot. 404 (via
/// [`ItemError::ItemNotFound`]) if the root is missing or soft-deleted.
///
/// No permission rule, as [`soft_delete_item`]. The preview for a
/// principal is [`preview_cascade_as`].
pub fn preview_cascade(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<CascadePreview, ItemError> {
    preview_planned(conn, item_type, item_id, None)
}

/// Preview what [`soft_delete_item_as`] would do for this principal
/// (COLLIERY-T-0234): the descendants that the archive would take, and the
/// live descendants that it would leave, with the reason. It is the same
/// plan as the archive, so the two cannot disagree. It writes nothing.
///
/// The answer is for the principal who asks. It does not say whether the
/// principal may archive the root: the archive checks that.
pub fn preview_cascade_as(
    conn: &mut PgConnection,
    principal: Principal<'_>,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<CascadePreview, ItemError> {
    preview_planned(conn, item_type, item_id, Some(principal))
}

/// Soft-delete an item and cascade to its descendants (KAIROS-A-0001), in
/// ONE transaction: load the `parent` edges below the root
/// ([`subtree_parent_edges`]), compute the descendant set with the pure
/// [`kairos_core::items::cascade_descendants`], stamp `deleted_at`
/// on the root and every live descendant across all five entity tables,
/// and write ONE `activity_log` `delete` row on the root recording the
/// cascade count and short codes. Already-deleted descendants are left
/// untouched.
///
/// Archived rows stay IN `searchable_items` and `entity_directory`
/// (KAIROS-T-0156 moved the liveness predicate out of the view bodies and
/// into the call sites, so each one names a mode). They are hidden by
/// every default listing and served to anyone who asks for them — that is
/// the whole of what archiving means (KAIROS-A-0020). Nothing removes the
/// rows: the KAIROS-T-0015 sweeper prunes `item_history` and
/// `activity_log` only, and is not wired into the server yet.
///
/// NO PERMISSION RULE (COLLIERY-T-0234). This function takes each
/// descendant and asks nothing: `actor` is only the name in the record. It
/// is for a caller that is not a principal, such as a test fixture. A
/// surface that acts for a principal must call [`soft_delete_item_as`].
pub fn soft_delete_item(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    actor: Uuid,
) -> Result<SoftDeleteOutcome, ItemError> {
    soft_delete_planned(conn, item_type, item_id, actor, None)
}

/// Archive an item FOR A PRINCIPAL (COLLIERY-T-0234): the soft delete of
/// [`soft_delete_item`], with the edit rule applied to each descendant.
///
/// THE ATTACK that this function stops. A principal creates an initiative,
/// so it can edit it. The link rule lets it write a `parent` edge from its
/// initiative to a task of a different team. It archives its initiative.
/// Until COLLIERY-T-0234 the task of the other team was archived too.
///
/// THE RULE. The archive takes a descendant that the principal may edit.
/// It stops at a descendant that the principal may not edit, and takes
/// nothing below it. Each live descendant that stays is named in
/// [`SoftDeleteOutcome::not_reached`], with the reason. It keeps its
/// `parent` edge: this function removes no edge.
///
/// THE ROOT is not checked here. The caller applies the edit rule to the
/// root before the call (`require_item_edit`), and the root is archived
/// when some descendants stay.
///
/// ONE TRANSACTION, as before: the plan, each write, the record and the
/// event. The archive takes the permitted subtree, or it takes nothing.
///
/// The record of the archive (`activity_log`) names the descendants that
/// stayed, after the ones that went: `not_reached:<codes>`.
pub fn soft_delete_item_as(
    conn: &mut PgConnection,
    principal: Principal<'_>,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<SoftDeleteOutcome, ItemError> {
    soft_delete_planned(conn, item_type, item_id, principal.user_id, Some(principal))
}

fn soft_delete_planned(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    actor: Uuid,
    principal: Option<Principal<'_>>,
) -> Result<SoftDeleteOutcome, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        let plan = plan_cascade(conn, item_type, item_id, principal)?;
        let root_short_code = plan.root_short_code;

        let mut ids = plan.reached;
        ids.push(item_id);

        let mut deleted: Vec<String> = Vec::new();
        for delete_in_table in [
            soft_delete_strategies,
            soft_delete_initiatives,
            soft_delete_tasks,
            soft_delete_documents,
            soft_delete_adrs,
        ] {
            deleted.extend(delete_in_table(conn, &ids, actor)?);
        }
        // KAIROS-T-0359: an archived task has no claim.
        crate::task_claims::drop_claims(conn, &ids, actor, "archive")?;
        // KAIROS-T-0731: an archived dictionary changes the checks of the
        // typed sections that it applied to.
        crate::section_checks::mark_dictionaries_stale(conn, &ids)?;
        // KAIROS-T-0381: an archived item no longer blocks.
        crate::notifications::on_blockers_cleared(
            conn,
            &ids,
            crate::notifications::Cleared::Archived,
            actor,
        )?;

        let mut cascaded_short_codes: Vec<String> = deleted
            .into_iter()
            .filter(|code| *code != root_short_code)
            .collect();
        cascaded_short_codes.sort();

        let mut details = if cascaded_short_codes.is_empty() {
            format!("short_code:{root_short_code} cascade:0")
        } else {
            format!(
                "short_code:{root_short_code} cascade:{} descendants:{}",
                cascaded_short_codes.len(),
                cascaded_short_codes.join(",")
            )
        };
        if !plan.not_reached.is_empty() {
            let codes: Vec<&str> = plan
                .not_reached
                .iter()
                .map(|item| item.short_code.as_str())
                .collect();
            details.push_str(&format!(" not_reached:{}", codes.join(",")));
        }
        log_activity(
            conn,
            actor,
            ActivityAction::Delete,
            item_id,
            item_type.entity_type(),
            details,
        )?;

        // KAIROS-T-0022: one thin event for the cascade root, delivered
        // after the commit (the outbox insert still sees the soft-deleted
        // row).
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemDeleted,
            item_type.entity_type(),
            item_id,
            actor,
        )?;
        // KAIROS-T-0574: and one for each cascaded descendant, with its own
        // board, so a socket filtered to the board of a descendant hears it.
        // One insert for all of them (KAIROS-T-0632).
        events::emit_item_events_by_codes(
            conn,
            EventKind::ItemDeleted,
            &cascaded_short_codes,
            actor,
        )?;

        Ok(SoftDeleteOutcome {
            root_short_code,
            cascaded_short_codes,
            not_reached: plan.not_reached,
        })
    })
}

/// The short code of an ARCHIVED row of this type, or `None` if the id is
/// unknown or the row is already live (restoring a live item is a no-op the
/// caller should hear about, not silently succeed at).
fn archived_short_code(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<Option<String>, DieselError> {
    macro_rules! archived_in {
        ($table:ident) => {{
            use crate::schema::$table::dsl;
            dsl::$table
                .filter(dsl::id.eq(item_id))
                .filter(dsl::deleted_at.is_not_null())
                .select(dsl::short_code)
                .first::<String>(conn)
                .optional()
        }};
    }
    match item_type {
        ItemType::Strategy => archived_in!(strategies),
        ItemType::Initiative => archived_in!(initiatives),
        ItemType::Task => archived_in!(tasks),
        ItemType::Document => archived_in!(documents),
        ItemType::Adr => archived_in!(adrs),
    }
}

/// The short code of an archived row with this id in ANY of the five
/// tables (the descendant set spans them).
fn any_archived_short_code(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Option<String>, DieselError> {
    for item_type in ItemType::ALL.iter().copied() {
        if let Some(code) = archived_short_code(conn, item_type, item_id)? {
            return Ok(Some(code));
        }
    }
    Ok(None)
}

/// Clear `deleted_at` on one row.
fn restore_row(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    macro_rules! restore_in {
        ($table:ident) => {{
            use crate::schema::$table::dsl;
            diesel::update(dsl::$table.filter(dsl::id.eq(item_id)))
                .set((
                    dsl::deleted_at.eq(None::<chrono::DateTime<chrono::Utc>>),
                    dsl::updated_by.eq(actor),
                    dsl::updated_at.eq(diesel::dsl::now),
                ))
                .execute(conn)
                .map(|_| ())
        }};
    }
    match item_type {
        ItemType::Strategy => restore_in!(strategies),
        ItemType::Initiative => restore_in!(initiatives),
        ItemType::Task => restore_in!(tasks),
        ItemType::Document => restore_in!(documents),
        ItemType::Adr => restore_in!(adrs),
    }
}

/// The board and the column of an item; each is `None` where the family
/// has none (a document has no column; an ADR may be off a board).
type Placement = (Option<Uuid>, Option<Uuid>);

/// The [`Placement`] of an item, per family; `None` when the row is not
/// there.
fn placement_of(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<Option<Placement>, DieselError> {
    use crate::schema::{adrs, documents, initiatives, strategies, tasks};
    Ok(match item_type {
        ItemType::Strategy => strategies::table
            .filter(strategies::id.eq(item_id))
            .select((strategies::board_id, strategies::column_id))
            .first::<(Uuid, Uuid)>(conn)
            .optional()?
            .map(|(b, c)| (Some(b), Some(c))),
        ItemType::Initiative => initiatives::table
            .filter(initiatives::id.eq(item_id))
            .select((initiatives::board_id, initiatives::column_id))
            .first::<(Uuid, Uuid)>(conn)
            .optional()?
            .map(|(b, c)| (Some(b), Some(c))),
        ItemType::Task => tasks::table
            .filter(tasks::id.eq(item_id))
            .select((tasks::board_id, tasks::column_id))
            .first::<(Uuid, Uuid)>(conn)
            .optional()?
            .map(|(b, c)| (Some(b), Some(c))),
        // ADRs may be off-board.
        ItemType::Adr => adrs::table
            .filter(adrs::id.eq(item_id))
            .select((adrs::board_id, adrs::column_id))
            .first::<(Option<Uuid>, Option<Uuid>)>(conn)
            .optional()?,
        // A document is never on a board, and has no column. The board
        // is its owner (COLLIERY-T-0269, required since COLLIERY-T-3109).
        ItemType::Document => documents::table
            .filter(documents::id.eq(item_id))
            .select(documents::board_id)
            .first::<Uuid>(conn)
            .optional()?
            .map(|board_id| (Some(board_id), None)),
    })
}

/// Everything an archived item needs back before it can be live, that is
/// itself gone. Empty = the restore may proceed.
///
/// A document has no column. It can name a board as its owner
/// (COLLIERY-T-0269), and that board is what it needs back: the delete of
/// a board looks at the live documents only, so the owner of an archived
/// document can be gone. The `impacts` links of a document or of an ADR
/// need nothing: a link to an archived repository stays, and it is shown
/// as such.
fn restore_blockers(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<Vec<String>, DieselError> {
    use crate::schema::{board_columns, boards, repositories, tasks, teams};

    let mut missing = Vec::new();

    if let Some((board_id, column_id)) = placement_of(conn, item_type, item_id)? {
        if let Some(board_id) = board_id {
            let board_live: Option<Uuid> = boards::table
                .filter(boards::id.eq(board_id))
                .filter(boards::deleted_at.is_null())
                .select(boards::id)
                .first(conn)
                .optional()?;
            if board_live.is_none() {
                missing.push("its board (deleted)".to_string());
            }
        }
        if let Some(column_id) = column_id {
            // KAIROS-T-0161 made this detectable: the column row survives
            // a removal with `deleted_at` set, so this is a clean check
            // rather than a foreign-key error at write time.
            let column_live: Option<Uuid> = board_columns::table
                .filter(board_columns::id.eq(column_id))
                .filter(board_columns::deleted_at.is_null())
                .select(board_columns::id)
                .first(conn)
                .optional()?;
            if column_live.is_none() {
                missing.push("its board column (removed)".to_string());
            }
        }
    }

    // KAIROS-A-0079: a dictionary comes back only on the board
    // `strategy`. KAIROS-T-0733: that board has one live dictionary, so an
    // archived dictionary does not come back next to a live one.
    if item_type == ItemType::Document
        && crate::dictionaries::is_dictionary_document(conn, item_id)?
    {
        use crate::schema::documents;
        let board_id: Uuid = documents::table
            .filter(documents::id.eq(item_id))
            .select(documents::board_id)
            .first(conn)?;
        let board = board_slug(conn, board_id)?;
        if board != crate::dictionaries::ORGANIZATION_BOARD_SLUG {
            missing.push(format!(
                "the board strategy (a dictionary can be only on the board strategy, and its \
                 board {board} is not the board strategy)"
            ));
        } else if let Some((_, existing)) =
            crate::dictionaries::live_dictionary_of_board(conn, board_id)?
        {
            missing.push(format!(
                "a board with no dictionary (the board {board} has the dictionary {existing}, \
                 and an organization has one dictionary)"
            ));
        }
    }

    // A task additionally carries an owning team and possibly a repository.
    if item_type == ItemType::Task {
        let bindings: Option<(Option<Uuid>, Option<Uuid>)> = tasks::table
            .filter(tasks::id.eq(item_id))
            .select((tasks::team_id, tasks::repository_id))
            .first(conn)
            .optional()?;
        if let Some((team_id, repository_id)) = bindings {
            if let Some(team_id) = team_id {
                let team_live: Option<Uuid> = teams::table
                    .filter(teams::id.eq(team_id))
                    .filter(teams::deleted_at.is_null())
                    .select(teams::id)
                    .first(conn)
                    .optional()?;
                if team_live.is_none() {
                    missing.push("its owning team (deleted)".to_string());
                }
            }
            if let Some(repository_id) = repository_id {
                let repo_live: Option<Uuid> = repositories::table
                    .filter(repositories::id.eq(repository_id))
                    .filter(repositories::deleted_at.is_null())
                    .select(repositories::id)
                    .first(conn)
                    .optional()?;
                if repo_live.is_none() {
                    missing.push("its repository (retired)".to_string());
                }
            }
        }
    }

    Ok(missing)
}

/// What a [`restore_item`] put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    /// The restored item's short code.
    pub short_code: String,
    /// Archived descendants that were NOT restored, sorted — see
    /// [`restore_item`] for why they are left alone, and named.
    pub still_archived_descendants: Vec<String>,
    /// The column the item is in again, `None` for a document or an ADR
    /// off a board (KAIROS-T-0566: the server checks its WIP limit).
    pub column_id: Option<Uuid>,
}

/// Why a restore was refused: everything missing that the item needs in
/// order to be live again, named so the caller can act (KAIROS-T-0160).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreBlockers {
    /// Human-readable, one per missing thing ("board", "column", ...).
    pub missing: Vec<String>,
}

/// Put an archived item back (KAIROS-A-0020): clear `deleted_at` on the
/// named item and nothing else.
///
/// # Why this does not un-cascade
///
/// A cascade delete is a deliberate act on a subtree; a restore is almost
/// always "I need this one thing back". Resurrecting descendants would
/// undo decisions nobody asked to revisit, and would do it invisibly. So
/// the archived descendants stay archived and are NAMED in the outcome,
/// the same way every other guard in this codebase names what it did not
/// do. Restoring them is another call.
///
/// # Why it refuses instead of re-homing
///
/// An item's board, column, owning team or repository may have been
/// retired while it was away. Silently moving it somewhere else would
/// destroy the placement the record is evidence of. Instead the refusal
/// names what is missing ([`RestoreBlockers`]) and the caller moves the
/// item deliberately.
pub fn restore_item(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    actor: Uuid,
) -> Result<Result<RestoreOutcome, RestoreBlockers>, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        let short_code =
            archived_short_code(conn, item_type, item_id)?.ok_or(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            })?;

        let missing = restore_blockers(conn, item_type, item_id)?;
        if !missing.is_empty() {
            return Ok(Err(RestoreBlockers { missing }));
        }

        restore_row(conn, item_type, item_id, actor)?;
        // KAIROS-T-0731: a restored dictionary changes the checks of the
        // typed sections that it applies to.
        if item_type == ItemType::Document {
            crate::section_checks::mark_dictionaries_stale(conn, &[item_id])?;
        }

        // Name the descendants still away, so "I restored it and half of it
        // is missing" is answered before it is asked.
        let edges = subtree_parent_edges(conn, item_id)?;
        let mut still_archived_descendants = Vec::new();
        for descendant in rules::cascade_descendants(item_id, &edges) {
            if let Some(code) = any_archived_short_code(conn, descendant)? {
                still_archived_descendants.push(code);
            }
        }
        still_archived_descendants.sort();

        let details = if still_archived_descendants.is_empty() {
            format!("short_code:{short_code} still_archived:0")
        } else {
            format!(
                "short_code:{short_code} still_archived:{} descendants:{}",
                still_archived_descendants.len(),
                still_archived_descendants.join(",")
            )
        };
        log_activity(
            conn,
            actor,
            ActivityAction::Restore,
            item_id,
            item_type.entity_type(),
            details,
        )?;
        // One event: a restore puts back the named item only. Its archived
        // descendants stay archived, so they get no event (KAIROS-T-0574).
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemRestored,
            item_type.entity_type(),
            item_id,
            actor,
        )?;

        let column_id = placement_of(conn, item_type, item_id)?.and_then(|(_, column)| column);
        Ok(Ok(RestoreOutcome {
            short_code,
            still_archived_descendants,
            column_id,
        }))
    })
}
