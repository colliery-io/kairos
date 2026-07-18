//! `/api/initiatives` (KAIROS-S-0005) — see [`super`] for the shared
//! T-0018 handler pattern.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::enums::{BucketType, Complexity};
use kairos_db::models::items::Initiative;
use kairos_db::{boards, items};
use serde_json::json;

use super::convert::IntoDto;
use super::{
    clamp_pagination, map_board_error, map_item_error, parse_enum, parse_opt_uuid, parse_uuid,
    require_capability, short_code_not_found,
};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 manage capability for this family.
const MANAGE: &str = "manage_initiatives";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/initiatives",
            get(list_initiatives).post(create_initiative),
        )
        .route(
            "/api/initiatives/{short_code}",
            get(get_initiative)
                .patch(update_initiative)
                .delete(delete_initiative),
        )
        .route(
            "/api/initiatives/{short_code}/transition",
            post(transition_initiative),
        )
}

/// Load the live initiative with this short code, or 404.
fn load(conn: &mut PgConnection, short_code: &str) -> Result<Initiative, ApiError> {
    use kairos_db::schema::initiatives::dsl;
    dsl::initiatives
        .filter(dsl::short_code.eq(short_code))
        .filter(dsl::deleted_at.is_null())
        .select(Initiative::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| short_code_not_found("initiative", short_code))
}

/// List initiatives (open tenant-wide, S-0005 list envelope).
#[utoipa::path(
    get,
    path = "/api/initiatives",
    tag = "initiatives",
    params(dto::Pagination),
    responses(
        (status = 200, description = "Page of initiatives", body = dto::ListEnvelope<dto::Initiative>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_initiatives(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(pagination): Query<dto::Pagination>,
) -> Result<Json<dto::ListEnvelope<dto::Initiative>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::initiatives::dsl;
            let total: i64 = dsl::initiatives
                .filter(dsl::deleted_at.is_null())
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Initiative> = dsl::initiatives
                .filter(dsl::deleted_at.is_null())
                .order(dsl::short_code.asc())
                .limit(limit)
                .offset(offset)
                .select(Initiative::as_select())
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

/// Get one initiative by short code (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/initiatives/{short_code}",
    tag = "initiatives",
    params(("short_code" = String, Path, description = "Initiative short code")),
    responses(
        (status = 200, description = "The initiative", body = dto::Initiative),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_initiative(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::Initiative>, ApiError> {
    let initiative = state
        .blocking
        .run(&tenant.slug, move |conn| {
            Ok(load(conn, &short_code)?.into_dto())
        })
        .await?;
    Ok(Json(initiative))
}

/// Create an initiative (requires `manage_initiatives` on the target
/// board). `bucket_type` set marks it as a bucket (`is_bucket` derived).
#[utoipa::path(
    post,
    path = "/api/initiatives",
    tag = "initiatives",
    request_body = dto::CreateInitiativeRequest,
    responses(
        (status = 201, description = "Created", body = dto::Initiative),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown board/column or bad enum value", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_initiative(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateInitiativeRequest>,
) -> Result<(StatusCode, Json<dto::Initiative>), ApiError> {
    let board_id = parse_uuid(&body.board_id, "board_id")?;
    let column_id = parse_opt_uuid(body.column_id.as_deref(), "column_id")?;
    let complexity = body
        .complexity
        .as_deref()
        .map(|v| parse_enum(v, "complexity", Complexity::ALL))
        .transpose()?;
    let bucket_type = body
        .bucket_type
        .as_deref()
        .map(|v| parse_enum(v, "bucket_type", BucketType::ALL))
        .transpose()?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, Some(board_id), user, MANAGE)?;
            let created = items::create_initiative(
                conn,
                items::CreateInitiative {
                    board_id,
                    column_id,
                    title: &body.title,
                    content: &body.content,
                    complexity,
                    bucket_type,
                },
                user,
            )
            .map_err(map_item_error)?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Update initiative content (KAIROS-A-0004 optimistic concurrency;
/// requires `manage_initiatives` on the initiative's board).
#[utoipa::path(
    patch,
    path = "/api/initiatives/{short_code}",
    tag = "initiatives",
    params(("short_code" = String, Path, description = "Initiative short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Initiative),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_initiative(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::UpdateContentRequest>,
) -> Result<Json<dto::Initiative>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let initiative = load(conn, &short_code)?;
            require_capability(conn, &slug, Some(initiative.board_id), user, MANAGE)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Initiative, initiative.id, update, user)
            {
                Ok(_) => Ok(load(conn, &short_code)?.into_dto()),
                Err(items::ItemError::VersionConflict {
                    expected_version,
                    current_version,
                    ..
                }) => {
                    let current = load(conn, &short_code)?.into_dto();
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

/// Soft-delete an initiative, cascading to its `parent` descendants
/// (KAIROS-A-0001; requires `manage_initiatives` on the initiative's
/// board).
#[utoipa::path(
    delete,
    path = "/api/initiatives/{short_code}",
    tag = "initiatives",
    params(("short_code" = String, Path, description = "Initiative short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_initiative(
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
            let initiative = load(conn, &short_code)?;
            require_capability(conn, &slug, Some(initiative.board_id), user, MANAGE)?;
            let outcome = items::soft_delete_item(conn, ItemType::Initiative, initiative.id, user)
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

/// Move an initiative to another column (requires `transition_items` on
/// the initiative's board).
#[utoipa::path(
    post,
    path = "/api/initiatives/{short_code}/transition",
    tag = "initiatives",
    params(("short_code" = String, Path, description = "Initiative short code")),
    request_body = dto::TransitionRequest,
    responses(
        (status = 200, description = "Transitioned (new column)", body = dto::Initiative),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Invalid transition; details.allowed_targets lists valid moves", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn transition_initiative(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::TransitionRequest>,
) -> Result<Json<dto::Initiative>, ApiError> {
    let to_column_id = parse_uuid(&body.to_column_id, "to_column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let transitioned = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let initiative = load(conn, &short_code)?;
            require_capability(
                conn,
                &slug,
                Some(initiative.board_id),
                user,
                "transition_items",
            )?;
            boards::transition_initiative(conn, initiative.id, to_column_id, user)
                .map_err(map_board_error)?;
            Ok(load(conn, &short_code)?.into_dto())
        })
        .await?;
    Ok(Json(transitioned))
}
