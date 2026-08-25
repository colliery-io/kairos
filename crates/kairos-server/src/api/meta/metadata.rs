//! `GET/PATCH /api/{entity_type}/{short_code}/metadata` (KAIROS-S-0005,
//! typed validation per KAIROS-A-0003).
//!
//! Reads are open tenant-wide. PATCH is gated by the item's
//! `manage_<type>` capability on its authorization board (A-0006:
//! documents inherit their parent's board via `supports`; off-board items
//! fall back to org-admin-only). Values are validated against the
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
use kairos_db::abac;
use kairos_db::models::templates::{MetadataDefinition, NewItemMetadata};
use uuid::Uuid;

use super::{
    item_metadata_response, manage_capability, resolve_family_item, validate_metadata_value,
};
use crate::api::{map_abac_error, require_capability};
use crate::app::AppState;
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
            let (item_id, _) = resolve_family_item(conn, &family, &short_code)?;
            item_metadata_response(conn, item_id, &short_code)
        })
        .await?;
    Ok(Json(response))
}

/// Set/update/clear metadata values on an item (requires the item's
/// `manage_<type>` capability on its authorization board). Every entry is
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
        (status = 403, description = "Missing capability on the item's board", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Unknown definition slug or invalid value for its type", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_metadata(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    Json(body): Json<dto::UpdateMetadataRequest>,
) -> Result<Json<dto::ItemMetadataResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, item_type) = resolve_family_item(conn, &family, &short_code)?;
            let board = abac::resolve_authorization_board(conn, item_id).map_err(map_abac_error)?;
            require_capability(conn, &slug, board, user, manage_capability(item_type))?;

            // Phase 1 — resolve + validate every entry (no writes yet):
            // a bad entry rejects the whole PATCH.
            use kairos_db::schema::metadata_definitions as definitions;
            let mut ops: Vec<(Uuid, Option<String>)> = Vec::with_capacity(body.values.len());
            for (definition_slug, value) in &body.values {
                let definition: MetadataDefinition = definitions::table
                    .filter(definitions::slug.eq(definition_slug))
                    .select(MetadataDefinition::as_select())
                    .first(conn)
                    .optional()
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| {
                        ApiError::validation(format!(
                            "unknown metadata definition slug {definition_slug:?}"
                        ))
                    })?;
                // KAIROS-T-0078: entity-type scoping is enforced on the
                // write path, not just hidden in pickers. Clears of
                // out-of-scope values are still allowed (cleanup).
                if value.is_some()
                    && !kairos_db::items::definition_applies_to(
                        conn,
                        definition.id,
                        item_type.entity_type(),
                    )
                    .map_err(ApiError::internal)?
                {
                    return Err(ApiError::validation(format!(
                        "metadata definition {definition_slug:?} does not apply to \
                         {} items",
                        item_type.entity_type()
                    )));
                }
                if let Some(value) = value {
                    validate_metadata_value(conn, &definition, value)?;
                }
                ops.push((definition.id, value.clone()));
            }

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
