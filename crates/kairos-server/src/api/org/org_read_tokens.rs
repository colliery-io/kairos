//! `/api/org/read-tokens` (KAIROS-T-0431): the read token of the
//! organization for each forge host. The builder of the code index uses
//! the token of the host of a repository before the token of the
//! repository ([`crate::credentials::fetch_token`]).
//!
//! - `GET /api/org/read-tokens`: the host, who set the token and when, for
//!   each host. Never the token.
//! - `PUT /api/org/read-tokens/{host}` (`{ "token": "…" }`): set or replace
//!   the token of the host.
//! - `DELETE /api/org/read-tokens/{host}`: remove the token of the host.
//!
//! Only an organization admin. The host is normalized
//! ([`kairos_core::repositories::normalize_host`]): `GitHub.com`,
//! `https://github.com/` and `github.com:443` are `github.com`.
//!
//! Kairos encrypts the token with `KAIROS_SECRETS_KEY`; with no key, a
//! write is refused with a 501 that names the setting. The set, the
//! replacement and the removal write an activity row with the actor and
//! the host ([`kairos_db::org_read_tokens`]). The handlers log nothing
//! about the token.

use axum::extract::{Extension, Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use kairos_client::types_auth::Secret;
use kairos_client::types_code_index as dto;
use kairos_db::org_read_tokens::{self, NewOrgToken, OrgTokenStatus};
use serde_json::json;
use uuid::Uuid;

use super::repository_credentials::secrets_key;
use crate::api::meta::require_org_admin;
use crate::app::AppState;
use crate::body::ApiJson;
use crate::credentials::token_fault;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;
use crate::secrets::{SecretsKey, org_read_token_aad};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/org/read-tokens", get(list_org_read_tokens))
        .route(
            "/api/org/read-tokens/{host}",
            put(set_org_read_token).delete(remove_org_read_token),
        )
}

fn dto_of(status: OrgTokenStatus) -> dto::OrgReadToken {
    dto::OrgReadToken {
        host: status.host,
        set_by: status.set_by.to_string(),
        set_by_name: status.set_by_name,
        set_at: status.set_at.to_rfc3339(),
    }
}

/// The host of a request, normalized, or the 422 that names the field.
fn host_of(value: &str) -> Result<String, ApiError> {
    kairos_core::repositories::normalize_host(value).map_err(|fault| {
        ApiError::validation(fault.message).with_details(json!({ "field": "host" }))
    })
}

/// The status of each host, by host.
pub(crate) fn list(conn: &mut PgConnection) -> Result<Vec<dto::OrgReadToken>, ApiError> {
    Ok(org_read_tokens::statuses(conn)
        .map_err(ApiError::internal)?
        .into_iter()
        .map(dto_of)
        .collect())
}

/// Set or replace the token of `host` for `user`. The caller checked the
/// admin role. The answer is the status of the host.
pub(crate) fn set(
    conn: &mut PgConnection,
    tenant: &str,
    user: Uuid,
    key: Result<SecretsKey, ApiError>,
    host: &str,
    token: &Secret,
) -> Result<dto::OrgReadToken, ApiError> {
    let host = host_of(host)?;
    let key = key?;
    if let Some(fault) = token_fault(token.expose()) {
        return Err(ApiError::validation(fault).with_details(json!({ "field": "token" })));
    }
    let sealed = key.seal(
        &org_read_token_aad(tenant, &host),
        token.expose().as_bytes(),
    );
    org_read_tokens::put(
        conn,
        NewOrgToken {
            host: host.clone(),
            ciphertext: sealed.ciphertext,
            nonce: sealed.nonce,
            key_id: sealed.key_id,
            set_by: user,
        },
    )
    .map_err(ApiError::internal)?;
    list(conn)?
        .into_iter()
        .find(|t| t.host == host)
        .ok_or_else(|| ApiError::internal("the token went away"))
}

/// Remove the token of `host` for `user`. The caller checked the admin
/// role. The answer is the list that stays. 404: the host has no token.
pub(crate) fn remove(
    conn: &mut PgConnection,
    user: Uuid,
    host: &str,
) -> Result<Vec<dto::OrgReadToken>, ApiError> {
    let host = host_of(host)?;
    if !org_read_tokens::delete(conn, &host, user).map_err(ApiError::internal)? {
        return Err(ApiError::not_found(format!(
            "The organization has no read token for {host:?}."
        ))
        .with_details(json!({ "field": "host", "host": host })));
    }
    list(conn)
}

/// The read tokens of the organization: the host, who set the token and
/// when (an organization admin). Never the token.
#[utoipa::path(
    get,
    path = "/api/org/read-tokens",
    tag = "organization",
    responses(
        (status = 200, description = "The token of each host, as a status", body = Vec<dto::OrgReadToken>),
        (status = 403, description = "Not an organization admin", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_org_read_tokens(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<Vec<dto::OrgReadToken>>, ApiError> {
    require_org_admin(&tenant)?;
    let tokens = state.blocking.run(&tenant.slug, list).await?;
    Ok(Json(tokens))
}

/// Set or replace the read token of the organization for a forge host (an
/// organization admin). Kairos encrypts the token. The builder uses it for
/// each repository on the host, before the token of the repository.
#[utoipa::path(
    put,
    path = "/api/org/read-tokens/{host}",
    tag = "organization",
    params(("host" = String, Path, description = "The forge host, for example github.com")),
    request_body = dto::SetOrgReadTokenRequest,
    responses(
        (status = 200, description = "The token is set", body = dto::OrgReadToken),
        (status = 403, description = "Not an organization admin", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "An unknown field, a host that is not a host, or a token that is empty, too long or has a space", body = kairos_client::types::ErrorEnvelope),
        (status = 501, description = "This deployment has no KAIROS_SECRETS_KEY", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_org_read_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(host): Path<String>,
    ApiJson(body): ApiJson<dto::SetOrgReadTokenRequest>,
) -> Result<Json<dto::OrgReadToken>, ApiError> {
    require_org_admin(&tenant)?;
    let user = auth.user_id;
    let tenant_slug = tenant.slug.clone();
    let key = secrets_key(&state);
    let status = state
        .blocking
        .run(&tenant.slug, move |conn| {
            set(conn, &tenant_slug, user, key, &host, &body.token)
        })
        .await?;
    Ok(Json(status))
}

/// Remove the read token of the organization for a forge host (an
/// organization admin). The next fetch of a repository on the host uses the
/// token of the repository, or no token. The answer is the list that stays.
#[utoipa::path(
    delete,
    path = "/api/org/read-tokens/{host}",
    tag = "organization",
    params(("host" = String, Path, description = "The forge host, for example github.com")),
    responses(
        (status = 200, description = "The token is removed; the tokens that stay", body = Vec<dto::OrgReadToken>),
        (status = 403, description = "Not an organization admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "The host has no token", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "A host that is not a host", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_org_read_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(host): Path<String>,
) -> Result<Json<Vec<dto::OrgReadToken>>, ApiError> {
    require_org_admin(&tenant)?;
    let user = auth.user_id;
    let tokens = state
        .blocking
        .run(&tenant.slug, move |conn| remove(conn, user, &host))
        .await?;
    Ok(Json(tokens))
}
