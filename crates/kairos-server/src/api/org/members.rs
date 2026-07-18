//! `/api/members` (KAIROS-T-0019 scope addition, day-zero review
//! 2026-07-10): organization membership administration. Tenant-scoped
//! (normal auth → tenant stack); writes are org-admin-only; the list stays
//! open tenant-wide per A-0006's read-open rule.
//!
//! Users are resolved by EMAIL from `public.users` — JIT provisioning
//! creates rows at first login, so "log in once first" is the contract for
//! unknown emails (404 with that guidance). An org must always retain at
//! least one admin: demoting or removing the last admin is 422
//! `LAST_ADMIN`.
//!
//! Activity rows (vocabulary decision recorded in the task doc): add →
//! `create`, remove → `delete`, both `entity_type='membership'`; role
//! changes log `create` with details `membership_role:{old}->{new}
//! user:{id}` (the existing ActivityAction vocabulary has no generic
//! update action).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types::{ListEnvelope, Pagination};
use kairos_client::types_org as dto;
use kairos_db::models::enums::{ActivityAction, OrgRole};
use kairos_db::models::graph::NewActivityLogEntry;
use kairos_db::models::{NewOrganizationMember, OrganizationMember, User};
use uuid::Uuid;

use super::super::{clamp_pagination, parse_enum, parse_uuid, require_capability};
use super::is_unique_violation;
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The pseudo-capability named in 403s for these org-admin-only writes.
const MANAGE: &str = "manage_org_members";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/members", get(list_members).post(add_member))
        .route(
            "/api/members/{user_id}",
            axum::routing::patch(update_member).delete(remove_member),
        )
}

/// One member's DTO view.
fn member_view(member: &OrganizationMember, user: &User) -> dto::OrgMember {
    dto::OrgMember {
        user_id: user.id.to_string(),
        external_id: user.external_id.clone(),
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        role: member.role.to_string(),
        joined_at: member
            .joined_at
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
    }
}

/// The org's membership row for `user_id`, or 404.
fn load_membership(
    conn: &mut PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<OrganizationMember, ApiError> {
    use kairos_db::schema::organization_members::dsl;
    dsl::organization_members
        .filter(dsl::organization_id.eq(org_id))
        .filter(dsl::user_id.eq(user_id))
        .select(OrganizationMember::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "user {user_id} is not a member of this organization"
            ))
        })
}

/// How many admins the org currently has.
fn admin_count(conn: &mut PgConnection, org_id: Uuid) -> Result<i64, ApiError> {
    use kairos_db::schema::organization_members::dsl;
    dsl::organization_members
        .filter(dsl::organization_id.eq(org_id))
        .filter(dsl::role.eq(OrgRole::Admin))
        .count()
        .get_result(conn)
        .map_err(ApiError::internal)
}

/// The 422 guard: an org must always retain at least one admin.
fn last_admin_error() -> ApiError {
    ApiError::unprocessable(
        "LAST_ADMIN",
        "this organization must retain at least one admin; \
         promote another member before demoting or removing this one",
    )
}

/// Insert one `activity_log` row for a membership mutation (tenant-schema
/// table; the connection is tenant-pinned).
fn log_membership_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    action: ActivityAction,
    user_id: Uuid,
    details: String,
) -> Result<(), ApiError> {
    diesel::insert_into(kairos_db::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action,
            entity_id: Some(user_id),
            entity_type: Some("membership".to_string()),
            details,
        })
        .execute(conn)
        .map_err(ApiError::internal)?;
    Ok(())
}

/// List the organization's members (open tenant-wide, paginated, ordered
/// by email).
#[utoipa::path(
    get,
    path = "/api/members",
    tag = "members",
    params(Pagination),
    responses(
        (status = 200, description = "Page of members", body = ListEnvelope<dto::OrgMember>),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_members(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<dto::OrgMember>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let org_id = tenant.org_id;
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::{organization_members, users};
            let total: i64 = organization_members::table
                .filter(organization_members::organization_id.eq(org_id))
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<(OrganizationMember, User)> = organization_members::table
                .inner_join(users::table)
                .filter(organization_members::organization_id.eq(org_id))
                .order(users::email.asc())
                .limit(limit)
                .offset(offset)
                .select((OrganizationMember::as_select(), User::as_select()))
                .load(conn)
                .map_err(ApiError::internal)?;
            Ok(ListEnvelope {
                items: rows
                    .iter()
                    .map(|(member, user)| member_view(member, user))
                    .collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Add a member by email. The user must have logged in once already (JIT
/// provisioning creates `public.users` rows at first login) — unknown
/// email is 404 with that guidance. Org-admin-only.
#[utoipa::path(
    post,
    path = "/api/members",
    tag = "members",
    request_body = dto::AddOrgMemberRequest,
    responses(
        (status = 201, description = "Added", body = dto::OrgMember),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No user with this email has logged in yet", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already a member", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "Bad role", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn add_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::AddOrgMemberRequest>,
) -> Result<(StatusCode, Json<dto::OrgMember>), ApiError> {
    let role = body
        .role
        .as_deref()
        .map(|v| parse_enum::<OrgRole>(v, "role", OrgRole::ALL))
        .transpose()?
        .unwrap_or(OrgRole::Member);
    let org_id = tenant.org_id;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let member = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::{organization_members, users};
            require_capability(conn, &slug, None, user, MANAGE)?;
            let target: Option<User> = users::table
                .filter(users::email.eq(&body.email))
                .select(User::as_select())
                .first(conn)
                .optional()
                .map_err(ApiError::internal)?;
            let target = target.ok_or_else(|| {
                ApiError::not_found(format!(
                    "no user with email {:?} exists yet; users are provisioned at first \
                     login, so ask them to log in once, then add them",
                    body.email
                ))
            })?;
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: org_id,
                    user_id: target.id,
                    role,
                })
                .execute(conn)
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        ApiError::conflict(format!(
                            "{:?} is already a member of this organization",
                            body.email
                        ))
                    } else {
                        ApiError::internal(e)
                    }
                })?;
            log_membership_activity(
                conn,
                user,
                ActivityAction::Create,
                target.id,
                format!("member:{} role:{role}", target.email),
            )?;
            let membership = load_membership(conn, org_id, target.id)?;
            Ok(member_view(&membership, &target))
        })
        .await?;
    Ok((StatusCode::CREATED, Json(member)))
}

/// Change a member's role. Demoting the LAST admin is rejected (422
/// `LAST_ADMIN`). Org-admin-only.
#[utoipa::path(
    patch,
    path = "/api/members/{user_id}",
    tag = "members",
    params(("user_id" = String, Path, description = "User id (UUID)")),
    request_body = dto::UpdateOrgMemberRequest,
    responses(
        (status = 200, description = "Updated", body = dto::OrgMember),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Not a member", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "LAST_ADMIN or bad role", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(user_id): Path<String>,
    Json(body): Json<dto::UpdateOrgMemberRequest>,
) -> Result<Json<dto::OrgMember>, ApiError> {
    let target = parse_uuid(&user_id, "user_id")?;
    let new_role = parse_enum::<OrgRole>(&body.role, "role", OrgRole::ALL)?;
    let org_id = tenant.org_id;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let member = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::organization_members::dsl;
            use kairos_db::schema::users;
            require_capability(conn, &slug, None, user, MANAGE)?;
            let membership = load_membership(conn, org_id, target)?;
            if membership.role == OrgRole::Admin
                && new_role != OrgRole::Admin
                && admin_count(conn, org_id)? <= 1
            {
                return Err(last_admin_error());
            }
            if membership.role != new_role {
                diesel::update(
                    dsl::organization_members
                        .filter(dsl::organization_id.eq(org_id))
                        .filter(dsl::user_id.eq(target)),
                )
                .set(dsl::role.eq(new_role))
                .execute(conn)
                .map_err(ApiError::internal)?;
                log_membership_activity(
                    conn,
                    user,
                    ActivityAction::Create,
                    target,
                    format!(
                        "membership_role:{}->{new_role} user:{target}",
                        membership.role
                    ),
                )?;
            }
            let membership = load_membership(conn, org_id, target)?;
            let identity: User = users::table
                .filter(users::id.eq(target))
                .select(User::as_select())
                .first(conn)
                .map_err(ApiError::internal)?;
            Ok(member_view(&membership, &identity))
        })
        .await?;
    Ok(Json(member))
}

/// Remove a member. Removing the LAST admin is rejected (422 `LAST_ADMIN`).
/// Org-admin-only.
#[utoipa::path(
    delete,
    path = "/api/members/{user_id}",
    tag = "members",
    params(("user_id" = String, Path, description = "User id (UUID)")),
    responses(
        (status = 200, description = "Removed", body = dto::RemoveOrgMemberResponse),
        (status = 403, description = "Not an org admin", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Not a member", body = kairos_client::types::ErrorEnvelope),
        (status = 422, description = "LAST_ADMIN", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn remove_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(user_id): Path<String>,
) -> Result<Json<dto::RemoveOrgMemberResponse>, ApiError> {
    let target = parse_uuid(&user_id, "user_id")?;
    let org_id = tenant.org_id;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::organization_members::dsl;
            require_capability(conn, &slug, None, user, MANAGE)?;
            let membership = load_membership(conn, org_id, target)?;
            if membership.role == OrgRole::Admin && admin_count(conn, org_id)? <= 1 {
                return Err(last_admin_error());
            }
            diesel::delete(
                dsl::organization_members
                    .filter(dsl::organization_id.eq(org_id))
                    .filter(dsl::user_id.eq(target)),
            )
            .execute(conn)
            .map_err(ApiError::internal)?;
            log_membership_activity(
                conn,
                user,
                ActivityAction::Delete,
                target,
                format!("member_remove:{target}"),
            )?;
            Ok(dto::RemoveOrgMemberResponse {
                user_id: target.to_string(),
                removed: true,
            })
        })
        .await?;
    Ok(Json(outcome))
}
