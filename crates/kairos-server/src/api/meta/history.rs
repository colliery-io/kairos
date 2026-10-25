//! `GET /api/{entity_type}/{short_code}/history` (KAIROS-S-0005, model
//! per KAIROS-A-0004): the append-only content version history.
//!
//! Open tenant-wide (A-0006 reads). Without `?version=`, the paginated
//! version list (newest first — `version`, `edited_by`, `edited_by_name`,
//! `edited_at`); with `?version=N`, that snapshot's full title + content.
//! Version 1 is the create-time baseline (T-0012), so every item has
//! history from birth. A version can be a diff row (KAIROS-T-0651): the
//! snapshot has the full text all the same.
//!
//! `GET /api/{entity_type}/{short_code}/activity` (KAIROS-T-0557) is the
//! activity of the item, newest first, with names
//! ([`kairos_db::item_activity`]). The CLI `<noun> history` reads both.

use axum::extract::{Extension, Path, State};
use axum::routing::get;
use axum::{Json, Router};
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
use crate::input::ApiQuery;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/{entity_type}/{short_code}/history", get(get_history))
        .route(
            "/api/{entity_type}/{short_code}/activity",
            get(get_item_activity),
        )
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
    ApiQuery(query): ApiQuery<dto::HistoryQuery>,
) -> Result<Json<Value>, ApiError> {
    let (limit, offset) = clamp_pagination(&dto_base::Pagination {
        limit: query.limit,
        offset: query.offset,
    });
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;

            if let Some(version) = query.version {
                // KAIROS-T-0651: a diff row gets its text from its base row.
                let snapshot: ItemHistory = kairos_db::history::version_row(conn, item_id, version)
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| {
                        ApiError::not_found(format!(
                            "{short_code} has no version {version} in its history."
                        ))
                        .with_details(serde_json::json!({ "parameter": "version" }))
                    })?;
                let snapshot: dto::HistorySnapshot = snapshot.into_dto();
                return serde_json::to_value(snapshot).map_err(ApiError::internal);
            }

            // KAIROS-T-0629: the list reads no title and no content.
            let (total, rows) = kairos_db::items::history_versions(conn, item_id, limit, offset)
                .map_err(ApiError::internal)?;
            // KAIROS-T-0557: the names of the editors of the page.
            let editors: Vec<uuid::Uuid> = rows.iter().map(|row| row.edited_by).collect();
            let names = kairos_db::task_claims::display_names(conn, &editors)
                .map_err(ApiError::internal)?;
            let envelope = dto_base::ListEnvelope::<dto::HistoryVersion> {
                items: rows
                    .into_iter()
                    .map(|row| {
                        let name = names.get(&row.edited_by).cloned();
                        let mut version: dto::HistoryVersion = row.into_dto();
                        version.edited_by_name = name;
                        version
                    })
                    .collect(),
                total,
                limit,
                offset,
            };
            serde_json::to_value(envelope).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(response))
}

/// The activity of one item, newest first (KAIROS-T-0557).
///
/// Each row has the action, the stored details, and `text`. The text is
/// the row as one line. It names the people, the boards, the columns and
/// the repositories.
///
/// The list has moves, cancels, claims, hand-offs and releases. It also
/// has the lane, metadata values, state writes and links. A link between
/// two items is in the list of the two items. An archived item has its
/// activity too.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/activity",
    tag = "history",
    params(
        ("entity_type" = String, Path, description = "Plural family name (strategies|initiatives|tasks|documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
        dto::ItemActivityQuery,
    ),
    responses(
        (status = 200, description = "Page of the activity of the item, newest first", body = dto_base::ListEnvelope<dto::ItemActivityEntry>),
        (status = 404, description = "Unknown family or short code", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_item_activity(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<dto::ItemActivityQuery>,
) -> Result<Json<dto_base::ListEnvelope<dto::ItemActivityEntry>>, ApiError> {
    let (limit, offset) = clamp_pagination(&dto_base::Pagination {
        limit: query.limit,
        offset: query.offset,
    });
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, _) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            // A retired code finds the item: the links match its current code.
            let code = crate::api::short_code_of(conn, item_id)?;
            let page = kairos_db::item_activity::of_item(conn, item_id, &code, limit, offset)
                .map_err(ApiError::internal)?;
            Ok(dto_base::ListEnvelope {
                items: page
                    .rows
                    .into_iter()
                    .map(|row| dto::ItemActivityEntry {
                        id: row.id.to_string(),
                        action: row.action.as_str().to_string(),
                        details: row.details,
                        text: row.text,
                        actor_id: row.actor_id.to_string(),
                        actor_name: row.actor_name,
                        agent_key_id: row.agent_key_id.map(|id| id.to_string()),
                        occurred_at: crate::api::convert_meta::timestamp(row.occurred_at),
                    })
                    .collect(),
                total: page.total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}
