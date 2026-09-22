//! `/api/forge-connections` (KAIROS-T-0097, design in KAIROS-I-0009):
//! registering the repositories whose branches and pull/merge requests
//! link to work items.
//!
//! Gating follows the other tenant-wide configuration families (A-0006):
//! **writes are org-admin only** via the `MANAGE` pseudo-capability with
//! `board_id: None`; **reads are open tenant-wide**, like teams and
//! streams. A connection is configuration, not work content.
//!
//! The webhook secret is DERIVED, never stored ([`crate::forge::auth`]),
//! so it is returned exactly once at create/rotate and cannot be re-read.
//! Rotation mints a NEW connection id — which changes the delivery URL,
//! so the operator must update the forge either way.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use kairos_client::types_forge as dto;
use kairos_db::forge::{self, ConnectionWithRepo, ForgeError};
use kairos_db::models::enums::Forge;
use kairos_db::models::forge::NewForgeConnection;
use kairos_db::models::repositories::{NewRepository, RepositoryChangeset};
use kairos_db::repositories::{self, RepositoryError};

use super::super::{parse_enum, parse_opt_uuid, parse_uuid, require_capability};
use crate::app::AppState;
use crate::error::ApiError;
use crate::forge::auth::derive_secret;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// Tenant-wide configuration → org-admin-only writes (A-0006 fallback).
const MANAGE: &str = "manage_teams";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/forge-connections",
            get(list_connections).post(create_connection),
        )
        .route(
            "/api/forge-connections/{id}",
            get(get_connection)
                .patch(update_connection)
                .delete(delete_connection),
        )
        .route(
            "/api/forge-connections/{id}/rotate",
            post(rotate_connection),
        )
}

/// [`ForgeError`] → HTTP.
fn map_error(e: ForgeError) -> ApiError {
    match e {
        ForgeError::ConnectionNotFound(id) => {
            ApiError::not_found(format!("no live forge connection {id} exists"))
        }
        ForgeError::RepoAlreadyConnected { repo } => {
            ApiError::conflict(format!("repository {repo:?} already has a live connection"))
        }
        ForgeError::RepositoryNotFound(id) => {
            ApiError::validation(format!("repository {id} does not exist"))
        }
        ForgeError::ForgeMismatch {
            connection,
            repository,
        } => ApiError::validation(format!(
            "connection forge {connection} does not match the repository's forge {repository}"
        )),
        ForgeError::Database(e) => ApiError::internal(e),
    }
}

/// [`RepositoryError`] → HTTP (the repository CRUD proper is KAIROS-T-0106;
/// this covers the find-or-create behind connection setup).
fn map_repo_error(e: RepositoryError) -> ApiError {
    match e {
        RepositoryError::NotFound(id) => {
            ApiError::validation(format!("repository {id} does not exist"))
        }
        RepositoryError::SlugNotFound(slug) => {
            ApiError::validation(format!("repository {slug:?} does not exist"))
        }
        RepositoryError::InvalidSlug(slug) => {
            ApiError::validation(format!("invalid repository slug {slug:?}"))
        }
        RepositoryError::SlugTaken(slug) => {
            ApiError::conflict(format!("repository slug {slug:?} is already taken"))
        }
        RepositoryError::AlreadyRegistered { forge, repo } => {
            ApiError::conflict(format!("{forge} repository {repo:?} is already registered"))
        }
        RepositoryError::TeamNotFound(id) => {
            ApiError::validation(format!("team {id} does not exist"))
        }
        RepositoryError::NoDeliveryBoard { team, count } => ApiError::validation(format!(
            "team {team} has {count} live delivery boards; exactly one is needed"
        )),
        RepositoryError::InUse {
            id,
            tasks,
            connections,
        } => ApiError::conflict(format!(
            "repository {id} is still referenced by {tasks} task(s) and {connections} connection(s)"
        )),
        RepositoryError::Database(e) => ApiError::internal(e),
    }
}

fn connection_dto(row: ConnectionWithRepo) -> dto::ForgeConnection {
    dto::ForgeConnection {
        id: row.connection.id.to_string(),
        forge: row.connection.forge.to_string(),
        repo_full_name: row.repository.repo_full_name,
        repo_url: row.repository.repo_url,
        team_id: Some(row.repository.team_id.to_string()),
        repository_id: row.repository.id.to_string(),
        created_at: row.connection.created_at.to_rfc3339(),
    }
}

/// The deployment's webhook signing key, or a 501 explaining that the
/// integration is not configured — a missing key is an operator setup
/// gap, not a client error.
fn signing_key(state: &AppState) -> Result<String, ApiError> {
    state.config.webhook_signing_key.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "FORGE_NOT_CONFIGURED",
            "this deployment has no KAIROS_WEBHOOK_SIGNING_KEY set, so forge \
             webhooks cannot be verified; set it and restart to enable the \
             integration",
        )
    })
}

/// The delivery URL an operator pastes into the forge.
fn webhook_url(
    state: &AppState,
    tenant: &str,
    forge: Forge,
    connection_id: &str,
) -> Result<String, ApiError> {
    let base = state.config.public_url.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "PUBLIC_URL_NOT_CONFIGURED",
            "this deployment has no KAIROS_PUBLIC_URL set, so it cannot state \
             the externally reachable webhook URL; set it and restart",
        )
    })?;
    Ok(format!("{base}/webhooks/{forge}/{tenant}/{connection_id}"))
}

/// Every live connection (open tenant-wide; secrets never included).
#[utoipa::path(
    get,
    path = "/api/forge-connections",
    tag = "forge",
    responses(
        (status = 200, description = "Live connections, newest first", body = Vec<dto::ForgeConnection>),
    ),
)]
pub(crate) async fn list_connections(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<Vec<dto::ForgeConnection>>, ApiError> {
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let rows = forge::list_connections(conn).map_err(map_error)?;
            Ok(rows.into_iter().map(connection_dto).collect::<Vec<_>>())
        })
        .await?;
    Ok(Json(rows))
}

/// One live connection (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/forge-connections/{id}",
    tag = "forge",
    params(("id" = String, Path, description = "Connection id (UUID)")),
    responses(
        (status = 200, description = "The connection", body = dto::ForgeConnection),
        (status = 404, description = "Unknown connection", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_connection(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::ForgeConnection>, ApiError> {
    let id = parse_uuid(&id, "id")?;
    let row = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let row = forge::load_connection_with_repo(conn, id).map_err(map_error)?;
            Ok(connection_dto(row))
        })
        .await?;
    Ok(Json(row))
}

/// Register a repository (org admin). Returns the delivery URL and the
/// secret — **the only time the secret is ever shown**.
#[utoipa::path(
    post,
    path = "/api/forge-connections",
    tag = "forge",
    request_body = dto::CreateForgeConnectionRequest,
    responses(
        (status = 201, description = "Connected; webhook_secret is shown once", body = dto::CreatedForgeConnection),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Repository already connected", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad forge or unknown team", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "Deployment not configured for webhooks", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_connection(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateForgeConnectionRequest>,
) -> Result<(StatusCode, Json<dto::CreatedForgeConnection>), ApiError> {
    let key = signing_key(&state)?;
    let forge_kind: Forge = parse_enum(&body.forge, "forge", Forge::ALL)?;
    if matches!(forge_kind, Forge::Other) {
        return Err(ApiError::validation(
            "forge must be github or gitlab: 'other' repositories have no webhook dialect",
        ));
    }
    let team_id = parse_opt_uuid(body.team_id.as_deref(), "team_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let url_slug = slug.clone();
    let created = state
        .blocking
        .run(&slug, move |conn| {
            require_capability(conn, &tenant.slug, None, user, MANAGE)?;
            // KAIROS-T-0103: the connection hangs off a repository. Reuse a
            // registered one by (forge, full name); otherwise register it
            // here, which needs an owning team (A-0019). The dedicated
            // repository API (KAIROS-T-0106) is the first-class path.
            let repository =
                match repositories::find_by_forge_name(conn, forge_kind, &body.repo_full_name)
                    .map_err(map_repo_error)?
                {
                    Some(existing) => existing,
                    None => {
                        let Some(team_id) = team_id else {
                            return Err(ApiError::validation(
                                "team_id is required: the repository is not registered yet and \
                             every repository has exactly one owning team (KAIROS-A-0019)",
                            ));
                        };
                        repositories::create(
                            conn,
                            NewRepository {
                                slug: kairos_core::repositories::slug_from_full_name(
                                    &body.repo_full_name,
                                ),
                                forge: forge_kind,
                                repo_full_name: body.repo_full_name.clone(),
                                repo_url: body.repo_url.clone(),
                                default_branch: "main".to_string(),
                                team_id,
                                description: String::new(),
                                created_by: user,
                                updated_by: user,
                            },
                        )
                        .map_err(map_repo_error)?
                    }
                };
            let created = forge::create_connection(
                conn,
                NewForgeConnection {
                    forge: forge_kind,
                    repository_id: repository.id,
                    created_by: user,
                },
            )
            .map_err(map_error)?;
            Ok(ConnectionWithRepo {
                connection: created,
                repository,
            })
        })
        .await?;
    let id = created.connection.id.to_string();
    let connection_id = created.connection.id;
    let response = dto::CreatedForgeConnection {
        webhook_url: webhook_url(&state, &url_slug, forge_kind, &id)?,
        webhook_secret: derive_secret(&key, connection_id),
        connection: connection_dto(created),
    };
    Ok((StatusCode::CREATED, Json(response)))
}

/// Re-home the connected repository to another team (KAIROS-T-0103: the
/// team lives on the repository and is required, so clearing is 422).
#[utoipa::path(
    patch,
    path = "/api/forge-connections/{id}",
    tag = "forge",
    params(("id" = String, Path, description = "Connection id (UUID)")),
    request_body = dto::UpdateForgeConnectionRequest,
    responses(
        (status = 200, description = "Updated", body = dto::ForgeConnection),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown connection", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_connection(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::UpdateForgeConnectionRequest>,
) -> Result<Json<dto::ForgeConnection>, ApiError> {
    let id = parse_uuid(&id, "id")?;
    if body.clear_team {
        return Err(ApiError::validation(
            "a repository always has an owning team (KAIROS-A-0019); pass a team_id to re-home it",
        ));
    }
    let Some(team_id) = parse_opt_uuid(body.team_id.as_deref(), "team_id")? else {
        return Err(ApiError::validation("team_id is required"));
    };
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let row = state
        .blocking
        .run(&slug, move |conn| {
            require_capability(conn, &tenant.slug, None, user, MANAGE)?;
            let current = forge::load_connection_with_repo(conn, id).map_err(map_error)?;
            let repository = repositories::update(
                conn,
                current.repository.id,
                RepositoryChangeset {
                    team_id: Some(team_id),
                    ..Default::default()
                },
                user,
            )
            .map_err(map_repo_error)?;
            Ok(connection_dto(ConnectionWithRepo {
                connection: current.connection,
                repository,
            }))
        })
        .await?;
    Ok(Json(row))
}

/// Disconnect a repository (soft delete; its links stop being reachable).
#[utoipa::path(
    delete,
    path = "/api/forge-connections/{id}",
    tag = "forge",
    params(("id" = String, Path, description = "Connection id (UUID)")),
    responses(
        (status = 200, description = "Disconnected", body = kairos_client::types_org::OrgDeleteResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown connection", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_connection(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<kairos_client::types_org::OrgDeleteResponse>, ApiError> {
    let uuid = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    state
        .blocking
        .run(&slug, move |conn| {
            require_capability(conn, &tenant.slug, None, user, MANAGE)?;
            forge::delete_connection(conn, uuid).map_err(map_error)
        })
        .await?;
    Ok(Json(kairos_client::types_org::OrgDeleteResponse {
        id,
        deleted: true,
    }))
}

/// Rotate a connection's secret. Because the secret is DERIVED from the
/// connection id, rotation mints a **new connection** (same repo, new id)
/// and disconnects the old one — so the delivery URL changes too and the
/// operator updates both fields in the forge.
#[utoipa::path(
    post,
    path = "/api/forge-connections/{id}/rotate",
    tag = "forge",
    params(("id" = String, Path, description = "Connection id (UUID)")),
    responses(
        (status = 200, description = "Rotated; new URL and secret shown once", body = dto::CreatedForgeConnection),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown connection", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "Deployment not configured for webhooks", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn rotate_connection(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::CreatedForgeConnection>, ApiError> {
    let key = signing_key(&state)?;
    let old_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let url_slug = slug.clone();
    let created = state
        .blocking
        .run(&slug, move |conn| {
            require_capability(conn, &tenant.slug, None, user, MANAGE)?;
            let old = forge::load_connection_with_repo(conn, old_id).map_err(map_error)?;
            // Same repo, fresh id: the old connection goes away in the same
            // transaction so the partial unique index never sees two live
            // rows for one repo.
            forge::delete_connection(conn, old_id).map_err(map_error)?;
            let created = forge::create_connection(
                conn,
                NewForgeConnection {
                    forge: old.connection.forge,
                    repository_id: old.repository.id,
                    created_by: user,
                },
            )
            .map_err(map_error)?;
            Ok(ConnectionWithRepo {
                connection: created,
                repository: old.repository,
            })
        })
        .await?;
    let forge_kind = created.connection.forge;
    let new_id = created.connection.id.to_string();
    let connection_id = created.connection.id;
    Ok(Json(dto::CreatedForgeConnection {
        webhook_url: webhook_url(&state, &url_slug, forge_kind, &new_id)?,
        webhook_secret: derive_secret(&key, connection_id),
        connection: connection_dto(created),
    }))
}
