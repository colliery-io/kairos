//! `kairos-db` model → [`kairos_client::types_meta`] DTO conversions for
//! the KAIROS-T-0020 endpoint families (relationships, metadata,
//! templates, history, activity). Same encoding contract as
//! [`super::convert`]: UUIDs as canonical strings, timestamps as RFC 3339
//! with microsecond precision.

use chrono::{DateTime, Utc};
use kairos_client::types_meta as dto;
use kairos_db::models::graph::{ActivityLogEntry, ItemHistory, ItemRelationship};
use kairos_db::models::templates::{MetadataDefinition, Template};

use super::convert::IntoDto;

/// RFC 3339 with microsecond precision (the same wire format as
/// [`super::convert`]; duplicated because that helper is private to the
/// T-0018 module). `pub(crate)` since KAIROS-T-0158, so the archived
/// markers on graph neighbours and subgraph nodes encode identically to
/// the `archived_at` on every entity DTO — one format, one meaning.
pub(crate) fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

impl IntoDto<dto::Relationship> for ItemRelationship {
    fn into_dto(self) -> dto::Relationship {
        dto::Relationship {
            id: self.id.to_string(),
            source_id: self.source_id.to_string(),
            target_id: self.target_id.to_string(),
            relationship: self.relationship.to_string(),
            created_at: timestamp(self.created_at),
        }
    }
}

impl IntoDto<dto::Template> for Template {
    fn into_dto(self) -> dto::Template {
        dto::Template {
            id: self.id.to_string(),
            name: self.name,
            slug: self.slug,
            content: self.content,
            is_system_default: self.is_system_default,
            created_at: timestamp(self.created_at),
            updated_at: timestamp(self.updated_at),
        }
    }
}

impl IntoDto<dto::HistoryVersion> for ItemHistory {
    fn into_dto(self) -> dto::HistoryVersion {
        dto::HistoryVersion {
            version: self.version,
            edited_by: self.edited_by.to_string(),
            edited_at: timestamp(self.edited_at),
        }
    }
}

impl IntoDto<dto::HistorySnapshot> for ItemHistory {
    fn into_dto(self) -> dto::HistorySnapshot {
        dto::HistorySnapshot {
            version: self.version,
            title: self.title,
            content: self.content,
            edited_by: self.edited_by.to_string(),
            edited_at: timestamp(self.edited_at),
        }
    }
}

impl IntoDto<dto::ActivityEntry> for ActivityLogEntry {
    fn into_dto(self) -> dto::ActivityEntry {
        dto::ActivityEntry {
            id: self.id.to_string(),
            actor_id: self.actor_id.to_string(),
            action: self.action.to_string(),
            entity_id: self.entity_id.map(|id| id.to_string()),
            entity_type: self.entity_type,
            details: self.details,
            occurred_at: timestamp(self.occurred_at),
        }
    }
}

/// A [`MetadataDefinition`] plus its option values and entity-type
/// scopes (loaded separately — the row itself carries neither) → the
/// definition DTO.
pub fn definition_dto(
    definition: MetadataDefinition,
    enum_options: Vec<String>,
    entity_types: Vec<String>,
) -> dto::MetadataDefinition {
    dto::MetadataDefinition {
        id: definition.id.to_string(),
        name: definition.name,
        slug: definition.slug,
        field_type: definition.field_type.to_string(),
        is_system_default: definition.is_system_default,
        enum_options,
        entity_types,
        created_at: timestamp(definition.created_at),
        updated_at: timestamp(definition.updated_at),
    }
}

/// A [`Template`] plus its hydrated metadata fields → the detail DTO
/// (`GET /api/templates/{id}` — content + associated definitions with
/// defaults/required, KAIROS-S-0005).
pub fn template_detail_dto(
    template: Template,
    metadata: Vec<dto::TemplateMetadataField>,
) -> dto::TemplateDetail {
    dto::TemplateDetail {
        id: template.id.to_string(),
        name: template.name,
        slug: template.slug,
        content: template.content,
        is_system_default: template.is_system_default,
        metadata,
        created_at: timestamp(template.created_at),
        updated_at: timestamp(template.updated_at),
    }
}
