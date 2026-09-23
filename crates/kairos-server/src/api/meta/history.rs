//! `GET /api/{entity_type}/{short_code}/history` (KAIROS-S-0005, model
//! per KAIROS-A-0004): the append-only content version history.
//!
//! Open tenant-wide (A-0006 reads). Without `?version=`, the paginated
//! version list (newest first — `version`, `edited_by`, `edited_at`);
//! with `?version=N`, that snapshot's full title + content. Version 1 is
//! the create-time baseline (T-0012), so every item has history from
//! birth.

use axum::extract::{Extension, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use diesel::prelude::*;
use kairos_client::types as dto_base;
use kairos_client::types_meta as dto;
use kairos_db::models::graph::ItemHistory;
use serde_json::Value;

use super::resolve_family_item;
use crate::api::Liveness;
use crate::api::clamp_pagination;
use crate::api::convert::IntoDto;
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/{entity_type}/{short_code}/history", get(get_history))
}

/// Content version history: the paginated version list, or one full
/// snapshot with `?version=N`.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/history",
    tag = "history",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
        dto::HistoryQuery,
    ),
    responses(
        (status = 200, description = "Version list envelope (or a HistorySnapshot with ?version=N)", body = dto_base::ListEnvelope<dto::HistoryVersion>),
        (status = 404, description = "Unknown family, short code, or version", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_history(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    Query(query): Query<dto::HistoryQuery>,
) -> Result<Json<Value>, ApiError> {
    let (limit, offset) = clamp_pagination(&dto_base::Pagination {
        limit: query.limit,
        offset: query.offset,
    });
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::item_history as history;
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;

            if let Some(version) = query.version {
                let snapshot: ItemHistory = history::table
                    .filter(history::item_id.eq(item_id))
                    .filter(history::version.eq(version))
                    .select(ItemHistory::as_select())
                    .first(conn)
                    .optional()
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| {
                        ApiError::not_found(format!(
                            "no history snapshot for {short_code} at version {version}"
                        ))
                    })?;
                let snapshot: dto::HistorySnapshot = snapshot.into_dto();
                return serde_json::to_value(snapshot).map_err(ApiError::internal);
            }

            let total: i64 = history::table
                .filter(history::item_id.eq(item_id))
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<ItemHistory> = history::table
                .filter(history::item_id.eq(item_id))
                .order(history::version.desc())
                .limit(limit)
                .offset(offset)
                .select(ItemHistory::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let envelope = dto_base::ListEnvelope::<dto::HistoryVersion> {
                items: rows.into_iter().map(IntoDto::into_dto).collect(),
                total,
                limit,
                offset,
            };
            serde_json::to_value(envelope).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(response))
}
