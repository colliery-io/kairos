//! `/api/me/agent-keys` (KAIROS-T-0359, KAIROS-A-0024): the agent keys of a
//! person.
//!
//! An agent key is an API key (`kairos_sk_<slug>_<secret>`, [`super::auth`])
//! whose `api_keys.user_id` is a PERSON (`users.kind = "human"`), not a service
//! account. A request with the key acts as that person, with the capabilities
//! of that person: the item history names the person.
//!
//! Each member of the tenant manages only their own keys. The rules:
//!
//! - A service account cannot use these routes (403). Its keys are for machine
//!   work, and an organization admin makes them on `/api/service-accounts`.
//! - A request made with an agent key cannot make another agent key (403). A
//!   person makes agent keys with their own login.
//! - The key id of another person is 404, the same as an unknown id, so the
//!   route does not tell that the key exists.
//!
//! The secret is returned once, by `POST`. Only its hash and a display prefix
//! are kept.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use kairos_db::api_keys::NewApiKey;
use kairos_db::models::enums::ActivityAction;
use uuid::Uuid;

use super::auth::{display_prefix, generate_key, hash_key};
use super::routes::{
    ApiKeyCreatedResponse, ApiKeyListResponse, CreateApiKeyRequest, DeletedResponse, key_view,
    log_activity, parse_expiry, rfc3339,
};
use crate::api::parse_uuid;
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/me/agent-keys",
            get(list_agent_keys).post(create_agent_key),
        )
        .route("/api/me/agent-keys/{key_id}", delete(revoke_agent_key))
}

/// Refuse a service account: agent keys are for people.
fn require_person(conn: &mut diesel::pg::PgConnection, user: Uuid) -> Result<(), ApiError> {
    use diesel::prelude::*;
    use kairos_db::schema::users;
    let kind: String = users::table
        .filter(users::id.eq(user))
        .select(users::kind)
        .first(conn)
        .map_err(ApiError::internal)?;
    if kind == kairos_db::models::USER_KIND_SERVICE_ACCOUNT {
        return Err(ApiError::forbidden(
            "A service account cannot have agent keys. Agent keys are for people. An \
             organization admin makes the keys of a service account with \
             POST /api/service-accounts/{id}/keys.",
        ));
    }
    Ok(())
}

/// Make an agent key for the caller: return the secret ONCE, keep only its
/// hash. Kairos refuses a service account, and a request made with an agent key.
#[utoipa::path(
    post,
    path = "/api/me/agent-keys",
    tag = "me",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "Key made; the secret is shown once", body = ApiKeyCreatedResponse),
        (status = 403, description = "The caller is a service account, or the request uses an agent key", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Empty name or bad expiry", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_agent_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyCreatedResponse>), ApiError> {
    if auth.agent_key.is_some() {
        return Err(ApiError::forbidden(
            "An agent key cannot make agent keys. Log in as yourself, then make the key \
             with `kairos keys create` or on the Agent keys page.",
        ));
    }
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::validation("The name is empty. Send a name."));
    }
    let expires_at = parse_expiry(body.expires_at.as_deref())?;

    let raw_key = generate_key(&tenant.slug);
    let token_hash = hash_key(&raw_key);
    let prefix = display_prefix(&raw_key);
    let user = auth.user_id;

    let row = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_person(conn, user)?;
            let row = kairos_db::api_keys::create_key(
                conn,
                NewApiKey {
                    user_id: user,
                    name,
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
                format!("agent_key:{}", row.name),
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

/// The caller's agent keys (metadata only, never a secret), newest first.
#[utoipa::path(
    get,
    path = "/api/me/agent-keys",
    tag = "me",
    responses(
        (status = 200, description = "Key metadata (never secrets)", body = ApiKeyListResponse),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_agent_keys(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<ApiKeyListResponse>, ApiError> {
    let user = auth.user_id;
    let rows = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_person(conn, user)?;
            kairos_db::api_keys::list_keys(conn, user).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(ApiKeyListResponse {
        total: rows.len() as i64,
        items: rows.iter().map(key_view).collect(),
    }))
}

/// Revoke one of the caller's agent keys (soft: `revoked_at` set). The key of
/// another person, or an unknown id → 404; already revoked → 409.
#[utoipa::path(
    delete,
    path = "/api/me/agent-keys/{key_id}",
    tag = "me",
    params(("key_id" = String, Path, description = "Agent key id (UUID)")),
    responses(
        (status = 200, description = "Key revoked", body = DeletedResponse),
        (status = 403, description = "The caller is a service account", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "The caller has no key with this id", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already revoked", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn revoke_agent_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(key_id): Path<String>,
) -> Result<Json<DeletedResponse>, ApiError> {
    let key_id = parse_uuid(&key_id, "key_id")?;
    let user = auth.user_id;
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_person(conn, user)?;
            let existing = kairos_db::api_keys::find_key(conn, key_id)
                .map_err(ApiError::internal)?
                .filter(|k| k.user_id == user)
                .ok_or_else(|| ApiError::not_found(format!("You have no agent key {key_id}.")))?;
            if existing.revoked_at.is_some() {
                return Err(ApiError::conflict(format!(
                    "You revoked the key {key_id} already."
                )));
            }
            let row = kairos_db::api_keys::revoke_key(conn, key_id).map_err(ApiError::internal)?;
            log_activity(
                conn,
                user,
                ActivityAction::Delete,
                row.id,
                "api_key",
                format!("agent_key_revoke:{}", row.name),
            )?;
            Ok(DeletedResponse {
                id: key_id.to_string(),
                deleted: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}
