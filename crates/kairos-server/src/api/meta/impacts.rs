//! `/api/{entity_type}/{short_code}/impacts` (COLLIERY-T-0269,
//! COLLIERY-I-0019): the relationship `impacts`, from a document or an ADR
//! to a repository.
//!
//! The owner decided the model on 2026-09-29. A document has two links,
//! and they say two different things:
//!
//! | Link | What it says |
//! |---|---|
//! | document -> board | The OWNER. The board gives the right to edit. |
//! | document -> repository, `impacts` | What the document is about. It gives NO right. |
//!
//! # Why these routes and not `/api/relationships`
//!
//! `POST /api/relationships` takes two short codes, and
//! `DELETE /api/relationships/{id}` takes the id of an edge between two
//! items. A repository is not an item and has no short code. The links are
//! in a table of their own ([`kairos_db::impacts`]), so the graph view,
//! the traversal of the search and the cascade of an archive do not read
//! them.
//!
//! # The rules
//!
//! The rules are asked in this order, and [`add`] and [`remove`] are the
//! ONE implementation for REST and for the MCP tools `link_items` and
//! `unlink_items`.
//!
//! 1. WHICH SUBJECT. Only a document or an ADR: 422 `RELATIONSHIP_RULE`
//!    for each other type. A task links to a repository with
//!    `PUT /api/tasks/{short_code}/repository`, and the refusal says so.
//! 2. WHO. The edit rule ([`crate::api::require_item_edit`]): the
//!    principal created the item, or holds `manage_<type>` on its
//!    authorization board, or is an organization admin. NO right on the
//!    repository is needed: the link gives no right, and it takes none.
//!    The refusal is 403 `FORBIDDEN`.
//! 3. WHICH TARGET. A new link needs a live repository of the tenant, of
//!    each team: 422 `VALIDATION` names the one that is not. A remove
//!    finds the repository among the links of the item, so a link to an
//!    archived repository can be removed.
//! 4. The link that is there already is 422 `ALREADY_LINKED`, as a
//!    duplicate edge is. The remove of a link that is not there is 404
//!    `NOT_FOUND`.
//!
//! WHO comes before the target, so a principal who may not edit the item
//! does not learn from the answer which repositories exist.
//!
//! Reads are open tenant-wide.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use kairos_client::types_repositories as dto;
use kairos_core::short_code::ItemType;
use kairos_db::impacts::{self, ImpactError, ImpactedRepository};
use kairos_db::repositories;
use serde_json::json;
use uuid::Uuid;

use super::resolve_family_item;
use crate::api::convert::impact;
use crate::api::{Liveness, manage_capability_of, missing_item_edit, short_code_of};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/{entity_type}/{short_code}/impacts",
            get(get_impacts).post(create_impact),
        )
        .route(
            "/api/{entity_type}/{short_code}/impacts/{repository}",
            delete(delete_impact),
        )
}

/// How a surface links a TASK to a repository, for the text of the
/// refusal of rule 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Surface {
    Rest,
    Mcp,
}

impl Surface {
    fn task_link(self, short_code: &str) -> String {
        match self {
            Surface::Rest => format!("PUT /api/tasks/{short_code}/repository"),
            Surface::Mcp => "the tool set_repository".to_string(),
        }
    }
}

/// Rule 1: only a document or an ADR can impact a repository.
pub(crate) fn require_subject(
    item_type: ItemType,
    short_code: &str,
    surface: Surface,
) -> Result<(), ApiError> {
    if impacts::is_subject(item_type) {
        return Ok(());
    }
    let mut message = format!(
        "{short_code} is a {item_type}. Only a document or an ADR can impact a repository."
    );
    if item_type == ItemType::Task {
        message.push_str(&format!(
            " A task links to a repository. To link {short_code} to a repository, use {}.",
            surface.task_link(short_code)
        ));
    }
    Err(
        ApiError::unprocessable("RELATIONSHIP_RULE", message).with_details(json!({
            "relationship": impacts::RELATIONSHIP,
            "source_type": item_type.entity_type(),
            "allowed_source_types": ["document", "adr"],
        })),
    )
}

/// Rule 2: the edit rule, with a text that says what the link needs.
fn require_edit(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
) -> Result<(), ApiError> {
    let Some(board_id) = missing_item_edit(conn, slug, user, item_id, item_type)? else {
        return Ok(());
    };
    let short_code = short_code_of(conn, item_id)?;
    let capability = manage_capability_of(item_type);
    let need = match board_id {
        Some(_) => format!("You need {capability:?} on the board of {short_code}."),
        None => format!("{short_code} has no board."),
    };
    Err(ApiError::forbidden(format!(
        "To change an impacts link of {short_code}, you must be able to edit {short_code}. \
         {need} The creator of {short_code} and an organization admin can also change it. \
         You need no right on the repository."
    ))
    .with_details(json!({
        "relationship": impacts::RELATIONSHIP,
        "required_capability": capability,
        "board_id": board_id,
    })))
}

/// [`ImpactError`] → HTTP. The subject rule and the liveness of the item
/// are asked before the service, so those arms are the answer to a race.
pub(crate) fn map_impact_error(e: ImpactError) -> ApiError {
    match e {
        e @ ImpactError::AlreadyLinked { .. } => {
            ApiError::unprocessable("ALREADY_LINKED", e.to_string())
        }
        e @ ImpactError::NotLinked { .. } => ApiError::not_found(e.to_string()),
        e @ ImpactError::SubjectType { .. } => {
            ApiError::unprocessable("RELATIONSHIP_RULE", e.to_string())
        }
        e @ (ImpactError::ItemNotFound(_) | ImpactError::RepositoryNotFound(_)) => {
            ApiError::validation(e.to_string())
        }
        ImpactError::Database(e) => ApiError::internal(e),
    }
}

/// Make the link "`item` impacts `repository`" as `user`. See the module
/// docs for the rules. The activity row and the event come from
/// [`impacts::link`].
pub(crate) fn add(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
    repository: &str,
    surface: Surface,
) -> Result<ImpactedRepository, ApiError> {
    let short_code = short_code_of(conn, item_id)?;
    require_subject(item_type, &short_code, surface)?;
    require_edit(conn, slug, user, (item_id, item_type))?;
    let repository =
        repositories::resolve(conn, repository).map_err(crate::api::tasks::map_repository_error)?;
    impacts::link(conn, item_id, repository.id, user).map_err(map_impact_error)
}

/// Remove the link "`item` impacts `repository`" as `user`. See the module
/// docs for the rules. The result is the repository of the link that was
/// removed.
pub(crate) fn remove(
    conn: &mut PgConnection,
    slug: &str,
    user: Uuid,
    (item_id, item_type): (Uuid, ItemType),
    repository: &str,
    surface: Surface,
) -> Result<ImpactedRepository, ApiError> {
    let short_code = short_code_of(conn, item_id)?;
    require_subject(item_type, &short_code, surface)?;
    require_edit(conn, slug, user, (item_id, item_type))?;
    let linked = impacts::linked_repository(conn, item_id, repository)
        .map_err(map_impact_error)?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "{short_code} does not impact the repository {repository:?}."
            ))
        })?;
    impacts::unlink(conn, item_id, linked.repository_id, user).map_err(map_impact_error)?;
    Ok(linked)
}

/// The repositories that a document or an ADR impacts (open tenant-wide).
///
/// A link to an archived repository is in the list, with `archived_at`.
/// The item can be archived: its links stay.
#[utoipa::path(
    get,
    path = "/api/{entity_type}/{short_code}/impacts",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    responses(
        (status = 200, description = "The impacts links of the item, by the slug of the repository", body = dto::ItemImpactsResponse),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "RELATIONSHIP_RULE: the item is not a document and not an ADR", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_impacts(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
) -> Result<Json<dto::ItemImpactsResponse>, ApiError> {
    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (item_id, item_type) =
                resolve_family_item(conn, &family, &short_code, Liveness::IncludeArchived)?;
            require_subject(item_type, &short_code, Surface::Rest)?;
            let links = impacts::repositories_of(conn, item_id).map_err(map_impact_error)?;
            Ok(dto::ItemImpactsResponse {
                short_code,
                impacts: links.into_iter().map(impact).collect(),
            })
        })
        .await?;
    Ok(Json(response))
}

/// Make an `impacts` link from a document or an ADR to a repository.
///
/// The edit rule applies (COLLIERY-T-0228). The caller can edit the item.
/// The caller needs no right on the repository. The link gives no right.
///
/// The repository is a live repository of the organization, of each team.
#[utoipa::path(
    post,
    path = "/api/{entity_type}/{short_code}/impacts",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
    ),
    request_body = dto::CreateImpactRequest,
    responses(
        (status = 201, description = "Link created (relationship_add activity row written)", body = dto::Impact),
        (status = 403, description = "Refused by the edit rule: the caller cannot edit the item", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "RELATIONSHIP_RULE: the item is not a document and not an ADR. ALREADY_LINKED: the link is there. VALIDATION: no live repository has that slug or id", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_impact(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code)): Path<(String, String)>,
    ApiJson(body): ApiJson<dto::CreateImpactRequest>,
) -> Result<(StatusCode, Json<dto::Impact>), ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let linked = add(conn, &slug, user, item, &body.repository, Surface::Rest)?;
            Ok(impact(linked))
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Remove an `impacts` link.
///
/// The edit rule applies (COLLIERY-T-0228), as for the create. The
/// repository can be archived.
#[utoipa::path(
    delete,
    path = "/api/{entity_type}/{short_code}/impacts/{repository}",
    tag = "relationships",
    params(
        ("entity_type" = String, Path, description = "Plural family name (documents|adrs)"),
        ("short_code" = String, Path, description = "Item short code"),
        ("repository" = String, Path, description = "Repository slug (or UUID)"),
    ),
    responses(
        (status = 200, description = "Link removed (relationship_remove activity row written)", body = dto::DeletedImpactResponse),
        (status = 403, description = "Refused by the edit rule: the caller cannot edit the item", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown family or short code, or the item does not impact that repository", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "RELATIONSHIP_RULE: the item is not a document and not an ADR", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_impact(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((family, short_code, repository)): Path<(String, String, String)>,
) -> Result<Json<dto::DeletedImpactResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let deleted = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = resolve_family_item(conn, &family, &short_code, Liveness::LiveOnly)?;
            let removed = remove(conn, &slug, user, item, &repository, Surface::Rest)?;
            Ok(dto::DeletedImpactResponse {
                short_code,
                repository: removed.slug,
            })
        })
        .await?;
    Ok(Json(deleted))
}
