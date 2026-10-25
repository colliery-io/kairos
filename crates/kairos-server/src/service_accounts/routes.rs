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
use axum::routing::{delete, get, post};
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
use crate::body::ApiJson;
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
        .route(
            "/api/service-accounts/{id}/keys/{key_id}/rotate",
            post(rotate_key),
        )
}

// ---------------------------------------------------------------------------
// request / response bodies
// ---------------------------------------------------------------------------

/// `POST /api/service-accounts` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub(crate) struct CreateApiKeyRequest {
    /// Operator label for the key ("gha-main", ...).
    pub name: String,
    /// Optional RFC 3339 expiry. Omitted: the key expires after the
    /// default life of the deployment (`KAIROS_API_KEY_TTL_DAYS`, 90 days
    /// by default), or never when that setting is 0.
    #[serde(default)]
    pub expires_at: Option<String>,
    /// `true`: the key never expires (KAIROS-T-0370). Do not send it with
    /// `expires_at`.
    #[serde(default)]
    pub no_expiry: bool,
    /// The scope of an agent key (KAIROS-T-0536), for
    /// `POST /api/me/agent-keys` only. Omitted: the default scope, each
    /// capability but `purge` and `manage_structure`, on each board. The key
    /// of a service account has no scope: that route refuses the field.
    #[serde(default)]
    pub scope: Option<KeyScopeRequest>,
}

/// The scope of a new agent key (KAIROS-T-0536). A scope never adds a
/// right. The key does an action when the person can do it and the scope
/// has it.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyScopeRequest {
    /// The capabilities of the key, from the vocabulary of the capabilities
    /// reference: the board capabilities, `manage_structure` and
    /// `file_backlog`. `[]` makes a read-only key. Omitted: the default
    /// list (each one but `purge` and `manage_structure`).
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    /// The boards where the key can act, by slug or id. Omitted: each
    /// board. With a list, an action that has no board is refused.
    #[serde(default)]
    pub boards: Option<Vec<String>>,
}

/// The scope of a key (KAIROS-T-0536). `null` in place of this object: the
/// key has no scope and no limit (a key made before KAIROS-T-0536).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct KeyScopeView {
    /// The capabilities of the key, sorted. Empty: a read-only key.
    pub capabilities: Vec<String>,
    /// The boards of the key. `null`: each board.
    pub boards: Option<Vec<KeyScopeBoard>>,
}

/// One board of a [`KeyScopeView`].
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct KeyScopeBoard {
    pub id: String,
    /// The slug of the board; `null` when the board is deleted.
    pub slug: Option<String>,
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
    /// The scope of the key (KAIROS-T-0536); `null` for the key of a
    /// service account.
    pub scope: Option<KeyScopeView>,
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
    /// The key that a rotation made in place of this key (KAIROS-T-0370).
    /// The old secret works until `expires_at`, the end of the grace time.
    pub replaced_by: Option<String>,
    /// The scope of the key (KAIROS-T-0536). `null`: no scope, no limit.
    pub scope: Option<KeyScopeView>,
}

/// The response of a rotation (KAIROS-T-0370). It has the new key and its
/// secret, and the end of the old secret. Kairos shows the secret once.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ApiKeyRotatedResponse {
    /// The id of the new key.
    pub id: String,
    pub name: String,
    /// The new secret. Shown once; store it now.
    pub key: String,
    pub prefix: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    /// The id of the old key.
    pub replaced_key_id: String,
    /// The old secret works until this time (RFC 3339): the end of the
    /// grace time (`KAIROS_API_KEY_ROTATION_GRACE_SECS`).
    pub replaced_key_expires_at: String,
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

pub(super) fn rfc3339(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn account_view(u: &User) -> ServiceAccountView {
    ServiceAccountView {
        id: u.id.to_string(),
        name: u.display_name.clone(),
        created_at: rfc3339(u.created_at),
    }
}

/// The expiry of a new key (KAIROS-T-0370): `expires_at` when the request
/// names it, none when the request sends `no_expiry: true`, else `now` plus
/// the default life of `ttl_days` (none when it is 0).
pub(super) fn resolve_expiry(
    body: &CreateApiKeyRequest,
    ttl_days: u32,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, ApiError> {
    if body.no_expiry && body.expires_at.is_some() {
        return Err(ApiError::validation(
            "The request sends expires_at and no_expiry: true. Send one of the two.",
        )
        .with_details(serde_json::json!({ "field": "no_expiry" })));
    }
    if body.no_expiry {
        return Ok(None);
    }
    if let Some(at) = parse_expiry(body.expires_at.as_deref())? {
        return Ok(Some(at));
    }
    Ok((ttl_days > 0).then(|| now + chrono::Duration::days(i64::from(ttl_days))))
}

/// Rotate a key (KAIROS-T-0370): a new key with a new secret, the same
/// name and the same principal, and the end of the old key after the grace
/// time. The new key has the life of the old key (its expiry minus its
/// creation), counted from now; a key with no expiry gives a key with no
/// expiry. The caller checks the right and the owner of `existing`.
pub(super) fn rotate(
    conn: &mut diesel::pg::PgConnection,
    tenant_slug: &str,
    existing: &ApiKey,
    actor: Uuid,
    grace_secs: u64,
    activity: &str,
) -> Result<ApiKeyRotatedResponse, ApiError> {
    let now = chrono::Utc::now();
    if existing.revoked_at.is_some() {
        return Err(ApiError::conflict(format!(
            "A person revoked the key {}. Make a new key.",
            existing.id
        )));
    }
    if let Some(next) = existing.replaced_by {
        return Err(ApiError::conflict(format!(
            "A rotation replaced the key {} already. Rotate the new key {next}.",
            existing.id
        )));
    }
    if existing.expires_at.is_some_and(|at| at <= now) {
        return Err(ApiError::conflict(format!(
            "The key {} expired. Make a new key.",
            existing.id
        )));
    }
    let raw_key = generate_key(tenant_slug);
    let expires_at = existing
        .expires_at
        .map(|at| now + (at - existing.created_at));
    let grace = chrono::Duration::seconds(i64::try_from(grace_secs).unwrap_or(i64::MAX / 1000));
    let new = NewApiKey {
        user_id: existing.user_id,
        name: existing.name.clone(),
        token_hash: hash_key(&raw_key),
        prefix: display_prefix(&raw_key),
        created_by: actor,
        expires_at,
    };
    let (created, old) = kairos_db::api_keys::rotate_key(conn, existing.id, new, now + grace)
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::conflict(format!(
                "A different request revoked or rotated the key {} already.",
                existing.id
            ))
        })?;
    log_activity(
        conn,
        actor,
        ActivityAction::Update,
        old.id,
        "api_key",
        format!("{activity}:{}", old.name),
    )?;
    Ok(ApiKeyRotatedResponse {
        id: created.id.to_string(),
        name: created.name,
        key: raw_key,
        prefix: created.prefix,
        created_at: rfc3339(created.created_at),
        expires_at: created.expires_at.map(rfc3339),
        replaced_key_id: old.id.to_string(),
        replaced_key_expires_at: old.expires_at.map(rfc3339).unwrap_or_default(),
    })
}

/// The optional RFC 3339 `expires_at` of a key request.
pub(super) fn parse_expiry(
    raw: Option<&str>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, ApiError> {
    raw.map(|raw| {
        chrono::DateTime::parse_from_rfc3339(raw)
            .map(|ts| ts.with_timezone(&chrono::Utc))
            .map_err(|_| {
                // KAIROS-T-0690: the value, not the text of chrono.
                ApiError::validation(format!(
                    "The value {raw:?} of expires_at is not a timestamp. Send an RFC 3339 \
                     timestamp, for example 2026-12-31T00:00:00Z."
                ))
                .with_details(serde_json::json!({ "field": "expires_at" }))
            })
    })
    .transpose()
}

/// The scope of a new agent key (KAIROS-T-0536): the request, or the
/// default. Kairos refuses an unknown capability and an unknown board, and
/// names it. A read-only key has `capabilities: []`.
pub(super) fn resolve_scope(
    conn: &mut diesel::pg::PgConnection,
    request: Option<&KeyScopeRequest>,
) -> Result<kairos_core::key_scope::KeyScope, ApiError> {
    use kairos_core::key_scope::{KeyScope, parse_capabilities, scope_capabilities};
    let mut scope = KeyScope::default_for_agent_key();
    let Some(request) = request else {
        return Ok(scope);
    };
    if let Some(names) = &request.capabilities {
        scope.capabilities = parse_capabilities(names).map_err(|name| {
            ApiError::validation(format!(
                "The scope names the capability {name:?}. A scope can name these \
                 capabilities: {}.",
                scope_capabilities().join(", ")
            ))
            .with_details(serde_json::json!({ "field": "scope.capabilities", "value": name }))
        })?;
    }
    if let Some(references) = &request.boards {
        if references.is_empty() {
            return Err(ApiError::validation(
                "The scope has an empty list of boards. Send one board or more, or omit \
                 `boards` for each board.",
            )
            .with_details(serde_json::json!({ "field": "scope.boards" })));
        }
        let mut boards = std::collections::BTreeSet::new();
        for reference in references {
            let id = crate::api::board_id_by_ref(conn, reference.trim()).map_err(|_| {
                ApiError::validation(format!(
                    "The scope names the board {reference:?}. No live board has that slug or \
                     id."
                ))
                .with_details(serde_json::json!({ "field": "scope.boards", "value": reference }))
            })?;
            boards.insert(id);
        }
        scope.boards = Some(boards);
    }
    Ok(scope)
}

/// The view of a scope (KAIROS-T-0536). With `conn`, each board gets its
/// slug; with none, the slugs are `null`.
pub(super) fn scope_view(
    conn: Option<&mut diesel::pg::PgConnection>,
    scope: &kairos_core::key_scope::KeyScope,
) -> Result<KeyScopeView, ApiError> {
    use diesel::prelude::*;
    let boards = match (&scope.boards, conn) {
        (None, _) => None,
        (Some(ids), conn) => {
            let ids: Vec<Uuid> = ids.iter().copied().collect();
            let slugs: Vec<(Uuid, String)> = match conn {
                Some(conn) => {
                    use kairos_db::schema::boards;
                    boards::table
                        .filter(boards::id.eq_any(&ids))
                        .filter(boards::deleted_at.is_null())
                        .select((boards::id, boards::slug))
                        .load(conn)
                        .map_err(ApiError::internal)?
                }
                None => Vec::new(),
            };
            Some(
                ids.iter()
                    .map(|id| KeyScopeBoard {
                        id: id.to_string(),
                        slug: slugs
                            .iter()
                            .find(|(b, _)| b == id)
                            .map(|(_, slug)| slug.clone()),
                    })
                    .collect(),
            )
        }
    };
    Ok(KeyScopeView {
        capabilities: scope.capabilities.iter().cloned().collect(),
        boards,
    })
}

/// [`key_view`] with the slugs of the boards of the scope.
pub(super) fn key_view_in(
    conn: &mut diesel::pg::PgConnection,
    k: &ApiKey,
) -> Result<ApiKeyView, ApiError> {
    let mut view = key_view(k);
    if let Some(scope) = k.scope() {
        view.scope = Some(scope_view(Some(conn), &scope)?);
    }
    Ok(view)
}

pub(super) fn key_view(k: &ApiKey) -> ApiKeyView {
    ApiKeyView {
        id: k.id.to_string(),
        name: k.name.clone(),
        prefix: k.prefix.clone(),
        created_at: rfc3339(k.created_at),
        expires_at: k.expires_at.map(rfc3339),
        last_used_at: k.last_used_at.map(rfc3339),
        revoked_at: k.revoked_at.map(rfc3339),
        replaced_by: k.replaced_by.map(|id| id.to_string()),
        scope: k.scope().and_then(|scope| scope_view(None, &scope).ok()),
    }
}

/// One tenant `activity_log` row for a service-account/key mutation.
pub(super) fn log_activity(
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
    ApiJson(body): ApiJson<CreateServiceAccountRequest>,
) -> Result<(StatusCode, Json<ServiceAccountView>), ApiError> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::validation("The name is empty. Send a name."));
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
                .ok_or_else(|| {
                    ApiError::not_found(format!("The service account {sa_id} does not exist."))
                })?;
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
    // KAIROS-T-0609: the next request with a key of the account is refused.
    state.auth_cache.drop_user(sa_id);
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
        (status = 422, description = "Empty name, bad expiry, expires_at with no_expiry, or a scope (the key of a service account has none)", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyCreatedResponse>), ApiError> {
    let sa_id = parse_uuid(&id, "id")?;
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::validation("The name is empty. Send a name."));
    }
    // KAIROS-T-0536: a scope is for an agent key. A service account has its
    // grants and no more.
    if body.scope.is_some() {
        return Err(ApiError::validation(
            "The key of a service account has no scope. Remove `scope`. Give the service \
             account only the grants that it needs.",
        )
        .with_details(serde_json::json!({ "field": "scope" })));
    }
    let expires_at = resolve_expiry(&body, state.config.api_key_ttl_days, chrono::Utc::now())?;

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
                .ok_or_else(|| {
                    ApiError::not_found(format!("The service account {sa_id} does not exist."))
                })?;
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
            scope: None,
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
                .ok_or_else(|| {
                    ApiError::not_found(format!("The service account {sa_id} does not exist."))
                })?;
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
                .ok_or_else(|| {
                    ApiError::not_found(format!("The service account {sa_id} does not exist."))
                })?;
            let existing = kairos_db::api_keys::find_key(conn, key_id)
                .map_err(ApiError::internal)?
                .filter(|k| k.user_id == sa_id)
                .ok_or_else(|| {
                    ApiError::not_found(format!("The service account {sa_id} has no key {key_id}."))
                })?;
            if existing.revoked_at.is_some() {
                return Err(ApiError::conflict(format!(
                    "An admin revoked the key {key_id} already."
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
    // KAIROS-T-0609: the next request with the key is refused.
    state.auth_cache.drop_key(key_id);
    Ok(Json(outcome))
}

/// Rotate a key of a service account (KAIROS-T-0370). A new key with a new
/// secret takes the place of the key. Kairos shows the secret once. The old
/// secret works for the grace time (`KAIROS_API_KEY_ROTATION_GRACE_SECS`, 1
/// day by default), then it ends. Org-admin-only. No body.
#[utoipa::path(
    post,
    path = "/api/service-accounts/{id}/keys/{key_id}/rotate",
    tag = "service-accounts",
    params(
        ("id" = String, Path, description = "Service account id (UUID)"),
        ("key_id" = String, Path, description = "API key id (UUID)"),
    ),
    responses(
        (status = 201, description = "The new key; the secret is shown once", body = ApiKeyRotatedResponse),
        (status = 403, description = "Org admin required", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown service account or key", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "The key is revoked, expired, or rotated already", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn rotate_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, key_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<ApiKeyRotatedResponse>), ApiError> {
    let sa_id = parse_uuid(&id, "id")?;
    let key_id = parse_uuid(&key_id, "key_id")?;
    let user = auth.user_id;
    let org_id = tenant.org_id;
    let slug = tenant.slug.clone();
    let grace = state.config.api_key_rotation_grace_secs;
    let rotated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            kairos_db::service_accounts::find_service_account(conn, org_id, sa_id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| {
                    ApiError::not_found(format!("The service account {sa_id} does not exist."))
                })?;
            let existing = kairos_db::api_keys::find_key(conn, key_id)
                .map_err(ApiError::internal)?
                .filter(|k| k.user_id == sa_id)
                .ok_or_else(|| {
                    ApiError::not_found(format!("The service account {sa_id} has no key {key_id}."))
                })?;
            rotate(conn, &slug, &existing, user, grace, "api_key_rotate")
        })
        .await?;
    // KAIROS-T-0609: the old key takes its new expiry at the next request.
    state.auth_cache.drop_key(key_id);
    Ok((StatusCode::CREATED, Json(rotated)))
}
