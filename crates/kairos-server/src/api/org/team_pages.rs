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
use kairos_db::models::team_pages::{
    NewTeamAnnouncement, TeamAnnouncement, TeamPage, TeamPageMeta,
};
use kairos_db::team_pages::{self, CreatePage, TeamPageError};
use serde_json::json;
use uuid::Uuid;

use super::super::{parse_enum, parse_opt_uuid, parse_uuid};
use super::teams::team_by_ref;
use crate::api::clamp_pagination;
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/pages", get(list_pages).post(create_page))
        .route(
            "/api/teams/{id}/pages/{page_id}",
            get(get_page).patch(update_page).delete(delete_page),
        )
        .route("/api/teams/{id}/archived-pages", get(list_archived))
        .route(
            "/api/teams/{id}/pages/{page_id}/restore",
            axum::routing::post(restore_page),
        )
        .route("/api/teams/{id}/pages/{page_id}/history", get(page_history))
        .route("/api/teams/{id}/tree-items", get(list_tree_items))
        .route(
            "/api/teams/{id}/tree-items/{item}",
            axum::routing::put(place_tree_item),
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
    exists.map(|_| ()).ok_or_else(|| {
        ApiError::not_found(format!("The team {team_id} does not exist."))
            .with_details(json!({ "field": "team" }))
    })
}

/// KAIROS-T-0536: a team page and an announcement are documents of the
/// team. The scope of an agent key must have `manage_documents` for a
/// write. A team is not a board, so a scope with a list of boards refuses
/// it.
fn require_page_scope() -> Result<(), ApiError> {
    match ApiError::outside_key_scope(kairos_core::abac::MANAGE_DOCUMENTS, None) {
        Some(refusal) => Err(refusal),
        None => Ok(()),
    }
}

/// Team-page writes require team membership or org admin (the
/// KAIROS-I-0007 permission model — NOT board ABAC).
fn require_team_member_or_admin(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    team_id: Uuid,
    user: Uuid,
) -> Result<(), ApiError> {
    require_page_scope()?;
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
            "Only a member of the team or an organization admin can edit the pages of \
             this team.",
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
            ApiError::not_found(format!("No live team page has the id {id}."))
                .with_details(json!({ "field": "page" }))
        }
        TeamPageError::SlugConflict(slug) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "SLUG_CONFLICT",
            format!("A page in the same folder has the slug {slug:?} already."),
        ),
        e @ (TeamPageError::ReservedSlug(_) | TeamPageError::BadSlug(_)) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "slug" }))
        }
        TeamPageError::BadParent(id) => ApiError::validation(format!(
            "The parent {id} is not a live folder of this team."
        ))
        .with_details(json!({ "field": "parent_id" })),
        TeamPageError::ProtectedPage(id) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "PROTECTED_PAGE",
            format!(
                "The page {id} is the Charter. You cannot change its slug, move it, or \
                 delete it."
            ),
        ),
        e @ TeamPageError::NotTeamItem(_) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "item" }))
        }
        TeamPageError::NotArchived(id) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "NOT_ARCHIVED",
            format!("The team page {id} is not archived. You can restore only an archived page."),
        ),
        TeamPageError::VersionConflict {
            expected_version,
            current_version,
            current_title,
            current_content,
            ..
        } => ApiError::conflict(format!(
            "The request has the version {expected_version}, and the current version is \
             {current_version}. Get the page again, and make the edit on the current \
             version."
        ))
        .with_details(json!({
            "current": {
                "version": current_version,
                "title": current_title,
                "content": current_content,
            }
        })),
        e @ TeamPageError::FolderContent(_) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "content" }))
        }
        TeamPageError::ContentTooLarge { actual } => ApiError::validation(format!(
            "The content has {actual} bytes. The limit is {} bytes.",
            team_pages::MAX_CONTENT_BYTES
        ))
        .with_details(json!({ "field": "content" })),
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

/// A node of the tree without its content (KAIROS-T-0629).
fn summary_dto(page: TeamPageMeta) -> dto::TeamPageSummary {
    dto::TeamPageSummary {
        id: page.id.to_string(),
        team_id: page.team_id.to_string(),
        parent_id: page.parent_id.map(|id| id.to_string()),
        kind: page.kind.to_string(),
        slug: page.slug,
        title: page.title,
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

/// The team's live page tree as a flat list, without content (open
/// tenant-wide). `GET /api/teams/{id}/pages/{page_id}` gives the content
/// of one page (KAIROS-T-0629).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/pages",
    tag = "team-pages",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    responses(
        (status = 200, description = "The page tree, flat, without content", body = Vec<dto::TeamPageSummary>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_pages(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::TeamPageSummary>>, ApiError> {
    let pages = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            list_as(conn, team_id)
        })
        .await?;
    Ok(Json(pages))
}

/// The live page tree of a team, flat, without content, as the REST route
/// gives it. MCP calls this too (KAIROS-T-0434).
pub(crate) fn list_as(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<Vec<dto::TeamPageSummary>, ApiError> {
    require_team(conn, team_id)?;
    let pages = team_pages::list_pages(conn, team_id).map_err(ApiError::internal)?;
    Ok(pages.into_iter().map(summary_dto).collect())
}

/// One live page of a team in full, as `GET /api/teams/{id}/pages/{page_id}`
/// gives it. MCP calls this too.
pub(crate) fn page_as(
    conn: &mut PgConnection,
    team_id: Uuid,
    page_id: Uuid,
) -> Result<dto::TeamPage, ApiError> {
    require_team(conn, team_id)?;
    let page = team_pages::load_page(conn, team_id, page_id).map_err(map_page_error)?;
    Ok(page_dto(page))
}

/// One live page (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/pages/{page_id}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
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
    let page_id = parse_uuid(&page_id, "page_id")?;
    let page = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            page_as(conn, team_id, page_id)
        })
        .await?;
    Ok(Json(page))
}

/// Create a page or folder (team member or org admin).
#[utoipa::path(
    post,
    path = "/api/teams/{id}/pages",
    tag = "team-pages",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team"), ("Idempotency-Key" = Option<String>, Header, description = crate::api::idempotency::HEADER_DOC)),
    request_body = dto::CreateTeamPageRequest,
    responses(
        (status = 201, description = "Created", body = dto::TeamPage),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "REQUEST_KEY_REUSED: the Idempotency-Key was used for a different request", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad kind/parent, sibling slug conflict, or content too large", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_page(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    idempotency: crate::api::idempotency::IdempotencyKey,
    ApiJson(body): ApiJson<dto::CreateTeamPageRequest>,
) -> Result<crate::api::idempotency::Answer<dto::TeamPage>, ApiError> {
    let keyed = idempotency.bind(&body)?;
    let slug = tenant.slug.clone();
    let user = auth.user_id;
    let created = state
        .blocking
        .run(&slug, move |conn| {
            crate::api::idempotency::run(conn, keyed, StatusCode::CREATED, |conn| {
                let team_id = team_by_ref(conn, &id)?;
                create_as(conn, &tenant, user, team_id, &body)
            })
        })
        .await?;
    Ok(created)
}

/// Create a page or a folder, as the REST route does: the gate, the write
/// and the activity row. MCP calls this too (KAIROS-T-0434).
pub(crate) fn create_as(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    team_id: Uuid,
    body: &dto::CreateTeamPageRequest,
) -> Result<dto::TeamPage, ApiError> {
    let kind: TeamPageKind = parse_enum(&body.kind, "kind", TeamPageKind::ALL)?;
    let parent_id = parse_opt_uuid(body.parent_id.as_deref(), "parent_id")?;
    require_team(conn, team_id)?;
    require_team_member_or_admin(conn, tenant, team_id, user)?;
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
}

/// Edit a page: EITHER a version-checked content save OR a rename/move —
/// not both in one call.
#[utoipa::path(
    patch,
    path = "/api/teams/{id}/pages/{page_id}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
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
    ApiJson(body): ApiJson<dto::UpdateTeamPageRequest>,
) -> Result<Json<dto::TeamPage>, ApiError> {
    let page_id = parse_uuid(&page_id, "page_id")?;
    let slug = tenant.slug.clone();
    let user = auth.user_id;
    let updated = state
        .blocking
        .run(&slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            update_as(conn, &tenant, user, team_id, page_id, &body)
        })
        .await?;
    Ok(Json(updated))
}

/// Edit a page, as the REST route does: either a version-checked content
/// save or a rename/move, not both. MCP calls this too (KAIROS-T-0434).
pub(crate) fn update_as(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    team_id: Uuid,
    page_id: Uuid,
    body: &dto::UpdateTeamPageRequest,
) -> Result<dto::TeamPage, ApiError> {
    let is_content_edit = body.content.is_some();
    let is_structure_edit = body.slug.is_some()
        || body.parent_id.is_some()
        || body.move_to_root
        || body.position.is_some();
    if is_content_edit && is_structure_edit {
        return Err(ApiError::validation(
            "One request cannot edit the content and move the page. Send content and \
             version in one request. Send slug, parent_id, move_to_root or position in a \
             different request.",
        )
        .with_details(json!({
            "field": "content",
            "fields": ["content", "slug", "parent_id", "move_to_root", "position"],
        })));
    }
    if !is_content_edit && !is_structure_edit {
        return Err(ApiError::validation(
            "The request has no field to change. Send content and version, or send one or \
             more of slug, parent_id, move_to_root and position.",
        )
        .with_details(json!({
            "field": "content",
            "fields": ["content", "version", "slug", "parent_id", "move_to_root", "position"],
        })));
    }
    if is_content_edit && body.version.is_none() {
        return Err(ApiError::validation(
            "The request has content and no version. An edit of the content must have the \
             version.",
        )
        .with_details(json!({ "field": "version" })));
    }
    let new_parent = if body.move_to_root {
        Some(None)
    } else {
        parse_opt_uuid(body.parent_id.as_deref(), "parent_id")?.map(Some)
    };
    require_team(conn, team_id)?;
    require_team_member_or_admin(conn, tenant, team_id, user)?;
    let before = team_pages::load_page(conn, team_id, page_id).map_err(map_page_error)?;
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
    // KAIROS-T-0268: a real change makes an activity entry. An edit
    // with the stored values writes nothing, so it makes none.
    if updated.updated_at != before.updated_at {
        log_page_activity(
            conn,
            user,
            kairos_db::models::enums::ActivityAction::Update,
            "team_page",
            page_id,
            format!("team:{team_id} slug:{}", updated.slug),
        )?;
    }
    Ok(page_dto(updated))
}

/// Archive a page, or a folder with everything in it (the Charter never).
#[utoipa::path(
    delete,
    path = "/api/teams/{id}/pages/{page_id}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        ("page_id" = String, Path, description = "Page id (UUID)"),
    ),
    responses(
        (status = 200, description = "Deleted", body = kairos_client::types_org::OrgDeleteResponse),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or page", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Protected page", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_page(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, page_id)): Path<(String, String)>,
) -> Result<Json<kairos_client::types_org::OrgDeleteResponse>, ApiError> {
    let page_uuid = parse_uuid(&page_id, "page_id")?;
    let slug = tenant.slug.clone();
    let user = auth.user_id;
    state
        .blocking
        .run(&slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            delete_as(conn, &tenant, user, team_id, page_uuid).map(|_| ())
        })
        .await?;
    Ok(Json(kairos_client::types_org::OrgDeleteResponse {
        id: page_id,
        deleted: true,
    }))
}

/// Archive a page, or a folder with everything in it, as the REST route does.
/// Gives the page as it was. MCP calls this too (KAIROS-T-0434).
pub(crate) fn delete_as(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    team_id: Uuid,
    page_id: Uuid,
) -> Result<dto::TeamPage, ApiError> {
    require_team(conn, team_id)?;
    require_team_member_or_admin(conn, tenant, team_id, user)?;
    let doomed = team_pages::load_page(conn, team_id, page_id).map_err(map_page_error)?;
    let archived =
        team_pages::soft_delete_page(conn, team_id, page_id, user).map_err(map_page_error)?;
    log_page_activity(
        conn,
        user,
        kairos_db::models::enums::ActivityAction::Delete,
        "team_page",
        page_id,
        format!("team:{team_id} slug:{} archived:{archived}", doomed.slug),
    )?;
    Ok(page_dto(doomed))
}

/// The archived nodes of a team, newest archive first (open tenant-wide,
/// KAIROS-T-0439).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/archived-pages",
    tag = "team-pages",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    responses(
        (status = 200, description = "The archived nodes", body = Vec<dto::ArchivedTeamPage>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_archived(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::ArchivedTeamPage>>, ApiError> {
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            list_archived_as(conn, team_id)
        })
        .await?;
    Ok(Json(rows))
}

/// The archived nodes of a team, as the REST route gives them.
pub(crate) fn list_archived_as(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<Vec<dto::ArchivedTeamPage>, ApiError> {
    require_team(conn, team_id)?;
    let rows = team_pages::list_archived_pages(conn, team_id).map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let archived_at = row.deleted_at.map(|at| at.to_rfc3339()).unwrap_or_default();
            dto::ArchivedTeamPage {
                page: summary_dto(row),
                archived_at,
            }
        })
        .collect())
}

/// Restore an archived node and what was archived with it (team member or
/// organization admin, KAIROS-T-0439).
#[utoipa::path(
    post,
    path = "/api/teams/{id}/pages/{page_id}/restore",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        ("page_id" = String, Path, description = "Page id (UUID)"),
    ),
    responses(
        (status = 200, description = "Restored", body = dto::TeamPage),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or page", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Not archived, or a live sibling has the slug", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn restore_page(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, page_id)): Path<(String, String)>,
) -> Result<Json<dto::TeamPage>, ApiError> {
    let page_id = parse_uuid(&page_id, "page_id")?;
    let slug = tenant.slug.clone();
    let user = auth.user_id;
    let restored = state
        .blocking
        .run(&slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            restore_as(conn, &tenant, user, team_id, page_id)
        })
        .await?;
    Ok(Json(restored))
}

/// Restore a node, as the REST route does: the gate, the write and the
/// activity row.
pub(crate) fn restore_as(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    team_id: Uuid,
    page_id: Uuid,
) -> Result<dto::TeamPage, ApiError> {
    require_team(conn, team_id)?;
    require_team_member_or_admin(conn, tenant, team_id, user)?;
    let restored =
        team_pages::restore_page(conn, team_id, page_id, user).map_err(map_page_error)?;
    log_page_activity(
        conn,
        user,
        kairos_db::models::enums::ActivityAction::Restore,
        "team_page",
        page_id,
        format!("team:{team_id} slug:{}", restored.slug),
    )?;
    Ok(page_dto(restored))
}

/// The saved versions of a page, newest first, without content, or one
/// version in full with `?version=N` (open tenant-wide, KAIROS-T-0439,
/// KAIROS-T-0629).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/pages/{page_id}/history",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        ("page_id" = String, Path, description = "Page id (UUID)"),
        dto::TeamPageHistoryQuery,
    ),
    responses(
        (status = 200, description = "One page of the versions, newest first, without content (or a TeamPageVersion with ?version=N)", body = kairos_client::types::ListEnvelope<dto::TeamPageVersionSummary>),
        (status = 404, description = "Unknown team, page, or version", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn page_history(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, page_id)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<dto::TeamPageHistoryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let page_id = parse_uuid(&page_id, "page_id")?;
    let (limit, offset) = clamp_pagination(&kairos_client::types::Pagination {
        limit: query.limit,
        offset: query.offset,
    });
    let answer = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            match query.version {
                Some(version) => serde_json::to_value(version_as(conn, team_id, page_id, version)?),
                None => serde_json::to_value(history_as(conn, team_id, page_id, limit, offset)?),
            }
            .map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(answer))
}

/// The display names of the editors `ids`.
fn editor_names(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, String>, ApiError> {
    use kairos_db::schema::users::dsl;
    Ok(dsl::users
        .filter(dsl::id.eq_any(ids))
        .select((dsl::id, dsl::display_name))
        .load::<(Uuid, String)>(conn)
        .map_err(ApiError::internal)?
        .into_iter()
        .collect())
}

/// One page of the saved versions of a page, without content, with the
/// name of each editor.
pub(crate) fn history_as(
    conn: &mut PgConnection,
    team_id: Uuid,
    page_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<kairos_client::types::ListEnvelope<dto::TeamPageVersionSummary>, ApiError> {
    require_team(conn, team_id)?;
    let (total, rows) =
        team_pages::page_history(conn, team_id, page_id, limit, offset).map_err(map_page_error)?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.edited_by).collect();
    let names = editor_names(conn, &ids)?;
    let items = rows
        .into_iter()
        .map(|row| dto::TeamPageVersionSummary {
            version: row.version,
            title: row.title,
            edited_by: row.edited_by.to_string(),
            edited_by_name: names
                .get(&row.edited_by)
                .cloned()
                .unwrap_or_else(|| row.edited_by.to_string()),
            edited_at: row.edited_at.to_rfc3339(),
            agent_key_id: row.agent_key_id.map(|key| key.to_string()),
        })
        .collect();
    Ok(kairos_client::types::ListEnvelope {
        items,
        total,
        limit,
        offset,
    })
}

/// One saved version of a page in full, with the name of its editor.
pub(crate) fn version_as(
    conn: &mut PgConnection,
    team_id: Uuid,
    page_id: Uuid,
    version: i32,
) -> Result<dto::TeamPageVersion, ApiError> {
    require_team(conn, team_id)?;
    let row = team_pages::page_version(conn, team_id, page_id, version)
        .map_err(map_page_error)?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "The team page {page_id} has no version {version} in its history."
            ))
            .with_details(json!({ "field": "version" }))
        })?;
    let names = editor_names(conn, &[row.edited_by])?;
    Ok(dto::TeamPageVersion {
        version: row.version,
        title: row.title,
        content: row.content,
        edited_by: row.edited_by.to_string(),
        edited_by_name: names
            .get(&row.edited_by)
            .cloned()
            .unwrap_or_else(|| row.edited_by.to_string()),
        edited_at: row.edited_at.to_rfc3339(),
        agent_key_id: row.agent_key_id.map(|key| key.to_string()),
    })
}

/// The documents and ADRs of a team with their places in its tree (open
/// tenant-wide, KAIROS-T-0437).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/tree-items",
    tag = "team-pages",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    responses(
        (status = 200, description = "The items of the team", body = Vec<dto::TeamTreeItem>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_tree_items(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<dto::TeamTreeItem>>, ApiError> {
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            tree_items_as(conn, team_id)
        })
        .await?;
    Ok(Json(rows))
}

/// The documents and ADRs of a team with their places, as the REST route
/// gives them. MCP calls this too.
pub(crate) fn tree_items_as(
    conn: &mut PgConnection,
    team_id: Uuid,
) -> Result<Vec<dto::TeamTreeItem>, ApiError> {
    require_team(conn, team_id)?;
    let rows = team_pages::team_tree_items(conn, team_id).map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|row| dto::TeamTreeItem {
            item_id: row.item_id.to_string(),
            short_code: row.short_code,
            entity_type: row.entity_type,
            title: row.title,
            status: row.status,
            parent_id: row.parent_id.map(|id| id.to_string()),
            position: row.position,
        })
        .collect())
}

/// Put a document or an ADR of the team in a folder of its tree
/// (KAIROS-T-0437). A team member or an organization admin can do this.
/// `item` is a short code or a UUID.
#[utoipa::path(
    put,
    path = "/api/teams/{id}/tree-items/{item}",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        ("item" = String, Path, description = "The short code or the id of a document or an ADR of the team"),
    ),
    request_body = dto::PlaceTeamItemRequest,
    responses(
        (status = 200, description = "Placed", body = dto::TeamTreeItem),
        (status = 403, description = "Not a team member", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Not an item of the team, or a parent that is not a live folder", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn place_tree_item(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, item)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::PlaceTeamItemRequest>,
) -> Result<Json<dto::TeamTreeItem>, ApiError> {
    let slug = tenant.slug.clone();
    let user = auth.user_id;
    let placed = state
        .blocking
        .run(&slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            place_item_as(conn, &tenant, user, team_id, &item, &body)
        })
        .await?;
    Ok(Json(placed))
}

/// Place an item, as the REST route does. MCP calls this too.
pub(crate) fn place_item_as(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    team_id: Uuid,
    item: &str,
    body: &dto::PlaceTeamItemRequest,
) -> Result<dto::TeamTreeItem, ApiError> {
    let parent_id = parse_opt_uuid(body.parent_id.as_deref(), "parent_id")?;
    require_team(conn, team_id)?;
    require_team_member_or_admin(conn, tenant, team_id, user)?;
    let items = tree_items_as(conn, team_id)?;
    let found = items
        .iter()
        .find(|row| row.short_code.eq_ignore_ascii_case(item) || row.item_id == item)
        .ok_or_else(|| {
            ApiError::validation(format!(
                "The item {item:?} is not a document or an ADR of this team. Send the short code \
                 or the id of a document or an ADR that a board of the team owns."
            ))
            .with_details(json!({ "field": "item" }))
        })?;
    let item_id = parse_uuid(&found.item_id, "item")?;
    team_pages::place_item(conn, team_id, item_id, parent_id, body.position, user)
        .map_err(map_page_error)?;
    log_page_activity(
        conn,
        user,
        kairos_db::models::enums::ActivityAction::Update,
        "team_page_placement",
        item_id,
        format!(
            "team:{team_id} item:{} position:{}",
            found.short_code, body.position
        ),
    )?;
    tree_items_as(conn, team_id)?
        .into_iter()
        .find(|row| row.item_id == found.item_id)
        .ok_or_else(|| ApiError::internal("the placed item is gone"))
}

/// One page of the team's announcements, pinned first then newest (open
/// tenant-wide; KAIROS-T-0629: limit and offset).
#[utoipa::path(
    get,
    path = "/api/teams/{id}/announcements",
    tag = "team-pages",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        kairos_client::types::Pagination,
    ),
    responses(
        (status = 200, description = "One page of the announcements", body = kairos_client::types::ListEnvelope<dto::TeamAnnouncement>),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_announcements(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<kairos_client::types::Pagination>,
) -> Result<Json<kairos_client::types::ListEnvelope<dto::TeamAnnouncement>>, ApiError> {
    let (limit, offset) = clamp_pagination(&query);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
            require_team(conn, team_id)?;
            let (total, rows) = team_pages::list_announcements(conn, team_id, limit, offset)
                .map_err(ApiError::internal)?;
            Ok(kairos_client::types::ListEnvelope {
                items: rows.into_iter().map(announcement_dto).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Post an announcement (team member or org admin; append-only — there
/// is no edit route).
#[utoipa::path(
    post,
    path = "/api/teams/{id}/announcements",
    tag = "team-pages",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
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
    ApiJson(body): ApiJson<dto::CreateTeamAnnouncementRequest>,
) -> Result<(StatusCode, Json<dto::TeamAnnouncement>), ApiError> {
    if body.body.len() > team_pages::MAX_CONTENT_BYTES {
        return Err(ApiError::validation(format!(
            "The body has {} bytes. The limit is {} bytes.",
            body.body.len(),
            team_pages::MAX_CONTENT_BYTES
        ))
        .with_details(json!({ "field": "body" })));
    }
    let slug = tenant.slug.clone();
    let user = auth.user_id;
    let created = state
        .blocking
        .run(&slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
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
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
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
    let announcement_uuid = parse_uuid(&announcement_id, "announcement_id")?;
    let user = auth.user_id;
    require_page_scope()?;
    let is_admin = tenant.role == kairos_db::models::enums::OrgRole::Admin;
    let slug = tenant.slug.clone();
    state
        .blocking
        .run(&slug, move |conn| {
            let team_id = team_by_ref(conn, &id)?;
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
                    "This team has no announcement {announcement_uuid}."
                ))
                .with_details(json!({ "field": "announcement" })));
            };
            if author != user && !is_admin {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "FORBIDDEN",
                    "Only the author of an announcement or an organization admin can \
                     delete it.",
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
