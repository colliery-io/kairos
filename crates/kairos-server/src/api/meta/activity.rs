//! `GET /api/activity` (KAIROS-S-0005, model per KAIROS-A-0004): the
//! audit-trail query endpoint.
//!
//! Open tenant-wide (A-0006 reads). The four filters — `entity_id`,
//! `actor_id`, `action`, `since` — are freely combinable and paginate
//! with `limit`/`offset` (S-0005); results are newest first. Malformed
//! filter values (bad UUID, unknown action, non-RFC-3339 `since`) are 422
//! `VALIDATION`.

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use kairos_client::types as dto_base;
use kairos_client::types_meta as dto;
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::graph::ActivityLogEntry;

use crate::api::convert::IntoDto;
use crate::api::{clamp_pagination, parse_enum, parse_uuid};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/activity", get(get_activity))
}

/// Query the activity log with combinable filters + pagination.
#[utoipa::path(
    get,
    path = "/api/activity",
    tag = "activity",
    params(dto::ActivityQuery),
    responses(
        (status = 200, description = "Page of activity entries, newest first", body = dto_base::ListEnvelope<dto::ActivityEntry>),
        (status = 422, description = "Malformed filter value", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_activity(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<dto::ActivityQuery>,
) -> Result<Json<dto_base::ListEnvelope<dto::ActivityEntry>>, ApiError> {
    let entity_id = query
        .entity_id
        .as_deref()
        .map(|value| parse_uuid(value, "entity_id"))
        .transpose()?;
    let actor_id = query
        .actor_id
        .as_deref()
        .map(|value| parse_uuid(value, "actor_id"))
        .transpose()?;
    let action = query
        .action
        .as_deref()
        .map(|value| parse_enum(value, "action", ActivityAction::ALL))
        .transpose()?;
    let since: Option<DateTime<Utc>> = query
        .since
        .as_deref()
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| {
                    ApiError::validation(format!(
                        "since must be an RFC 3339 timestamp, got {value:?}"
                    ))
                })
        })
        .transpose()?;
    let (limit, offset) = clamp_pagination(&dto_base::Pagination {
        limit: query.limit,
        offset: query.offset,
    });

    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::activity_log as log;

            /// Apply the combinable S-0005 filters to any boxed
            /// `activity_log` query (used for both the count and the
            /// page).
            macro_rules! filtered {
                ($query:expr) => {{
                    let mut query = $query;
                    if let Some(entity_id) = entity_id {
                        query = query.filter(log::entity_id.eq(entity_id));
                    }
                    if let Some(actor_id) = actor_id {
                        query = query.filter(log::actor_id.eq(actor_id));
                    }
                    if let Some(action) = action {
                        query = query.filter(log::action.eq(action));
                    }
                    if let Some(since) = since {
                        query = query.filter(log::occurred_at.ge(since));
                    }
                    query
                }};
            }

            let total: i64 = filtered!(log::table.select(diesel::dsl::count_star()).into_boxed())
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<ActivityLogEntry> = filtered!(
                log::table
                    .select(ActivityLogEntry::as_select())
                    .into_boxed()
            )
            .order(log::occurred_at.desc())
            .limit(limit)
            .offset(offset)
            .load(conn)
            .map_err(ApiError::internal)?;

            Ok(dto_base::ListEnvelope {
                items: rows.into_iter().map(IntoDto::into_dto).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}
