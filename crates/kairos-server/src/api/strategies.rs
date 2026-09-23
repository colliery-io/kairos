//! `/api/strategies` (KAIROS-S-0005) — the reference implementation of the
//! T-0018 handler pattern; see [`super`] for the shared conventions.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::items::Strategy;
use kairos_db::{boards, items};
use serde_json::json;

use super::convert::IntoDto;
use super::{
    Liveness, clamp_list, map_board_error, map_item_error, parse_opt_uuid, parse_uuid,
    require_capability, short_code_not_found,
};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 manage capability for this family.
const MANAGE: &str = "manage_strategies";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/strategies",
            get(list_strategies).post(create_strategy),
        )
        .route(
            "/api/strategies/{short_code}",
            get(get_strategy)
                .patch(update_strategy)
                .delete(delete_strategy),
        )
        .route(
            "/api/strategies/{short_code}/transition",
            post(transition_strategy),
        )
}

/// Load the live strategy with this short code, or 404.
fn load(
    conn: &mut PgConnection,
    short_code: &str,
    liveness: Liveness,
) -> Result<Strategy, ApiError> {
    use kairos_db::schema::strategies::dsl;
    let mut query = dsl::strategies
        .filter(dsl::short_code.eq(short_code))
        .into_boxed();
    if liveness == Liveness::LiveOnly {
        query = query.filter(dsl::deleted_at.is_null());
    }
    query
        .select(Strategy::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| short_code_not_found("strategy", short_code))
}

/// List strategies (open tenant-wide, S-0005 list envelope).
///
/// `?include_deleted=true` widens the listing to archived work, each row
/// marked with `archived_at` (KAIROS-A-0020 rule 2). Default false: rule 3
/// is that a listing nobody asked hides put-away work.
#[utoipa::path(
    get,
    path = "/api/strategies",
    tag = "strategies",
    params(dto::ListQuery),
    responses(
        (status = 200, description = "Page of strategies", body = dto::ListEnvelope<dto::Strategy>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_strategies(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<dto::ListQuery>,
) -> Result<Json<dto::ListEnvelope<dto::Strategy>>, ApiError> {
    let (limit, offset, liveness) = clamp_list(&query);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::strategies::dsl;
            // ONE predicate, applied to both the count and the page
            // (KAIROS-T-0159): the two can never disagree.
            let visible = || {
                let mut query = dsl::strategies.into_boxed();
                if liveness == Liveness::LiveOnly {
                    query = query.filter(dsl::deleted_at.is_null());
                }
                query
            };
            let total: i64 = visible()
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Strategy> = visible()
                .order(dsl::short_code.asc())
                .limit(limit)
                .offset(offset)
                .select(Strategy::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            Ok(dto::ListEnvelope {
                items: rows.into_iter().map(IntoDto::into_dto).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Get one strategy by short code (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/strategies/{short_code}",
    tag = "strategies",
    params(("short_code" = String, Path, description = "Strategy short code")),
    responses(
        (status = 200, description = "The strategy", body = dto::Strategy),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_strategy(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::Strategy>, ApiError> {
    let strategy = state
        .blocking
        .run(&tenant.slug, move |conn| {
            Ok(load(conn, &short_code, Liveness::IncludeArchived)?.into_dto())
        })
        .await?;
    Ok(Json(strategy))
}

/// Create a strategy (requires `manage_strategies` on the target board).
#[utoipa::path(
    post,
    path = "/api/strategies",
    tag = "strategies",
    request_body = dto::CreateStrategyRequest,
    responses(
        (status = 201, description = "Created", body = dto::Strategy),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown board/column", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_strategy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateStrategyRequest>,
) -> Result<(StatusCode, Json<dto::Strategy>), ApiError> {
    let board_id = parse_uuid(&body.board_id, "board_id")?;
    let column_id = parse_opt_uuid(body.column_id.as_deref(), "column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, Some(board_id), user, MANAGE)?;
            let created = items::create_strategy(
                conn,
                items::CreateStrategy {
                    board_id,
                    column_id,
                    title: &body.title,
                    content: &body.content,
                    hypothesis: body.hypothesis.as_deref(),
                },
                user,
            )
            .map_err(map_item_error)?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Update strategy content (KAIROS-A-0004 optimistic concurrency; requires
/// `manage_strategies` on the strategy's board).
#[utoipa::path(
    patch,
    path = "/api/strategies/{short_code}",
    tag = "strategies",
    params(("short_code" = String, Path, description = "Strategy short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Strategy),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_strategy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::UpdateContentRequest>,
) -> Result<Json<dto::Strategy>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let strategy = load(conn, &short_code, Liveness::LiveOnly)?;
            require_capability(conn, &slug, Some(strategy.board_id), user, MANAGE)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Strategy, strategy.id, update, user) {
                Ok(_) => Ok(load(conn, &short_code, Liveness::LiveOnly)?.into_dto()),
                Err(items::ItemError::VersionConflict {
                    expected_version,
                    current_version,
                    ..
                }) => {
                    let current = load(conn, &short_code, Liveness::LiveOnly)?.into_dto();
                    Err(ApiError::conflict(format!(
                        "version mismatch: expected {expected_version}, current is {current_version}"
                    ))
                    .with_details(json!({ "current": current })))
                }
                Err(e) => Err(map_item_error(e)),
            }
        })
        .await?;
    Ok(Json(updated))
}

/// Soft-delete a strategy, cascading to its `parent` descendants
/// (KAIROS-A-0001; requires `manage_strategies` on the strategy's board).
#[utoipa::path(
    delete,
    path = "/api/strategies/{short_code}",
    tag = "strategies",
    params(("short_code" = String, Path, description = "Strategy short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_strategy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::DeleteResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let strategy = load(conn, &short_code, Liveness::LiveOnly)?;
            require_capability(conn, &slug, Some(strategy.board_id), user, MANAGE)?;
            let outcome = items::soft_delete_item(conn, ItemType::Strategy, strategy.id, user)
                .map_err(map_item_error)?;
            Ok(dto::DeleteResponse {
                short_code: outcome.root_short_code,
                cascade_count: outcome.cascaded_short_codes.len() as i64,
                cascaded_short_codes: outcome.cascaded_short_codes,
            })
        })
        .await?;
    Ok(Json(outcome))
}

/// Move a strategy to another column (requires `transition_items` on the
/// strategy's board; the move must exist in the board's transition graph).
#[utoipa::path(
    post,
    path = "/api/strategies/{short_code}/transition",
    tag = "strategies",
    params(("short_code" = String, Path, description = "Strategy short code")),
    request_body = dto::TransitionRequest,
    responses(
        (status = 200, description = "Transitioned (new column)", body = dto::Strategy),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Invalid transition; details.allowed_targets lists valid moves", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn transition_strategy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::TransitionRequest>,
) -> Result<Json<dto::Strategy>, ApiError> {
    let to_column_id = parse_uuid(&body.to_column_id, "to_column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let transitioned = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let strategy = load(conn, &short_code, Liveness::LiveOnly)?;
            require_capability(
                conn,
                &slug,
                Some(strategy.board_id),
                user,
                "transition_items",
            )?;
            boards::transition_strategy(conn, strategy.id, to_column_id, user)
                .map_err(map_board_error)?;
            Ok(load(conn, &short_code, Liveness::LiveOnly)?.into_dto())
        })
        .await?;
    Ok(Json(transitioned))
}
