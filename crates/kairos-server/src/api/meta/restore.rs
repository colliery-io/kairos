//! `POST /api/{entity_type}/{short_code}/restore` (KAIROS-T-0160,
//! KAIROS-A-0020): put archived work back.
//!
//! One wildcard route rather than five per-family ones, for the same
//! reason `/history` is one: the verb is identical across the families and
//! the family segment resolves through [`resolve_family_item`].
//!
//! # Why this is not a new privilege
//!
//! Restoring is the inverse of archiving, so it asks for the same
//! capability the delete asked for — `manage_<family>` on the item's board.
//! Inventing a `restore_items` capability would mean every existing
//! deployment had someone who could put work away and nobody who could put
//! it back.
//!
//! # Why it refuses instead of re-homing
//!
//! An item's board, column, owning team or repository may have been retired
//! while it was away. Silently moving it somewhere else would destroy the
//! placement the record is evidence of, so the refusal NAMES what is
//! missing and the caller moves the item deliberately — the same shape as
//! `live_board_item_codes` refusing a team delete.

use axum::extract::{Extension, Path, State};
use axum::routing::post;
use axum::{Json, Router};
use kairos_client::types as dto;
use kairos_db::items;
use serde_json::json;

use super::{manage_capability, resolve_family_item};
use crate::api::{Liveness, map_item_error};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/{entity_type}/{short_code}/restore",
        post(restore_item),
    )
}

/// Put an archived item back on its board.
#[utoipa::path(
    post,
    path = "/api/{entity_type}/{short_code}/restore",
    tag = "restore",
    params(
        ("entity_type" = String, Path, description = "Plural family name"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "Restored", body = dto::RestoreResponse),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code, or the item is not archived", body = dto::ErrorEnvelope),
        (status = 422, description = "Its board, column, team or repository is gone; details.missing names them", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn restore_item(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::RestoreResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, item_type) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            let board = kairos_db::abac::resolve_authorization_board(conn, item_id)
                .map_err(crate::api::map_abac_error)?;
            crate::api::require_capability(conn, &slug, board, user, manage_capability(item_type))?;

            match items::restore_item(conn, item_type, item_id, user).map_err(map_item_error)? {
                Ok(outcome) => Ok(dto::RestoreResponse {
                    short_code: outcome.short_code,
                    still_archived_count: outcome.still_archived_descendants.len() as i64,
                    still_archived_short_codes: outcome.still_archived_descendants,
                }),
                Err(blocked) => Err(ApiError::unprocessable(
                    "RESTORE_BLOCKED",
                    format!(
                        "{short_code} cannot be restored because {} is gone; \
                         move it somewhere that still exists, or restore what \
                         it needs first",
                        blocked.missing.join(" and ")
                    ),
                )
                .with_details(json!({ "missing": blocked.missing }))),
            }
        })
        .await?;
    Ok(Json(response))
}
