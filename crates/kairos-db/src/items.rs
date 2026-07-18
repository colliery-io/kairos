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
//! Numbers come from the per-type `seq_*_code` PostgreSQL sequences
//! (S-0004) via `nextval` — concurrency-safe by construction (sequences
//! never hand out the same value twice; aborted transactions may leave
//! numeric gaps, which is expected sequence behavior). The PREFIX default
//! is derived from the tenant: `current_schema()` is `org_{slug}` on every
//! tenant-pinned connection, and the prefix is
//! [`kairos_core::short_code::default_prefix`] of the slug (upper-cased,
//! `[A-Z0-9]`). A per-organization prefix override is a future API-layer
//! setting (no DDL column exists); interpretation recorded in
//! KAIROS-T-0012.
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
//!
//! # Soft delete (KAIROS-A-0001)
//!
//! [`soft_delete_item`] loads the tenant's `parent` edges in one query,
//! computes the descendant set with the pure
//! [`kairos_core::items::cascade_descendants`] (chosen over a recursive CTE
//! so the traversal decision lives in core and is unit-testable; the edge
//! count per tenant is small at M1 scale), and stamps `deleted_at` on the
//! root plus every live descendant across all five entity tables. One
//! `activity_log` row (action `delete`) on the root records the cascade
//! count and the cascaded short codes.

use chrono::NaiveDate;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use uuid::Uuid;

use kairos_core::items as rules;
use kairos_core::short_code::{self, ItemType};

use crate::events::{self, EventKind};

use crate::models::enums::{ActivityAction, BucketType, Complexity, RelationshipType, TaskType};
use crate::models::graph::{NewActivityLogEntry, NewItemHistory};
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
    #[error("{entity_type} {id} does not exist")]
    ItemNotFound { entity_type: &'static str, id: Uuid },
    /// KAIROS-A-0004 optimistic-concurrency conflict: the submitted base
    /// version is stale. Carries the item's current version, title, and
    /// content so the client can reconcile (maps to HTTP 409).
    #[error(
        "version conflict on {item_id}: submitted base version \
         {expected_version}, current version {current_version}"
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
    #[error("no history snapshot for item {item_id} at version {version}")]
    HistoryNotFound { item_id: Uuid, version: i32 },
    /// No live board with this id exists.
    #[error("board {0} does not exist")]
    BoardNotFound(Uuid),
    /// The board exists but has no columns to default-place an item into.
    #[error("board {0} has no columns")]
    BoardHasNoColumns(Uuid),
    /// An explicit `column_id` does not belong to the given board.
    #[error("column {column_id} is not a column of board {board_id}")]
    ColumnNotOnBoard { board_id: Uuid, column_id: Uuid },
    /// No template with this id exists.
    #[error("template {0} does not exist")]
    TemplateNotFound(Uuid),
    /// Any other database error.
    #[error("database error: {0}")]
    Database(#[from] DieselError),
}

// ---------------------------------------------------------------------------
// Short codes (S-0004 sequences + core formatting)
// ---------------------------------------------------------------------------

/// The S-0004 sequence backing each entity type's short-code numbers.
fn sequence_name(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Strategy => "seq_strategy_code",
        ItemType::Initiative => "seq_initiative_code",
        ItemType::Task => "seq_task_code",
        ItemType::Document => "seq_document_code",
        ItemType::Adr => "seq_adr_code",
    }
}

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

/// Allocate the next short code for `item_type` in the current tenant
/// schema: `nextval` on the type's `seq_*_code` sequence (atomic, no two
/// callers ever receive the same number), formatted by
/// [`kairos_core::short_code::format_short_code`] with the tenant's default
/// prefix (see module docs — derived from `current_schema()`; a `org_`
/// prefix is stripped, so `org_acme` yields `ACME-T-0001`).
pub fn next_short_code(conn: &mut PgConnection, item_type: ItemType) -> Result<String, ItemError> {
    let schema: SchemaName =
        sql_query("SELECT COALESCE(current_schema()::text, 'public') AS name").get_result(conn)?;
    let slug = schema
        .name
        .strip_prefix("org_")
        .unwrap_or(schema.name.as_str());
    let prefix = short_code::default_prefix(slug);

    let seq: SeqValue = sql_query(format!(
        "SELECT nextval('{}') AS value",
        sequence_name(item_type)
    ))
    .get_result(conn)?;
    Ok(short_code::format_short_code(&prefix, item_type, seq.value))
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

/// Append one `item_history` snapshot (append-only, KAIROS-A-0004).
fn insert_history(
    conn: &mut PgConnection,
    item_id: Uuid,
    version: i32,
    title: &str,
    content: &str,
    actor: Uuid,
) -> Result<(), DieselError> {
    diesel::insert_into(crate::schema::item_history::table)
        .values(NewItemHistory {
            item_id,
            version,
            title: title.to_string(),
            content: content.to_string(),
            edited_by: actor,
        })
        .execute(conn)?;
    Ok(())
}

/// Create-time bookkeeping shared by all five create services: the v1
/// baseline `item_history` snapshot (see module docs), the `activity_log`
/// create row, and the `item_created` thin event (KAIROS-T-0022 — emitted
/// inside the transaction, delivered by PostgreSQL on commit).
fn finish_create(
    conn: &mut PgConnection,
    actor: Uuid,
    item_type: ItemType,
    item_id: Uuid,
    code: &str,
    title: &str,
    content: &str,
) -> Result<(), DieselError> {
    insert_history(conn, item_id, 1, title, content, actor)?;
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
    )
}

/// Resolve an item's column placement on `board_id`: an explicit
/// `column_id` is validated to belong to the board; `None` defaults to the
/// board's first column (position order, i.e. position 0).
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

    match column_id {
        Some(column_id) => board_columns::table
            .filter(board_columns::id.eq(column_id))
            .filter(board_columns::board_id.eq(board_id))
            .select(board_columns::id)
            .first(conn)
            .optional()?
            .ok_or(ItemError::ColumnNotOnBoard {
                board_id,
                column_id,
            }),
        None => board_columns::table
            .filter(board_columns::board_id.eq(board_id))
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

/// Generate the three per-table primitives the generic write path
/// dispatches over: the atomic version-checked content UPDATE, the
/// live-row load, and the bulk soft-delete.
macro_rules! content_table_ops {
    ($table:ident, $apply_fn:ident, $load_fn:ident, $delete_fn:ident, $preview_fn:ident) => {
        /// Atomic KAIROS-A-0004 write: `UPDATE … SET version = version + 1
        /// … WHERE id = ? AND version = expected AND deleted_at IS NULL`.
        /// `Ok(None)` = no row matched (stale version, missing, or
        /// soft-deleted); `Ok(Some((new_version, title, content)))` on
        /// success.
        fn $apply_fn(
            conn: &mut PgConnection,
            item_id: Uuid,
            update: &ContentUpdate<'_>,
            actor: Uuid,
        ) -> Result<Option<(i32, String, String)>, DieselError> {
            use crate::schema::$table::dsl;
            let target = dsl::$table
                .filter(dsl::id.eq(item_id))
                .filter(dsl::version.eq(update.expected_version))
                .filter(dsl::deleted_at.is_null());
            let row = match update.new_title {
                Some(new_title) => diesel::update(target)
                    .set((
                        dsl::version.eq(dsl::version + 1),
                        dsl::title.eq(new_title),
                        dsl::content.eq(update.new_content),
                        dsl::updated_by.eq(actor),
                        dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .returning((dsl::version, dsl::title, dsl::content))
                    .get_result(conn)
                    .optional()?,
                None => diesel::update(target)
                    .set((
                        dsl::version.eq(dsl::version + 1),
                        dsl::content.eq(update.new_content),
                        dsl::updated_by.eq(actor),
                        dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .returning((dsl::version, dsl::title, dsl::content))
                    .get_result(conn)
                    .optional()?,
            };
            Ok(row)
        }

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
        fn $delete_fn(
            conn: &mut PgConnection,
            ids: &[Uuid],
            actor: Uuid,
        ) -> Result<Vec<String>, DieselError> {
            use crate::schema::$table::dsl;
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
    apply_content_strategy,
    load_live_strategy,
    soft_delete_strategies,
    live_short_codes_strategies
);
content_table_ops!(
    initiatives,
    apply_content_initiative,
    load_live_initiative,
    soft_delete_initiatives,
    live_short_codes_initiatives
);
content_table_ops!(
    tasks,
    apply_content_task,
    load_live_task,
    soft_delete_tasks,
    live_short_codes_tasks
);
content_table_ops!(
    documents,
    apply_content_document,
    load_live_document,
    soft_delete_documents,
    live_short_codes_documents
);
content_table_ops!(
    adrs,
    apply_content_adr,
    load_live_adr,
    soft_delete_adrs,
    live_short_codes_adrs
);

/// Dispatch the atomic content UPDATE to the item's table.
fn apply_content_update(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    update: &ContentUpdate<'_>,
    actor: Uuid,
) -> Result<Option<(i32, String, String)>, DieselError> {
    match item_type {
        ItemType::Strategy => apply_content_strategy(conn, item_id, update, actor),
        ItemType::Initiative => apply_content_initiative(conn, item_id, update, actor),
        ItemType::Task => apply_content_task(conn, item_id, update, actor),
        ItemType::Document => apply_content_document(conn, item_id, update, actor),
        ItemType::Adr => apply_content_adr(conn, item_id, update, actor),
    }
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
        let code = next_short_code(conn, ItemType::Strategy)?;
        let created: Strategy = diesel::insert_into(crate::schema::strategies::table)
            .values(NewStrategy {
                short_code: code,
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
        finish_create(
            conn,
            actor,
            ItemType::Strategy,
            created.id,
            &created.short_code,
            &created.title,
            &created.content,
        )?;
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
        let code = next_short_code(conn, ItemType::Initiative)?;
        let created: Initiative = diesel::insert_into(crate::schema::initiatives::table)
            .values(NewInitiative {
                short_code: code,
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
        finish_create(
            conn,
            actor,
            ItemType::Initiative,
            created.id,
            &created.short_code,
            &created.title,
            &created.content,
        )?;
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
    pub team_id: Option<Uuid>,
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
        let code = next_short_code(conn, ItemType::Task)?;
        let created: Task = diesel::insert_into(crate::schema::tasks::table)
            .values(NewTask {
                short_code: code,
                title: input.title.to_string(),
                content: input.content.to_string(),
                board_id: input.board_id,
                column_id,
                task_type: input.task_type,
                team_id: input.team_id,
                created_by: actor,
                updated_by: actor,
            })
            .returning(Task::as_returning())
            .get_result(conn)?;
        finish_create(
            conn,
            actor,
            ItemType::Task,
            created.id,
            &created.short_code,
            &created.title,
            &created.content,
        )?;
        Ok(created)
    })
}

/// Input for [`create_document`].
#[derive(Debug, Clone)]
pub struct CreateDocument<'a> {
    pub title: &'a str,
    /// Explicit content wins; `None` copies the template's content when
    /// `template_id` is given, else empty.
    pub content: Option<&'a str>,
    /// KAIROS-A-0003: when given, the template's content seeds the document
    /// and its `template_metadata` defaults are stamped as `item_metadata`
    /// rows.
    pub template_id: Option<Uuid>,
}

/// Create a document (documents do not live on boards). With a
/// `template_id`, the template's content is copied (unless `content`
/// overrides it) and every `template_metadata` row carrying a
/// `default_value` is stamped as an `item_metadata` row (KAIROS-A-0003).
pub fn create_document(
    conn: &mut PgConnection,
    input: CreateDocument<'_>,
    actor: Uuid,
) -> Result<Document, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::{item_metadata, template_metadata, templates};

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
        let content = input
            .content
            .unwrap_or_else(|| template.as_ref().map_or("", |t| t.content.as_str()));

        let code = next_short_code(conn, ItemType::Document)?;
        let created: Document = diesel::insert_into(crate::schema::documents::table)
            .values(NewDocument {
                short_code: code,
                title: input.title.to_string(),
                content: content.to_string(),
                template_id: input.template_id,
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
            let stamps: Vec<NewItemMetadata> = defaults
                .into_iter()
                .filter_map(|(metadata_definition_id, default_value)| {
                    default_value.map(|value| NewItemMetadata {
                        item_id: created.id,
                        metadata_definition_id,
                        value,
                    })
                })
                .collect();
            if !stamps.is_empty() {
                diesel::insert_into(item_metadata::table)
                    .values(&stamps)
                    .execute(conn)?;
            }
        }

        finish_create(
            conn,
            actor,
            ItemType::Document,
            created.id,
            &created.short_code,
            &created.title,
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
        let code = next_short_code(conn, ItemType::Adr)?;
        let created: Adr = diesel::insert_into(crate::schema::adrs::table)
            .values(NewAdr {
                short_code: code,
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
        finish_create(
            conn,
            actor,
            ItemType::Adr,
            created.id,
            &created.short_code,
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
pub fn update_item_content(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    update: ContentUpdate<'_>,
    actor: Uuid,
) -> Result<i32, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        match apply_content_update(conn, item_type, item_id, &update, actor)? {
            Some((new_version, title, content)) => {
                insert_history(conn, item_id, new_version, &title, &content, actor)?;
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
                let (current_version, current_title, current_content, _) = load_live_content(
                    conn, item_type, item_id,
                )?
                .ok_or(ItemError::ItemNotFound {
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
    })
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
        use crate::schema::item_history;

        let snapshot: Option<(String, String)> = item_history::table
            .filter(item_history::item_id.eq(item_id))
            .filter(item_history::version.eq(to_version))
            .select((item_history::title, item_history::content))
            .first(conn)
            .optional()?;
        let (title, content) = snapshot.ok_or(ItemError::HistoryNotFound {
            item_id,
            version: to_version,
        })?;

        let (current_version, ..) =
            load_live_content(conn, item_type, item_id)?.ok_or(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            })?;

        update_item_content(
            conn,
            item_type,
            item_id,
            ContentUpdate {
                new_title: Some(&title),
                new_content: &content,
                expected_version: current_version,
            },
            actor,
        )
    })
}

// ---------------------------------------------------------------------------
// Soft-delete cascade (KAIROS-A-0001)
// ---------------------------------------------------------------------------

/// What [`soft_delete_item`] deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftDeleteOutcome {
    /// The root item's short code.
    pub root_short_code: String,
    /// Short codes of the descendants cascaded to (root excluded; only rows
    /// that were live), sorted.
    pub cascaded_short_codes: Vec<String>,
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
}

/// Preview the KAIROS-A-0001 soft-delete cascade WITHOUT mutating anything
/// (KAIROS-T-0051): the read half of [`soft_delete_item`], sharing its
/// exact machinery so the two can never disagree. Loads the tenant's
/// `parent` edges, computes the descendant set with the SAME pure
/// [`kairos_core::items::cascade_descendants`] BFS, and reads the LIVE
/// short codes of those descendants across all five entity tables (the
/// read-only mirror of the delete's per-table filter). Runs in one
/// read transaction for a consistent edge/row snapshot. 404 (via
/// [`ItemError::ItemNotFound`]) if the root is missing or soft-deleted.
pub fn preview_cascade(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
) -> Result<CascadePreview, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::item_relationships;

        let (.., root_short_code) =
            load_live_content(conn, item_type, item_id)?.ok_or(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            })?;

        let edges: Vec<rules::ParentEdge> = item_relationships::table
            .filter(item_relationships::relationship.eq(RelationshipType::Parent))
            .select((item_relationships::source_id, item_relationships::target_id))
            .load::<(Uuid, Uuid)>(conn)?
            .into_iter()
            .map(|(parent_id, child_id)| rules::ParentEdge {
                parent_id,
                child_id,
            })
            .collect();

        // `cascade_descendants` excludes the root, so these ids are the
        // descendants a delete would cascade to — exactly the ids
        // `soft_delete_item` collects short codes for (minus the root it
        // then filters out).
        let ids = rules::cascade_descendants(item_id, &edges);

        let mut cascaded_short_codes: Vec<String> = Vec::new();
        for preview_in_table in [
            live_short_codes_strategies,
            live_short_codes_initiatives,
            live_short_codes_tasks,
            live_short_codes_documents,
            live_short_codes_adrs,
        ] {
            cascaded_short_codes.extend(preview_in_table(conn, &ids)?);
        }
        cascaded_short_codes.sort();

        Ok(CascadePreview {
            root_short_code,
            cascaded_short_codes,
        })
    })
}

/// Soft-delete an item and cascade to its descendants (KAIROS-A-0001), in
/// ONE transaction: load the tenant's `parent` edges, compute the
/// descendant set with the pure [`kairos_core::items::cascade_descendants`]
/// (see module docs for why this over a recursive CTE), stamp `deleted_at`
/// on the root and every live descendant across all five entity tables,
/// and write ONE `activity_log` `delete` row on the root recording the
/// cascade count and short codes. Already-deleted descendants are left
/// untouched. Soft-deleted rows disappear from `searchable_items` and
/// `entity_directory` (the views filter `deleted_at IS NULL`); hard-delete
/// cleanup is the KAIROS-T-0015 sweeper.
pub fn soft_delete_item(
    conn: &mut PgConnection,
    item_type: ItemType,
    item_id: Uuid,
    actor: Uuid,
) -> Result<SoftDeleteOutcome, ItemError> {
    conn.transaction::<_, ItemError, _>(|conn| {
        use crate::schema::item_relationships;

        let (.., root_short_code) =
            load_live_content(conn, item_type, item_id)?.ok_or(ItemError::ItemNotFound {
                entity_type: item_type.entity_type(),
                id: item_id,
            })?;

        let edges: Vec<rules::ParentEdge> = item_relationships::table
            .filter(item_relationships::relationship.eq(RelationshipType::Parent))
            .select((item_relationships::source_id, item_relationships::target_id))
            .load::<(Uuid, Uuid)>(conn)?
            .into_iter()
            .map(|(parent_id, child_id)| rules::ParentEdge {
                parent_id,
                child_id,
            })
            .collect();

        let mut ids = rules::cascade_descendants(item_id, &edges);
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

        let mut cascaded_short_codes: Vec<String> = deleted
            .into_iter()
            .filter(|code| *code != root_short_code)
            .collect();
        cascaded_short_codes.sort();

        let details = if cascaded_short_codes.is_empty() {
            format!("short_code:{root_short_code} cascade:0")
        } else {
            format!(
                "short_code:{root_short_code} cascade:{} descendants:{}",
                cascaded_short_codes.len(),
                cascaded_short_codes.join(",")
            )
        };
        log_activity(
            conn,
            actor,
            ActivityAction::Delete,
            item_id,
            item_type.entity_type(),
            details,
        )?;

        // KAIROS-T-0022: one thin event for the cascade root, delivered on
        // commit ([`events::item_placement`] still sees the soft-deleted
        // row). Clients re-fetch, which also reveals cascaded descendants.
        events::emit_item_event_by_id(
            conn,
            EventKind::ItemDeleted,
            item_type.entity_type(),
            item_id,
            actor,
        )?;

        Ok(SoftDeleteOutcome {
            root_short_code,
            cascaded_short_codes,
        })
    })
}
