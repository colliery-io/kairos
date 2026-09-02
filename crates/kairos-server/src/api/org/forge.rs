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
use kairos_db::forge::{self, ForgeError};
use kairos_db::models::enums::Forge;
use kairos_db::models::forge::{ForgeConnection, NewForgeConnection};

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
        .route("/api/forge-connections/{id}/rotate", post(rotate_connection))
}

/// [`ForgeError`] → HTTP.
fn map_error(e: ForgeError) -> ApiError {
    match e {
        ForgeError::ConnectionNotFound(id) => {
            ApiError::not_found(format!("no live forge connection {id} exists"))
        }
        ForgeError::RepoAlreadyConnected { forge, repo } => ApiError::conflict(format!(
            "{forge} repository {repo:?} already has a live connection"
        )),
        ForgeError::TeamNotFound(id) => {
            ApiError::validation(format!("team {id} does not exist"))
        }
        ForgeError::Database(e) => ApiError::internal(e),
    }
}

fn connection_dto(row: ForgeConnection) -> dto::ForgeConnection {
    dto::ForgeConnection {
        id: row.id.to_string(),
        forge: row.forge.to_string(),
        repo_full_name: row.repo_full_name,
        repo_url: row.repo_url,
        team_id: row.team_id.map(|id| id.to_string()),
        created_at: row.created_at.to_rfc3339(),
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
            let row = forge::load_connection(conn, id).map_err(map_error)?;
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
    let team_id = parse_opt_uuid(body.team_id.as_deref(), "team_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let url_slug = slug.clone();
    let created = state
        .blocking
        .run(&slug, move |conn| {
            require_capability(conn, &tenant.slug, None, user, MANAGE)?;
            let created = forge::create_connection(
                conn,
                NewForgeConnection {
                    forge: forge_kind,
                    repo_full_name: body.repo_full_name.clone(),
                    repo_url: body.repo_url.clone(),
                    team_id,
                    created_by: user,
                },
            )
            .map_err(map_error)?;
            Ok(created)
        })
        .await?;
    let id = created.id.to_string();
    let response = dto::CreatedForgeConnection {
        webhook_url: webhook_url(&state, &url_slug, forge_kind, &id)?,
        webhook_secret: derive_secret(&key, created.id),
        connection: connection_dto(created),
    };
    Ok((StatusCode::CREATED, Json(response)))
}

/// Re-attribute a connection to a team, or clear the attribution.
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
    let team_id = if body.clear_team {
        None
    } else {
        parse_opt_uuid(body.team_id.as_deref(), "team_id")?
    };
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let row = state
        .blocking
        .run(&slug, move |conn| {
            require_capability(conn, &tenant.slug, None, user, MANAGE)?;
            let row = forge::set_connection_team(conn, id, team_id).map_err(map_error)?;
            Ok(connection_dto(row))
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
            let old = forge::load_connection(conn, old_id).map_err(map_error)?;
            // Same repo, fresh id: the old connection goes away in the same
            // transaction so the partial unique index never sees two live
            // rows for one repo.
            forge::delete_connection(conn, old_id).map_err(map_error)?;
            let created = forge::create_connection(
                conn,
                NewForgeConnection {
                    forge: old.forge,
                    repo_full_name: old.repo_full_name,
                    repo_url: old.repo_url,
                    team_id: old.team_id,
                    created_by: user,
                },
            )
            .map_err(map_error)?;
            Ok(created)
        })
        .await?;
    let forge_kind = created.forge;
    let new_id = created.id.to_string();
    Ok(Json(dto::CreatedForgeConnection {
        webhook_url: webhook_url(&state, &url_slug, forge_kind, &new_id)?,
        webhook_secret: derive_secret(&key, created.id),
        connection: connection_dto(created),
    }))
}
