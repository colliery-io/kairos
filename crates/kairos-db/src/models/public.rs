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
    organization_members, organizations, system_board_defaults, system_metadata_definitions,
    system_metadata_enum_options, system_template_metadata, system_templates, users,
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

/// Insert for [`Organization`]; `id`/timestamps come from column defaults.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = organizations)]
pub struct NewOrganization {
    pub name: String,
    pub slug: String,
}

/// Partial update for [`Organization`] (`None` = leave unchanged).
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = organizations)]
pub struct OrganizationChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// users
// ---------------------------------------------------------------------------

/// `users.kind` value for an ordinary OIDC-backed human (the default).
pub const USER_KIND_HUMAN: &str = "human";
/// `users.kind` value for a service-account principal (KAIROS-A-0017): a
/// machine identity authenticated by an API key, never by interactive login.
pub const USER_KIND_SERVICE_ACCOUNT: &str = "service_account";

/// A user principal (`public.users`) — an OIDC-backed human
/// (`kind = "human"`) or a service account (`kind = "service_account"`,
/// KAIROS-A-0017). The rest of the stack (membership, ABAC, activity) keys on
/// `id` regardless of kind.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct User {
    pub id: Uuid,
    pub external_id: String,
    pub email: String,
    pub display_name: String,
    /// `"human"` (default) or `"service_account"` (see the `USER_KIND_*` consts).
    pub kind: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    /// True when this principal is a service account (KAIROS-A-0017).
    pub fn is_service_account(&self) -> bool {
        self.kind == USER_KIND_SERVICE_ACCOUNT
    }
}

/// Insert for a human [`User`]. `kind` is omitted, so the column default
/// (`'human'`) applies — service accounts use [`NewServiceAccountUser`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub external_id: String,
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
        Self {
            external_id: external_id.into(),
            email: email.into(),
            display_name: display_name.into(),
            kind: USER_KIND_SERVICE_ACCOUNT.to_string(),
        }
    }
}

/// Partial update for [`User`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = users)]
pub struct UserChangeset {
    pub external_id: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
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

/// Partial update for [`OrganizationMember`] (role changes).
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = organization_members)]
pub struct OrganizationMemberChangeset {
    pub role: Option<OrgRole>,
}

// ---------------------------------------------------------------------------
// system_templates
// ---------------------------------------------------------------------------

/// System default template (`public.system_templates`), copied into tenant
/// `templates` at provision time (KAIROS-A-0003).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = system_templates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SystemTemplate {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`SystemTemplate`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = system_templates)]
pub struct NewSystemTemplate {
    pub name: String,
    pub slug: String,
    pub content: String,
}

/// Partial update for [`SystemTemplate`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = system_templates)]
pub struct SystemTemplateChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub content: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
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

/// Insert for [`SystemMetadataDefinition`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = system_metadata_definitions)]
pub struct NewSystemMetadataDefinition {
    pub name: String,
    pub slug: String,
    pub field_type: FieldType,
}

/// Partial update for [`SystemMetadataDefinition`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = system_metadata_definitions)]
pub struct SystemMetadataDefinitionChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub field_type: Option<FieldType>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// system_metadata_enum_options
// ---------------------------------------------------------------------------

/// Enum option for a system metadata definition
/// (`public.system_metadata_enum_options`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = system_metadata_enum_options)]
#[diesel(belongs_to(SystemMetadataDefinition, foreign_key = metadata_definition_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SystemMetadataEnumOption {
    pub id: Uuid,
    pub metadata_definition_id: Uuid,
    pub value: String,
    pub position: i32,
}

/// Insert for [`SystemMetadataEnumOption`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = system_metadata_enum_options)]
pub struct NewSystemMetadataEnumOption {
    pub metadata_definition_id: Uuid,
    pub value: String,
    pub position: i32,
}

/// Partial update for [`SystemMetadataEnumOption`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = system_metadata_enum_options)]
pub struct SystemMetadataEnumOptionChangeset {
    pub value: Option<String>,
    pub position: Option<i32>,
}

// ---------------------------------------------------------------------------
// system_template_metadata
// ---------------------------------------------------------------------------

/// Template-to-metadata association for system defaults
/// (`public.system_template_metadata`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = system_template_metadata)]
#[diesel(belongs_to(SystemTemplate, foreign_key = template_id))]
#[diesel(belongs_to(SystemMetadataDefinition, foreign_key = metadata_definition_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SystemTemplateMetadata {
    pub id: Uuid,
    pub template_id: Uuid,
    pub metadata_definition_id: Uuid,
    pub default_value: Option<String>,
    pub required: bool,
}

/// Insert for [`SystemTemplateMetadata`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = system_template_metadata)]
pub struct NewSystemTemplateMetadata {
    pub template_id: Uuid,
    pub metadata_definition_id: Uuid,
    pub default_value: Option<String>,
    pub required: bool,
}

/// Partial update for [`SystemTemplateMetadata`]. `default_value` is
/// double-`Option`: `None` = unchanged, `Some(None)` = SET NULL.
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = system_template_metadata)]
pub struct SystemTemplateMetadataChangeset {
    pub default_value: Option<Option<String>>,
    pub required: Option<bool>,
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

/// Insert for [`SystemBoardDefault`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = system_board_defaults)]
pub struct NewSystemBoardDefault {
    pub board_level: BoardLevel,
    #[diesel(column_name = columns_)]
    pub columns: String,
    pub transitions: String,
}

/// Partial update for [`SystemBoardDefault`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = system_board_defaults)]
pub struct SystemBoardDefaultChangeset {
    pub board_level: Option<BoardLevel>,
    #[diesel(column_name = columns_)]
    pub columns: Option<String>,
    pub transitions: Option<String>,
}
