//! `/api/service-accounts` (KAIROS-A-0017 / KAIROS-T-0059): org-admin
//! management of service-account principals and their API keys. Mounted behind
//! the NORMAL OIDC auth → tenant stack; every route is org-admin-only (the
//! `manage_service_accounts` pseudo-capability, board_id = None), because a key
//! is a credential surface, not work content — A-0006's read-open rule does not
//! apply.
//!
//! A minted key (`kairos_sk_<slug>_<secret>`, [`super::auth`]) is returned
//! EXACTLY ONCE by the mint endpoint; only its SHA-256 hash and a display
//! prefix land in the tenant's `api_keys` table, and GET never returns the
//! secret or hash.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use kairos_db::api_keys::{ApiKey, NewApiKey};
use kairos_db::models::User;
use kairos_db::models::enums::ActivityAction;
use kairos_db::models::graph::NewActivityLogEntry;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::auth::{display_prefix, generate_key, hash_key};
use crate::api::{parse_uuid, require_capability};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The pseudo-capability named in 403s (org-admin fallback, A-0006).
const MANAGE: &str = "manage_service_accounts";

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/service-accounts",
            get(list_service_accounts).post(create_service_account),
        )
        .route("/api/service-accounts/{id}", delete(delete_service_account))
        .route(
            "/api/service-accounts/{id}/keys",
            get(list_keys).post(create_key),
        )
        .route(
            "/api/service-accounts/{id}/keys/{key_id}",
            delete(revoke_key),
        )
}

// ---------------------------------------------------------------------------
// request / response bodies
// ---------------------------------------------------------------------------

/// `POST /api/service-accounts` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateServiceAccountRequest {
    /// Operator label ("ci-deploy", ...).
    pub name: String,
}

/// A service account (never a secret).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ServiceAccountView {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

/// `GET /api/service-accounts` envelope.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ServiceAccountListResponse {
    pub items: Vec<ServiceAccountView>,
    pub total: i64,
}

/// `POST /api/service-accounts/{id}/keys` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateApiKeyRequest {
    /// Operator label for the key ("gha-main", ...).
    pub name: String,
    /// Optional RFC 3339 expiry; omitted = never expires.
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// `POST /api/service-accounts/{id}/keys` response — the ONLY place the secret
/// appears.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ApiKeyCreatedResponse {
    pub id: String,
    pub name: String,
    /// The full API key. Shown once; store it now.
    pub key: String,
    pub prefix: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

/// One row of `GET /api/service-accounts/{id}/keys` — never a secret or hash.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ApiKeyView {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// `GET /api/service-accounts/{id}/keys` envelope.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ApiKeyListResponse {
    pub items: Vec<ApiKeyView>,
    pub total: i64,
}

/// `DELETE` response for both service accounts and keys.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct DeletedResponse {
    pub id: String,
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn rfc3339(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn account_view(u: &User) -> ServiceAccountView {
    ServiceAccountView {
        id: u.id.to_string(),
        name: u.display_name.clone(),
        created_at: rfc3339(u.created_at),
    }
}

fn key_view(k: &ApiKey) -> ApiKeyView {
    ApiKeyView {
        id: k.id.to_string(),
        name: k.name.clone(),
        prefix: k.prefix.clone(),
        created_at: rfc3339(k.created_at),
        expires_at: k.expires_at.map(rfc3339),
        last_used_at: k.last_used_at.map(rfc3339),
        revoked_at: k.revoked_at.map(rfc3339),
    }
}

/// One tenant `activity_log` row for a service-account/key mutation.
fn log_activity(
    conn: &mut diesel::pg::PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    entity_id: Uuid,
    entity_type: &str,
    details: String,
) -> Result<(), ApiError> {
    use diesel::prelude::*;
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(NewActivityLogEntry {
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

// ---------------------------------------------------------------------------
// service accounts
// ---------------------------------------------------------------------------

/// Create a service account (a machine principal + its org membership).
/// Org-admin-only.
#[utoipa::path(
    post,
    path = "/api/service-accounts",
    tag = "service-accounts",
    request_body = CreateServiceAccountRequest,
    responses(
        (status = 201, description = "Service account created", body = ServiceAccountView),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Empty name", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_service_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<CreateServiceAccountRequest>,
) -> Result<(StatusCode, Json<ServiceAccountView>), ApiError> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::validation("name must not be empty"));
    }
    let user = auth.user_id;
    let org_id = tenant.org_id;
    let slug = tenant.slug.clone();
    let account = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            let sa = kairos_db::service_accounts::create_service_account(conn, org_id, &name)
                .map_err(ApiError::internal)?;
            log_activity(
                conn,
                user,
                ActivityAction::Create,
                sa.id,
                "service_account",
                format!("service_account:{}", sa.display_name),
            )?;
            Ok(sa)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(account_view(&account))))
}

/// List the org's service accounts. Org-admin-only.
#[utoipa::path(
    get,
    path = "/api/service-accounts",
    tag = "service-accounts",
    responses(
        (status = 200, description = "Service accounts", body = ServiceAccountListResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_service_accounts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<ServiceAccountListResponse>, ApiError> {
    let user = auth.user_id;
    let org_id = tenant.org_id;
    let slug = tenant.slug.clone();
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            kairos_db::service_accounts::list_service_accounts(conn, org_id)
                .map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(ServiceAccountListResponse {
        total: rows.len() as i64,
        items: rows.iter().map(account_view).collect(),
    }))
}

/// Delete a service account and all its keys. Org-admin-only. Unknown id → 404.
#[utoipa::path(
    delete,
    path = "/api/service-accounts/{id}",
    tag = "service-accounts",
    params(("id" = String, Path, description = "Service account id (UUID)")),
    responses(
        (status = 200, description = "Deleted", body = DeletedResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown service account", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_service_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<DeletedResponse>, ApiError> {
    let sa_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let org_id = tenant.org_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            let sa = kairos_db::service_accounts::find_service_account(conn, org_id, sa_id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::not_found(format!("no service account {sa_id}")))?;
            kairos_db::service_accounts::delete_service_account(conn, org_id, sa_id)
                .map_err(ApiError::internal)?;
            log_activity(
                conn,
                user,
                ActivityAction::Delete,
                sa_id,
                "service_account",
                format!("service_account_delete:{}", sa.display_name),
            )?;
            Ok(DeletedResponse {
                id: sa_id.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}

// ---------------------------------------------------------------------------
// keys
// ---------------------------------------------------------------------------

/// Mint an API key for a service account: return the secret ONCE, store only
/// its hash. Org-admin-only. Unknown service account → 404.
#[utoipa::path(
    post,
    path = "/api/service-accounts/{id}/keys",
    tag = "service-accounts",
    params(("id" = String, Path, description = "Service account id (UUID)")),
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "Key minted; secret shown once", body = ApiKeyCreatedResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown service account", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Empty name or bad expiry", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyCreatedResponse>), ApiError> {
    let sa_id = parse_uuid(&id, "id")?;
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::validation("name must not be empty"));
    }
    let expires_at = match body.expires_at.as_deref() {
        Some(raw) => Some(
            chrono::DateTime::parse_from_rfc3339(raw)
                .map_err(|e| ApiError::validation(format!("expires_at is not RFC 3339: {e}")))?
                .with_timezone(&chrono::Utc),
        ),
        None => None,
    };

    let raw_key = generate_key(&tenant.slug);
    let token_hash = hash_key(&raw_key);
    let prefix = display_prefix(&raw_key);
    let user = auth.user_id;
    let org_id = tenant.org_id;
    let slug = tenant.slug.clone();

    let row = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            kairos_db::service_accounts::find_service_account(conn, org_id, sa_id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::not_found(format!("no service account {sa_id}")))?;
            let row = kairos_db::api_keys::create_key(
                conn,
                NewApiKey {
                    user_id: sa_id,
                    name: name.clone(),
                    token_hash,
                    prefix,
                    created_by: user,
                    expires_at,
                },
            )
            .map_err(ApiError::internal)?;
            log_activity(
                conn,
                user,
                ActivityAction::Create,
                row.id,
                "api_key",
                format!("api_key:{}", row.name),
            )?;
            Ok(row)
        })
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(ApiKeyCreatedResponse {
            id: row.id.to_string(),
            name: row.name,
            key: raw_key,
            prefix: row.prefix,
            created_at: rfc3339(row.created_at),
            expires_at: row.expires_at.map(rfc3339),
        }),
    ))
}

/// List a service account's keys (metadata only). Org-admin-only.
#[utoipa::path(
    get,
    path = "/api/service-accounts/{id}/keys",
    tag = "service-accounts",
    params(("id" = String, Path, description = "Service account id (UUID)")),
    responses(
        (status = 200, description = "Key metadata (never secrets)", body = ApiKeyListResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown service account", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_keys(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<ApiKeyListResponse>, ApiError> {
    let sa_id = parse_uuid(&id, "id")?;
    let user = auth.user_id;
    let org_id = tenant.org_id;
    let slug = tenant.slug.clone();
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            kairos_db::service_accounts::find_service_account(conn, org_id, sa_id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::not_found(format!("no service account {sa_id}")))?;
            kairos_db::api_keys::list_keys(conn, sa_id).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(ApiKeyListResponse {
        total: rows.len() as i64,
        items: rows.iter().map(key_view).collect(),
    }))
}

/// Revoke a key (soft: `revoked_at` set). Org-admin-only. Unknown ids → 404;
/// already revoked → 409.
#[utoipa::path(
    delete,
    path = "/api/service-accounts/{id}/keys/{key_id}",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account id (UUID)"),
        ("key_id" = String, Path, description = "API key id (UUID)"),
    ),
    responses(
        (status = 200, description = "Key revoked", body = DeletedResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown service account or key", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already revoked", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn revoke_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, key_id)): Path<(String, String)>,
) -> Result<Json<DeletedResponse>, ApiError> {
    let sa_id = parse_uuid(&id, "id")?;
    let key_id = parse_uuid(&key_id, "key_id")?;
    let user = auth.user_id;
    let org_id = tenant.org_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            kairos_db::service_accounts::find_service_account(conn, org_id, sa_id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::not_found(format!("no service account {sa_id}")))?;
            let existing = kairos_db::api_keys::find_key(conn, key_id)
                .map_err(ApiError::internal)?
                .filter(|k| k.user_id == sa_id)
                .ok_or_else(|| {
                    ApiError::not_found(format!("no key {key_id} for service account {sa_id}"))
                })?;
            if existing.revoked_at.is_some() {
                return Err(ApiError::conflict(format!(
                    "key {key_id} is already revoked"
                )));
            }
            let row = kairos_db::api_keys::revoke_key(conn, key_id).map_err(ApiError::internal)?;
            log_activity(
                conn,
                user,
                ActivityAction::Delete,
                row.id,
                "api_key",
                format!("api_key_revoke:{}", row.name),
            )?;
            Ok(DeletedResponse {
                id: key_id.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}
