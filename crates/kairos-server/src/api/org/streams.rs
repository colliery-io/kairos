//! `/api/delivery-streams` (KAIROS-S-0005 Delivery Streams family,
//! KAIROS-T-0019): stream CRUD and team membership. Writes are
//! org-admin-only (tenant-wide org structure, the A-0006 tenant-config
//! fallback); reads are open tenant-wide.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_org as dto;
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::models::teams::{
    DeliveryStream, DeliveryStreamChangeset, NewDeliveryStream, Team, TeamDeliveryStream,
};
use uuid::Uuid;

use super::super::convert::IntoDto;
use super::super::convert_org::team_to_dto;
use super::super::{clamp_pagination, parse_uuid, require_capability};
use super::is_unique_violation;
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The pseudo-capability named in 403s for these org-admin-only writes.
const MANAGE: &str = "manage_streams";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/delivery-streams",
            get(list_streams).post(create_stream),
        )
        .route(
            "/api/delivery-streams/{id}",
            get(get_stream).patch(update_stream).delete(delete_stream),
        )
        .route(
            "/api/delivery-streams/{id}/teams",
            get(list_stream_teams).post(add_stream_team),
        )
        .route(
            "/api/delivery-streams/{id}/teams/{team_id}",
            axum::routing::delete(remove_stream_team),
        )
}

/// Load the live stream with this id, or 404.
fn load_stream(conn: &mut PgConnection, stream_id: Uuid) -> Result<DeliveryStream, ApiError> {
    use kairos_db::schema::delivery_streams::dsl;
    dsl::delivery_streams
        .filter(dsl::id.eq(stream_id))
        .filter(dsl::deleted_at.is_null())
        .select(DeliveryStream::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("no live delivery stream {stream_id}")))
}

/// Insert one `activity_log` row for a stream mutation.
fn log_stream_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    stream_id: Uuid,
    details: String,
) -> Result<(), ApiError> {
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(stream_id),
            entity_type: Some("delivery_stream".to_string()),
            details,
        })
        .execute(conn)
        .map_err(ApiError::internal)?;
    Ok(())
}

/// List delivery streams (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/delivery-streams",
    tag = "delivery-streams",
    params(Pagination),
    responses(
        (status = 200, description = "Page of streams", body = ListEnvelope<dto::DeliveryStream>),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_streams(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<dto::DeliveryStream>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::delivery_streams::dsl;
            let total: i64 = dsl::delivery_streams
                .filter(dsl::deleted_at.is_null())
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<DeliveryStream> = dsl::delivery_streams
                .filter(dsl::deleted_at.is_null())
                .order(dsl::slug.asc())
                .limit(limit)
                .offset(offset)
                .select(DeliveryStream::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            Ok(ListEnvelope {
                items: rows.into_iter().map(IntoDto::into_dto).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Get one delivery stream (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/delivery-streams/{id}",
    tag = "delivery-streams",
    params(("id" = String, Path, description = "Stream id (UUID)")),
    responses(
        (status = 200, description = "The stream", body = dto::DeliveryStream),
        (status = 404, description = "Unknown stream", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_stream(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::DeliveryStream>, ApiError> {
    let stream_id = parse_uuid(&id, "id")?;
    let stream = state
        .blocking
        .run(&tenant.slug, move |conn| {
            Ok(load_stream(conn, stream_id)?.into_dto())
        })
        .await?;
    Ok(Json(stream))
}

/// Create a delivery stream. Org-admin-only.
#[utoipa::path(
    post,
    path = "/api/delivery-streams",
    tag = "delivery-streams",
    request_body = dto::CreateStreamRequest,
    responses(
        (status = 201, description = "Created", body = dto::DeliveryStream),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug already in use", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_stream(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateStreamRequest>,
) -> Result<(StatusCode, Json<dto::DeliveryStream>), ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let stream = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::delivery_streams;
            require_capability(conn, &slug, None, user, MANAGE)?;
            let created: DeliveryStream = diesel::insert_into(delivery_streams::table)
                .values(NewDeliveryStream {
                    name: body.name.clone(),
                    slug: body.slug.clone(),
                    description: body.description.clone(),
                })
                .returning(DeliveryStream::as_returning())
                .get_result(conn)
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        ApiError::conflict(format!(
                            "a delivery stream with slug {:?} already exists",
                            body.slug
                        ))
                    } else {
                        ApiError::internal(e)
                    }
                })?;
            log_stream_activity(
                conn,
                user,
                ActivityAction::Create,
                created.id,
                format!("delivery_stream:{}", created.slug),
            )?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(stream)))
}

/// Update a delivery stream. Org-admin-only.
#[utoipa::path(
    patch,
    path = "/api/delivery-streams/{id}",
    tag = "delivery-streams",
    params(("id" = String, Path, description = "Stream id (UUID)")),
    request_body = dto::UpdateStreamRequest,
    responses(
        (status = 200, description = "Updated", body = dto::DeliveryStream),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown stream", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Slug already in use", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Empty body", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_stream(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::UpdateStreamRequest>,
) -> Result<Json<dto::DeliveryStream>, ApiError> {
    let stream_id = parse_uuid(&id, "id")?;
    if body.name.is_none() && body.slug.is_none() && body.description.is_none() {
        return Err(ApiError::validation(
            "at least one of name, slug, description is required",
        ));
    }
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let stream = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::delivery_streams::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            load_stream(conn, stream_id)?;
            let updated: DeliveryStream =
                diesel::update(dsl::delivery_streams.filter(dsl::id.eq(stream_id)))
                    .set((
                        DeliveryStreamChangeset {
                            name: body.name.clone(),
                            slug: body.slug.clone(),
                            description: body.description.clone().map(Some),
                            ..Default::default()
                        },
                        dsl::updated_at.eq(diesel::dsl::now),
                    ))
                    .returning(DeliveryStream::as_returning())
                    .get_result(conn)
                    .map_err(|e| {
                        if is_unique_violation(&e) {
                            ApiError::conflict("a delivery stream with that slug already exists")
                        } else {
                            ApiError::internal(e)
                        }
                    })?;
            log_stream_activity(
                conn,
                user,
                ActivityAction::Create,
                stream_id,
                format!("delivery_stream_settings:{}", updated.slug),
            )?;
            Ok(updated.into_dto())
        })
        .await?;
    Ok(Json(stream))
}

/// Soft-delete a delivery stream (team links are left in place — the
/// stream is recoverable). Org-admin-only.
#[utoipa::path(
    delete,
    path = "/api/delivery-streams/{id}",
    tag = "delivery-streams",
    params(("id" = String, Path, description = "Stream id (UUID)")),
    responses(
        (status = 200, description = "Soft-deleted", body = dto::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown stream", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_stream(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let stream_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::delivery_streams::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            let stream = load_stream(conn, stream_id)?;
            diesel::update(dsl::delivery_streams.filter(dsl::id.eq(stream_id)))
                .set((
                    dsl::deleted_at.eq(diesel::dsl::now),
                    dsl::updated_at.eq(diesel::dsl::now),
                ))
                .execute(conn)
                .map_err(ApiError::internal)?;
            log_stream_activity(
                conn,
                user,
                ActivityAction::Delete,
                stream_id,
                format!("delivery_stream:{}", stream.slug),
            )?;
            Ok(dto::OrgDeleteResponse {
                id: stream_id.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}

// ---------------------------------------------------------------------------
// Stream team membership
// ---------------------------------------------------------------------------

/// Teams in this stream (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/delivery-streams/{id}/teams",
    tag = "delivery-streams",
    params(("id" = String, Path, description = "Stream id (UUID)")),
    responses(
        (status = 200, description = "Teams in the stream", body = [dto::Team]),
        (status = 404, description = "Unknown stream", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_stream_teams(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::Team>>, ApiError> {
    let stream_id = parse_uuid(&id, "id")?;
    let teams = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::{boards, team_delivery_streams, teams};
            load_stream(conn, stream_id)?;
            let rows: Vec<Team> = team_delivery_streams::table
                .inner_join(teams::table)
                .filter(team_delivery_streams::delivery_stream_id.eq(stream_id))
                .filter(teams::deleted_at.is_null())
                .order(teams::slug.asc())
                .select(Team::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let team_ids: Vec<Uuid> = rows.iter().map(|t| t.id).collect();
            let delivery_boards: Vec<(Option<Uuid>, Uuid)> = boards::table
                .filter(boards::team_id.eq_any(&team_ids))
                .filter(boards::deleted_at.is_null())
                .select((boards::team_id, boards::id))
                .load(conn)
                .map_err(ApiError::internal)?;
            Ok(rows
                .into_iter()
                .map(|team| {
                    let board = delivery_boards
                        .iter()
                        .find(|(team_id, _)| *team_id == Some(team.id))
                        .map(|(_, board_id)| *board_id);
                    team_to_dto(team, board)
                })
                .collect())
        })
        .await?;
    Ok(Json(teams))
}

/// Add a team to a stream. Org-admin-only.
#[utoipa::path(
    post,
    path = "/api/delivery-streams/{id}/teams",
    tag = "delivery-streams",
    params(("id" = String, Path, description = "Stream id (UUID)")),
    request_body = dto::AddStreamTeamRequest,
    responses(
        (status = 201, description = "Added", body = dto::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown stream", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Team already in the stream", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn add_stream_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::AddStreamTeamRequest>,
) -> Result<(StatusCode, Json<dto::OrgDeleteResponse>), ApiError> {
    let stream_id = parse_uuid(&id, "id")?;
    let team_id = parse_uuid(&body.team_id, "team_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::team_delivery_streams::dsl;
            use kairos_db::schema::teams;
            require_capability(conn, &slug, None, user, MANAGE)?;
            load_stream(conn, stream_id)?;
            let team_exists: Option<Uuid> = teams::table
                .filter(teams::id.eq(team_id))
                .filter(teams::deleted_at.is_null())
                .select(teams::id)
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?;
            if team_exists.is_none() {
                return Err(ApiError::validation(format!(
                    "team {team_id} does not exist"
                )));
            }
            diesel::insert_into(dsl::team_delivery_streams)
                .values(TeamDeliveryStream {
                    team_id,
                    delivery_stream_id: stream_id,
                })
                .execute(conn)
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        ApiError::conflict(format!(
                            "team {team_id} is already in stream {stream_id}"
                        ))
                    } else {
                        ApiError::internal(e)
                    }
                })?;
            log_stream_activity(
                conn,
                user,
                ActivityAction::Create,
                stream_id,
                format!("stream_team_add:{team_id}"),
            )?;
            Ok(dto::OrgDeleteResponse {
                id: team_id.to_string(),
                deleted: false,
            })
        })
        .await?;
    Ok((StatusCode::CREATED, Json(outcome)))
}

/// Remove a team from a stream. Org-admin-only.
#[utoipa::path(
    delete,
    path = "/api/delivery-streams/{id}/teams/{team_id}",
    tag = "delivery-streams",
    params(
        ("id" = String, Path, description = "Stream id (UUID)"),
        ("team_id" = String, Path, description = "Team id (UUID)"),
    ),
    responses(
        (status = 200, description = "Removed", body = dto::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown stream or team not in stream", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_stream_team(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, team_id)): Path<(String, String)>,
) -> Result<Json<dto::OrgDeleteResponse>, ApiError> {
    let stream_id = parse_uuid(&id, "id")?;
    let team_id = parse_uuid(&team_id, "team_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::team_delivery_streams::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            load_stream(conn, stream_id)?;
            let deleted = diesel::delete(
                dsl::team_delivery_streams
                    .filter(dsl::delivery_stream_id.eq(stream_id))
                    .filter(dsl::team_id.eq(team_id)),
            )
            .execute(conn)
            .map_err(ApiError::internal)?;
            if deleted == 0 {
                return Err(ApiError::not_found(format!(
                    "team {team_id} is not in stream {stream_id}"
                )));
            }
            log_stream_activity(
                conn,
                user,
                ActivityAction::Delete,
                stream_id,
                format!("stream_team_remove:{team_id}"),
            )?;
            Ok(dto::OrgDeleteResponse {
                id: team_id.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}
