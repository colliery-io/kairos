//! `/api/repositories/{slug}/write-token` (KAIROS-T-0386): the write token
//! of the write-back of a repository: the pull requests of a GitHub
//! repository, and the issues of a GitHub or GitLab repository that became
//! tasks (KAIROS-T-0575).
//!
//! - `GET …/write-token`: the status of the write-back. Open tenant-wide,
//!   like the repository.
//! - `PUT …/write-token` (`{ "token": "…" }`): set or replace the token.
//!   Kairos then writes each open pull request that links to an item.
//! - `DELETE …/write-token`: remove the token. Kairos writes nothing more.
//!
//! The token is write-only: no response has it. Kairos seals it with
//! `KAIROS_SECRETS_KEY` like the read token, with its own associated data
//! ([`crate::secrets::write_token_aad`]); with no key, a write is refused
//! with a 501 that names the setting. Only a GitHub or a GitLab repository
//! takes a write token.
//!
//! Gating: the writes need the right to change the repository (org admin,
//! or a member of the owner team), as the read token
//! ([`super::repositories::changeable`]). The set, the replacement and the
//! removal write an activity row with the actor, with no token.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use kairos_client::types_repositories as dto;
use kairos_db::models::enums::Forge;
use kairos_db::repositories;
use kairos_db::writeback::{self, NewWriteToken};
use serde_json::json;

use super::repositories::{changeable, map_error};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::credentials::{token_fault, write_back_of};
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;
use crate::secrets::{SECRETS_KEY_VAR, write_token_aad};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/repositories/{slug}/write-token",
        get(get_write_back)
            .put(set_write_token)
            .delete(remove_write_token),
    )
}

/// The status of the GitHub write-back of a repository. It never has the
/// token.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/write-token",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The status of the write-back", body = dto::RepositoryWriteBack),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_write_back(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::RepositoryWriteBack>, ApiError> {
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            write_back_of(conn, repo.id).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(status))
}

/// Set or replace the write token of a GitHub or GitLab repository. An
/// organization admin or a member of the owner team can do it. Kairos
/// encrypts the token. Then Kairos writes each open pull request that
/// links to an item, and each issue whose task changed. The response is
/// the status, with no token.
#[utoipa::path(
    put,
    path = "/api/repositories/{slug}/write-token",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    request_body = dto::SetRepositoryCredentialRequest,
    responses(
        (status = 200, description = "The token is set", body = dto::RepositoryWriteBack),
        (status = 403, description = "Not an organization admin, and not a member of the owner team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Not a GitHub or GitLab repository, an unknown field, or a token that is empty, too long or has a space", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "This deployment has no KAIROS_SECRETS_KEY", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_write_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiJson(body): ApiJson<dto::SetRepositoryCredentialRequest>,
) -> Result<Json<dto::RepositoryWriteBack>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let key = state.config.secrets_key.clone();
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = changeable(conn, &tenant_slug, user, &slug)?;
            if !matches!(repo.forge, Forge::Github | Forge::Gitlab) {
                return Err(ApiError::validation(format!(
                    "The repository {:?} is on {}. Only a GitHub or a GitLab repository has a \
                     write-back.",
                    repo.slug, repo.forge
                ))
                .with_details(json!({ "field": "token" })));
            }
            // After the gate: a caller with no right learns nothing about
            // the deployment.
            let key = key.ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_IMPLEMENTED,
                    "SECRETS_NOT_CONFIGURED",
                    "This deployment has no KAIROS_SECRETS_KEY, so Kairos cannot keep a token. \
                     An operator sets KAIROS_SECRETS_KEY to 32 random bytes in base64 and \
                     starts the server again.",
                )
                .with_details(json!({ "setting": SECRETS_KEY_VAR }))
            })?;
            if let Some(fault) = token_fault(body.token.expose()) {
                return Err(ApiError::validation(fault).with_details(json!({ "field": "token" })));
            }
            let sealed = key.seal(
                &write_token_aad(&tenant_slug, repo.id),
                body.token.expose().as_bytes(),
            );
            crate::api::org::run_in_transaction(conn, |conn| {
                writeback::put_token(
                    conn,
                    &NewWriteToken {
                        repository_id: repo.id,
                        ciphertext: sealed.ciphertext,
                        nonce: sealed.nonce,
                        key_id: sealed.key_id,
                        set_by: user,
                    },
                    &repo.slug,
                )
                .map_err(ApiError::internal)?;
                writeback::queue_open_prs(conn, repo.id).map_err(ApiError::internal)?;
                // KAIROS-T-0575: the issues whose task is done, cancelled or
                // reopened since the intake.
                kairos_db::issue_writeback::queue_changed_issues(conn, repo.id)
                    .map_err(ApiError::internal)?;
                Ok(())
            })?;
            write_back_of(conn, repo.id).map_err(ApiError::internal)
        })
        .await?;
    state.jobs.wake();
    Ok(Json(status))
}

/// Remove the write token of a repository (an organization admin, or a
/// member of the owner team). Kairos writes nothing more to its pull
/// requests.
#[utoipa::path(
    delete,
    path = "/api/repositories/{slug}/write-token",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The token is removed", body = dto::RepositoryWriteBack),
        (status = 403, description = "Not an organization admin, and not a member of the owner team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository, or the repository has no write token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_write_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::RepositoryWriteBack>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = changeable(conn, &tenant_slug, user, &slug)?;
            let removed = writeback::delete_token(conn, repo.id, user, &repo.slug)
                .map_err(ApiError::internal)?;
            if !removed {
                return Err(ApiError::not_found(format!(
                    "The repository {:?} has no write token.",
                    repo.slug
                ))
                .with_details(json!({ "field": "repository" })));
            }
            write_back_of(conn, repo.id).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(status))
}
