//! Models for the tenant template & metadata tables (KAIROS-A-0003/S-0004):
//! templates, metadata definitions with enum options, template-to-metadata
//! associations, and metadata values on items.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use super::enums::FieldType;
use crate::schema::metadata_definition_scopes;
use crate::schema::{
    item_metadata, metadata_definitions, metadata_enum_options, template_metadata, templates,
};

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

/// A tenant document template (`templates`), seeded from
/// `public.system_templates` at provision time.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = templates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub content: String,
    pub is_system_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`Template`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = templates)]
pub struct NewTemplate {
    pub name: String,
    pub slug: String,
    pub content: String,
    pub is_system_default: bool,
}

/// Partial update for [`Template`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = templates)]
pub struct TemplateChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub content: Option<String>,
    pub is_system_default: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// metadata_definitions
// ---------------------------------------------------------------------------

/// A tenant metadata field definition (`metadata_definitions`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = metadata_definitions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MetadataDefinition {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub field_type: FieldType,
    pub is_system_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert for [`MetadataDefinition`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = metadata_definitions)]
pub struct NewMetadataDefinition {
    pub name: String,
    pub slug: String,
    pub field_type: FieldType,
    pub is_system_default: bool,
}

/// Partial update for [`MetadataDefinition`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = metadata_definitions)]
pub struct MetadataDefinitionChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub field_type: Option<FieldType>,
    pub is_system_default: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// metadata_definition_scopes (KAIROS-T-0078)
// ---------------------------------------------------------------------------

/// One entity type a metadata definition applies to
/// (`metadata_definition_scopes`). A definition with NO scope rows
/// applies to every entity type; the vocabulary is the five entity-type
/// strings ('strategy'|'initiative'|'task'|'document'|'adr'), enforced by
/// the DDL CHECK and matched against
/// `kairos_core::short_code::ItemType::entity_type()` at the write paths.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Insertable)]
#[diesel(table_name = metadata_definition_scopes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MetadataDefinitionScope {
    pub metadata_definition_id: Uuid,
    pub entity_type: String,
}

// ---------------------------------------------------------------------------
// metadata_enum_options
// ---------------------------------------------------------------------------

/// An enum option for a metadata definition (`metadata_enum_options`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = metadata_enum_options)]
#[diesel(belongs_to(MetadataDefinition, foreign_key = metadata_definition_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MetadataEnumOption {
    pub id: Uuid,
    pub metadata_definition_id: Uuid,
    pub value: String,
    pub position: i32,
}

/// Insert for [`MetadataEnumOption`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = metadata_enum_options)]
pub struct NewMetadataEnumOption {
    pub metadata_definition_id: Uuid,
    pub value: String,
    pub position: i32,
}

/// Partial update for [`MetadataEnumOption`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = metadata_enum_options)]
pub struct MetadataEnumOptionChangeset {
    pub value: Option<String>,
    pub position: Option<i32>,
}

// ---------------------------------------------------------------------------
// template_metadata
// ---------------------------------------------------------------------------

/// Which metadata fields a template carries (`template_metadata`).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = template_metadata)]
#[diesel(belongs_to(Template, foreign_key = template_id))]
#[diesel(belongs_to(MetadataDefinition, foreign_key = metadata_definition_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TemplateMetadata {
    pub id: Uuid,
    pub template_id: Uuid,
    pub metadata_definition_id: Uuid,
    pub default_value: Option<String>,
    pub required: bool,
}

/// Insert for [`TemplateMetadata`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = template_metadata)]
pub struct NewTemplateMetadata {
    pub template_id: Uuid,
    pub metadata_definition_id: Uuid,
    pub default_value: Option<String>,
    pub required: bool,
}

/// Partial update for [`TemplateMetadata`]. `default_value` is
/// double-`Option`: `None` = unchanged, `Some(None)` = SET NULL.
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = template_metadata)]
pub struct TemplateMetadataChangeset {
    pub default_value: Option<Option<String>>,
    pub required: Option<bool>,
}

// ---------------------------------------------------------------------------
// item_metadata
// ---------------------------------------------------------------------------

/// A metadata value on an entity (`item_metadata`). `item_id` is any entity
/// UUID (shared UUID space, KAIROS-A-0001).
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = item_metadata)]
#[diesel(belongs_to(MetadataDefinition, foreign_key = metadata_definition_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ItemMetadata {
    pub id: Uuid,
    pub item_id: Uuid,
    pub metadata_definition_id: Uuid,
    pub value: String,
}

/// Insert for [`ItemMetadata`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = item_metadata)]
pub struct NewItemMetadata {
    pub item_id: Uuid,
    pub metadata_definition_id: Uuid,
    pub value: String,
}

/// Partial update for [`ItemMetadata`].
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = item_metadata)]
pub struct ItemMetadataChangeset {
    pub value: Option<String>,
}
