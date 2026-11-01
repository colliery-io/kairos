//! Models for the shared `public` schema tables (KAIROS-S-0004): tenants,
//! users, org membership, and the `system_*` defaults copied into tenant
//! schemas at provision time.
//!
//! The corresponding `schema.rs` tables are declared SCHEMA-QUALIFIED
//! (`public.*`), so these models work on any pooled connection regardless of
//! the tenant `search_path` pinned on it.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::{BoardLevel, FieldType, OrgRole};
use crate::schema::{
    organization_members, organizations, system_board_defaults, system_metadata_definitions, users,
};

// ---------------------------------------------------------------------------
// organizations
// ---------------------------------------------------------------------------

/// A tenant organization (`public.organizations`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = organizations)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// users
// ---------------------------------------------------------------------------

/// `users.kind` value for an ordinary OIDC-backed human (the default).
pub const USER_KIND_HUMAN: &str = "human";
/// `users.kind` value for a service-account principal (KAIROS-A-0017): a
/// machine identity authenticated by an API key, never by interactive login.
pub const USER_KIND_SERVICE_ACCOUNT: &str = "service_account";
/// `users.kind` value for a system actor (KAIROS-T-0481): the forge actor of
/// a tenant. Not a login: no password, no OIDC login, no API key.
pub const USER_KIND_SYSTEM: &str = "system";

/// A user principal (`public.users`) — an OIDC-backed human
/// (`kind = "human"`) or a service account (`kind = "service_account"`,
/// KAIROS-A-0017). The rest of the stack (membership, ABAC, activity) keys on
/// `id` regardless of kind.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct User {
    pub id: Uuid,
    /// The OIDC `sub`. Logins join on this (`middleware::auth`), which is why
    /// SCIM cannot change it — see [`Self::user_name`].
    pub external_id: String,
    /// SCIM `userName` (KAIROS-T-0184). Distinct from `external_id` because they
    /// are distinct things: an IdP commonly uses the login email here while the
    /// `sub` is opaque. Mutable, and what a `userName` filter searches.
    pub user_name: String,
    pub email: String,
    pub display_name: String,
    /// `"human"` (default) or `"service_account"` (see the `USER_KIND_*` consts).
    pub kind: String,
    /// argon2id PHC string for a local account, or `None` (KAIROS-T-0201).
    ///
    /// `None` is the normal case and means **this person has no local password**,
    /// not that they are locked out: an OIDC-authenticated user never has one.
    /// A login attempt against a `None` must fail exactly as a wrong password
    /// does, or the difference is an account-enumeration oracle.
    pub password_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    /// True when this principal is a service account (KAIROS-A-0017).
    pub fn is_service_account(&self) -> bool {
        self.kind == USER_KIND_SERVICE_ACCOUNT
    }

    /// True when this principal is a system actor (KAIROS-T-0481).
    pub fn is_system(&self) -> bool {
        self.kind == USER_KIND_SYSTEM
    }
}

/// Insert for a human [`User`]. `kind` is omitted, so the column default
/// (`'human'`) applies — service accounts use [`NewServiceAccountUser`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub external_id: String,
    pub user_name: String,
    pub email: String,
    pub display_name: String,
}

/// Insert for a service-account [`User`] (KAIROS-A-0017): sets
/// `kind = "service_account"` explicitly. `external_id` is synthetic
/// (`"svc:<uuid>"`) and cannot collide with an OIDC subject.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = users)]
pub struct NewServiceAccountUser {
    pub external_id: String,
    pub user_name: String,
    pub email: String,
    pub display_name: String,
    pub kind: String,
}

impl NewServiceAccountUser {
    /// A service-account insert with a synthetic `external_id` and the
    /// service-account `kind`. `email`/`display_name` are operator-facing
    /// labels (no mailbox is implied).
    pub fn new(
        external_id: impl Into<String>,
        email: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        let external_id = external_id.into();
        Self {
            // A service account has no IdP and so no meaningful `userName`; the
            // synthetic `svc:<uuid>` keeps the column unique without implying one.
            user_name: external_id.clone(),
            external_id,
            email: email.into(),
            display_name: display_name.into(),
            kind: USER_KIND_SERVICE_ACCOUNT.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// organization_members
// ---------------------------------------------------------------------------

/// Org membership + role (`public.organization_members`, composite PK).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = organization_members)]
#[diesel(primary_key(organization_id, user_id))]
#[diesel(belongs_to(Organization))]
#[diesel(belongs_to(User))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OrganizationMember {
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub role: OrgRole,
    pub joined_at: DateTime<Utc>,
}

/// Insert for [`OrganizationMember`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = organization_members)]
pub struct NewOrganizationMember {
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub role: OrgRole,
}

// ---------------------------------------------------------------------------
// system_metadata_definitions
// ---------------------------------------------------------------------------

/// System default metadata definition (`public.system_metadata_definitions`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = system_metadata_definitions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SystemMetadataDefinition {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub field_type: FieldType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// system_board_defaults
// ---------------------------------------------------------------------------

/// Default board configuration per flight level
/// (`public.system_board_defaults`, KAIROS-A-0002). The `columns` column is
/// mapped to `columns_` by diesel print-schema (reserved identifier).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = system_board_defaults)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SystemBoardDefault {
    pub id: Uuid,
    pub board_level: BoardLevel,
    /// Ordered column names, newline-separated (DB column `columns`).
    #[diesel(column_name = columns_)]
    pub columns: String,
    /// `"from -> to"` pairs, newline-separated.
    pub transitions: String,
}
