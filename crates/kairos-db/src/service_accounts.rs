//! Service-account principal storage (KAIROS-A-0017 / KAIROS-T-0059).
//!
//! A service account is a `public.users` row (`kind = "service_account"`)
//! that belongs to exactly ONE org via a single `organization_members` row
//! (`role = "member"`; a service account never gets org-admin). Its API keys
//! live in the tenant `api_keys` table (see [`crate::api_keys`]).
//!
//! Every function here runs on a connection pinned to the owning tenant's
//! schema (the server's blocking pool): `public.*` tables are schema-qualified
//! and resolve regardless, and the unqualified tenant tables (`api_keys`,
//! `board_member_capabilities`) resolve to the pinned schema.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::models::public::{NewServiceAccountUser, User};
use crate::models::{NewOrganizationMember, OrgRole};
use crate::schema::{api_keys, board_member_capabilities, organization_members, users};

/// Create a service account: a `users` row (`kind='service_account'`, synthetic
/// `external_id`/`email`) plus its single `organization_members` row. Atomic.
pub fn create_service_account(
    conn: &mut PgConnection,
    org_id: Uuid,
    name: &str,
) -> QueryResult<User> {
    conn.transaction(|conn| {
        // A fresh synthetic identity: `svc:<uuid>` can never collide with an
        // OIDC subject, and the email is a non-routable placeholder.
        let sa_uuid = Uuid::new_v4();
        let user: User = diesel::insert_into(users::table)
            .values(NewServiceAccountUser::new(
                format!("svc:{sa_uuid}"),
                format!("{sa_uuid}@service.local"),
                name,
            ))
            .returning(User::as_returning())
            .get_result(conn)?;

        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: user.id,
                role: OrgRole::Member,
            })
            .execute(conn)?;

        Ok(user)
    })
}

/// All service accounts belonging to `org_id`, newest first.
pub fn list_service_accounts(conn: &mut PgConnection, org_id: Uuid) -> QueryResult<Vec<User>> {
    users::table
        .inner_join(organization_members::table.on(organization_members::user_id.eq(users::id)))
        .filter(organization_members::organization_id.eq(org_id))
        .filter(users::kind.eq(crate::models::public::USER_KIND_SERVICE_ACCOUNT))
        .order(users::created_at.desc())
        .select(User::as_select())
        .load(conn)
}

/// The service account `user_id`, but ONLY if it is a service account AND a
/// member of `org_id` (so one org cannot see or touch another's, and a human
/// user id is never mistaken for a service account).
pub fn find_service_account(
    conn: &mut PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> QueryResult<Option<User>> {
    users::table
        .inner_join(organization_members::table.on(organization_members::user_id.eq(users::id)))
        .filter(organization_members::organization_id.eq(org_id))
        .filter(users::id.eq(user_id))
        .filter(users::kind.eq(crate::models::public::USER_KIND_SERVICE_ACCOUNT))
        .select(User::as_select())
        .first(conn)
        .optional()
}

/// Delete a service account and everything attached to it: its API keys and
/// board capability grants (tenant schema) plus its membership and `users` row
/// (public). Atomic. The caller must have already resolved it via
/// [`find_service_account`] (org scoping + kind check).
pub fn delete_service_account(
    conn: &mut PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> QueryResult<()> {
    conn.transaction(|conn| {
        diesel::delete(api_keys::table.filter(api_keys::user_id.eq(user_id))).execute(conn)?;
        diesel::delete(
            board_member_capabilities::table.filter(board_member_capabilities::user_id.eq(user_id)),
        )
        .execute(conn)?;
        diesel::delete(
            organization_members::table
                .filter(organization_members::organization_id.eq(org_id))
                .filter(organization_members::user_id.eq(user_id)),
        )
        .execute(conn)?;
        diesel::delete(users::table.filter(users::id.eq(user_id))).execute(conn)?;
        Ok(())
    })
}
