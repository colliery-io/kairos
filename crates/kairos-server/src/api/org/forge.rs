//! `/api/forge-connections` (KAIROS-T-0097, design in KAIROS-I-0009;
//! re-keyed on repositories by KAIROS-T-0106 per A-0019): the webhook
//! wiring of a REGISTERED repository (`/api/repositories`), so its
//! branches and pull/merge requests link to work items. Ownership and
//! repo identity live on the repository; a connection is one row per
//! repository carrying the delivery endpoint.
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
use kairos_db::repositories;

use super::super::convert::repository_ref;
use super::super::{parse_uuid, require_capability};
use super::repositories::map_error as map_repo_error;
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
            get(get_connection).delete(delete_connection),
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

fn connection_dto(row: ConnectionWithRepo) -> dto::ForgeConnection {
    dto::ForgeConnection {
        id: row.connection.id.to_string(),
        forge: row.connection.forge.to_string(),
        repository: repository_ref(&row.repository),
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

/// Connect webhooks for a registered repository (org admin). Returns the
/// delivery URL and the secret — **the only time the secret is ever
/// shown**. The repository's forge is the webhook dialect; `other` repos
/// cannot be connected.
#[utoipa::path(
    post,
    path = "/api/forge-connections",
    tag = "forge",
    request_body = dto::CreateForgeConnectionRequest,
    responses(
        (status = 201, description = "Connected; webhook_secret is shown once", body = dto::CreatedForgeConnection),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Repository already connected", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Repository has no webhook-capable forge", body = kairos_client::types::ErrorEnvelope),
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
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let url_slug = slug.clone();
    let created = state
        .blocking
        .run(&slug, move |conn| {
            require_capability(conn, &tenant.slug, None, user, MANAGE)?;
            let repository =
                repositories::resolve(conn, &body.repository).map_err(map_repo_error)?;
            if matches!(repository.forge, Forge::Other) {
                return Err(ApiError::validation(format!(
                    "repository {} has forge 'other': only github and gitlab repositories \
                     can receive webhooks",
                    repository.slug
                )));
            }
            let created = forge::create_connection(
                conn,
                NewForgeConnection {
                    forge: repository.forge,
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
    let forge_kind = created.connection.forge;
    let response = dto::CreatedForgeConnection {
        webhook_url: webhook_url(&state, &url_slug, forge_kind, &id)?,
        webhook_secret: derive_secret(&key, connection_id),
        connection: connection_dto(created),
    };
    Ok((StatusCode::CREATED, Json(response)))
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
