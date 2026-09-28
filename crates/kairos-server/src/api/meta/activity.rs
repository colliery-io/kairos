//! `GET /api/activity` (KAIROS-S-0005, model per KAIROS-A-0004): the
//! audit-trail query endpoint.
//!
//! Open tenant-wide (A-0006 reads). The four filters — `entity_id`,
//! `actor_id`, `action`, `since` — are freely combinable and paginate
//! with `limit`/`offset` (S-0005); results are newest first. Malformed
//! filter values (bad UUID, unknown action, non-RFC-3339 `since`) are 422
//! `VALIDATION`.
//!
//! An entry about an item has the short code and the title of the item
//! (COLLIERY-T-0262), so that a client can make a link to the item and
//! does not read the lists of the items. The handler reads the items of
//! one page with one query for each item type on the page: 5 queries at
//! most, for a page of each size.

use std::collections::{BTreeSet, HashMap};

use axum::extract::{Extension, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto_base;
use kairos_client::types_meta as dto;
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::graph::ActivityLogEntry;
use uuid::Uuid;

use crate::api::convert::IntoDto;
use crate::api::convert_meta::timestamp;
use crate::api::{clamp_pagination, parse_enum, parse_uuid};
use crate::app::AppState;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/activity", get(get_activity))
}

/// The short code, the title and the time of the archive of an item.
type ItemHead = (String, String, Option<DateTime<Utc>>);

/// The items that the entries of one page are about, by id
/// (COLLIERY-T-0262). An archived item is in the result: a person can
/// open it. An entry about a board, a team or a member has no item, and
/// its id is not in the result.
///
/// One query for each item type that the page has, and no query for an
/// entry: the number of queries does not grow with the page.
fn item_heads(
    conn: &mut PgConnection,
    rows: &[ActivityLogEntry],
) -> Result<HashMap<Uuid, ItemHead>, ApiError> {
    use kairos_db::schema::{adrs, documents, initiatives, strategies, tasks};

    let mut heads = HashMap::new();
    /// Read the heads of one item type into `heads`.
    macro_rules! read_heads {
        ($table:ident, $entity_type:literal) => {{
            let ids: BTreeSet<Uuid> = rows
                .iter()
                .filter(|row| row.entity_type.as_deref() == Some($entity_type))
                .filter_map(|row| row.entity_id)
                .collect();
            if !ids.is_empty() {
                let found: Vec<(Uuid, String, String, Option<DateTime<Utc>>)> = $table::table
                    .filter($table::id.eq_any(ids))
                    .select((
                        $table::id,
                        $table::short_code,
                        $table::title,
                        $table::deleted_at,
                    ))
                    .load(conn)
                    .map_err(ApiError::internal)?;
                for (id, short_code, title, archived_at) in found {
                    heads.insert(id, (short_code, title, archived_at));
                }
            }
        }};
    }
    read_heads!(strategies, "strategy");
    read_heads!(initiatives, "initiative");
    read_heads!(tasks, "task");
    read_heads!(documents, "document");
    read_heads!(adrs, "adr");
    Ok(heads)
}

/// Query the activity log with combinable filters + pagination.
///
/// An entry about an item has `entity_short_code`, `entity_title` and
/// `entity_archived_at` (COLLIERY-T-0262). The three fields are null for
/// an entry that is not about an item. They are null too for an item
/// that Kairos does not have.
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
    ApiQuery(query): ApiQuery<dto::ActivityQuery>,
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
                        "The value {value:?} of since is not a timestamp. Send an RFC \
                         3339 timestamp."
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

            // COLLIERY-T-0262: the short code and the title of each item
            // of the page.
            let heads = item_heads(conn, &rows)?;
            let items = rows
                .into_iter()
                .map(|row| {
                    let head = row.entity_id.and_then(|id| heads.get(&id)).cloned();
                    let mut entry: dto::ActivityEntry = row.into_dto();
                    if let Some((short_code, title, archived_at)) = head {
                        entry.entity_short_code = Some(short_code);
                        entry.entity_title = Some(title);
                        entry.entity_archived_at = archived_at.map(timestamp);
                    }
                    entry
                })
                .collect();

            Ok(dto_base::ListEnvelope {
                items,
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}
