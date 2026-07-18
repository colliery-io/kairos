//! `/api/admin/tenants` (KAIROS-S-0005 Tenant Provisioning, KAIROS-T-0019)
//! over the T-0008 provisioning services.
//!
//! # Deployment-admin authority (decision recorded in KAIROS-T-0019;
//! # addendum candidate for KAIROS-A-0010)
//!
//! These routes are CROSS-TENANT (they create/list/destroy tenants, and a
//! fresh deployment has no organization to be a member of), so they are
//! registered OUTSIDE the tenant middleware but behind the auth middleware
//! (valid bearer token, JIT user provisioning). Authorization: the
//! authenticated principal's `external_id` (OIDC `sub` — a human user or an
//! A-0010 service account) must be listed in `KAIROS_DEPLOYMENT_ADMINS`
//! (comma-separated, [`crate::config::AppConfig::deployment_admins`]).
//! Empty/unset → always 403. That is the entire authority model.
//!
//! # Scope addition (day-zero review, 2026-07-10)
//!
//! Tenant creation seeds the org's FIRST ADMIN: `initial_admin_external_id`
//! (defaults to the caller) must name a `public.users` row (users are
//! JIT-provisioned at first login → 422 otherwise); the membership insert
//! shares the provisioning transaction and the response reports it.
//!
//! DB access here uses a per-request sync connection (admin operations are
//! rare and cross-tenant, so the tenant-pinned blocking pool does not fit).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router, middleware as axum_middleware};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_org as dto;
use kairos_db::models::enums::OrgRole;
use kairos_db::models::{NewOrganizationMember, User};
use kairos_db::tenant::{self, TenantError};
use serde::Deserialize;
use uuid::Uuid;

use super::super::clamp_pagination;
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::{self, AuthContext};

/// The admin router, wrapped in its own auth layer (no tenant layer — see
/// module docs). Merged into the top-level router by [`crate::app::router`].
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/admin/tenants", get(list_tenants).post(create_tenant))
        .route(
            "/api/admin/tenants/{slug}",
            axum::routing::delete(delete_tenant),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state,
            auth::require_auth,
        ))
}

/// The KAIROS-T-0019 deployment-admin gate: the caller's OIDC `sub` must be
/// listed in `KAIROS_DEPLOYMENT_ADMINS` (empty list → always 403).
fn require_deployment_admin(state: &AppState, auth: &AuthContext) -> Result<(), ApiError> {
    if state
        .config
        .deployment_admins
        .iter()
        .any(|sub| sub == &auth.external_id)
    {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "this action requires deployment-admin privileges \
             (the caller's OIDC sub must be listed in KAIROS_DEPLOYMENT_ADMINS)",
        )
        .with_details(serde_json::json!({ "required": "deployment_admin" })))
    }
}

/// Run `f` on a fresh sync connection off the async runtime (cross-tenant:
/// no `search_path` pinning; all tables touched here are `public.*`).
async fn run_admin<T, F>(state: &AppState, f: F) -> Result<T, ApiError>
where
    F: FnOnce(&mut PgConnection) -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    let database_url = state.config.database_url.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = PgConnection::establish(&database_url).map_err(ApiError::internal)?;
        f(&mut conn)
    })
    .await
    .map_err(ApiError::internal)?
}

/// [`TenantError`] → HTTP: bad slug → 422 `VALIDATION`; existing tenant →
/// 409; unknown tenant → 404; missing `?confirm=true` → 422
/// `CONFIRMATION_REQUIRED` (T-0008 destructive-operation semantics).
fn map_tenant_error(e: TenantError) -> ApiError {
    match e {
        TenantError::InvalidSlug(slug) => ApiError::validation(format!(
            "invalid tenant slug {slug:?}: must match ^[a-z][a-z0-9_-]{{1,62}}$"
        )),
        TenantError::AlreadyExists(slug) => {
            ApiError::conflict(format!("tenant {slug:?} already exists"))
        }
        TenantError::NotFound(slug) => {
            ApiError::not_found(format!("tenant {slug:?} does not exist"))
        }
        TenantError::ConfirmationRequired(slug) => ApiError::unprocessable(
            "CONFIRMATION_REQUIRED",
            format!(
                "dropping tenant {slug:?} is destructive and unrecoverable; \
                 repeat the request with ?confirm=true"
            ),
        ),
        e @ (TenantError::Migration { .. } | TenantError::Board(_) | TenantError::Database(_)) => {
            ApiError::internal(e)
        }
    }
}

/// Create a tenant: T-0008 provisioning (org row, schema, migrations,
/// defaults, boards) PLUS the initial org-admin membership, in one
/// transaction. Deployment-admin only.
#[utoipa::path(
    post,
    path = "/api/admin/tenants",
    tag = "admin",
    request_body = dto::CreateTenantRequest,
    responses(
        (status = 201, description = "Provisioned; initial_admin reports the seeded org admin", body = dto::TenantCreatedResponse),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "Caller is not a deployment admin", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Tenant already exists", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Invalid slug, or the initial admin has never logged in", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_tenant(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(body): Json<dto::CreateTenantRequest>,
) -> Result<(StatusCode, Json<dto::TenantCreatedResponse>), ApiError> {
    require_deployment_admin(&state, &auth)?;
    let admin_sub = body
        .initial_admin_external_id
        .clone()
        .unwrap_or_else(|| auth.external_id.clone());
    let response = run_admin(&state, move |conn| {
        use kairos_db::schema::{organization_members, organizations, users};

        // The initial admin must already exist (JIT provisioning creates
        // users at first login) — checked before provisioning anything.
        let admin: Option<User> = users::table
            .filter(users::external_id.eq(&admin_sub))
            .select(User::as_select())
            .first(conn)
            .optional()
            .map_err(ApiError::internal)?;
        let admin = admin.ok_or_else(|| {
            ApiError::validation(format!(
                "no user with external_id {admin_sub:?} exists yet; users are \
                 provisioned at first login, so the initial admin must authenticate \
                 once first"
            ))
        })?;

        super::run_in_transaction(conn, |conn| {
            let report =
                tenant::provision_tenant(conn, &body.slug, &body.name).map_err(map_tenant_error)?;
            let org_id: Uuid = organizations::table
                .filter(organizations::slug.eq(&body.slug))
                .select(organizations::id)
                .first(conn)
                .map_err(ApiError::internal)?;
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id: admin.id,
                    role: OrgRole::Admin,
                })
                .execute(conn)
                .map_err(ApiError::internal)?;
            Ok(dto::TenantCreatedResponse {
                slug: report.slug,
                schema: report.schema,
                migrations_applied: report.migrations_applied,
                boards_created: report.boards_created,
                templates_copied: report.templates_copied as i64,
                metadata_definitions_copied: report.metadata_definitions_copied as i64,
                initial_admin: dto::TenantInitialAdmin {
                    user_id: admin.id.to_string(),
                    external_id: admin.external_id.clone(),
                    email: admin.email.clone(),
                    role: OrgRole::Admin.to_string(),
                },
            })
        })
    })
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// List provisioned tenants. Deployment-admin only.
#[utoipa::path(
    get,
    path = "/api/admin/tenants",
    tag = "admin",
    params(Pagination),
    responses(
        (status = 200, description = "Page of tenants", body = ListEnvelope<dto::TenantSummary>),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "Caller is not a deployment admin", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_tenants(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<dto::TenantSummary>>, ApiError> {
    require_deployment_admin(&state, &auth)?;
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = run_admin(&state, move |conn| {
        let all = tenant::list_tenants(conn).map_err(map_tenant_error)?;
        let total = all.len() as i64;
        let items = all
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|t| dto::TenantSummary {
                slug: t.slug,
                name: t.name,
                schema_exists: t.schema_exists,
            })
            .collect();
        Ok(ListEnvelope {
            items,
            total,
            limit,
            offset,
        })
    })
    .await?;
    Ok(Json(envelope))
}

/// `?confirm=true` — required by the T-0008 destructive-operation guard.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ConfirmParams {
    /// Must be `true` to actually drop the tenant.
    #[serde(default)]
    confirm: Option<bool>,
}

/// Drop a tenant: removes the `org_{slug}` schema (CASCADE) and the
/// organization row. Destructive and unrecoverable — requires
/// `?confirm=true` (422 `CONFIRMATION_REQUIRED` otherwise). Deployment-admin
/// only.
#[utoipa::path(
    delete,
    path = "/api/admin/tenants/{slug}",
    tag = "admin",
    params(
        ("slug" = String, Path, description = "Tenant slug"),
        ConfirmParams,
    ),
    responses(
        (status = 200, description = "Dropped", body = dto::TenantDeletedResponse),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "Caller is not a deployment admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown tenant", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "CONFIRMATION_REQUIRED (missing ?confirm=true)", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_tenant(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(slug): Path<String>,
    Query(params): Query<ConfirmParams>,
) -> Result<Json<dto::TenantDeletedResponse>, ApiError> {
    require_deployment_admin(&state, &auth)?;
    let confirm = params.confirm.unwrap_or(false);
    let response = run_admin(&state, move |conn| {
        use kairos_db::schema::{organization_members, organizations};
        super::run_in_transaction(conn, |conn| {
            // Membership rows reference the org row without ON DELETE
            // CASCADE; clear them in the same transaction (rolled back if
            // the drop is refused — e.g. missing ?confirm=true).
            diesel::delete(
                organization_members::table.filter(
                    organization_members::organization_id.eq_any(
                        organizations::table
                            .filter(organizations::slug.eq(&slug))
                            .select(organizations::id),
                    ),
                ),
            )
            .execute(conn)
            .map_err(ApiError::internal)?;
            tenant::drop_tenant(conn, &slug, confirm).map_err(map_tenant_error)?;
            Ok(dto::TenantDeletedResponse {
                slug,
                dropped: true,
            })
        })
    })
    .await?;
    Ok(Json(response))
}
