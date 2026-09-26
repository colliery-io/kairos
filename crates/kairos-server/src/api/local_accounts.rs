//! `/api/local-accounts` (KAIROS-T-0204, KAIROS-I-0018): how a local password
//! account comes into existence, gets its password reset, and has its sessions
//! inspected and ended.
//!
//! Mounted only when `KAIROS_LOCAL_AUTH` is on, behind the normal auth → tenant
//! stack, and **org-admin only** on every route including the reads. A-0006's
//! read-open rule does not reach here: a list of someone's live sessions is a
//! security surface, not work content.
//!
//! # Why this exists rather than `POST /api/members`
//!
//! `/api/members` resolves people by email and 404s on an unknown one, with the
//! guidance "users are provisioned at first login, so ask them to log in once".
//! That contract assumes an issuer. With local accounts there is nothing to log in
//! to yet, so somebody has to create the row — which is this.
//!
//! Creating an account also makes the person a **member of the current
//! organization**, and that is the point rather than a convenience: an account that
//! belongs to no organization can log in and then see nothing, which reads to the
//! person as a broken deployment.
//!
//! # One person, one row
//!
//! Adding a password for an email that already signed in through the issuer sets the
//! password on THAT row (KAIROS-T-0197). It does not create a second person with the
//! same address, and it does not rewrite their `external_id` — doing so would break
//! their next SSO login. The response says which happened, because the admin means
//! something different by each.
//!
//! # No new capability
//!
//! `manage_local_accounts` is a pseudo-capability: a string in the 403, enforced as
//! org-admin, exactly like `manage_org_members` and `manage_service_accounts`.
//! KAIROS-T-0182 deleted two capabilities for being grantable but unenforceable, so
//! nothing here invents a third.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use kairos_db::local_auth as db_local_auth;
use kairos_db::models::enums::OrgRole;
use kairos_db::models::{NewOrganizationMember, User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{parse_enum, parse_uuid, require_capability};
use crate::app::AppState;
use crate::error::ApiError;
use crate::local_auth::{hash_password, validate_password};
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The pseudo-capability named in 403s. Enforced as org-admin.
const MANAGE: &str = "manage_local_accounts";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/local-accounts", post(create_local_account))
        .route(
            "/api/local-accounts/{user_id}/password",
            axum::routing::put(set_local_password),
        )
        .route(
            "/api/local-accounts/{user_id}/sessions",
            get(list_sessions).delete(revoke_sessions),
        )
}

/// `POST /api/local-accounts` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateLocalAccountRequest {
    /// The person's email. Lower-cased and trimmed; it is the login identifier.
    pub email: String,
    /// Display name. Defaults to the email when absent.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The initial password. At least [`crate::local_auth::MIN_PASSWORD_LEN`]
    /// characters.
    pub password: String,
    /// Organization role, `member` (default) or `admin`.
    #[serde(default)]
    pub role: Option<String>,
}

/// `POST /api/local-accounts` 201 body.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LocalAccountView {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    /// True when a new person was created; false when a password was added to an
    /// existing row (an OIDC identity for the same email — KAIROS-T-0197).
    pub created: bool,
    /// True when this call added the org membership; false when they were already a
    /// member.
    pub membership_added: bool,
}

/// `PUT /api/local-accounts/{user_id}/password` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetPasswordRequest {
    pub password: String,
}

/// One session, for the audit listing. Never the token or its hash.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionView {
    pub id: String,
    pub created_at: String,
    pub expires_at: String,
    /// `null` until the session is first used.
    pub last_used_at: Option<String>,
    /// `null` while the session is live.
    pub revoked_at: Option<String>,
    /// Whether the session works right now — revocation and expiry collapsed, so a
    /// reader does not have to compare timestamps to answer the only question they
    /// actually have.
    pub active: bool,
}

/// `GET /api/local-accounts/{user_id}/sessions` envelope.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionListResponse {
    pub items: Vec<SessionView>,
    pub total: i64,
}

fn rfc3339(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Map a too-short password onto a 422. Validated before hashing, so a rejected
/// password costs no argon2 work.
fn hash_or_reject(password: &str) -> Result<String, ApiError> {
    validate_password(password)
        .map_err(|e| ApiError::unprocessable("WEAK_PASSWORD", e.to_string()))?;
    hash_password(password).map_err(ApiError::internal)
}

/// `POST /api/local-accounts` — create a local account (or add a password to an
/// existing person) and make them a member of this organization.
#[utoipa::path(
    post,
    path = "/api/local-accounts",
    tag = "local-accounts",
    request_body = CreateLocalAccountRequest,
    responses(
        (status = 201, description = "The account", body = LocalAccountView),
        (status = 403, description = "Not an org admin"),
        (status = 422, description = "Password too short"),
    )
)]
pub async fn create_local_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<CreateLocalAccountRequest>,
) -> Result<(StatusCode, Json<LocalAccountView>), ApiError> {
    let role = body
        .role
        .as_deref()
        .map(|v| parse_enum::<OrgRole>(v, "role", OrgRole::ALL))
        .transpose()?
        .unwrap_or(OrgRole::Member);
    let email = body.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(ApiError::validation(
            "email must be an address; it is the login identifier",
        ));
    }
    let display_name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(&email)
        .to_string();
    let password_hash = hash_or_reject(&body.password)?;

    let org_id = tenant.org_id;
    let caller = auth.user_id;
    let slug = tenant.slug.clone();
    let view = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use diesel::prelude::*;
            use kairos_db::schema::organization_members;
            require_capability(conn, &slug, None, caller, MANAGE)?;

            let (user, created) =
                db_local_auth::upsert_local_user(conn, &email, &display_name, &password_hash)
                    .map_err(ApiError::internal)?;

            // A service account is a machine principal with API keys; giving it a
            // password would make it a person that nobody is.
            if user.is_service_account() {
                return Err(ApiError::conflict(format!(
                    "{email:?} is a service account; service accounts authenticate \
                     with API keys, not passwords"
                )));
            }

            // ON CONFLICT DO NOTHING rather than an error: the admin's intent is
            // "this person should be able to work in my org", and someone who is
            // already a member satisfies it. Reporting a 409 here would make the
            // obvious retry after a network blip fail.
            let inserted = diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id: user.id,
                    role,
                })
                .on_conflict_do_nothing()
                .execute(conn)
                .map_err(ApiError::internal)?;

            Ok(LocalAccountView {
                user_id: user.id.to_string(),
                email: user.email,
                display_name: user.display_name,
                created,
                membership_added: inserted > 0,
            })
        })
        .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// `PUT /api/local-accounts/{user_id}/password` — reset a password. There is no
/// email subsystem, so an admin reset is the recovery path (KAIROS-I-0018).
#[utoipa::path(
    put,
    path = "/api/local-accounts/{user_id}/password",
    tag = "local-accounts",
    params(("user_id" = String, Path, description = "public.users.id")),
    request_body = SetPasswordRequest,
    responses(
        (status = 204, description = "Set; every session that person held is revoked"),
        (status = 403, description = "Not an org admin"),
        (status = 404, description = "No such user in this organization"),
        (status = 422, description = "Password too short"),
    )
)]
pub async fn set_local_password(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(user_id): Path<String>,
    Json(body): Json<SetPasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let target = parse_uuid(&user_id, "user_id")?;
    let password_hash = hash_or_reject(&body.password)?;
    let org_id = tenant.org_id;
    let caller = auth.user_id;
    let slug = tenant.slug.clone();

    state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, caller, MANAGE)?;
            require_member_of(conn, org_id, target)?;
            // The revocation is inside `set_password` (KAIROS-T-0203), so an admin
            // reset cannot leave an attacker's session working — which is the entire
            // reason someone asks for a reset.
            let revoked = db_local_auth::set_password(conn, target, &password_hash)
                .map_err(ApiError::internal)?;
            tracing::info!(
                user_id = %target,
                sessions_revoked = revoked,
                "an org admin reset a local password"
            );
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/local-accounts/{user_id}/sessions`.
#[utoipa::path(
    get,
    path = "/api/local-accounts/{user_id}/sessions",
    tag = "local-accounts",
    params(("user_id" = String, Path, description = "public.users.id")),
    responses(
        (status = 200, description = "Sessions, newest first", body = SessionListResponse),
        (status = 403, description = "Not an org admin"),
        (status = 404, description = "No such user in this organization"),
    )
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(user_id): Path<String>,
) -> Result<Json<SessionListResponse>, ApiError> {
    let target = parse_uuid(&user_id, "user_id")?;
    let org_id = tenant.org_id;
    let caller = auth.user_id;
    let slug = tenant.slug.clone();

    let items = state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, caller, MANAGE)?;
            require_member_of(conn, org_id, target)?;
            let now = chrono::Utc::now();
            let rows =
                db_local_auth::list_sessions_for_user(conn, target).map_err(ApiError::internal)?;
            Ok(rows
                .into_iter()
                .map(|s| SessionView {
                    id: s.id.to_string(),
                    created_at: rfc3339(s.created_at),
                    expires_at: rfc3339(s.expires_at),
                    last_used_at: s.last_used_at.map(rfc3339),
                    revoked_at: s.revoked_at.map(rfc3339),
                    active: s.is_valid_at(now),
                })
                .collect::<Vec<_>>())
        })
        .await?;
    let total = items.len() as i64;
    Ok(Json(SessionListResponse { items, total }))
}

/// `DELETE /api/local-accounts/{user_id}/sessions` — end every session, without
/// changing the password.
///
/// Separate from a reset on purpose: "log this person out of everywhere" and "they
/// have forgotten their password" are different incidents, and forcing the first to
/// change the password would mean telling the person a new one they did not ask for.
#[utoipa::path(
    delete,
    path = "/api/local-accounts/{user_id}/sessions",
    tag = "local-accounts",
    params(("user_id" = String, Path, description = "public.users.id")),
    responses(
        (status = 204, description = "Every session is revoked"),
        (status = 403, description = "Not an org admin"),
        (status = 404, description = "No such user in this organization"),
    )
)]
pub async fn revoke_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(user_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let target = parse_uuid(&user_id, "user_id")?;
    let org_id = tenant.org_id;
    let caller = auth.user_id;
    let slug = tenant.slug.clone();

    state
        .blocking
        .run(&tenant.slug, move |conn| {
            require_capability(conn, &slug, None, caller, MANAGE)?;
            require_member_of(conn, org_id, target)?;
            let revoked = db_local_auth::revoke_all_sessions_for_user(conn, target)
                .map_err(ApiError::internal)?;
            tracing::info!(
                user_id = %target,
                sessions_revoked = revoked,
                "an org admin revoked every session for a local account"
            );
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The target must be a member of THIS organization.
///
/// `public.users` is deployment-wide, so without this an org admin could reset the
/// password of anybody in any other organization — a cross-tenant write reached
/// through a tenant-scoped endpoint, which is the isolation rule KAIROS-A-0005 is
/// built on. The 404 is deliberately the same for "no such user" and "not in this
/// org": which of the two it is would tell an admin whether an address exists
/// somewhere else in the deployment.
fn require_member_of(
    conn: &mut diesel::pg::PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<User, ApiError> {
    use diesel::prelude::*;
    use kairos_db::schema::{organization_members, users};

    let row: Option<User> = users::table
        .inner_join(organization_members::table.on(organization_members::user_id.eq(users::id)))
        .filter(organization_members::organization_id.eq(org_id))
        .filter(users::id.eq(user_id))
        .select(User::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?;
    row.ok_or_else(|| ApiError::not_found(format!("no user {user_id} in this organization")))
}
