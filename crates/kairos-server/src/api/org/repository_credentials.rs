//! `/api/repositories/{slug}/credential` (COLLIERY-T-3105): the read token
//! that the builder of the base code index gives to git to fetch a private
//! repository.
//!
//! - `GET …/credential`: the status. Open tenant-wide, like the repository.
//! - `PUT …/credential` (`{ "token": "…" }`): set or replace the token.
//! - `DELETE …/credential`: remove the token.
//! - `POST …/credential/check`: run `git ls-remote` with the token, and
//!   keep the result.
//!
//! The token is write-only: no response has it. Each response is the
//! status ([`dto::RepositoryCredential`]). Kairos encrypts the token with
//! `KAIROS_SECRETS_KEY` ([`crate::secrets`]); with no key, a write is
//! refused with a 501 that names the setting.
//!
//! Gating: the writes and the check need the right to change the
//! repository (org admin, or `manage_tasks` on the delivery board of its
//! owner team), the same check as `PATCH /api/repositories/{slug}`
//! ([`super::repositories::changeable`]).
//!
//! The set, the replacement and the removal write an activity row with the
//! actor ([`kairos_db::repository_credentials`]). The row has the slug, not
//! the token. The handlers log nothing about the token.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use kairos_client::types_repositories as dto;
use kairos_db::repositories;
use kairos_db::repository_credentials::{self, NewCredential};
use serde_json::json;

use super::repositories::{changeable, map_error};
use crate::app::AppState;
use crate::body::ApiJson;
use crate::credentials::{read_token, status_of, token_fault, token_refusal};
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;
use crate::secrets::{SECRETS_KEY_VAR, SecretsKey, credential_aad};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/repositories/{slug}/credential",
            get(get_credential)
                .put(set_credential)
                .delete(remove_credential),
        )
        .route(
            "/api/repositories/{slug}/credential/check",
            post(check_credential),
        )
}

/// The key of the deployment, or the 501 that names the setting.
fn secrets_key(state: &AppState) -> Result<SecretsKey, ApiError> {
    state.config.secrets_key.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "SECRETS_NOT_CONFIGURED",
            "This deployment has no KAIROS_SECRETS_KEY, so Kairos cannot keep a token. An \
             operator sets KAIROS_SECRETS_KEY to 32 random bytes in base64 and starts the \
             server again.",
        )
        .with_details(json!({ "setting": SECRETS_KEY_VAR }))
    })
}

/// The status of the read token of a repository. It never has the token.
#[utoipa::path(
    get,
    path = "/api/repositories/{slug}/credential",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The status of the token", body = dto::RepositoryCredential),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_credential(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::RepositoryCredential>, ApiError> {
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = repositories::resolve(conn, &slug).map_err(map_error)?;
            status_of(conn, repo.id).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(status))
}

/// Set or replace the read token of a repository (an organization admin, or
/// a member of the owner team). Kairos encrypts the token. The response is the status, with no
/// token.
#[utoipa::path(
    put,
    path = "/api/repositories/{slug}/credential",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    request_body = dto::SetRepositoryCredentialRequest,
    responses(
        (status = 200, description = "The token is set", body = dto::RepositoryCredential),
        (status = 403, description = "Not an organization admin, and not a member of the owner team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "An unknown field, or a token that is empty, too long or has a space", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "This deployment has no KAIROS_SECRETS_KEY", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_credential(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
    ApiJson(body): ApiJson<dto::SetRepositoryCredentialRequest>,
) -> Result<Json<dto::RepositoryCredential>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let key = secrets_key(&state);
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = changeable(conn, &tenant_slug, user, &slug)?;
            // After the gate: a caller with no right learns nothing about
            // the deployment.
            let key = key?;
            if let Some(fault) = token_fault(body.token.expose()) {
                return Err(ApiError::validation(fault).with_details(json!({ "field": "token" })));
            }
            let sealed = key.seal(
                &credential_aad(&tenant_slug, repo.id),
                body.token.expose().as_bytes(),
            );
            repository_credentials::put(
                conn,
                NewCredential {
                    repository_id: repo.id,
                    ciphertext: sealed.ciphertext,
                    nonce: sealed.nonce,
                    key_id: sealed.key_id,
                    set_by: user,
                },
                &repo.slug,
            )
            .map_err(ApiError::internal)?;
            status_of(conn, repo.id).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(status))
}

/// Remove the read token of a repository (an organization admin, or a
/// member of the owner team).
/// The next fetch has no credential.
#[utoipa::path(
    delete,
    path = "/api/repositories/{slug}/credential",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The token is removed", body = dto::RepositoryCredential),
        (status = 403, description = "Not an organization admin, and not a member of the owner team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository, or the repository has no token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_credential(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::RepositoryCredential>, ApiError> {
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let repo = changeable(conn, &tenant_slug, user, &slug)?;
            let removed = repository_credentials::delete(conn, repo.id, user, &repo.slug)
                .map_err(ApiError::internal)?;
            if !removed {
                return Err(ApiError::not_found(format!(
                    "The repository {:?} has no read token.",
                    repo.slug
                )));
            }
            status_of(conn, repo.id).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(status))
}

/// Test the read token: `git ls-remote` of the URL of the repository with
/// the token. The result is kept, and the response has it. A failed check
/// is a 200 with `last_check_ok: false` and the error of git, with the
/// token removed.
#[utoipa::path(
    post,
    path = "/api/repositories/{slug}/credential/check",
    tag = "repositories",
    params(("slug" = String, Path, description = "Repository slug (or UUID)")),
    responses(
        (status = 200, description = "The check ran. `last_check_ok` has the result", body = dto::RepositoryCredential),
        (status = 403, description = "Not an organization admin, and not a member of the owner team", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown repository, or the repository has no token", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "The stored token does not decrypt with the key of the deployment", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn check_credential(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(slug): Path<String>,
) -> Result<Json<dto::RepositoryCredential>, ApiError> {
    let user = auth.user_id;
    let key = state.config.secrets_key.clone();
    let (repo, token) = {
        let tenant_slug = tenant.slug.clone();
        state
            .blocking
            .run(&tenant.slug, move |conn| {
                let repo = changeable(conn, &tenant_slug, user, &slug)?;
                let token = read_token(conn, &tenant_slug, repo.id, key.as_ref())
                    .map_err(token_refusal)?
                    .ok_or_else(|| {
                        ApiError::not_found(format!(
                            "The repository {:?} has no read token. Set one first.",
                            repo.slug
                        ))
                    })?;
                Ok((repo, token))
            })
            .await?
    };
    let remote = match &state.code_index {
        Some(service) => service.remote_of(&repo),
        None => repo.repo_url.clone(),
    };
    let result = tokio::task::spawn_blocking(move || {
        crate::code_index::git::ls_remote(&remote, Some(&token)).map_err(|e| e.to_string())
    })
    .await
    .map_err(ApiError::internal)?;
    let repo_id = repo.id;
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (ok, error) = match &result {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e.as_str())),
            };
            repository_credentials::record_check(conn, repo_id, ok, error)
                .map_err(ApiError::internal)?;
            status_of(conn, repo_id).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(status))
}
