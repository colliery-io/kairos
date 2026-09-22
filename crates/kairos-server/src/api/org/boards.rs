//! `/api/boards` (KAIROS-S-0005 Boards + Board Authorization families,
//! KAIROS-T-0019): board CRUD, the grouped items view, column/transition
//! configuration over the T-0010 services (typed rule violations → 422),
//! and capability administration over the T-0011 grant/revoke services.
//!
//! Gating: board creation/deletion is org-admin-only (no/whole-board
//! context → the A-0006 tenant-config fallback); PATCH board + column +
//! transition writes require `configure_boards` on the board; member/
//! capability writes require `manage_members`. Reads are open tenant-wide.

use std::collections::{BTreeSet, HashMap};

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_org as dto;
use kairos_db::models::boards::{Board, BoardColumn, BoardTransition};
use kairos_db::models::enums::{ActivityAction, BoardLevel};
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::models::items::{Adr, Initiative, Strategy, Task};
use kairos_db::{abac, boards};
use serde_json::json;
use uuid::Uuid;

use super::super::convert::IntoDto;
use super::super::{clamp_pagination, parse_enum, parse_uuid, require_capability};
use super::{
    count_board_items, is_unique_violation, load_board, map_config_error, map_grant_error,
    require_user_exists, validate_capabilities,
};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 capability for board configuration writes.
const CONFIGURE: &str = "configure_boards";
/// The A-0006 capability for board membership/capability administration.
const MANAGE_MEMBERS: &str = "manage_members";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/boards", get(list_boards).post(create_board))
        .route(
            "/api/boards/{id}",
            get(get_board).patch(update_board).delete(delete_board),
        )
        .route("/api/boards/{id}/items", get(board_items))
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

/// Columns of a board in position order.
fn load_columns(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<BoardColumn>, ApiError> {
    use kairos_db::schema::board_columns::dsl;
    dsl::board_columns
        .filter(dsl::board_id.eq(board_id))
        .order(dsl::position.asc())
        .select(BoardColumn::as_select())
        .load(conn)
        .map_err(ApiError::internal)
}

/// Transition edges of a board.
fn load_transitions(
    conn: &mut PgConnection,
    board_id: Uuid,
) -> Result<Vec<BoardTransition>, ApiError> {
    use kairos_db::schema::board_transitions::dsl;
    dsl::board_transitions
        .filter(dsl::board_id.eq(board_id))
        .select(BoardTransition::as_select())
        .load(conn)
        .map_err(ApiError::internal)
}

/// The board + full configuration as the `BoardDetail` DTO.
fn board_detail(conn: &mut PgConnection, board: Board) -> Result<dto::BoardDetail, ApiError> {
    let columns = load_columns(conn, board.id)?;
    let transitions = load_transitions(conn, board.id)?;
    Ok(dto::BoardDetail {
        board: board.into_dto(),
        columns: columns.into_iter().map(IntoDto::into_dto).collect(),
        transitions: transitions.into_iter().map(IntoDto::into_dto).collect(),
    })
}

/// A column of `board_id` by id, or 404 (also 404 when the column belongs
/// to a different board — path scoping).
fn load_column_of_board(
    conn: &mut PgConnection,
    board_id: Uuid,
    column_id: Uuid,
) -> Result<BoardColumn, ApiError> {
    use kairos_db::schema::board_columns::dsl;
    dsl::board_columns
        .filter(dsl::id.eq(column_id))
        .filter(dsl::board_id.eq(board_id))
        .select(BoardColumn::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("no column {column_id} on board {board_id}")))
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
    Query(pagination): Query<Pagination>,
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

/// Board detail: the board plus its columns and transitions (open
/// tenant-wide).
#[utoipa::path(
    get,
    path = "/api/boards/{id}",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
    responses(
        (status = 200, description = "The board with columns and transitions", body = dto::BoardDetail),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_board(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::BoardDetail>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let detail = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board = load_board(conn, board_id)?;
            board_detail(conn, board)
        })
        .await?;
    Ok(Json(detail))
}

/// Create a board seeded with the system default columns/transitions for
/// its level (KAIROS-A-0002). Org-admin-only: a new board has no capability
/// context yet (A-0006 tenant-config fallback).
#[utoipa::path(
    post,
    path = "/api/boards",
    tag = "boards",
    request_body = dto::CreateBoardRequest,
    responses(
        (status = 201, description = "Created, with the seeded configuration", body = dto::BoardDetail),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug already in use", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad level/team reference", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_board(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateBoardRequest>,
) -> Result<(StatusCode, Json<dto::BoardDetail>), ApiError> {
    let level = parse_enum::<BoardLevel>(&body.board_level, "board_level", BoardLevel::ALL)?;
    let team_id = body
        .team_id
        .as_deref()
        .map(|v| parse_uuid(v, "team_id"))
        .transpose()?;
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
                        "team {team_id} does not exist"
                    )));
                }
            }
            let board =
                boards::create_board(conn, level, &body.name, &body.slug, team_id, Some(user))
                    .map_err(|e| match e {
                        boards::BoardError::Database(ref db) if is_unique_violation(db) => {
                            ApiError::conflict(format!(
                                "a board with slug {:?} already exists",
                                body.slug
                            ))
                        }
                        e => map_config_error(e),
                    })?;
            board_detail(conn, board)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

/// Update board settings (name/slug). Requires `configure_boards` on the
/// board.
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
        (status = 409, description = "Slug already in use", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_board(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::UpdateBoardRequest>,
) -> Result<Json<dto::Board>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    if body.name.is_none() && body.slug.is_none() {
        return Err(ApiError::validation(
            "at least one of name, slug is required",
        ));
    }
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::boards::dsl;
            let board = load_board(conn, board_id)?;
            require_capability(conn, &slug, Some(board.id), user, CONFIGURE)?;
            let updated: Board = diesel::update(dsl::boards.filter(dsl::id.eq(board_id)))
                .set((
                    kairos_db::models::boards::BoardChangeset {
                        name: body.name.clone(),
                        slug: body.slug.clone(),
                        ..Default::default()
                    },
                    dsl::updated_at.eq(diesel::dsl::now),
                ))
                .returning(Board::as_returning())
                .get_result(conn)
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        ApiError::conflict("a board with that slug already exists")
                    } else {
                        ApiError::internal(e)
                    }
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
#[utoipa::path(
    delete,
    path = "/api/boards/{id}",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
    responses(
        (status = 200, description = "Soft-deleted", body = dto::OrgDeleteResponse),
        (status = 403, description = "Missing capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "BOARD_NOT_EMPTY", body = kairos_client::types::ErrorEnvelope),
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
            let item_count = count_board_items(conn, board_id)?;
            if item_count > 0 {
                return Err(ApiError::unprocessable(
                    "BOARD_NOT_EMPTY",
                    format!(
                        "board {:?} still contains {item_count} item(s); \
                         move or delete them before removing the board",
                        board.name
                    ),
                )
                .with_details(json!({ "item_count": item_count })));
            }
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
        .await?;
    Ok(Json(outcome))
}

// ---------------------------------------------------------------------------
// Items view
// ---------------------------------------------------------------------------

/// All live items on the board, grouped by column (columns in position
/// order; every entity type — strategies, initiatives, tasks, ADRs). Open
/// tenant-wide.
#[utoipa::path(
    get,
    path = "/api/boards/{id}/items",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
    responses(
        (status = 200, description = "Items grouped by column", body = dto::BoardItemsResponse),
        (status = 404, description = "Unknown board", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn board_items(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::BoardItemsResponse>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::{adrs, initiatives, strategies, tasks};

            let board = load_board(conn, board_id)?;
            let columns = load_columns(conn, board_id)?;

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
            // Parent short codes for the children-progress map
            // (KAIROS-T-0080): collected while bucketing so the rollup
            // stays ONE grouped query for the whole board.
            let mut item_codes: Vec<(Uuid, String)> = Vec::new();

            let strategy_rows: Vec<Strategy> = strategies::table
                .filter(strategies::board_id.eq(board_id))
                .filter(strategies::deleted_at.is_null())
                .order(strategies::short_code.asc())
                .select(Strategy::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            for row in strategy_rows {
                if let Some(&i) = index_of.get(&row.column_id.to_string()) {
                    item_codes.push((row.id, row.short_code.clone()));
                    groups[i].strategies.push(row.into_dto());
                }
            }
            let initiative_rows: Vec<Initiative> = initiatives::table
                .filter(initiatives::board_id.eq(board_id))
                .filter(initiatives::deleted_at.is_null())
                .order(initiatives::short_code.asc())
                .select(Initiative::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            for row in initiative_rows {
                if let Some(&i) = index_of.get(&row.column_id.to_string()) {
                    item_codes.push((row.id, row.short_code.clone()));
                    groups[i].initiatives.push(row.into_dto());
                }
            }
            let task_rows: Vec<Task> = tasks::table
                .filter(tasks::board_id.eq(board_id))
                .filter(tasks::deleted_at.is_null())
                .order(tasks::short_code.asc())
                .select(Task::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            for row in task_rows {
                if let Some(&i) = index_of.get(&row.column_id.to_string()) {
                    item_codes.push((row.id, row.short_code.clone()));
                    groups[i].tasks.push(row.into_dto());
                }
            }
            let adr_rows: Vec<Adr> = adrs::table
                .filter(adrs::board_id.eq(board_id))
                .filter(adrs::deleted_at.is_null())
                .order(adrs::short_code.asc())
                .select(Adr::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            for row in adr_rows {
                if let Some(column_id) = row.column_id
                    && let Some(&i) = index_of.get(&column_id.to_string())
                {
                    item_codes.push((row.id, row.short_code.clone()));
                    groups[i].adrs.push(row.into_dto());
                }
            }

            let progress = kairos_db::graph::board_children_progress(conn, board_id)
                .map_err(ApiError::internal)?;
            // KAIROS-T-0091: blocked-by/blocks counts, one grouped query;
            // only items with at least one live blocks edge get an entry.
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
                children_progress,
                blocks_summary,
            })
        })
        .await?;
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Columns
// ---------------------------------------------------------------------------

/// List a board's columns in position order (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/boards/{id}/columns",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
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
    let board_id = parse_uuid(&id, "id")?;
    let columns = state
        .blocking
        .run(&tenant.slug, move |conn| {
            load_board(conn, board_id)?;
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
    Json(body): Json<dto::CreateColumnRequest>,
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
    Json(body): Json<dto::UpdateColumnRequest>,
) -> Result<Json<dto::BoardColumn>, ApiError> {
    let board_id = parse_uuid(&id, "id")?;
    let column_id = parse_uuid(&col_id, "col_id")?;
    if body.name.is_none() && body.position.is_none() && body.is_done.is_none() {
        return Err(ApiError::validation(
            "at least one of name, position, is_done is required",
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
            if let Some(position) = body.position {
                if position < 0 {
                    return Err(ApiError::validation(format!(
                        "position must be >= 0, got {position}"
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

/// Remove a column. Only allowed when no item occupies it — the T-0010
/// rule surfaces as 422 `COLUMN_NOT_EMPTY`. Requires `configure_boards`.
#[utoipa::path(
    delete,
    path = "/api/boards/{id}/columns/{col_id}",
    tag = "boards",
    params(
        ("id" = String, Path, description = "Board id (UUID)"),
        ("col_id" = String, Path, description = "Column id (UUID)"),
    ),
    responses(
        (status = 200, description = "Removed (its transition edges cascade)", body = dto::OrgDeleteResponse),
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
#[utoipa::path(
    get,
    path = "/api/boards/{id}/transitions",
    tag = "boards",
    params(("id" = String, Path, description = "Board id (UUID)")),
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
    let board_id = parse_uuid(&id, "id")?;
    let transitions = state
        .blocking
        .run(&tenant.slug, move |conn| {
            load_board(conn, board_id)?;
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
    Json(body): Json<dto::CreateTransitionRequest>,
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
                ApiError::not_found(format!("no transition {transition_id} on board {board_id}"))
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
#[utoipa::path(
    get,
    path = "/api/boards/{id}/members",
    tag = "board-members",
    params(("id" = String, Path, description = "Board id (UUID)")),
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
    let board_id = parse_uuid(&id, "id")?;
    let members = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::board_member_capabilities::dsl;
            use kairos_db::schema::users;
            load_board(conn, board_id)?;
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
/// Requires `manage_members` on the board (or org admin).
#[utoipa::path(
    post,
    path = "/api/boards/{id}/members",
    tag = "board-members",
    params(("id" = String, Path, description = "Board id (UUID)")),
    request_body = dto::AddBoardMemberRequest,
    responses(
        (status = 201, description = "Granted", body = dto::BoardMember),
        (status = 403, description = "Missing manage_members", body = kairos_client::types::ErrorEnvelope),
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
    Json(body): Json<dto::AddBoardMemberRequest>,
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
            require_capability(conn, &slug, Some(board.id), user, MANAGE_MEMBERS)?;
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
/// Requires `manage_members` on the board (or org admin).
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
        (status = 403, description = "Missing manage_members", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown board, or user is not a member", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Capability outside the A-0006 vocabulary", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn replace_capabilities(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, target)): Path<(String, String)>,
    Json(body): Json<dto::ReplaceCapabilitiesRequest>,
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
            require_capability(conn, &slug, Some(board.id), user, MANAGE_MEMBERS)?;
            let current = capabilities_of(conn, board_id, target)?;
            if current.is_empty() {
                return Err(ApiError::not_found(format!(
                    "user {target} is not a member of board {board_id}"
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
/// revokes, one activity row each). Requires `manage_members` (or org
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
        (status = 403, description = "Missing manage_members", body = kairos_client::types::ErrorEnvelope),
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
            require_capability(conn, &slug, Some(board.id), user, MANAGE_MEMBERS)?;
            let current = capabilities_of(conn, board_id, target)?;
            if current.is_empty() {
                return Err(ApiError::not_found(format!(
                    "user {target} is not a member of board {board_id}"
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
