//! The KAIROS-T-0019 organizational + admin endpoint families
//! (KAIROS-S-0005): boards (+columns/transitions/items/members), teams
//! (+members), delivery streams (+teams), organization membership, and
//! deployment-admin tenant provisioning.
//!
//! Follows the T-0018 handler pattern from [`super`] (one module per
//! family, DTOs from `kairos_client::types_org`, every write in one
//! blocking-pool closure with the ABAC check on the same connection).
//! Gating (decisions recorded in KAIROS-T-0019):
//!
//! - board config writes (PATCH board, columns, transitions):
//!   `configure_boards` on that board (A-0006);
//! - board member/capability writes: `manage_members` on that board;
//! - board create/delete, teams, streams, org membership: tenant-wide
//!   config → org-admin-only (the A-0006 fallback; the 403 names the
//!   pseudo-capability with `board_id: null`);
//! - reads stay open tenant-wide;
//! - `/api/admin/tenants` ([`admin`]) is cross-tenant: registered OUTSIDE
//!   the tenant middleware, gated by `KAIROS_DEPLOYMENT_ADMINS`.

pub mod admin;
pub mod boards;
pub mod forge;
pub mod members;
pub mod repositories;
pub mod streams;
pub mod team_pages;
pub mod teams;

use axum::Router;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_core::board::ColumnRuleError;
use kairos_db::models::boards::Board;
use kairos_db::{AbacError, BoardError};
use serde_json::json;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;

/// The tenant-scoped T-0019 families, merged (mounted behind the full
/// auth → tenant middleware stack; [`admin`] is registered separately).
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(boards::router())
        .merge(forge::router())
        .merge(repositories::router())
        .merge(teams::router())
        .merge(team_pages::router())
        .merge(streams::router())
        .merge(members::router())
}

// ---------------------------------------------------------------------------
// Capability vocabulary (KAIROS-A-0006, validated at the API layer)
// ---------------------------------------------------------------------------

/// The fixed A-0006 capability vocabulary plus its glob forms — the values
/// a grant may carry. ONE source of truth: `kairos_core::abac::{CAPABILITIES,
/// GLOBS}` (KAIROS-T-0116 removed the duplicated list). The ABAC layer
/// stores free text (extensible by design); the API layer is where the
/// vocabulary is enforced.
fn capability_vocabulary() -> impl Iterator<Item = &'static str> {
    kairos_core::abac::CAPABILITIES
        .iter()
        .chain(kairos_core::abac::GLOBS.iter())
        .copied()
}

/// 422 `VALIDATION` unless every entry is in the A-0006 vocabulary (list
/// must be non-empty).
pub fn validate_capabilities(capabilities: &[String]) -> Result<(), ApiError> {
    if capabilities.is_empty() {
        return Err(ApiError::validation(
            "capabilities must contain at least one capability",
        ));
    }
    for capability in capabilities {
        if !capability_vocabulary().any(|known| known == capability.as_str()) {
            return Err(ApiError::validation(format!(
                "unknown capability {capability:?}; allowed: [{}]",
                capability_vocabulary().collect::<Vec<_>>().join(", ")
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared loading + error mapping
// ---------------------------------------------------------------------------

/// Load a live board by id, or 404.
pub fn load_board(conn: &mut PgConnection, board_id: Uuid) -> Result<Board, ApiError> {
    use kairos_db::schema::boards::dsl;
    dsl::boards
        .filter(dsl::id.eq(board_id))
        .filter(dsl::deleted_at.is_null())
        .select(Board::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("no live board {board_id}")))
}

/// How many workflow items (strategies/initiatives/tasks/ADRs) reference
/// `board_id` — soft-deleted rows included, mirroring T-0010's
/// column-removal rule (they still hold the FK and would orphan on
/// restore). Non-zero blocks board deletion (422 `BOARD_NOT_EMPTY`).
pub fn count_board_items(conn: &mut PgConnection, board_id: Uuid) -> Result<i64, ApiError> {
    use kairos_db::schema::{adrs, initiatives, strategies, tasks};

    let mut total: i64 = 0;
    total += strategies::table
        .filter(strategies::board_id.eq(board_id))
        .count()
        .get_result::<i64>(conn)
        .map_err(ApiError::internal)?;
    total += initiatives::table
        .filter(initiatives::board_id.eq(board_id))
        .count()
        .get_result::<i64>(conn)
        .map_err(ApiError::internal)?;
    total += tasks::table
        .filter(tasks::board_id.eq(board_id))
        .count()
        .get_result::<i64>(conn)
        .map_err(ApiError::internal)?;
    total += adrs::table
        .filter(adrs::board_id.eq(board_id))
        .count()
        .get_result::<i64>(conn)
        .map_err(ApiError::internal)?;
    Ok(total)
}

/// Run `f` inside ONE database transaction, keeping `ApiError` as the
/// handler error type (diesel's `transaction` needs `From<DieselError>`,
/// which `ApiError` deliberately does not implement — internal mapping
/// stays explicit).
pub fn run_in_transaction<T, F>(conn: &mut PgConnection, f: F) -> Result<T, ApiError>
where
    F: FnOnce(&mut PgConnection) -> Result<T, ApiError>,
{
    enum TxError {
        Api(ApiError),
        Db(diesel::result::Error),
    }
    impl From<diesel::result::Error> for TxError {
        fn from(e: diesel::result::Error) -> Self {
            TxError::Db(e)
        }
    }
    match conn.transaction::<_, TxError, _>(|conn| f(conn).map_err(TxError::Api)) {
        Ok(value) => Ok(value),
        Err(TxError::Api(e)) => Err(e),
        Err(TxError::Db(e)) => Err(ApiError::internal(e)),
    }
}

/// Whether a diesel error is a unique-constraint violation (mapped to 409
/// `CONFLICT` at call sites where the colliding value is a client input,
/// e.g. team/board/stream slugs).
pub fn is_unique_violation(e: &diesel::result::Error) -> bool {
    matches!(
        e,
        diesel::result::Error::DatabaseError(diesel::result::DatabaseErrorKind::UniqueViolation, _)
    )
}

/// [`BoardError`] → HTTP for the BOARD CONFIGURATION endpoints (columns /
/// transitions / board creation), where T-0010's typed rule errors are
/// client mistakes: `COLUMN_NOT_EMPTY`, `DUPLICATE_COLUMN_NAME`,
/// `DUPLICATE_COLUMN_POSITION`, `DUPLICATE_TRANSITION` (all 422); path ids
/// that resolve to nothing are 404; malformed body references are 422
/// `VALIDATION`.
pub fn map_config_error(e: BoardError) -> ApiError {
    match e {
        BoardError::BoardNotFound(id) => ApiError::not_found(format!("no live board {id}")),
        BoardError::ColumnNotFound(id) => ApiError::not_found(format!("no column {id}")),
        BoardError::TransitionNotFound { board_id, from, to } => {
            ApiError::not_found(format!("no transition {from} -> {to} on board {board_id}"))
        }
        BoardError::Rule(rule) => map_column_rule_error(rule),
        e @ (BoardError::MissingDefaults(_) | BoardError::InvalidDefaults { .. }) => {
            // Provisioning seeds all four default configs; absence is an
            // operator/data problem, not a client mistake.
            ApiError::internal(e)
        }
        // Item/transition errors cannot arise from configuration calls.
        e @ (BoardError::ItemNotFound { .. }
        | BoardError::ItemNotOnBoard { .. }
        | BoardError::Transition(_)) => ApiError::internal(e),
        BoardError::Database(e) => ApiError::internal(e),
    }
}

/// The T-0010 column/transition rule violations as typed 422s.
fn map_column_rule_error(e: ColumnRuleError) -> ApiError {
    match e {
        ColumnRuleError::ColumnNotEmpty { column, item_count } => ApiError::unprocessable(
            "COLUMN_NOT_EMPTY",
            format!(
                "column {:?} still contains {item_count} item(s); move them before removing it",
                column.name
            ),
        )
        .with_details(json!({
            "column": {"id": column.id, "name": column.name},
            "item_count": item_count,
        })),
        ColumnRuleError::DuplicateName(name) => ApiError::unprocessable(
            "DUPLICATE_COLUMN_NAME",
            format!("board already has a column named {name:?}"),
        ),
        ColumnRuleError::DuplicatePosition(position) => ApiError::unprocessable(
            "DUPLICATE_COLUMN_POSITION",
            format!("board already has a column at position {position}"),
        ),
        ColumnRuleError::DuplicateTransition => {
            ApiError::unprocessable("DUPLICATE_TRANSITION", "transition already exists")
        }
        e @ (ColumnRuleError::UnknownColumn(_)
        | ColumnRuleError::EmptyName
        | ColumnRuleError::NegativePosition(_)
        | ColumnRuleError::ReorderLengthMismatch { .. }
        | ColumnRuleError::ReorderDuplicateColumn(_)
        | ColumnRuleError::SelfTransition) => ApiError::validation(e.to_string()),
    }
}

/// [`AbacError`] → HTTP for the board-member GRANT/REVOKE endpoints, where
/// T-0011's typed errors are client-visible: duplicate grant → 409
/// `CONFLICT`, missing grant → 404.
pub fn map_grant_error(e: AbacError) -> ApiError {
    match e {
        AbacError::AlreadyGranted {
            user_id,
            capability,
            ..
        } => ApiError::conflict(format!(
            "capability {capability:?} is already granted to user {user_id} on this board"
        )),
        AbacError::GrantNotFound {
            user_id,
            capability,
            ..
        } => ApiError::not_found(format!(
            "capability {capability:?} is not granted to user {user_id} on this board"
        )),
        AbacError::EmptyCapability => ApiError::validation("capability must not be empty"),
        AbacError::Database(e) => ApiError::internal(e),
    }
}

/// Load a `public.users` row by id; 422 `VALIDATION` when it does not exist
/// (the id came from a request body).
pub fn require_user_exists(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<kairos_db::models::User, ApiError> {
    use kairos_db::schema::users;
    users::table
        .filter(users::id.eq(user_id))
        .select(kairos_db::models::User::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::validation(format!("user {user_id} does not exist")))
}
