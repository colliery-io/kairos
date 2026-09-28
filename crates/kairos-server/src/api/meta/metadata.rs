//! `GET/PATCH /api/{entity_type}/{short_code}/metadata` (KAIROS-S-0005,
//! typed validation per KAIROS-A-0003).
//!
//! Reads are open tenant-wide. PATCH takes the edit rule
//! (COLLIERY-T-0228): the caller created the item, or holds the item's
//! `manage_<type>` capability on its authorization board (A-0006:
//! documents inherit their parent's board via `supports`), or is an org
//! admin. Values are validated against the
//! referenced `metadata_definitions` row — enum membership, date parse,
//! string passthrough — then upserted into `item_metadata` (`null`
//! clears). Per A-0004, metadata writes are NOT versioned and write no
//! history/activity rows.

use axum::extract::{Extension, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use kairos_client::types_meta as dto;
use kairos_db::models::templates::NewItemMetadata;

use super::{item_metadata_response, resolve_family_item, validated_metadata_ops};
use crate::api::Liveness;
use crate::api::require_item_edit;
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/{entity_type}/{short_code}/metadata",
        get(get_metadata).patch(update_metadata),
    )
}

/// An item's metadata values with their definitions (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/metadata",
    tag = "metadata",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "The item's metadata values", body = dto::ItemMetadataResponse),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_metadata(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::ItemMetadataResponse>, ApiError> {
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            item_metadata_response(conn, item_id, &short_code)
        })
        .await?;
    Ok(Json(response))
}

/// Set/update/clear metadata values on an item.
///
/// The edit rule applies (COLLIERY-T-0228). The caller created the
/// item, holds `manage_<type>` on its authorization board, or is an organization admin.
///
/// Every entry is
/// validated BEFORE anything is written (A-0003: unknown slug and invalid
/// values are 422 `VALIDATION`); the writes then apply atomically.
#[utoipa::path(
    patch,
    path = "/api/{entity_type}/{short_code}/metadata",
    tag = "metadata",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    request_body = dto::UpdateMetadataRequest,
    responses(
        (status = 200, description = "The item's metadata after the update", body = dto::ItemMetadataResponse),
        (status = 403, description = "Refused by the edit rule: the caller did not create the item and lacks the capability", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Unknown definition slug or invalid value for its type", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_metadata(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::UpdateMetadataRequest>,
) -> Result<Json<dto::ItemMetadataResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, item_type) =
                resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            require_item_edit(conn, &slug, user, item_id, item_type)?;

            // Phase 1 — resolve + validate every entry (no writes yet): a bad
            // entry rejects the whole PATCH. Shared with MCP `set_metadata`
            // (KAIROS-T-0096), which is where the T-0078 guard used to be absent.
            let ops = validated_metadata_ops(conn, item_type, &body.values)?;

            // Phase 2 — apply all upserts/deletes in one transaction.
            use kairos_db::schema::item_metadata;
            conn.transaction::<_, DieselError, _>(|conn| {
                for (definition_id, value) in &ops {
                    match value {
                        Some(value) => {
                            diesel::insert_into(item_metadata::table)
                                .values(NewItemMetadata {
                                    item_id,
                                    metadata_definition_id: *definition_id,
                                    value: value.clone(),
                                })
                                .on_conflict((
                                    item_metadata::item_id,
                                    item_metadata::metadata_definition_id,
                                ))
                                .do_update()
                                .set(item_metadata::value.eq(value))
                                .execute(conn)?;
                        }
                        None => {
                            diesel::delete(
                                item_metadata::table
                                    .filter(item_metadata::item_id.eq(item_id))
                                    .filter(
                                        item_metadata::metadata_definition_id.eq(*definition_id),
                                    ),
                            )
                            .execute(conn)?;
                        }
                    }
                }
                Ok(())
            })
            .map_err(ApiError::internal)?;

            // KAIROS-T-0022: thin `metadata_changed` event. The upsert
            // transaction above has committed, so the NOTIFY (autocommit
            // on this connection) is post-commit by ordering.
            kairos_db::events::emit_item_event_by_id(
                conn,
                kairos_db::events::EventKind::MetadataChanged,
                item_type.entity_type(),
                item_id,
                user,
            )
            .map_err(ApiError::internal)?;

            item_metadata_response(conn, item_id, &short_code)
        })
        .await?;
    Ok(Json(response))
}
