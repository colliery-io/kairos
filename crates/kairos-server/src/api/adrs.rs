//! `/api/adrs` (KAIROS-S-0005) — see [`super`] for the shared T-0018
//! handler pattern. ADR board placement is optional (both `board_id` and
//! `column_id`, or neither): on-board ADRs authorize against their board;
//! off-board ADRs have no board context, so the create falls back to the
//! org-admin-only policy (KAIROS-A-0006) and transitions are 422
//! `ITEM_NOT_ON_BOARD` (T-0010's typed error). An edit of an off-board ADR
//! is for its creator or an org admin (the edit rule, COLLIERY-T-0228).

use axum::extract::{Extension, Path, State};
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

use super::convert::{IntoDto, attach_impact, attach_impacts};
use super::documents::{clamp_impact_list, impacting_ids};
use super::{
    Liveness, map_board_error, map_item_error, opt_board_id_by_ref, parse_opt_uuid, parse_uuid,
    require_capability, require_item_edit,
};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
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

/// An ADR as the wire type, with its `impacts` links (COLLIERY-T-0269).
fn render(conn: &mut PgConnection, adr: Adr) -> Result<dto::Adr, ApiError> {
    attach_impact(conn, adr.into_dto())
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
    let found = query
        .select(Adr::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?;
    // COLLIERY-T-3100: a read follows a retired code; a write is refused.
    super::found_or_follow_retired(conn, found, "adr", short_code, liveness, load)
}

/// List ADRs (open tenant-wide, S-0005 list envelope).
///
/// `?include_deleted=true` widens the listing to archived work, each row
/// marked with `archived_at` (KAIROS-A-0020 rule 2). Default false: rule 3
/// is that a listing nobody asked hides put-away work.
///
/// `?repository=` keeps the ADRs that impact that repository
/// (COLLIERY-T-0269). The value is the slug or the id of a live
/// repository. An unknown repository is a 422 `VALIDATION`.
#[utoipa::path(
    get,
    path = "/api/adrs",
    tag = "adrs",
    params(dto::ImpactListQuery),
    responses(
        (status = 200, description = "Page of ADRs", body = dto::ListEnvelope<dto::Adr>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
        (status = 422, description = "Unknown repository", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_adrs(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    ApiQuery(query): ApiQuery<dto::ImpactListQuery>,
) -> Result<Json<dto::ListEnvelope<dto::Adr>>, ApiError> {
    let (limit, offset, liveness) = clamp_impact_list(&query);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::adrs::dsl;
            let impacting = impacting_ids(conn, query.repository.as_deref())?;
            // ONE predicate, applied to both the count and the page
            // (KAIROS-T-0159): the two can never disagree.
            let visible = || {
                let mut query = dsl::adrs.into_boxed();
                if liveness == Liveness::LiveOnly {
                    query = query.filter(dsl::deleted_at.is_null());
                }
                if let Some(ids) = &impacting {
                    query = query.filter(dsl::id.eq_any(ids.clone()));
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
            let mut items: Vec<dto::Adr> = rows.into_iter().map(IntoDto::into_dto).collect();
            attach_impacts(conn, &mut items)?;
            Ok(dto::ListEnvelope {
                items,
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
            let adr = load(conn, &short_code, Liveness::IncludeArchived)?;
            render(conn, adr)
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
    ApiJson(body): ApiJson<dto::CreateAdrRequest>,
) -> Result<(StatusCode, Json<dto::Adr>), ApiError> {
    // KAIROS-T-0150: slug or UUID; resolved in the closure below.
    let column_id = parse_opt_uuid(body.column_id.as_deref(), "column_id")?;
    let decision_date = body
        .decision_date
        .as_deref()
        .map(|value| {
            value.parse::<NaiveDate>().map_err(|_| {
                ApiError::validation(format!(
                    "The value {value:?} of decision_date is not a date. Send the date as \
                     YYYY-MM-DD."
                ))
            })
        })
        .transpose()?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let board_id = opt_board_id_by_ref(conn, body.board_id.as_deref())?;
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
            render(conn, created)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Update ADR content (KAIROS-A-0004 optimistic concurrency).
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// ADR, holds `manage_adrs` on its board, or is an organization admin.
/// An off-board ADR has no board: its creator or an organization admin.
#[utoipa::path(
    patch,
    path = "/api/adrs/{short_code}",
    tag = "adrs",
    params(("short_code" = String, Path, description = "ADR short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Adr),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_adr(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    ApiJson(body): ApiJson<dto::UpdateContentRequest>,
) -> Result<Json<dto::Adr>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let adr = load(conn, &short_code, Liveness::LiveOnly)?;
            require_item_edit(conn, &slug, user, adr.id, ItemType::Adr)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Adr, adr.id, update, user) {
                Ok(_) => {
                    let updated = load(conn, &short_code, Liveness::LiveOnly)?;
                    render(conn, updated)
                }
                Err(items::ItemError::VersionConflict {
                    expected_version,
                    current_version,
                    ..
                }) => {
                    let current = load(conn, &short_code, Liveness::LiveOnly)?;
                    let current = render(conn, current)?;
                    Err(ApiError::conflict(format!(
                        "The request has the version {expected_version}, and the current \
                         version is {current_version}. Get the item again, and make the \
                         edit on the current version."
                    ))
                    .with_details(json!({ "current": current })))
                }
                Err(e) => Err(map_item_error(e)),
            }
        })
        .await?;
    Ok(Json(updated))
}

/// Soft-delete an ADR.
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// ADR, holds `manage_adrs` on its board, or is an organization admin.
/// An off-board ADR has no board: its creator or an organization admin.
#[utoipa::path(
    delete,
    path = "/api/adrs/{short_code}",
    tag = "adrs",
    params(("short_code" = String, Path, description = "ADR short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = dto::ErrorEnvelope),
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
            // The edit rule for the ADR, and then for each descendant
            // (COLLIERY-T-0234): `archive_item` does the two.
            let outcome = super::cascade::archive_item(conn, &slug, user, adr.id, ItemType::Adr)?;
            Ok(super::cascade::delete_response(outcome))
        })
        .await?;
    Ok(Json(outcome))
}

/// Move an ADR to another column of its board (requires `transition_items`
/// on the ADR's board). The creator of the ADR gets no right here
/// (COLLIERY-T-0228). An off-board ADR cannot be transitioned: 422
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
    ApiJson(body): ApiJson<dto::TransitionRequest>,
) -> Result<Json<dto::Adr>, ApiError> {
    let to_column_id = parse_uuid(&body.to_column_id, "to_column_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let transitioned = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let adr = load(conn, &short_code, Liveness::LiveOnly)?;
            // NOT an edit (COLLIERY-T-0228): creation grants no right here.
            // The creator of an item needs this capability as all others do,
            // because a team controls its own plan (COLLIERY-T-0218).
            require_capability(conn, &slug, adr.board_id, user, "transition_items")?;
            boards::transition_adr(conn, adr.id, to_column_id, user).map_err(map_board_error)?;
            let moved = load(conn, &short_code, Liveness::LiveOnly)?;
            render(conn, moved)
        })
        .await?;
    Ok(Json(transitioned))
}
