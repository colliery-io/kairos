//! The KAIROS-S-0005 entity endpoint families (KAIROS-T-0018): strategies,
//! initiatives, tasks, documents, and ADRs — list/get/create/PATCH/DELETE
//! (+ transition for board items), wired to the T-0012 write services and
//! T-0010 transition services with A-0006 ABAC on every write.
//!
//! # The handler/module pattern (what the remaining endpoint tasks copy)
//!
//! One module per resource family, each exporting `router()`; this module
//! merges them and owns the shared plumbing:
//!
//! - request DTOs come from [`kairos_client::types`] (the shared wire
//!   types, A-0015); conversions from `kairos-db` models live in
//!   [`convert`];
//! - every write runs inside ONE [`crate::blocking::BlockingTenantPool`]
//!   closure: resolve the short code, [`require_capability`], call the
//!   kairos-db service, convert to the DTO — so the ABAC check and the
//!   write use the same tenant-pinned connection;
//! - reads are open tenant-wide (A-0006): list/get do no capability check
//!   beyond the middleware stack;
//! - service errors map to the S-0005 envelope here (`map_*_error`), NOT
//!   in kairos-db: 404 for unknown short codes, 409 `CONFLICT` with
//!   `details.current` (the handler reloads the entity), 422
//!   `INVALID_TRANSITION` with `details.allowed_targets`, 422
//!   `ITEM_NOT_ON_BOARD`, 422 `VALIDATION` for malformed/unknown body
//!   references, 403 via [`crate::error::ApiError::capability_required`];
//! - every handler carries a `#[utoipa::path]` annotation; the OpenAPI
//!   aggregation endpoint is KAIROS-T-0023.

pub mod adrs;
// KAIROS-T-0051: pre-delete cascade preview (generic {entity_type} read).
pub mod cascade;
pub mod convert;
pub mod convert_meta;
pub mod convert_org;
pub mod documents;
pub mod initiatives;
pub mod meta;
pub mod org;
pub mod search;
pub mod strategies;
pub mod tasks;
// KAIROS-T-0023: OpenAPI aggregation (/api/openapi.json) + dev Swagger UI.
pub mod openapi;

use axum::Router;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use kairos_client::types as dto;
use kairos_core::board::TransitionError;
use kairos_core::short_code::ItemType;
use kairos_db::{AbacError, BoardError, GraphError, ItemError, abac};
use serde_json::json;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;

/// All five entity family routers, merged (mounted behind the full
/// auth → tenant middleware stack in [`crate::app::router`]).
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(strategies::router())
        .merge(initiatives::router())
        .merge(tasks::router())
        .merge(documents::router())
        .merge(adrs::router())
        // KAIROS-T-0051: GET /api/{entity_type}/{short_code}/cascade-preview.
        .merge(cascade::router())
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Default page size when `?limit=` is omitted.
const DEFAULT_LIMIT: i64 = 50;
/// Hard cap on `?limit=`.
const MAX_LIMIT: i64 = 200;

/// Clamp raw S-0005 pagination params to `(limit, offset)`.
pub fn clamp_pagination(pagination: &dto::Pagination) -> (i64, i64) {
    let limit = pagination
        .limit
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let offset = pagination.offset.unwrap_or(0).max(0);
    (limit, offset)
}

// ---------------------------------------------------------------------------
// Body parsing helpers (wire strings → typed values, 422 VALIDATION)
// ---------------------------------------------------------------------------

/// Parse a UUID body field (`422 VALIDATION` on malformed input — the DTO
/// crate carries ids as strings, see `kairos_client::types`).
pub fn parse_uuid(value: &str, field: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value)
        .map_err(|_| ApiError::validation(format!("{field} must be a UUID, got {value:?}")))
}

/// [`parse_uuid`] over an optional field.
pub fn parse_opt_uuid(value: Option<&str>, field: &str) -> Result<Option<Uuid>, ApiError> {
    value.map(|v| parse_uuid(v, field)).transpose()
}

/// Parse a TEXT-backed enum body field (`task_type`, `complexity`,
/// `bucket_type`) via its `FromStr`, naming the allowed values on failure.
pub fn parse_enum<T>(value: &str, field: &str, allowed: &[T]) -> Result<T, ApiError>
where
    T: std::str::FromStr + std::fmt::Display + Copy,
{
    value.parse::<T>().map_err(|_| {
        let allowed: Vec<String> = allowed.iter().map(|v| v.to_string()).collect();
        ApiError::validation(format!(
            "{field} must be one of [{}], got {value:?}",
            allowed.join(", ")
        ))
    })
}

// ---------------------------------------------------------------------------
// ABAC (KAIROS-A-0006)
// ---------------------------------------------------------------------------

/// The A-0006 write gate: org admins bypass; otherwise the caller needs a
/// matching `board_member_capabilities` grant on `board_id`. `board_id:
/// None` = no board context exists (off-board ADR, document with no
/// resolvable parent board) → the org-admin-only fallback. Denial is 403
/// with the missing capability named in `details`.
pub fn require_capability(
    conn: &mut PgConnection,
    slug: &str,
    board_id: Option<Uuid>,
    user_id: Uuid,
    capability: &str,
) -> Result<(), ApiError> {
    let allowed = match board_id {
        Some(board_id) => abac::authorize(conn, slug, board_id, user_id, capability),
        None => abac::is_org_admin(conn, slug, user_id),
    }
    .map_err(map_abac_error)?;
    if allowed {
        Ok(())
    } else {
        Err(ApiError::capability_required(capability, board_id))
    }
}

// ---------------------------------------------------------------------------
// Short-code resolution
// ---------------------------------------------------------------------------

#[derive(QueryableByName)]
struct DirectoryRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    entity_type: String,
}

/// Whether a lookup may return work that has been archived.
///
/// Archiving is a visibility default and nothing more (KAIROS-A-0020), so
/// the question "may I see put-away work?" belongs at the call site rather
/// than buried in a query. Every listing and every write path names
/// [`Liveness::LiveOnly`] explicitly: a reviewer checking that this
/// initiative changed no default behaviour can do it by looking for call
/// sites that say anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Live rows only.
    LiveOnly,
    /// Archived rows too. Callers that pass this MUST mark what they serve
    /// — an auditor may never mistake archived work for live work.
    IncludeArchived,
}

/// Resolve a short code to `(id, entity_type)` across all five entity
/// tables. `Ok(None)` = unknown, or archived when `liveness` is
/// [`Liveness::LiveOnly`].
///
/// Both modes read `entity_directory`. Between KAIROS-T-0154 and
/// KAIROS-T-0156 the archived mode had to re-derive the directory from the
/// five base tables by hand, because the view filtered `deleted_at IS NULL`
/// in its own body and so could not be asked about the rows it had already
/// dropped. T-0156 moved that predicate out here, where it is a mode rather
/// than a fact, and the duplicate UNION went with it.
pub fn resolve_short_code(
    conn: &mut PgConnection,
    short_code: &str,
    liveness: Liveness,
) -> Result<Option<(Uuid, ItemType)>, ApiError> {
    let sql = match liveness {
        Liveness::LiveOnly => {
            "SELECT id, entity_type FROM entity_directory \
             WHERE short_code = $1 AND deleted_at IS NULL"
        }
        Liveness::IncludeArchived => {
            "SELECT id, entity_type FROM entity_directory WHERE short_code = $1"
        }
    };
    let row: Option<DirectoryRow> = sql_query(sql)
        .bind::<Text, _>(short_code)
        .get_result(conn)
        .optional()
        .map_err(ApiError::internal)?;
    row.map(|row| {
        ItemType::ALL
            .iter()
            .copied()
            .find(|t| t.entity_type() == row.entity_type)
            .map(|item_type| (row.id, item_type))
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "entity_directory returned unknown entity_type {:?}",
                    row.entity_type
                ))
            })
    })
    .transpose()
}

/// The type of a live item by id (documents included), via the same
/// directory view — for callers that hold an id, not a code
/// (KAIROS-T-0111: the edge-permission check on an existing relationship).
/// [`Liveness::LiveOnly`] is not a parameter here on purpose: the one
/// caller is a permission check guarding a WRITE, and writes see live rows
/// only (KAIROS-I-0015 D5).
pub fn resolve_item_type(conn: &mut PgConnection, id: Uuid) -> Result<Option<ItemType>, ApiError> {
    let row: Option<DirectoryRow> = sql_query(
        "SELECT id, entity_type FROM entity_directory WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(conn)
    .optional()
    .map_err(ApiError::internal)?;
    Ok(row.and_then(|row| {
        ItemType::ALL
            .iter()
            .copied()
            .find(|t| t.entity_type() == row.entity_type)
    }))
}

/// The 404 for `/{short_code}` path segments that resolve to nothing.
///
/// It used to say "no live …", which was accurate while archiving hid work
/// from every surface. Under KAIROS-A-0020 a reader can ask for archived
/// work, so a 404 now means the code is unknown outright — claiming
/// otherwise would send someone looking for an item that never existed.
pub fn short_code_not_found(entity_type: &str, short_code: &str) -> ApiError {
    ApiError::not_found(format!("no {entity_type} with short code {short_code:?}"))
}

// ---------------------------------------------------------------------------
// Service-error → S-0005 envelope mapping
// ---------------------------------------------------------------------------

/// [`AbacError`] never carries a client mistake on the check path (grants
/// and revokes surface their typed errors through the admin endpoints,
/// KAIROS-T-0019) — anything here is internal.
pub fn map_abac_error(e: AbacError) -> ApiError {
    ApiError::internal(e)
}

/// [`ItemError`] → HTTP. `VersionConflict` is NOT expected here: PATCH
/// handlers catch it themselves to attach the full current entity as
/// `details.current`; this fallback still returns a correct 409 from the
/// typed fields should another path surface one.
pub fn map_item_error(e: ItemError) -> ApiError {
    match e {
        ItemError::ItemNotFound { entity_type, id } => {
            ApiError::not_found(format!("{entity_type} {id} does not exist"))
        }
        ItemError::HistoryNotFound { item_id, version } => ApiError::not_found(format!(
            "no history snapshot for item {item_id} at version {version}"
        )),
        ItemError::VersionConflict {
            expected_version,
            current_version,
            current_title,
            current_content,
            ..
        } => ApiError::conflict(format!(
            "version mismatch: expected {expected_version}, current is {current_version}"
        ))
        .with_details(json!({
            "current": {
                "version": current_version,
                "title": current_title,
                "content": current_content,
            }
        })),
        ItemError::BoardNotFound(id) => ApiError::validation(format!("board {id} does not exist")),
        ItemError::BoardHasNoColumns(id) => {
            ApiError::validation(format!("board {id} has no columns to place the item in"))
        }
        ItemError::ColumnNotOnBoard {
            board_id,
            column_id,
        } => ApiError::validation(format!(
            "column {column_id} is not a column of board {board_id}"
        )),
        ItemError::TemplateNotFound(id) => {
            ApiError::validation(format!("template {id} does not exist"))
        }
        ItemError::RepositoryNotFound(id) => {
            ApiError::validation(format!("repository {id} does not exist"))
        }
        ItemError::Database(e) => ApiError::internal(e),
    }
}

/// [`BoardError`] → HTTP, for the transition endpoints: invalid moves are
/// 422 `INVALID_TRANSITION` carrying `details.allowed_targets` (S-0005 /
/// core `TransitionError::NotAllowed`); an off-board ADR is 422
/// `ITEM_NOT_ON_BOARD` (T-0010's typed error).
pub fn map_board_error(e: BoardError) -> ApiError {
    match e {
        BoardError::ItemNotFound { entity_type, id } => {
            ApiError::not_found(format!("{entity_type} {id} does not exist"))
        }
        BoardError::ItemNotOnBoard { entity_type, id } => ApiError::unprocessable(
            "ITEM_NOT_ON_BOARD",
            format!("{entity_type} {id} is not placed on a board, so it cannot be transitioned"),
        ),
        BoardError::Transition(TransitionError::NotAllowed {
            from,
            to,
            allowed_targets,
        }) => ApiError::unprocessable(
            "INVALID_TRANSITION",
            format!(
                "transition {:?} -> {:?} is not allowed by this board",
                from.name, to.name
            ),
        )
        .with_details(json!({
            "from": {"id": from.id, "name": from.name},
            "to": {"id": to.id, "name": to.name},
            "allowed_targets": allowed_targets
                .iter()
                .map(|c| json!({"id": c.id, "name": c.name}))
                .collect::<Vec<_>>(),
        })),
        BoardError::Transition(e) => ApiError::unprocessable("INVALID_TRANSITION", e.to_string()),
        BoardError::ColumnNotFound(id) => {
            ApiError::validation(format!("column {id} does not exist"))
        }
        BoardError::BoardNotFound(id) => ApiError::validation(format!("board {id} does not exist")),
        // KAIROS-I-0012: moving a task between delivery boards.
        BoardError::SameBoard(id) => {
            ApiError::unprocessable("SAME_BOARD", format!("the task is already on board {id}"))
        }
        BoardError::NotDeliveryBoard(id) => ApiError::unprocessable(
            "NOT_DELIVERY_BOARD",
            format!("board {id} is not a delivery board; tasks move between delivery boards only"),
        ),
        BoardError::RepositoryOwnerMismatch {
            repository,
            owner_board_id,
            detail,
        } => {
            let message = match (owner_board_id, detail) {
                (Some(board), _) => format!(
                    "the task is bound to repository {repository:?}, whose owning team's delivery \
                     board is {board}; move it there, or unbind it first \
                     (PUT /api/tasks/{{code}}/repository with repository: null)"
                ),
                (None, detail) => format!(
                    "the task is bound to repository {repository:?} but its owner has no single \
                     delivery board to move to ({}); unbind it first",
                    detail.unwrap_or_default()
                ),
            };
            ApiError::unprocessable("REPOSITORY_OWNER_MISMATCH", message).with_details(json!({
                "repository": repository,
                "owner_board_id": owner_board_id,
            }))
        }
        BoardError::NoEntryColumn(id) => ApiError::unprocessable(
            "NO_ENTRY_COLUMN",
            format!("board {id} has no columns to land in"),
        ),
        // Board-configuration errors cannot arise from the entity routes;
        // reaching one here is a bug, not a client mistake.
        e @ (BoardError::TransitionNotFound { .. }
        | BoardError::MissingDefaults(_)
        | BoardError::InvalidDefaults { .. }
        | BoardError::Rule(_)) => ApiError::internal(e),
        BoardError::Database(e) => ApiError::internal(e),
    }
}

/// [`GraphError`] → HTTP, for the document-create `supports` edge: rule
/// violations restate the allowed shape (422); the rest cannot be caused
/// by a request that got this far.
pub fn map_graph_error(e: GraphError) -> ApiError {
    match e {
        GraphError::Rule(e) => ApiError::validation(e.to_string()),
        GraphError::ItemNotFound(id) => {
            ApiError::validation(format!("related item {id} does not exist"))
        }
        e @ (GraphError::SelfLink(_)
        | GraphError::CycleDetected { .. }
        | GraphError::AlreadyLinked { .. }
        | GraphError::NotLinked { .. }) => ApiError::validation(e.to_string()),
        GraphError::Database(e) => ApiError::internal(e),
    }
}
