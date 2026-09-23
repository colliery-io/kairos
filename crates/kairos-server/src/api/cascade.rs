//! `GET /api/{entity_type}/{short_code}/cascade-preview` (KAIROS-T-0051):
//! the AUTHORITATIVE pre-delete cascade preview.
//!
//! Found by KAIROS-T-0041: the GUI delete confirm can only warn with an
//! item's DIRECT relationship children before the fact; the full
//! transitive descendant set (KAIROS-A-0001) was known only from the
//! post-delete `DeleteResponse`. This read-only endpoint closes that gap —
//! it returns exactly the descendant set a soft-delete WOULD cascade to,
//! computed by the SAME `kairos_core::items::cascade_descendants` BFS the
//! delete uses (single source of truth; see
//! [`kairos_db::items::preview_cascade`]), without deleting anything.
//!
//! Shape: a side-effect-free GET on a per-item subresource — the RESTful,
//! idempotent choice, and the same generic `{entity_type}` pattern as the
//! `relationships`/`metadata`/`history` reads. Open tenant-wide like every
//! other read (KAIROS-A-0006): no capability check beyond the middleware
//! stack.

use axum::extract::{Extension, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use kairos_client::types as dto;
use kairos_db::items;

use super::map_item_error;
use super::meta::resolve_family_item;
use crate::api::Liveness;
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/{entity_type}/{short_code}/cascade-preview",
        get(cascade_preview),
    )
}

/// The authoritative KAIROS-A-0001 descendant set a delete of this item
/// would cascade to, computed without deleting (open tenant-wide read).
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/cascade-preview",
    tag = "cascade",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "The transitive descendant set a soft-delete would cascade to (root excluded), matching the eventual DeleteResponse", body = dto::CascadePreviewResponse),
        (status = 404, description = "Unknown family or short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn cascade_preview(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::CascadePreviewResponse>, ApiError> {
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, item_type) =
                resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let preview =
                items::preview_cascade(conn, item_type, item_id).map_err(map_item_error)?;
            Ok(dto::CascadePreviewResponse {
                short_code: preview.root_short_code,
                cascade_count: preview.cascaded_short_codes.len() as i64,
                cascaded_short_codes: preview.cascaded_short_codes,
            })
        })
        .await?;
    Ok(Json(response))
}
