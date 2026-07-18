//! `/api/scim-tokens` (KAIROS-T-0025): org-admin management of the
//! per-tenant SCIM bearer tokens. Mounted behind the NORMAL OIDC auth →
//! tenant stack; every route (list included) is org-admin-only — token
//! metadata is a security surface, not work content, so A-0006's
//! read-open rule deliberately does not apply here.
//!
//! The token secret (`kairos_scim_<slug>_<64-hex>`, [`super::auth`]) is
//! returned EXACTLY ONCE by POST; only its SHA-256 lands in the tenant's
//! `scim_tokens` table, and GET never returns secrets or hashes.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::scim::{NewScimToken, ScimToken};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::auth::{generate_token, hash_token};
use crate::api::{parse_uuid, require_capability};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The pseudo-capability named in 403s (org-admin fallback, A-0006).
const MANAGE: &str = "manage_scim_tokens";

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/scim-tokens", get(list_tokens).post(create_token))
        .route("/api/scim-tokens/{id}", axum::routing::delete(revoke_token))
}

/// `POST /api/scim-tokens` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateScimTokenRequest {
    /// Operator label ("okta-prod", ...).
    pub name: String,
}

/// `POST /api/scim-tokens` response — the ONLY place the secret appears.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ScimTokenCreatedResponse {
    pub id: String,
    pub name: String,
    /// The full bearer token. Shown once; store it in the IdP now.
    pub token: String,
    pub created_at: String,
}

/// One row of `GET /api/scim-tokens` — never a secret, never a hash.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ScimTokenView {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

/// `DELETE /api/scim-tokens/{id}` response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ScimTokenRevokedResponse {
    pub id: String,
    pub revoked: bool,
}

/// `GET /api/scim-tokens` envelope.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ScimTokenListResponse {
    pub items: Vec<ScimTokenView>,
    pub total: i64,
}

fn rfc3339(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn token_view(row: &ScimToken) -> ScimTokenView {
    ScimTokenView {
        id: row.id.to_string(),
        name: row.name.clone(),
        created_by: row.created_by.to_string(),
        created_at: rfc3339(row.created_at),
        revoked_at: row.revoked_at.map(rfc3339),
    }
}

/// One tenant `activity_log` row for a token-management mutation (actor =
/// the calling org admin).
fn log_token_activity(
    conn: &mut diesel::pg::PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    token_id: Uuid,
    details: String,
) -> Result<(), ApiError> {
    use diesel::prelude::*;
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(token_id),
            entity_type: Some("scim_token".to_string()),
            details,
        })
        .execute(conn)
        .map_err(ApiError::internal)?;
    Ok(())
}

/// Create a SCIM token: mint the secret, store only its hash, return the
/// secret ONCE. Org-admin-only.
#[utoipa::path(
    post,
    path = "/api/scim-tokens",
    tag = "scim-tokens",
    request_body = CreateScimTokenRequest,
    responses(
        (status = 201, description = "Token created; secret shown once", body = ScimTokenCreatedResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Empty name", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<CreateScimTokenRequest>,
) -> Result<(StatusCode, Json<ScimTokenCreatedResponse>), ApiError> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::validation("name must not be empty"));
    }
    let token = generate_token(&tenant.slug);
    let token_hash = hash_token(&token);
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let row = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            let row = kairos_db::scim::create_token(
                conn,
                NewScimToken {
                    name: name.clone(),
                    token_hash,
                    created_by: user,
                },
            )
            .map_err(ApiError::internal)?;
            log_token_activity(
                conn,
                user,
                ActivityAction::Create,
                row.id,
                format!("scim_token:{}", row.name),
            )?;
            Ok(row)
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(ScimTokenCreatedResponse {
            id: row.id.to_string(),
            name: row.name,
            token,
            created_at: rfc3339(row.created_at),
        }),
    ))
}

/// List the tenant's SCIM tokens (metadata only). Org-admin-only.
#[utoipa::path(
    get,
    path = "/api/scim-tokens",
    tag = "scim-tokens",
    responses(
        (status = 200, description = "Token metadata (never secrets)", body = ScimTokenListResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_tokens(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<ScimTokenListResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            kairos_db::scim::list_tokens(conn).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(ScimTokenListResponse {
        total: rows.len() as i64,
        items: rows.iter().map(token_view).collect(),
    }))
}

/// Revoke a SCIM token (soft: `revoked_at` set, row retained for audit).
/// Unknown id → 404; already revoked → 409. Org-admin-only.
#[utoipa::path(
    delete,
    path = "/api/scim-tokens/{id}",
    tag = "scim-tokens",
    params(("id" = String, Path, description = "SCIM token id (UUID)")),
    responses(
        (status = 200, description = "Token revoked", body = ScimTokenRevokedResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown token id", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already revoked", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn revoke_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<ScimTokenRevokedResponse>, ApiError> {
    let token_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            let existing = kairos_db::scim::find_token(conn, token_id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::not_found(format!("no SCIM token {token_id}")))?;
            if existing.revoked_at.is_some() {
                return Err(ApiError::conflict(format!(
                    "SCIM token {token_id} is already revoked"
                )));
            }
            let row = kairos_db::scim::revoke_token(conn, token_id).map_err(ApiError::internal)?;
            log_token_activity(
                conn,
                user,
                ActivityAction::Delete,
                row.id,
                format!("scim_token_revoke:{}", row.name),
            )?;
            Ok(ScimTokenRevokedResponse {
                id: row.id.to_string(),
                revoked: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}
