//! `/api/teams/{id}/pages` + `/api/teams/{id}/announcements`
//! (KAIROS-T-0083, design in KAIROS-I-0007): the team landing-page
//! content store.
//!
//! Permission model — DELIBERATELY not board ABAC: writes require the
//! caller to be a MEMBER of the team (or org admin); reads are open
//! tenant-wide, matching the rest of the org surface. Announcements are
//! append-only: there is no edit route anywhere (the "no comments, one
//! way" decision).

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types_team_pages as dto;
use kairos_db::models::enums::TeamPageKind;
use kairos_db::models::team_pages::{NewTeamAnnouncement, TeamAnnouncement, TeamPage};
use kairos_db::team_pages::{self, CreatePage, TeamPageError};
use serde_json::json;
use uuid::Uuid;

use super::super::{parse_enum, parse_opt_uuid, parse_uuid};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/teams/{id}/pages",
            get(list_pages).post(create_page),
        )
        .route(
            "/api/teams/{id}/pages/{page_id}",
            get(get_page).patch(update_page).delete(delete_page),
        )
        .route(
            "/api/teams/{id}/announcements",
            get(list_announcements).post(create_announcement),
        )
        .route(
            "/api/teams/{id}/announcements/{announcement_id}",
            axum::routing::delete(delete_announcement),
        )
}

/// 404 unless a live team with this id exists.
fn require_team(conn: &mut PgConnection, team_id: Uuid) -> Result<(), ApiError> {
    use kairos_db::schema::teams::dsl;
    let exists: Option<Uuid> = dsl::teams
        .filter(dsl::id.eq(team_id))
        .filter(dsl::deleted_at.is_null())
        .select(dsl::id)
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?;
    exists
        .map(|_| ())
        .ok_or_else(|| ApiError::not_found(format!("no team {team_id} exists")))
}

/// Team-page writes require team membership or org admin (the
/// KAIROS-I-0007 permission model — NOT board ABAC).
fn require_team_member_or_admin(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    team_id: Uuid,
    user: Uuid,
) -> Result<(), ApiError> {
    if tenant.role == kairos_db::models::enums::OrgRole::Admin {
        return Ok(());
    }
    use kairos_db::schema::team_members::dsl;
    let member: Option<Uuid> = dsl::team_members
        .filter(dsl::team_id.eq(team_id))
        .filter(dsl::user_id.eq(user))
        .select(dsl::user_id)
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?;
    member.map(|_| ()).ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "editing this team's pages requires team membership (or org admin)",
        )
    })
}

/// Activity row for a page/announcement lifecycle event. Content edits
/// are NOT logged here — `team_page_history` is their record (the A-0004
/// pattern items.rs follows; decision recorded on KAIROS-T-0083).
fn log_page_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: kairos_db::models::enums::ActivityAction,
    entity_type: &str,
    entity_id: Uuid,
    details: String,
) -> Result<(), ApiError> {
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(kairos_db::models::NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(entity_id),
            entity_type: Some(entity_type.to_string()),
            details,
        })
        .execute(conn)
        .map_err(ApiError::internal)?;
    Ok(())
}

/// [`TeamPageError`] → HTTP: version conflicts carry the current page in
/// `details.current` (the A-0004 merge-UI contract); structure errors map
/// to 422 with reason codes; content overflow names the limit.
fn map_page_error(e: TeamPageError) -> ApiError {
    match e {
        TeamPageError::PageNotFound(id) => {
            ApiError::not_found(format!("no live team page {id} exists"))
        }
        TeamPageError::SlugConflict(slug) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "SLUG_CONFLICT",
            format!("a sibling with slug {slug:?} already exists"),
        ),
        TeamPageError::BadParent(id) => ApiError::validation(format!(
            "parent {id} is not a live folder of this team"
        )),
        TeamPageError::ProtectedPage(id) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "PROTECTED_PAGE",
            format!("page {id} is protected (the Charter): it cannot be renamed, moved, or deleted"),
        ),
        TeamPageError::FolderNotEmpty { folder, children } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "FOLDER_NOT_EMPTY",
            format!("folder {folder} still contains {children} live page(s); move or delete them first"),
        ),
        TeamPageError::VersionConflict {
            expected_version,
            current_version,
            current_title,
            current_content,
            ..
        } => ApiError::conflict(format!(
            "version mismatch: expected {expected_version}, current is {current_version}"
        ))
        .with_details(json!({
            "current": {
                "version": current_version,
                "title": current_title,
                "content": current_content,
            }
        })),
        TeamPageError::ContentTooLarge { actual } => ApiError::validation(format!(
            "content is {actual} bytes; the limit is {} bytes",
            team_pages::MAX_CONTENT_BYTES
        )),
        TeamPageError::Database(e) => ApiError::internal(e),
    }
}

fn page_dto(page: TeamPage) -> dto::TeamPage {
    dto::TeamPage {
        id: page.id.to_string(),
        team_id: page.team_id.to_string(),
        parent_id: page.parent_id.map(|id| id.to_string()),
        kind: page.kind.to_string(),
        slug: page.slug,
        title: page.title,
        content: page.content,
        position: page.position,
        is_protected: page.is_protected,
        version: page.version,
        created_at: page.created_at.to_rfc3339(),
        updated_at: page.updated_at.to_rfc3339(),
    }
}

fn announcement_dto(row: TeamAnnouncement) -> dto::TeamAnnouncement {
    dto::TeamAnnouncement {
        id: row.id.to_string(),
        team_id: row.team_id.to_string(),
        body: row.body,
        pinned: row.pinned,
        created_by: row.created_by.to_string(),
        created_at: row.created_at.to_rfc3339(),
    }
}

/// The team's live page tree as a flat list (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/pages",
    tag = "team-pages",
    params(("id" = String, Path, description = "Team id (UUID)")),
    responses(
        (status = 200, description = "The page tree, flat", body = Vec<dto::TeamPage>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_pages(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::TeamPage>>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let pages = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_team(conn, team_id)?;
            let pages = team_pages::list_pages(conn, team_id).map_err(ApiError::internal)?;
            Ok(pages.into_iter().map(page_dto).collect::<Vec<_>>())
        })
        .await?;
    Ok(Json(pages))
}

/// One live page (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/pages/{page_id}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "Team id (UUID)"),
        ("page_id" = String, Path, description = "Page id (UUID)"),
    ),
    responses(
        (status = 200, description = "The page", body = dto::TeamPage),
        (status = 404, description = "Unknown team or page", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_page(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, page_id)): Path<(String, String)>,
) -> Result<Json<dto::TeamPage>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let page_id = parse_uuid(&page_id, "page_id")?;
    let page = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_team(conn, team_id)?;
            let page = team_pages::load_page(conn, team_id, page_id).map_err(map_page_error)?;
            Ok(page_dto(page))
        })
        .await?;
    Ok(Json(page))
}

/// Create a page or folder (team member or org admin).
#[utoipa::path(
    post,
    path = "/api/teams/{id}/pages",
    tag = "team-pages",
    params(("id" = String, Path, description = "Team id (UUID)")),
    request_body = dto::CreateTeamPageRequest,
    responses(
        (status = 201, description = "Created", body = dto::TeamPage),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad kind/parent, sibling slug conflict, or content too large", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_page(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::CreateTeamPageRequest>,
) -> Result<(StatusCode, Json<dto::TeamPage>), ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let kind: TeamPageKind = parse_enum(&body.kind, "kind", TeamPageKind::ALL)?;
    let parent_id = parse_opt_uuid(body.parent_id.as_deref(), "parent_id")?;
        let slug = tenant.slug.clone();
let user = auth.user_id;
    let created = state
        .blocking
        .run(&slug, move |conn| {
            require_team(conn, team_id)?;
            require_team_member_or_admin(conn, &tenant, team_id, user)?;
            let created = team_pages::create_page(
                conn,
                team_id,
                CreatePage {
                    parent_id,
                    kind,
                    slug: &body.slug,
                    title: &body.title,
                    content: &body.content,
                    position: body.position,
                },
                user,
            )
            .map_err(map_page_error)?;
            log_page_activity(
                conn,
                user,
                kairos_db::models::enums::ActivityAction::Create,
                "team_page",
                created.id,
                format!("team:{team_id} kind:{} slug:{}", created.kind, created.slug),
            )?;
            Ok(page_dto(created))
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Edit a page: EITHER a version-checked content save OR a rename/move —
/// not both in one call.
#[utoipa::path(
    patch,
    path = "/api/teams/{id}/pages/{page_id}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "Team id (UUID)"),
        ("page_id" = String, Path, description = "Page id (UUID)"),
    ),
    request_body = dto::UpdateTeamPageRequest,
    responses(
        (status = 200, description = "Updated", body = dto::TeamPage),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or page", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current page", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Mixed edit kinds, protected page, slug conflict, or bad parent", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_page(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, page_id)): Path<(String, String)>,
    Json(body): Json<dto::UpdateTeamPageRequest>,
) -> Result<Json<dto::TeamPage>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let page_id = parse_uuid(&page_id, "page_id")?;
    let is_content_edit = body.content.is_some();
    let is_structure_edit = body.slug.is_some() || body.parent_id.is_some() || body.move_to_root || body.position.is_some();
    if is_content_edit && is_structure_edit {
        return Err(ApiError::validation(
            "a content edit (content/version) and a rename/move (slug/parent_id/position) \
             cannot be combined in one call",
        ));
    }
    if !is_content_edit && !is_structure_edit {
        return Err(ApiError::validation(
            "nothing to do: provide content (+version) or slug/parent_id/move_to_root/position",
        ));
    }
    if is_content_edit && body.version.is_none() {
        return Err(ApiError::validation(
            "content edits are version-checked: version is required",
        ));
    }
    let new_parent = if body.move_to_root {
        Some(None)
    } else {
        parse_opt_uuid(body.parent_id.as_deref(), "parent_id")?.map(Some)
    };
        let slug = tenant.slug.clone();
let user = auth.user_id;
    let updated = state
        .blocking
        .run(&slug, move |conn| {
            require_team(conn, team_id)?;
            require_team_member_or_admin(conn, &tenant, team_id, user)?;
            let updated = if let Some(content) = &body.content {
                team_pages::update_page_content(
                    conn,
                    team_id,
                    page_id,
                    body.title.as_deref(),
                    content,
                    body.version.expect("checked above"),
                    user,
                )
                .map_err(map_page_error)?
            } else {
                team_pages::rename_move_page(
                    conn,
                    team_id,
                    page_id,
                    body.slug.as_deref(),
                    new_parent,
                    body.position,
                    user,
                )
                .map_err(map_page_error)?
            };
            Ok(page_dto(updated))
        })
        .await?;
    Ok(Json(updated))
}

/// Soft-delete a page (folders must be empty; the Charter never).
#[utoipa::path(
    delete,
    path = "/api/teams/{id}/pages/{page_id}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "Team id (UUID)"),
        ("page_id" = String, Path, description = "Page id (UUID)"),
    ),
    responses(
        (status = 200, description = "Deleted", body = kairos_client::types_org::OrgDeleteResponse),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or page", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Protected page or non-empty folder", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_page(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, page_id)): Path<(String, String)>,
) -> Result<Json<kairos_client::types_org::OrgDeleteResponse>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let page_uuid = parse_uuid(&page_id, "page_id")?;
        let slug = tenant.slug.clone();
let user = auth.user_id;
    state
        .blocking
        .run(&slug, move |conn| {
            require_team(conn, team_id)?;
            require_team_member_or_admin(conn, &tenant, team_id, user)?;
            let doomed = team_pages::load_page(conn, team_id, page_uuid).map_err(map_page_error)?;
            team_pages::soft_delete_page(conn, team_id, page_uuid, user)
                .map_err(map_page_error)?;
            log_page_activity(
                conn,
                user,
                kairos_db::models::enums::ActivityAction::Delete,
                "team_page",
                page_uuid,
                format!("team:{team_id} slug:{}", doomed.slug),
            )
        })
        .await?;
    Ok(Json(kairos_client::types_org::OrgDeleteResponse {
        id: page_id,
        deleted: true,
    }))
}

/// The team's announcements, pinned first then newest (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/announcements",
    tag = "team-pages",
    params(("id" = String, Path, description = "Team id (UUID)")),
    responses(
        (status = 200, description = "Announcements", body = Vec<dto::TeamAnnouncement>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_announcements(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::TeamAnnouncement>>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_team(conn, team_id)?;
            let rows =
                team_pages::list_announcements(conn, team_id).map_err(ApiError::internal)?;
            Ok(rows.into_iter().map(announcement_dto).collect::<Vec<_>>())
        })
        .await?;
    Ok(Json(rows))
}

/// Post an announcement (team member or org admin; append-only — there
/// is no edit route).
#[utoipa::path(
    post,
    path = "/api/teams/{id}/announcements",
    tag = "team-pages",
    params(("id" = String, Path, description = "Team id (UUID)")),
    request_body = dto::CreateTeamAnnouncementRequest,
    responses(
        (status = 201, description = "Posted", body = dto::TeamAnnouncement),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Body too large", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_announcement(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::CreateTeamAnnouncementRequest>,
) -> Result<(StatusCode, Json<dto::TeamAnnouncement>), ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    if body.body.len() > team_pages::MAX_CONTENT_BYTES {
        return Err(ApiError::validation(format!(
            "body is {} bytes; the limit is {} bytes",
            body.body.len(),
            team_pages::MAX_CONTENT_BYTES
        )));
    }
        let slug = tenant.slug.clone();
let user = auth.user_id;
    let created = state
        .blocking
        .run(&slug, move |conn| {
            require_team(conn, team_id)?;
            require_team_member_or_admin(conn, &tenant, team_id, user)?;
            let created: TeamAnnouncement =
                diesel::insert_into(kairos_db::schema::team_announcements::table)
                    .values(NewTeamAnnouncement {
                        team_id,
                        body: body.body.clone(),
                        pinned: body.pinned,
                        created_by: user,
                    })
                    .returning(TeamAnnouncement::as_returning())
                    .get_result(conn)
                    .map_err(ApiError::internal)?;
            log_page_activity(
                conn,
                user,
                kairos_db::models::enums::ActivityAction::Create,
                "team_announcement",
                created.id,
                format!("team:{team_id} pinned:{}", created.pinned),
            )?;
            Ok(announcement_dto(created))
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Delete an announcement (its author or an org admin).
#[utoipa::path(
    delete,
    path = "/api/teams/{id}/announcements/{announcement_id}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "Team id (UUID)"),
        ("announcement_id" = String, Path, description = "Announcement id (UUID)"),
    ),
    responses(
        (status = 200, description = "Deleted", body = kairos_client::types_org::OrgDeleteResponse),
        (status = 403, description = "Not the author (and not org admin)", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or announcement", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_announcement(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, announcement_id)): Path<(String, String)>,
) -> Result<Json<kairos_client::types_org::OrgDeleteResponse>, ApiError> {
    let team_id = parse_uuid(&id, "id")?;
    let announcement_uuid = parse_uuid(&announcement_id, "announcement_id")?;
    let user = auth.user_id;
    let is_admin = tenant.role == kairos_db::models::enums::OrgRole::Admin;
    let slug = tenant.slug.clone();
    state
        .blocking
        .run(&slug, move |conn| {
            use kairos_db::schema::team_announcements::dsl;
            require_team(conn, team_id)?;
            let author: Option<Uuid> = dsl::team_announcements
                .filter(dsl::id.eq(announcement_uuid))
                .filter(dsl::team_id.eq(team_id))
                .select(dsl::created_by)
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?;
            let Some(author) = author else {
                return Err(ApiError::not_found(format!(
                    "no announcement {announcement_uuid} exists on this team"
                )));
            };
            if author != user && !is_admin {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "FORBIDDEN",
                    "announcements can be deleted by their author or an org admin",
                ));
            }
            diesel::delete(dsl::team_announcements.filter(dsl::id.eq(announcement_uuid)))
                .execute(conn)
                .map_err(ApiError::internal)?;
            Ok(())
        })
        .await?;
    Ok(Json(kairos_client::types_org::OrgDeleteResponse {
        id: announcement_id,
        deleted: true,
    }))
}
