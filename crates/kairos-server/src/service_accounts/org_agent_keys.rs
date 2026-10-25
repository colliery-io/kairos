//! `/api/org/agent-keys` (KAIROS-T-0388): an organization admin lists the
//! agent keys of each person of the organization and revokes any one.
//!
//! A person makes, rotates and revokes their own agent keys on
//! `/api/me/agent-keys` ([`super::agent_keys`]). A key can have no expiry
//! (KAIROS-T-0370), so an organization admin must be able to stop it too.
//! The rules:
//!
//! - Each route is for an organization admin only (the pseudo-capability
//!   `manage_agent_keys`, no board). The refusal names the role.
//! - The list has the keys of persons only (`users.kind = 'human'`), with
//!   the owner, and never a secret. A revoked key stays in the list.
//! - A revoke writes an activity row, and tells the owner with a
//!   notification of the kind `agent_key_revoked` that names the key, in
//!   one transaction. The id of a service-account key is 404 here: an
//!   admin revokes it on `/api/service-accounts/{id}/keys/{key_id}`.
//!
//! The removal of a person from the organization (`DELETE
//! /api/members/{user_id}` and the SCIM deprovision) revokes each agent key
//! of the person in the transaction of the removal
//! ([`kairos_db::api_keys::revoke_person_keys`]).

use axum::extract::{Extension, Path, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use diesel::pg::PgConnection;
use kairos_client::types::ListEnvelope;
use kairos_client::types_service_accounts::{OrgAgentKey, OrgAgentKeysQuery};
use kairos_db::api_keys::PersonKey;
use kairos_db::models::enums::ActivityAction;
use serde_json::json;
use uuid::Uuid;

use super::routes::{DeletedResponse, log_activity, rfc3339};
use crate::api::org::run_in_transaction;
use crate::api::{clamp_pagination, parse_uuid, require_capability};
use crate::app::AppState;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The pseudo-capability named in 403s (org-admin fallback, A-0006).
const MANAGE: &str = "manage_agent_keys";

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/org/agent-keys", get(list_org_agent_keys))
        .route("/api/org/agent-keys/{key_id}", delete(revoke_org_agent_key))
}

fn view(row: &PersonKey) -> OrgAgentKey {
    let k = &row.key;
    OrgAgentKey {
        id: k.id.to_string(),
        name: k.name.clone(),
        prefix: k.prefix.clone(),
        owner_id: k.user_id.to_string(),
        owner_email: row.owner_email.clone(),
        owner_name: row.owner_name.clone(),
        created_at: rfc3339(k.created_at),
        expires_at: k.expires_at.map(rfc3339),
        last_used_at: k.last_used_at.map(rfc3339),
        revoked_at: k.revoked_at.map(rfc3339),
        replaced_by: k.replaced_by.map(|id| id.to_string()),
    }
}

/// The user id of the `user` filter: a UUID or an email of a person.
/// Kairos refuses a value that names no person.
fn resolve_owner(conn: &mut PgConnection, raw: &str) -> Result<Uuid, ApiError> {
    use diesel::prelude::*;
    use kairos_db::schema::users;
    let raw = raw.trim();
    let found: Option<Uuid> = match Uuid::parse_str(raw) {
        Ok(id) => users::table
            .filter(users::id.eq(id))
            .filter(users::kind.eq(kairos_db::models::USER_KIND_HUMAN))
            .select(users::id)
            .first(conn)
            .optional(),
        // KAIROS-T-0589: an email, or a user name.
        Err(_) if raw.contains('@') => users::table
            .filter(users::email.eq(raw))
            .filter(users::kind.eq(kairos_db::models::USER_KIND_HUMAN))
            .select(users::id)
            .first(conn)
            .optional(),
        Err(_) => users::table
            .filter(users::user_name.eq(raw))
            .filter(users::kind.eq(kairos_db::models::USER_KIND_HUMAN))
            .select(users::id)
            .first(conn)
            .optional(),
    }
    .map_err(ApiError::internal)?;
    found.ok_or_else(|| {
        ApiError::validation(format!(
            "No person has the user id, the email or the user name {raw:?}. Send the user id, \
             the email or the user name of a person."
        ))
        .with_details(serde_json::json!({ "field": "user" }))
    })
}

/// The agent keys of the persons of the organization, newest first (never
/// a secret). Org-admin-only.
#[utoipa::path(
    get,
    path = "/api/org/agent-keys",
    tag = "service-accounts",
    params(OrgAgentKeysQuery),
    responses(
        (status = 200, description = "A page of the agent keys, with the owner of each", body = ListEnvelope<OrgAgentKey>),
        (status = 403, description = "The organization admin role is required", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "The user filter names no person", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_org_agent_keys(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiQuery(query): ApiQuery<OrgAgentKeysQuery>,
) -> Result<Json<ListEnvelope<OrgAgentKey>>, ApiError> {
    let (limit, offset) = clamp_pagination(&kairos_client::types::Pagination {
        limit: query.limit,
        offset: query.offset,
    });
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            let owner = query
                .user
                .as_deref()
                .map(|raw| resolve_owner(conn, raw))
                .transpose()?;
            let (total, rows) = kairos_db::api_keys::list_person_keys(conn, owner, limit, offset)
                .map_err(ApiError::internal)?;
            Ok(ListEnvelope {
                items: rows.iter().map(view).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Revoke the agent key of any person (soft: `revoked_at` set). Writes an
/// activity row and tells the owner. Org-admin-only. An unknown id, or the
/// id of a service-account key → 404; already revoked → 409.
#[utoipa::path(
    delete,
    path = "/api/org/agent-keys/{key_id}",
    tag = "service-accounts",
    params(("key_id" = String, Path, description = "Agent key id (UUID)")),
    responses(
        (status = 200, description = "Key revoked", body = DeletedResponse),
        (status = 403, description = "The organization admin role is required", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No agent key has this id", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already revoked", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn revoke_org_agent_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(key_id): Path<String>,
) -> Result<Json<DeletedResponse>, ApiError> {
    let key_id = parse_uuid(&key_id, "key_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, user, MANAGE)?;
            run_in_transaction(conn, |conn| {
                let existing = kairos_db::api_keys::find_person_key(conn, key_id)
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| {
                        ApiError::not_found(format!(
                            "No agent key has the id {key_id}. To revoke the key of a service \
                             account, use DELETE /api/service-accounts/{{id}}/keys/{{key_id}}."
                        ))
                        .with_details(json!({ "field": "key" }))
                    })?;
                if existing.key.revoked_at.is_some() {
                    return Err(ApiError::conflict(format!(
                        "A person revoked the key {key_id} already."
                    )));
                }
                let row =
                    kairos_db::api_keys::revoke_key(conn, key_id).map_err(ApiError::internal)?;
                log_activity(
                    conn,
                    user,
                    ActivityAction::Delete,
                    row.id,
                    "api_key",
                    format!(
                        "agent_key_admin_revoke:{} owner:{}",
                        row.name, existing.owner_email
                    ),
                )?;
                kairos_db::notifications::on_agent_key_revoked(
                    conn,
                    row.user_id,
                    &row.name,
                    &row.prefix,
                    user,
                )
                .map_err(ApiError::internal)?;
                Ok(DeletedResponse {
                    id: key_id.to_string(),
                    deleted: true,
                })
            })
        })
        .await?;
    // KAIROS-T-0609: the next request with the key is refused.
    state.auth_cache.drop_key(key_id);
    Ok(Json(outcome))
}
