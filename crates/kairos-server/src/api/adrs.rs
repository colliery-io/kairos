//! `/api/adrs` (KAIROS-S-0005) — see [`super`] for the shared T-0018
//! handler pattern. ADR board placement is optional (both `board_id` and
//! `column_id`, or neither): on-board ADRs authorize against their board;
//! off-board ADRs have no board context, so writes fall back to the
//! org-admin-only policy (KAIROS-A-0006) and transitions are 422
//! `ITEM_NOT_ON_BOARD` (T-0010's typed error).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::NaiveDate;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::items::Adr;
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
const MANAGE: &str = "manage_adrs";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/adrs", get(list_adrs).post(create_adr))
        .route(
            "/api/adrs/{short_code}",
            get(get_adr).patch(update_adr).delete(delete_adr),
        )
        .route("/api/adrs/{short_code}/transition", post(transition_adr))
}

/// Load the live ADR with this short code, or 404.
fn load(conn: &mut PgConnection, short_code: &str, liveness: Liveness) -> Result<Adr, ApiError> {
    use kairos_db::schema::adrs::dsl;
    let mut query = dsl::adrs
        .filter(dsl::short_code.eq(short_code))
        .into_boxed();
    if liveness == Liveness::LiveOnly {
        query = query.filter(dsl::deleted_at.is_null());
    }
    query
        .select(Adr::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| short_code_not_found("adr", short_code))
}

/// List ADRs (open tenant-wide, S-0005 list envelope).
///
/// `?include_deleted=true` widens the listing to archived work, each row
/// marked with `archived_at` (KAIROS-A-0020 rule 2). Default false: rule 3
/// is that a listing nobody asked hides put-away work.
#[utoipa::path(
    get,
    path = "/api/adrs",
    tag = "adrs",
    params(dto::ListQuery),
    responses(
        (status = 200, description = "Page of ADRs", body = dto::ListEnvelope<dto::Adr>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_adrs(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<dto::ListQuery>,
) -> Result<Json<dto::ListEnvelope<dto::Adr>>, ApiError> {
    let (limit, offset, liveness) = clamp_list(&query);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::adrs::dsl;
            // ONE predicate, applied to both the count and the page
            // (KAIROS-T-0159): the two can never disagree.
            let visible = || {
                let mut query = dsl::adrs.into_boxed();
                if liveness == Liveness::LiveOnly {
                    query = query.filter(dsl::deleted_at.is_null());
                }
                query
            };
            let total: i64 = visible()
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Adr> = visible()
                .order(dsl::short_code.asc())
                .limit(limit)
                .offset(offset)
                .select(Adr::as_select())
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

/// Get one ADR by short code (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/adrs/{short_code}",
    tag = "adrs",
    params(("short_code" = String, Path, description = "ADR short code")),
    responses(
        (status = 200, description = "The ADR", body = dto::Adr),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_adr(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::Adr>, ApiError> {
    let adr = state
        .blocking
        .run(&tenant.slug, move |conn| {
            Ok(load(conn, &short_code, Liveness::IncludeArchived)?.into_dto())
        })
        .await?;
    Ok(Json(adr))
}

/// Create an ADR. On a board: requires `manage_adrs` on that board.
/// Off-board (no `board_id`): org-admin-only (A-0006 fallback).
#[utoipa::path(
    post,
    path = "/api/adrs",
    tag = "adrs",
    request_body = dto::CreateAdrRequest,
    responses(
        (status = 201, description = "Created", body = dto::Adr),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown board/column or bad date", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_adr(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateAdrRequest>,
) -> Result<(StatusCode, Json<dto::Adr>), ApiError> {
    let board_id = parse_opt_uuid(body.board_id.as_deref(), "board_id")?;
    let column_id = parse_opt_uuid(body.column_id.as_deref(), "column_id")?;
    let decision_date = body
        .decision_date
        .as_deref()
        .map(|value| {
            value.parse::<NaiveDate>().map_err(|_| {
                ApiError::validation(format!("decision_date must be YYYY-MM-DD, got {value:?}"))
            })
        })
        .transpose()?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, board_id, user, MANAGE)?;
            let created = items::create_adr(
                conn,
                items::CreateAdr {
                    board_id,
                    column_id,
                    title: &body.title,
                    content: &body.content,
                    decision_maker: body.decision_maker.as_deref(),
                    decision_date,
                },
                user,
            )
            .map_err(map_item_error)?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Update ADR content (KAIROS-A-0004 optimistic concurrency; requires
/// `manage_adrs` on the ADR's board, or org admin for off-board ADRs).
#[utoipa::path(
    patch,
    path = "/api/adrs/{short_code}",
    tag = "adrs",
    params(("short_code" = String, Path, description = "ADR short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Adr),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_adr(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::UpdateContentRequest>,
) -> Result<Json<dto::Adr>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let adr = load(conn, &short_code, Liveness::LiveOnly)?;
            require_capability(conn, &slug, adr.board_id, user, MANAGE)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Adr, adr.id, update, user) {
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

/// Soft-delete an ADR (requires `manage_adrs` on the ADR's board, or org
/// admin for off-board ADRs).
#[utoipa::path(
    delete,
    path = "/api/adrs/{short_code}",
    tag = "adrs",
    params(("short_code" = String, Path, description = "ADR short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_adr(
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
            let adr = load(conn, &short_code, Liveness::LiveOnly)?;
            require_capability(conn, &slug, adr.board_id, user, MANAGE)?;
            let outcome = items::soft_delete_item(conn, ItemType::Adr, adr.id, user)
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

/// Move an ADR to another column of its board (requires `transition_items`
/// on the ADR's board). An off-board ADR cannot be transitioned: 422
/// `ITEM_NOT_ON_BOARD`.
#[utoipa::path(
    post,
    path = "/api/adrs/{short_code}/transition",
    tag = "adrs",
    params(("short_code" = String, Path, description = "ADR short code")),
    request_body = dto::TransitionRequest,
    responses(
        (status = 200, description = "Transitioned (new column)", body = dto::Adr),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Invalid transition (details.allowed_targets) or ITEM_NOT_ON_BOARD", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn transition_adr(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::TransitionRequest>,
) -> Result<Json<dto::Adr>, ApiError> {
    let to_column_id = parse_uuid(&body.to_column_id, "to_column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let transitioned = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let adr = load(conn, &short_code, Liveness::LiveOnly)?;
            require_capability(conn, &slug, adr.board_id, user, "transition_items")?;
            boards::transition_adr(conn, adr.id, to_column_id, user).map_err(map_board_error)?;
            Ok(load(conn, &short_code, Liveness::LiveOnly)?.into_dto())
        })
        .await?;
    Ok(Json(transitioned))
}
