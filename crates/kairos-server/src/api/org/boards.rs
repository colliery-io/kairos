//! `/api/boards` (KAIROS-S-0005 Boards + Board Authorization families,
//! KAIROS-T-0019): board CRUD, the grouped items view, column/transition
//! configuration over the T-0010 services (typed rule violations → 422),
//! and capability administration over the T-0011 grant/revoke services.
//!
//! Gating: board creation/deletion is org-admin-only (no/whole-board
//! context → the A-0006 tenant-config fallback); PATCH board + column +
//! transition writes require `configure_boards` on the board; member/
//! capability writes require `administer_members`. Reads are open tenant-wide.

use std::collections::{BTreeSet, HashMap};

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_org as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::boards::{Board, BoardColumn, BoardTransition};
use kairos_db::models::enums::{ActivityAction, BoardLevel};
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::models::items::{Adr, Initiative, Strategy, Task};
use kairos_db::{abac, boards, items};
use serde_json::json;
use uuid::Uuid;

use super::super::convert::{IntoDto, attach_repositories};
use super::super::{clamp_pagination, parse_enum, parse_uuid, require_capability};
use super::{
    check_slug_form, count_live_board_items, load_board, load_board_by_ref, map_config_error,
    map_grant_error, require_user_exists, run_in_transaction, validate_capabilities,
};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 capability for board configuration writes.
const CONFIGURE: &str = "configure_boards";
/// The A-0006 capability for board membership/capability administration.
const ADMINISTER_MEMBERS: &str = "administer_members";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/boards", get(list_boards).post(create_board))
        .route(
            "/api/boards/{id}",
            get(get_board).patch(update_board).delete(delete_board),
        )
        .route("/api/boards/{id}/items", get(board_items))
        .route(
            "/api/boards/{id}/code-sequences/{item_type}",
            axum::routing::put(set_code_sequence),
        )
        .route(
            "/api/boards/{id}/columns",
            get(list_columns).post(add_column),
        )
        .route(
            "/api/boards/{id}/columns/{col_id}",
            axum::routing::patch(update_column).delete(remove_column),
        )
        .route(
            "/api/boards/{id}/transitions",
            get(list_transitions).post(add_transition),
        )
        .route(
            "/api/boards/{id}/transitions/{transition_id}",
            axum::routing::delete(remove_transition),
        )
        .route(
            "/api/boards/{id}/members",
            get(list_board_members).post(add_board_member),
        )
        .route(
            "/api/boards/{id}/members/{user_id}",
            axum::routing::patch(replace_capabilities).delete(remove_board_member),
        )
}

// ---------------------------------------------------------------------------
// Shared loading
// ---------------------------------------------------------------------------

/// LIVE columns of a board in position order. A removed column
/// (KAIROS-T-0161) keeps its row so the archived cards in it still know
/// where they were put away, but it is not part of the board any more and
/// must not render on one.
fn load_columns(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError> {
    load_columns_including_removed(conn, board_id, false)
}

/// Columns of a board in position order, optionally including the removed
/// ones (`removed_at` set on the DTO).
///
/// The only caller that asks for removed columns is a reader that already
/// has an ARCHIVED item in its hand and needs the name of the column it
/// was put away in (KAIROS-T-0164's item page). Nothing that renders or
/// validates a live board may pass `true` — see [`load_columns`].
fn load_columns_including_removed(
    conn: &mut PgConnection,
    board_id: Uuid,
    include_removed: bool,
) -> Result<Vec<BoardColumn>, ApiError> {
    use kairos_db::schema::board_columns::dsl;
    let mut query = dsl::board_columns
        .filter(dsl::board_id.eq(board_id))
        .into_boxed();
    if !include_removed {
        query = query.filter(dsl::deleted_at.is_null());
    }
    query
        .order(dsl::position.asc())
        .select(BoardColumn::as_select())
        .load(conn)
        .map_err(ApiError::internal)
}

/// Transition edges of a board, between LIVE columns.
///
/// Removing a column used to cascade its edges away (the `ON DELETE
/// CASCADE` on `board_transitions`); a soft delete cascades nothing, so
/// the edges are filtered here instead. The board's wiring therefore
/// survives a column removal, which is the better behaviour and the reason
/// this is a filter rather than a delete (KAIROS-T-0161).
fn load_transitions(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<Vec<BoardTransition>, ApiError> {
    use kairos_db::schema::board_columns;
    use kairos_db::schema::board_transitions::dsl;

    let live = || {
        board_columns::table
            .filter(board_columns::board_id.eq(board_id))
            .filter(board_columns::deleted_at.is_null())
            .select(board_columns::id)
    };
    dsl::board_transitions
        .filter(dsl::board_id.eq(board_id))
        .filter(dsl::from_column_id.eq_any(live()))
        .filter(dsl::to_column_id.eq_any(live()))
        .select(BoardTransition::as_select())
        .load(conn)
        .map_err(ApiError::internal)
}

/// The board + full configuration as the `BoardDetail` DTO.
///
/// `include_removed_columns` adds the columns that have been removed from
/// the board (each carrying `removed_at`); it is off for every caller but
/// KAIROS-T-0164's archived-item read. Transitions stay live-only either
/// way — a removed column is never a legal move target.
fn board_detail(
    conn: &mut PgConnection,
    board: Board,
    include_removed_columns: bool,
) -> Result<dto::BoardDetail, ApiError> {
    let columns = load_columns_including_removed(conn, board.id, include_removed_columns)?;
    let transitions = load_transitions(conn, board.id)?;
    Ok(dto::BoardDetail {
        board: board.into_dto(),
        columns: columns.into_iter().map(IntoDto::into_dto).collect(),
        transitions: transitions.into_iter().map(IntoDto::into_dto).collect(),
    })
}

/// A LIVE column of `board_id` by id, or 404 (also 404 when the column
/// belongs to a different board — path scoping — or has been removed).
fn load_column_of_board(
    conn: &mut PgConnection,
    board_id: Uuid,
    column_id: Uuid,
) -> Result<BoardColumn, ApiError> {
    use kairos_db::schema::board_columns::dsl;
    dsl::board_columns
        .filter(dsl::id.eq(column_id))
        .filter(dsl::board_id.eq(board_id))
        .filter(dsl::deleted_at.is_null())
        .select(BoardColumn::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::not_found(format!("The board {board_id} has no column {column_id}."))
        })
}

/// Insert one `activity_log` row (same shape as the kairos-db services).
fn log_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    entity_id: Uuid,
    entity_type: &str,
    details: String,
) -> Result<(), ApiError> {
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(entity_id),
            entity_type: Some(entity_type.to_string()),
            details,
        })
        .execute(conn)
        .map_err(ApiError::internal)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Boards CRUD
// ---------------------------------------------------------------------------

/// List boards (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/boards",
    tag = "boards",
    params(Pagination),
    responses(
        (status = 200, description = "Page of boards", body = ListEnvelope<dto::Board>),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_boards(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    ApiQuery(pagination): ApiQuery<Pagination>,
) -> Result<Json<ListEnvelope<dto::Board>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::boards::dsl;
            let total: i64 = dsl::boards
                .filter(dsl::deleted_at.is_null())
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Board> = dsl::boards
                .filter(dsl::deleted_at.is_null())
                .order(dsl::slug.asc())
                .limit(limit)
                .offset(offset)
                .select(Board::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            Ok(ListEnvelope {
                items: rows.into_iter().map(IntoDto::into_dto).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Query of `GET /api/boards/{id}`.
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(crate) struct BoardDetailQuery {
    /// Also return the columns that have been REMOVED from this board,
    /// each carrying `removed_at` (default `false` — a removed column is
    /// not part of the board and must not render on one, KAIROS-T-0161).
    ///
    /// The one legitimate caller is a reader holding an ARCHIVED item that
    /// still points at such a column: "which column was this put away in?"
    /// has to stay answerable (KAIROS-A-0020, KAIROS-T-0164).
    #[serde(default)]
    include_removed_columns: bool,
}

/// Board detail: the board plus its columns and transitions (open
/// tenant-wide).
///
/// The path takes the slug or the id of the board (COLLIERY-T-0265). An
/// unknown slug and an unknown id are a 404.
#[utoipa::path(
    get,
    path = "/api/boards/{id}",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board"), BoardDetailQuery),
    responses(
        (status = 200, description = "The board with columns and transitions", body = dto::BoardDetail),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_board(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<BoardDetailQuery>,
) -> Result<Json<dto::BoardDetail>, ApiError> {
    let detail = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board_by_ref(conn, &id)?;
            board_detail(conn, board, query.include_removed_columns)
        })
        .await?;
    Ok(Json(detail))
}

/// Create a board seeded with the system default columns/transitions for
/// its level (KAIROS-A-0002). Org-admin-only: a new board has no capability
/// context yet (A-0006 tenant-config fallback).
///
/// A delivery board needs a team (COLLIERY-T-0230). A request for a
/// delivery board with no `team_id` is a 422. The board of a task decides
/// its team, and a request to a board with no team goes to nobody. The
/// check is in `kairos_db::boards::create_board`, which every entry point
/// shares.
///
/// A strategy or initiative board has no `team_id`. Its team is the list
/// of its members (`GET /api/boards/{id}/members`). A request for such a
/// board with a `team_id` is a 422 (COLLIERY-T-0242).
///
/// An ADR board can have a team (COLLIERY-T-3102). That board holds the
/// delivery ADRs of the team. Its `code_prefix` must be the
/// prefix of the delivery board of the team. If not, the request is a 422
/// `VALIDATION` with `details.field` = `code_prefix` and
/// `details.expected`. An ADR board with no `team_id` is a board of the
/// organization.
///
/// A team has one ADR board. A request for a second one is a 422
/// `TEAM_HAS_ADR_BOARD`, and the refusal names the board that the team has.
///
/// A team has one delivery board (COLLIERY-T-0240). A request for a
/// delivery board for a team that has a live delivery board is a 422
/// `TEAM_HAS_DELIVERY_BOARD`. The refusal names the board that the team
/// has. A deleted board does not count.
///
/// A `slug` must have the form of a board slug (COLLIERY-T-0258). It must
/// match `^[a-z][a-z0-9_-]{1,62}$`, and it cannot have the form of a UUID.
/// If not, the request is a 422 `VALIDATION` with `details.field` = `slug`.
///
/// A live board has its slug alone (COLLIERY-T-0255). A request with the
/// slug of a live board is a 409 `CONFLICT`. The refusal names the slug
/// and the board that has it (`details.slug`, `details.board`). A deleted
/// board does not keep its slug.
///
/// Each board has a short-code prefix, `code_prefix` (COLLIERY-T-3099). It
/// is required, and it never changes. An item on the board gets the code
/// `{code_prefix}-{type letter}-{number}`. The number comes from the
/// sequence of the prefix and the type.
///
/// A prefix that does not match `^[A-Z][A-Z0-9]{1,9}$` is a 422
/// `VALIDATION` with `details.field` = `code_prefix`.
///
/// Boards can share a prefix when they hold different types. The level
/// gives the types. A prefix that a live board of the same level has is a
/// 409 `CONFLICT`. The refusal names the prefix and the board that has it
/// (`details.code_prefix`, `details.board`).
#[utoipa::path(
    post,
    path = "/api/boards",
    tag = "boards",
    request_body = dto::CreateBoardRequest,
    responses(
        (status = 201, description = "Created, with the seeded configuration", body = dto::BoardDetail),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A live board has the slug, or a live board of the same level has the prefix; details.board names it", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad level/team reference, a slug that does not have the form of a board slug, a code_prefix that is absent or does not match the rule, a delivery board with no team, a strategy or initiative board with a team, the prefix of a team ADR board that is not the prefix of the team, TEAM_HAS_DELIVERY_BOARD, or TEAM_HAS_ADR_BOARD", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_board(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<dto::CreateBoardRequest>,
) -> Result<(StatusCode, Json<dto::BoardDetail>), ApiError> {
    let level = parse_enum::<BoardLevel>(&body.board_level, "board_level", BoardLevel::ALL)?;
    let team_id = body
        .team_id
        .as_deref()
        .map(|v| parse_uuid(v, "team_id"))
        .transpose()?;
    // COLLIERY-T-0258: the form of the slug that the caller sent.
    check_slug_form("board", &body.slug)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let detail = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, CONFIGURE)?;
            if let Some(team_id) = team_id {
                use kairos_db::schema::teams::dsl;
                let exists: Option<Uuid> = dsl::teams
                    .filter(dsl::id.eq(team_id))
                    .filter(dsl::deleted_at.is_null())
                    .select(dsl::id)
                    .first(conn)
                    .optional()
                    .map_err(ApiError::internal)?;
                if exists.is_none() {
                    return Err(ApiError::validation(format!(
                        "The team {team_id} is not in the organization. Send the id of a \
                         team of the organization as team_id."
                    )));
                }
            }
            // COLLIERY-T-0255: a slug that a live board has is a 409 that
            // names the board (`BoardError::SlugTaken`).
            // COLLIERY-T-3099: a bad prefix is a 422, and a prefix that a
            // live board of the same level has is a 409 that names the board.
            let board = boards::create_board(
                conn,
                level,
                &body.name,
                &body.slug,
                boards::CodePrefix::Given(&body.code_prefix),
                team_id,
                Some(user),
            )
            .map_err(map_config_error)?;
            board_detail(conn, board, false)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

/// Update board settings (name/slug). Requires `configure_boards` on the
/// board.
///
/// The team of a board does not change (COLLIERY-T-0230, COLLIERY-T-0243).
/// A `team_id` that is not the team of the board is a 422
/// `BOARD_TEAM_IS_FIXED`, and the update writes nothing. To give work to a
/// different team, move the task (`POST /api/tasks/{code}/move`).
///
/// The server accepts a `team_id` equal to the team of the board, and
/// changes nothing. A client can thus send back the `team_id` that it
/// read. That `team_id` is not a field to update: the body must have `name`
/// or `slug`.
///
/// A new `slug` must have the form of a board slug (COLLIERY-T-0258). If
/// not, the request is a 422 `VALIDATION`, and the update writes nothing.
/// A board with a slug from before the rule keeps that slug. An update of
/// the name of that board passes.
///
/// A `slug` that a different live board has is a 409 `CONFLICT`, and the
/// update writes nothing (COLLIERY-T-0255). The refusal names the slug and
/// the board that has it. A board can keep its slug in an update.
///
/// The body has the fields `name`, `slug` and `team_id` only. A different
/// field of the board (`id`, `board_level`) is a 422 `VALIDATION` that
/// names the field (COLLIERY-T-0249).
///
/// The short-code prefix of a board does not change (COLLIERY-T-3099). A
/// body with `code_prefix` is a 422 `CODE_PREFIX_IS_FIXED` that says so
/// (COLLIERY-T-3101), and the update writes nothing.
#[utoipa::path(
    patch,
    path = "/api/boards/{id}",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
    request_body = dto::UpdateBoardRequest,
    responses(
        (status = 200, description = "Updated", body = dto::Board),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A live board has the slug; details.board names it", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "No field to update, BOARD_TEAM_IS_FIXED, or CODE_PREFIX_IS_FIXED", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_board(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<dto::UpdateBoardRequest>,
) -> Result<Json<dto::Board>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    // Outer `None`: the body has no `team_id`. Inner `None`: a null.
    let sent_team = body
        .team_id
        .as_ref()
        .map(|team| {
            team.as_deref()
                .map(|v| parse_uuid(v, "team_id"))
                .transpose()
        })
        .transpose()?;
    let nothing_to_update = || {
        ApiError::validation(
            "The request has no field to change. Send one or more of name, slug and team_id.",
        )
    };
    if body.name.is_none() && body.slug.is_none() && sent_team.is_none() {
        return Err(nothing_to_update());
    }
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::boards::dsl;
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;
            // COLLIERY-T-0243: before the check for an empty body, so that
            // a body with only a different team gets the refusal that
            // tells the caller what to do.
            boards::check_board_team(&board, sent_team).map_err(map_config_error)?;
            if body.name.is_none() && body.slug.is_none() {
                return Err(nothing_to_update());
            }
            // COLLIERY-T-0258: the form of a slug that the caller sent. A
            // board keeps a slug from before the rule, so a request that
            // sends the slug that the board has is not a change.
            if let Some(new_slug) = body.slug.as_deref()
                && new_slug != board.slug
            {
                check_slug_form("board", new_slug)?;
            }
            // COLLIERY-T-0255: two live boards cannot have the same slug.
            if let Some(new_slug) = body.slug.as_deref() {
                boards::check_board_slug(conn, new_slug, Some(board_id))
                    .map_err(map_config_error)?;
            }
            let updated = diesel::update(dsl::boards.filter(dsl::id.eq(board_id)))
                .set((
                    kairos_db::models::boards::BoardChangeset {
                        name: body.name.clone(),
                        slug: body.slug.clone(),
                        ..Default::default()
                    },
                    dsl::updated_at.eq(diesel::dsl::now),
                ))
                .returning(Board::as_returning())
                .get_result::<Board>(conn);
            // An update at the same time got the slug first: the index
            // refused this one. The refusal has the same form.
            let updated = updated.map_err(|e| {
                let slug = body.slug.as_deref().unwrap_or(&board.slug);
                map_config_error(boards::slug_violation(conn, slug, Some(board_id), e))
            })?;
            log_activity(
                conn,
                user,
                ActivityAction::BoardConfig,
                board_id,
                "board",
                format!("board_settings:{}", updated.slug),
            )?;
            Ok(updated.into_dto())
        })
        .await?;
    Ok(Json(updated))
}

/// Soft-delete a board. Only allowed when NO workflow item references it
/// (422 `BOARD_NOT_EMPTY` otherwise — the T-0010 empty rule applied at
/// board scope). Requires `configure_boards` on the board.
///
/// A board that is the owner of a live document is not deleted
/// (COLLIERY-T-0269): 422 `BOARD_OWNS_DOCUMENTS`. A document always has
/// an owner. Name a different board for each document, or archive it.
///
/// The only delivery board of a team is not deleted (COLLIERY-T-0241): 422
/// `LAST_DELIVERY_BOARD`. A team always has a delivery board. To remove
/// the board, delete the team (`DELETE /api/teams/{id}`), which removes
/// the team and its board together. This check comes before the check for
/// live items, because no retry can pass it.
#[utoipa::path(
    delete,
    path = "/api/boards/{id}",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
    responses(
        (status = 200, description = "Soft-deleted", body = dto::OrgDeleteResponse),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "BOARD_NOT_EMPTY, LAST_DELIVERY_BOARD, or BOARD_OWNS_DOCUMENTS", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_board(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::boards::dsl;
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;
            // One transaction: `check_board_delete` locks the team until
            // the board is deleted (COLLIERY-T-0241).
            run_in_transaction(conn, |conn| {
                boards::check_board_delete(conn, &board).map_err(map_config_error)?;
                let item_count = count_live_board_items(conn, board_id)?;
                if item_count > 0 {
                    let items = super::live_board_item_codes(conn, board_id, 20)?;
                    return Err(ApiError::unprocessable(
                        "BOARD_NOT_EMPTY",
                        format!(
                            "The board {:?} has {item_count} live card{}: [{}]. Move each card \
                             to a different board or delete it. Then delete the board.",
                            board.name,
                            if item_count == 1 { "" } else { "s" },
                            items.join(", ")
                        ),
                    )
                    .with_details(json!({ "item_count": item_count, "items": items })));
                }
                // COLLIERY-T-0269: a document that names the board is not a
                // card, and the board is its owner.
                super::check_board_owns_no_document(
                    conn,
                    board_id,
                    &board.name,
                    "Then delete the board.",
                )?;
                diesel::update(dsl::boards.filter(dsl::id.eq(board_id)))
                    .set((
                        dsl::deleted_at.eq(diesel::dsl::now),
                        dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .execute(conn)
                    .map_err(ApiError::internal)?;
                log_activity(
                    conn,
                    user,
                    ActivityAction::Delete,
                    board_id,
                    "board",
                    format!("board:{}", board.slug),
                )?;
                Ok(dto::OrgDeleteResponse {
                    id: board_id.to_string(),
                    deleted: true,
                })
            })
        })
        .await?;
    Ok(Json(outcome))
}

// ---------------------------------------------------------------------------
// Code sequences (COLLIERY-T-3104)
// ---------------------------------------------------------------------------

/// The types of item that a board of `level` gives a code to. Each board
/// can own documents (COLLIERY-T-0269).
fn types_of_level(level: BoardLevel) -> &'static [ItemType] {
    match level {
        BoardLevel::Strategy => &[ItemType::Strategy, ItemType::Document],
        BoardLevel::Initiative => &[ItemType::Initiative, ItemType::Document],
        BoardLevel::Delivery => &[ItemType::Task, ItemType::Document],
        BoardLevel::Adr => &[ItemType::Adr, ItemType::Document],
    }
}

/// Set the number of the next code of a type on a board (COLLIERY-T-3104).
/// Org-admin-only.
///
/// The next create of `item_type` on the board gets the code
/// `{code_prefix}-{type letter}-{next_number}`. The Metis import uses this
/// route to keep the Metis numbers on a board (`--codes keep`).
///
/// A sequence never goes back. `next_number` must be above the last
/// number of the sequence of the prefix and the type. It must also be above
/// the number of each code of that prefix and type: live, archived or
/// retired. The next number of the sequence changes nothing. Thus you can
/// send the same request 2 times.
///
/// - A code that an item has (live or archived) is a 409 `CODE_IN_USE`.
/// - A retired code (COLLIERY-T-3100) is a 409 `CODE_RETIRED`, with
///   `details.current_code` when the item exists.
/// - A different number at or below the sequence is a 409
///   `SEQUENCE_IS_PAST`, with `details.next_code`.
/// - An `item_type` that the board does not hold is a 422 `VALIDATION` with
///   `details.parameter` = `item_type` and `details.allowed`. A board holds
///   the type of its level and documents.
/// - A `next_number` below 1 is a 422 `VALIDATION` with `details.field` =
///   `next_number`. An unknown field is a 422 that names it.
#[utoipa::path(
    put,
    path = "/api/boards/{id}/code-sequences/{item_type}",
    tag = "boards",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the board"),
        ("item_type" = String, Path, description = "`strategy|initiative|task|document|adr`: a type that the board holds"),
    ),
    request_body = dto::SetCodeSequenceRequest,
    responses(
        (status = 200, description = "The sequence; next_code is the code of the next create", body = dto::CodeSequence),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "CODE_IN_USE, CODE_RETIRED, or SEQUENCE_IS_PAST", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "An item_type that the board does not hold, a next_number below 1, or an unknown field", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_code_sequence(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, item_type)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::SetCodeSequenceRequest>,
) -> Result<Json<dto::CodeSequence>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let sequence = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, CONFIGURE)?;
            let board = load_board_by_ref(conn, &id)?;
            let allowed = types_of_level(board.board_level);
            let Some(item_type) = allowed
                .iter()
                .copied()
                .find(|t| t.entity_type() == item_type)
            else {
                let names: Vec<&str> = allowed.iter().map(|t| t.entity_type()).collect();
                return Err(ApiError::validation(format!(
                    "The board {:?} does not give codes to the type {item_type:?}. Use one of \
                     these values for item_type: {}.",
                    board.slug,
                    names.join(", ")
                ))
                .with_details(json!({ "parameter": "item_type", "allowed": names })));
            };
            let sequence =
                items::set_next_code_number(conn, &board.code_prefix, item_type, body.next_number)
                    .map_err(map_code_sequence_error)?;
            log_activity(
                conn,
                user,
                ActivityAction::Update,
                board.id,
                "board",
                format!("code_sequence:{}", sequence.next_code),
            )?;
            Ok(dto::CodeSequence {
                code_prefix: sequence.code_prefix,
                item_type: sequence.item_type.entity_type().to_string(),
                last_number: sequence.last_number,
                next_code: sequence.next_code,
            })
        })
        .await?;
    Ok(Json(sequence))
}

/// [`items::CodeSequenceError`] to its HTTP refusal.
fn map_code_sequence_error(e: items::CodeSequenceError) -> ApiError {
    use items::CodeSequenceError as E;
    match e {
        E::NotPositive(_) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "next_number" }))
        }
        E::CodeInUse { ref code } => {
            ApiError::new(StatusCode::CONFLICT, "CODE_IN_USE", e.to_string())
                .with_details(json!({ "code": code }))
        }
        E::CodeRetired {
            ref code,
            ref current_code,
        } => ApiError::new(StatusCode::CONFLICT, "CODE_RETIRED", e.to_string())
            .with_details(json!({ "code": code, "current_code": current_code })),
        E::Behind {
            last_number,
            ref next_code,
            ..
        } => ApiError::new(StatusCode::CONFLICT, "SEQUENCE_IS_PAST", e.to_string())
            .with_details(json!({ "last_number": last_number, "next_code": next_code })),
        E::Db(e) => ApiError::internal(e),
    }
}

// ---------------------------------------------------------------------------
// Items view
// ---------------------------------------------------------------------------

/// The `(limit, offset)` of one page of the items of a board
/// (COLLIERY-T-0261). The rule is that of the other lists
/// ([`clamp_pagination`]): the server changes a value that is out of the
/// limits, and it does not refuse it. The limits are different, because a
/// board view needs each card: the default is 200 and the maximum is
/// 1000.
fn clamp_board_items_page(query: &dto::BoardItemsQuery) -> (i64, i64) {
    let limit = query
        .limit
        .unwrap_or(dto::BOARD_ITEMS_DEFAULT_LIMIT)
        .clamp(1, dto::BOARD_ITEMS_MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);
    (limit, offset)
}

/// The rows of one item type with the given ids, by id.
macro_rules! rows_by_id {
    ($conn:expr, $table:ident, $model:ty, $ids:expr) => {{
        let ids: Vec<Uuid> = $ids;
        if ids.is_empty() {
            HashMap::new()
        } else {
            $table::table
                .filter($table::id.eq_any(ids))
                .select(<$model>::as_select())
                .load::<$model>($conn)
                .map_err(ApiError::internal)?
                .into_iter()
                .map(|row| (row.id, row))
                .collect::<HashMap<Uuid, $model>>()
        }
    }};
}

/// One page of the items on the board, grouped by column (columns in
/// position order; every entity type — strategies, initiatives, tasks,
/// ADRs). Open tenant-wide. `?repository=` narrows the tasks
/// (KAIROS-T-0104).
///
/// The route has pages (COLLIERY-T-0261). `limit` is 200 by default and
/// 1000 at most. `total` is the number of items after the filters, on all
/// pages. Each column is in each page. `children_progress` and
/// `blocks_summary` have the items of the page.
///
/// The order of the items is: the position of the column, then the type,
/// then the short code. The order of the types is: strategy, initiative,
/// task, ADR.
///
/// `?include_deleted=true` adds the archived cards back, in the column
/// each was put away in and marked with `archived_at` (KAIROS-T-0159).
/// That is an audit view, not a board view — "what was in Done last
/// quarter?" rather than "what is on the board now?". The children-
/// progress rollup and the blocks summary are deliberately NOT widened by
/// it: ADR-20 rule 5 says archived work is not live work, so the counts
/// keep counting live rows however the listing is asked for.
///
/// `blocks_summary` counts only the `blocks` edges that can still block
/// (COLLIERY-T-0214). Done work does not block, and nothing blocks done
/// work. An edge with either end in a terminal column (`is_done`) adds to
/// neither card. The edge stays on the relationship list of each item,
/// with a mark on the done end.
///
/// The path takes the slug or the id of the board (COLLIERY-T-0265). An
/// unknown slug and an unknown id are a 404.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/items",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board"), dto::BoardItemsQuery),
    responses(
        (status = 200, description = "One page of the items, grouped by column", body = dto::BoardItemsResponse),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn board_items(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<dto::BoardItemsQuery>,
) -> Result<Json<dto::BoardItemsResponse>, ApiError> {
    let (limit, offset) = clamp_board_items_page(&query);
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::board_items::{BoardItemFilter, CardKind, board_item_page};
            use kairos_db::schema::{adrs, initiatives, strategies, tasks};

            let board = load_board_by_ref(conn, &id)?;
            let board_id = board.id;
            // An archived card keeps a NOT NULL FK to the column it was
            // put away in, and that column may itself have been removed
            // since (KAIROS-T-0161). Widening the cards without widening
            // the columns would drop exactly the oldest audit rows on the
            // floor — and silently, since a card whose column is missing
            // from `index_of` is simply never bucketed.
            let columns = load_columns_including_removed(conn, board_id, query.include_deleted)?;
            let repository_filter: Option<Uuid> = query
                .repository
                .as_deref()
                .map(|reference| {
                    kairos_db::repositories::resolve(conn, reference)
                        .map(|r| r.id)
                        .map_err(crate::api::tasks::map_repository_error)
                })
                .transpose()?;
            // KAIROS-T-0321: the team filter of the strategies and the
            // initiatives.
            let team_filter = crate::api::meta::item_teams::team_filter(
                conn,
                query.team.as_deref(),
                query.no_team,
            )?;

            // COLLIERY-T-0261: the ids of the page, in the order of the
            // page, and the count after the filters. 2 queries.
            let column_ids: Vec<Uuid> = columns.iter().map(|column| column.id).collect();
            let page = board_item_page(
                conn,
                &BoardItemFilter {
                    board_id,
                    column_ids: &column_ids,
                    include_archived: query.include_deleted,
                    repository_id: repository_filter,
                    team: team_filter,
                },
                limit,
                offset,
            )
            .map_err(ApiError::internal)?;

            let mut groups: Vec<dto::BoardColumnItems> = columns
                .into_iter()
                .map(|column| dto::BoardColumnItems {
                    column: column.into_dto(),
                    strategies: vec![],
                    initiatives: vec![],
                    tasks: vec![],
                    adrs: vec![],
                })
                .collect();
            let index_of: HashMap<String, usize> = groups
                .iter()
                .enumerate()
                .map(|(i, g)| (g.column.id.clone(), i))
                .collect();

            // The rows of the page: one query for each type that the page
            // has, and no query for a card.
            let ids_of = |kind: CardKind| -> Vec<Uuid> {
                page.cards
                    .iter()
                    .filter(|(card_kind, _)| *card_kind == kind)
                    .map(|(_, id)| *id)
                    .collect()
            };
            let mut strategy_rows =
                rows_by_id!(conn, strategies, Strategy, ids_of(CardKind::Strategy));
            let mut initiative_rows =
                rows_by_id!(conn, initiatives, Initiative, ids_of(CardKind::Initiative));
            let mut task_rows = rows_by_id!(conn, tasks, Task, ids_of(CardKind::Task));
            let mut adr_rows = rows_by_id!(conn, adrs, Adr, ids_of(CardKind::Adr));

            // Parent short codes for the children-progress map
            // (KAIROS-T-0080): collected while bucketing so the rollup
            // stays ONE grouped query for the whole board.
            let mut item_codes: Vec<(Uuid, String)> = Vec::new();
            // The cards go to their columns in the order of the page, so
            // each list of a column has the order of the page.
            for (kind, id) in &page.cards {
                match kind {
                    CardKind::Strategy => {
                        if let Some(row) = strategy_rows.remove(id)
                            && let Some(&i) = index_of.get(&row.column_id.to_string())
                        {
                            item_codes.push((row.id, row.short_code.clone()));
                            groups[i].strategies.push(row.into_dto());
                        }
                    }
                    CardKind::Initiative => {
                        if let Some(row) = initiative_rows.remove(id)
                            && let Some(&i) = index_of.get(&row.column_id.to_string())
                        {
                            item_codes.push((row.id, row.short_code.clone()));
                            groups[i].initiatives.push(row.into_dto());
                        }
                    }
                    CardKind::Task => {
                        if let Some(row) = task_rows.remove(id)
                            && let Some(&i) = index_of.get(&row.column_id.to_string())
                        {
                            item_codes.push((row.id, row.short_code.clone()));
                            groups[i].tasks.push(row.into_dto());
                        }
                    }
                    CardKind::Adr => {
                        if let Some(row) = adr_rows.remove(id)
                            && let Some(column_id) = row.column_id
                            && let Some(&i) = index_of.get(&column_id.to_string())
                        {
                            item_codes.push((row.id, row.short_code.clone()));
                            groups[i].adrs.push(row.into_dto());
                        }
                    }
                }
            }
            // KAIROS-T-0104: embed the repository ref on every task — ONE
            // query for the whole page (collect, attach, redistribute).
            {
                let mut all: Vec<kairos_client::types::Task> = groups
                    .iter_mut()
                    .flat_map(|g| std::mem::take(&mut g.tasks))
                    .collect();
                attach_repositories(conn, &mut all).map_err(ApiError::internal)?;
                // KAIROS-T-0359: the claim of each task, in the same way.
                crate::claims::attach_claims(conn, &mut all).map_err(ApiError::internal)?;
                let mut by_column: HashMap<String, Vec<kairos_client::types::Task>> =
                    HashMap::new();
                for task in all {
                    by_column
                        .entry(task.column_id.clone())
                        .or_default()
                        .push(task);
                }
                for group in groups.iter_mut() {
                    group.tasks = by_column.remove(&group.column.id).unwrap_or_default();
                }
            }
            // COLLIERY-T-0269: the `impacts` links of each ADR, ONE query
            // for the whole page.
            {
                let mut all: Vec<kairos_client::types::Adr> = groups
                    .iter_mut()
                    .flat_map(|g| std::mem::take(&mut g.adrs))
                    .collect();
                super::super::convert::attach_impacts(conn, &mut all)?;
                for adr in all {
                    let column = adr.column_id.clone().unwrap_or_default();
                    if let Some(&i) = index_of.get(&column) {
                        groups[i].adrs.push(adr);
                    }
                }
            }

            let progress = kairos_db::graph::board_children_progress(conn, board_id)
                .map_err(ApiError::internal)?;
            // KAIROS-T-0091: blocked-by/blocks counts, one grouped query;
            // only items with at least one open blocks edge get an entry.
            // Open means both ends can still move (COLLIERY-T-0214): a
            // card in a terminal column has no entry, and adds nothing to
            // the entry of the card at the other end.
            let item_ids: Vec<Uuid> = item_codes.iter().map(|(id, _)| *id).collect();
            let blocks =
                kairos_db::graph::blocks_summary(conn, &item_ids).map_err(ApiError::internal)?;
            let blocks_summary: std::collections::BTreeMap<String, dto::BlocksCounts> = item_codes
                .iter()
                .filter_map(|(id, code)| {
                    blocks.get(id).map(|counts| {
                        (
                            code.clone(),
                            dto::BlocksCounts {
                                blocked_by: counts.blocked_by,
                                blocks: counts.blocks,
                            },
                        )
                    })
                })
                .collect();
            // KAIROS-T-0321: the teams of each strategy and initiative of
            // the page, one query for the board.
            let item_teams =
                crate::api::meta::item_teams::board_item_teams(conn, board_id, &item_codes)?;
            let children_progress: std::collections::BTreeMap<String, dto::ProgressCounts> =
                item_codes
                    .into_iter()
                    .filter_map(|(id, code)| {
                        progress.get(&id).map(|counts| {
                            (
                                code,
                                dto::ProgressCounts {
                                    done: counts.done,
                                    total: counts.total,
                                    has_done: counts.has_done,
                                },
                            )
                        })
                    })
                    .collect();

            Ok(dto::BoardItemsResponse {
                board: board.into_dto(),
                columns: groups,
                total: page.total,
                limit,
                offset,
                children_progress,
                blocks_summary,
                item_teams,
            })
        })
        .await?;
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Columns
// ---------------------------------------------------------------------------

/// List a board's columns in position order (open tenant-wide).
///
/// The path takes the slug or the id of the board (COLLIERY-T-0265). An
/// unknown slug and an unknown id are a 404.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/columns",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    responses(
        (status = 200, description = "Columns in position order", body = [dto::BoardColumn]),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_columns(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::BoardColumn>>, ApiError> {
    let columns = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board_id = load_board_by_ref(conn, &id)?.id;
            Ok(load_columns(conn, board_id)?
                .into_iter()
                .map(IntoDto::into_dto)
                .collect())
        })
        .await?;
    Ok(Json(columns))
}

/// Add a column (T-0010 rules: unique name and position). Requires
/// `configure_boards` on the board.
#[utoipa::path(
    post,
    path = "/api/boards/{id}/columns",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
    request_body = dto::CreateColumnRequest,
    responses(
        (status = 201, description = "Created", body = dto::BoardColumn),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "DUPLICATE_COLUMN_NAME / DUPLICATE_COLUMN_POSITION / VALIDATION", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn add_column(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<dto::CreateColumnRequest>,
) -> Result<(StatusCode, Json<dto::BoardColumn>), ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;
            let created = boards::add_column(conn, board_id, &body.name, body.position, user)
                .map_err(map_config_error)?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Rename and/or move a column (T-0010 rules; moving reorders the board's
/// columns around the new position). Requires `configure_boards`.
///
/// This route also sets the flags `is_done` and `claims` (KAIROS-T-0359).
/// A person who moves a task into a column with `claims` gets the claim of
/// the task. When the flag goes off, the claims of the tasks in the column
/// end.
#[utoipa::path(
    patch,
    path = "/api/boards/{id}/columns/{col_id}",
    tag = "boards",
    params(
        ("id" = String, Path, description = "Board id (UUID)"),
        ("col_id" = String, Path, description = "Column id (UUID)"),
    ),
    request_body = dto::UpdateColumnRequest,
    responses(
        (status = 200, description = "Updated", body = dto::BoardColumn),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board/column", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "DUPLICATE_COLUMN_NAME / VALIDATION", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_column(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, col_id)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::UpdateColumnRequest>,
) -> Result<Json<dto::BoardColumn>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let column_id = parse_uuid(&col_id, "col_id")?;
    if body.name.is_none()
        && body.position.is_none()
        && body.is_done.is_none()
        && body.claims.is_none()
    {
        return Err(ApiError::validation(
            "The request has no field to change. Send one or more of name, position, is_done \
             and claims.",
        ));
    }
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board(conn, board_id)?;
            load_column_of_board(conn, board_id, column_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;

            if let Some(name) = &body.name {
                boards::rename_column(conn, column_id, name, user).map_err(map_config_error)?;
            }
            if let Some(is_done) = body.is_done {
                boards::set_column_done(conn, column_id, is_done, user)
                    .map_err(map_config_error)?;
            }
            // KAIROS-T-0359: the same path and capability as `is_done`.
            if let Some(claims) = body.claims {
                boards::set_column_claims(conn, column_id, claims, user)
                    .map_err(map_config_error)?;
            }
            if let Some(position) = body.position {
                if position < 0 {
                    return Err(ApiError::validation(format!(
                        "The position {position} is not correct. A position is 0 or more."
                    )));
                }
                let mut order: Vec<Uuid> = load_columns(conn, board_id)?
                    .into_iter()
                    .map(|c| c.id)
                    .collect();
                order.retain(|&c| c != column_id);
                let index = (position as usize).min(order.len());
                order.insert(index, column_id);
                boards::reorder_columns(conn, board_id, &order, user).map_err(map_config_error)?;
            }
            Ok(load_column_of_board(conn, board_id, column_id)?.into_dto())
        })
        .await?;
    Ok(Json(updated))
}

/// Remove a column — a soft delete (KAIROS-T-0161). Only allowed when no
/// LIVE item occupies it; the T-0010 rule surfaces as 422
/// `COLUMN_NOT_EMPTY`. Archived cards may stay behind and keep reporting
/// the column they were put away in. Requires `configure_boards`.
#[utoipa::path(
    delete,
    path = "/api/boards/{id}/columns/{col_id}",
    tag = "boards",
    params(
        ("id" = String, Path, description = "Board id (UUID)"),
        ("col_id" = String, Path, description = "Column id (UUID)"),
    ),
    responses(
        (status = 200, description = "Removed (soft: archived cards keep its name; its transition edges are kept but no longer apply)", body = dto::OrgDeleteResponse),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board/column", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "COLUMN_NOT_EMPTY (details.item_count)", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_column(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, col_id)): Path<(String, String)>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let column_id = parse_uuid(&col_id, "col_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board(conn, board_id)?;
            load_column_of_board(conn, board_id, column_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;
            boards::remove_column(conn, column_id, user).map_err(map_config_error)?;
            Ok(dto::OrgDeleteResponse {
                id: column_id.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}

// ---------------------------------------------------------------------------
// Transitions
// ---------------------------------------------------------------------------

/// List a board's allowed transitions (open tenant-wide).
///
/// The path takes the slug or the id of the board (COLLIERY-T-0265). An
/// unknown slug and an unknown id are a 404.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/transitions",
    tag = "boards",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    responses(
        (status = 200, description = "Transition edges", body = [dto::BoardTransition]),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_transitions(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::BoardTransition>>, ApiError> {
    let transitions = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board_id = load_board_by_ref(conn, &id)?.id;
            Ok(load_transitions(conn, board_id)?
                .into_iter()
                .map(IntoDto::into_dto)
                .collect())
        })
        .await?;
    Ok(Json(transitions))
}

/// Add a transition edge (T-0010 rules: endpoints on the board, distinct,
/// not duplicate). Requires `configure_boards`.
#[utoipa::path(
    post,
    path = "/api/boards/{id}/transitions",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
    request_body = dto::CreateTransitionRequest,
    responses(
        (status = 201, description = "Created", body = dto::BoardTransition),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "DUPLICATE_TRANSITION / VALIDATION", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn add_transition(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<dto::CreateTransitionRequest>,
) -> Result<(StatusCode, Json<dto::BoardTransition>), ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let from = parse_uuid(&body.from_column_id, "from_column_id")?;
    let to = parse_uuid(&body.to_column_id, "to_column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::board_transitions::dsl;
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;
            boards::add_transition(conn, board_id, from, to, user).map_err(map_config_error)?;
            let created: BoardTransition = dsl::board_transitions
                .filter(dsl::board_id.eq(board_id))
                .filter(dsl::from_column_id.eq(from))
                .filter(dsl::to_column_id.eq(to))
                .select(BoardTransition::as_select())
                .first(conn)
                .map_err(ApiError::internal)?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Remove a transition edge by id. Requires `configure_boards`.
#[utoipa::path(
    delete,
    path = "/api/boards/{id}/transitions/{transition_id}",
    tag = "boards",
    params(
        ("id" = String, Path, description = "Board id (UUID)"),
        ("transition_id" = String, Path, description = "Transition id (UUID)"),
    ),
    responses(
        (status = 200, description = "Removed", body = dto::OrgDeleteResponse),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board/transition", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_transition(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, transition_id)): Path<(String, String)>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let transition_id = parse_uuid(&transition_id, "transition_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::board_transitions::dsl;
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;
            let edge: Option<BoardTransition> = dsl::board_transitions
                .filter(dsl::id.eq(transition_id))
                .filter(dsl::board_id.eq(board_id))
                .select(BoardTransition::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?;
            let edge = edge.ok_or_else(|| {
                ApiError::not_found(format!(
                    "The board {board_id} has no transition {transition_id}."
                ))
            })?;
            boards::remove_transition(conn, board_id, edge.from_column_id, edge.to_column_id, user)
                .map_err(map_config_error)?;
            Ok(dto::OrgDeleteResponse {
                id: transition_id.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}

// ---------------------------------------------------------------------------
// Board authorization (members + capabilities, KAIROS-T-0011 semantics)
// ---------------------------------------------------------------------------

/// The `capabilities` a user holds on a board, sorted.
fn capabilities_of(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<String>, ApiError> {
    use kairos_db::schema::board_member_capabilities::dsl;
    dsl::board_member_capabilities
        .filter(dsl::board_id.eq(board_id))
        .filter(dsl::user_id.eq(user_id))
        .order(dsl::capability.asc())
        .select(dsl::capability)
        .load(conn)
        .map_err(ApiError::internal)
}

/// One user's `BoardMember` view (joins `public.users` for identity).
fn board_member_view(
    conn: &mut PgConnection,
    board_id: Uuid,
    user_id: Uuid,
) -> Result<dto::BoardMember, ApiError> {
    let user = require_user_exists(conn, user_id)?;
    Ok(dto::BoardMember {
        user_id: user.id.to_string(),
        email: user.email,
        display_name: user.display_name,
        capabilities: capabilities_of(conn, board_id, user_id)?,
    })
}

/// List a board's members and their capability grants (open tenant-wide —
/// A-0006 auditability).
///
/// The path takes the slug or the id of the board (COLLIERY-T-0265). An
/// unknown slug and an unknown id are a 404.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/members",
    tag = "board-members",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the board")),
    responses(
        (status = 200, description = "Members with their capabilities", body = [dto::BoardMember]),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_board_members(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::BoardMember>>, ApiError> {
    let members = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::board_member_capabilities::dsl;
            use kairos_db::schema::users;
            let board_id = load_board_by_ref(conn, &id)?.id;
            let grants: Vec<(Uuid, String)> = dsl::board_member_capabilities
                .filter(dsl::board_id.eq(board_id))
                .order((dsl::user_id.asc(), dsl::capability.asc()))
                .select((dsl::user_id, dsl::capability))
                .load(conn)
                .map_err(ApiError::internal)?;
            let user_ids: Vec<Uuid> = grants
                .iter()
                .map(|(u, _)| *u)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let identities: Vec<kairos_db::models::User> = users::table
                .filter(users::id.eq_any(&user_ids))
                .select(kairos_db::models::User::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let identity_of: HashMap<Uuid, &kairos_db::models::User> =
                identities.iter().map(|u| (u.id, u)).collect();

            let mut members: Vec<dto::BoardMember> = Vec::new();
            for user_id in user_ids {
                let capabilities: Vec<String> = grants
                    .iter()
                    .filter(|(u, _)| *u == user_id)
                    .map(|(_, c)| c.clone())
                    .collect();
                let (email, display_name) = identity_of
                    .get(&user_id)
                    .map(|u| (u.email.clone(), u.display_name.clone()))
                    .unwrap_or_default();
                members.push(dto::BoardMember {
                    user_id: user_id.to_string(),
                    email,
                    display_name,
                    capabilities,
                });
            }
            members.sort_by(|a, b| a.email.cmp(&b.email));
            Ok(members)
        })
        .await?;
    Ok(Json(members))
}

/// Add a member with capabilities (T-0011 grants; duplicate grant → 409).
/// Requires `administer_members` on the board (or org admin).
#[utoipa::path(
    post,
    path = "/api/boards/{id}/members",
    tag = "board-members",
    params(("id" = String, Path, description = "Board id (UUID)")),
    request_body = dto::AddBoardMemberRequest,
    responses(
        (status = 201, description = "Granted", body = dto::BoardMember),
        (status = 403, description = "Missing administer_members", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "A capability was already granted", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Unknown user or capability outside the A-0006 vocabulary", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn add_board_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<dto::AddBoardMemberRequest>,
) -> Result<(StatusCode, Json<dto::BoardMember>), ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let target = parse_uuid(&body.user_id, "user_id")?;
    validate_capabilities(&body.capabilities)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let member = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, ADMINISTER_MEMBERS)?;
            require_user_exists(conn, target)?;
            for capability in &body.capabilities {
                abac::grant_capability(conn, board_id, target, capability, user)
                    .map_err(map_grant_error)?;
            }
            board_member_view(conn, board_id, target)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(member)))
}

/// Replace a member's capability set (revokes what is absent, grants what
/// is new — each change is a T-0011 grant/revoke with its activity row).
/// Requires `administer_members` on the board (or org admin).
#[utoipa::path(
    patch,
    path = "/api/boards/{id}/members/{user_id}",
    tag = "board-members",
    params(
        ("id" = String, Path, description = "Board id (UUID)"),
        ("user_id" = String, Path, description = "User id (UUID)"),
    ),
    request_body = dto::ReplaceCapabilitiesRequest,
    responses(
        (status = 200, description = "The member's new capability set", body = dto::BoardMember),
        (status = 403, description = "Missing administer_members", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board, or user is not a member", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Capability outside the A-0006 vocabulary", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn replace_capabilities(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::ReplaceCapabilitiesRequest>,
) -> Result<Json<dto::BoardMember>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let target = parse_uuid(&target, "user_id")?;
    validate_capabilities(&body.capabilities)?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let member = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, ADMINISTER_MEMBERS)?;
            let current = capabilities_of(conn, board_id, target)?;
            if current.is_empty() {
                return Err(ApiError::not_found(format!(
                    "The user {target} is not a member of the board {board_id}."
                )));
            }
            let desired: BTreeSet<&str> = body.capabilities.iter().map(String::as_str).collect();
            let existing: BTreeSet<&str> = current.iter().map(String::as_str).collect();
            for capability in existing.difference(&desired) {
                abac::revoke_capability(conn, board_id, target, capability, user)
                    .map_err(map_grant_error)?;
            }
            for capability in desired.difference(&existing) {
                abac::grant_capability(conn, board_id, target, capability, user)
                    .map_err(map_grant_error)?;
            }
            board_member_view(conn, board_id, target)
        })
        .await?;
    Ok(Json(member))
}

/// Remove a member: revoke ALL their capabilities on the board (T-0011
/// revokes, one activity row each). Requires `administer_members` (or org
/// admin).
#[utoipa::path(
    delete,
    path = "/api/boards/{id}/members/{user_id}",
    tag = "board-members",
    params(
        ("id" = String, Path, description = "Board id (UUID)"),
        ("user_id" = String, Path, description = "User id (UUID)"),
    ),
    responses(
        (status = 200, description = "All grants revoked", body = dto::RemoveBoardMemberResponse),
        (status = 403, description = "Missing administer_members", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board, or user is not a member", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_board_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target)): Path<(String, String)>,
) -> Result<Json<dto::RemoveBoardMemberResponse>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let target = parse_uuid(&target, "user_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, ADMINISTER_MEMBERS)?;
            let current = capabilities_of(conn, board_id, target)?;
            if current.is_empty() {
                return Err(ApiError::not_found(format!(
                    "The user {target} is not a member of the board {board_id}."
                )));
            }
            for capability in &current {
                abac::revoke_capability(conn, board_id, target, capability, user)
                    .map_err(map_grant_error)?;
            }
            Ok(dto::RemoveBoardMemberResponse {
                user_id: target.to_string(),
                revoked_capabilities: current,
            })
        })
        .await?;
    Ok(Json(outcome))
}
