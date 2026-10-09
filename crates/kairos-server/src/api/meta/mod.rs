//! The KAIROS-T-0020 endpoint families (KAIROS-S-0005): relationships,
//! item metadata, metadata definitions, templates, content history, and
//! the activity log — following the T-0018 handler/module pattern
//! ([`crate::api`]).
//!
//! # Shape of this module tree
//!
//! One submodule per family, each exporting `router()`; [`router`] merges
//! them and is itself merged in [`crate::app::router`] behind the same
//! auth → tenant stack as the entity families (registered there, not in
//! `api::router()`, so concurrently developed endpoint tasks register at
//! distinct anchors).
//!
//! # Authorization (KAIROS-A-0006)
//!
//! - Reads are open tenant-wide, including definition/template lists (the
//!   S-0005 "(org admin)" annotations apply to the writes: A-0006 says
//!   only org admins can *create, modify, or delete* tenant-wide
//!   configuration).
//! - Metadata-definition and template writes are org-admin only
//!   ([`require_org_admin`] — checked against the membership role the
//!   tenant middleware already resolved).
//! - Relationship writes take the LINK rule
//!   ([`crate::api::require_edge_write`], COLLIERY-T-0228): the caller may
//!   edit the item at either end. No relationship type needs the admin
//!   role. The `supports` edge of a document is narrower, and a document
//!   keeps its last one ([`crate::api::require_edge_remove`],
//!   COLLIERY-T-0235).
//! - `impacts` writes take the EDIT rule of the document or of the ADR
//!   ([`impacts`], COLLIERY-T-0269). No right on the repository is
//!   needed.
//! - Item-metadata writes and restore take the EDIT rule
//!   ([`crate::api::require_item_edit`], COLLIERY-T-0228): the caller
//!   created the item, or holds its `manage_<type>` capability on its
//!   authorization board
//!   ([`kairos_db::abac::resolve_authorization_board`]: own board for
//!   board items; for a document the board that it names, or the parent's
//!   board via `supports`), or is an org admin.
//!
//! # The `{entity_type}` path segment
//!
//! `GET /api/{entity_type}/{short_code}/{relationships|metadata|history}`
//! are registered as wildcard routes (S-0005 spells them generically);
//! the family segment must be one of the five plural family names and the
//! short code must resolve to exactly that type ([`resolve_family_item`],
//! 404 otherwise — matching the per-family route behavior).

pub mod activity;
pub mod definitions;
pub mod history;
// COLLIERY-T-0269: the relationship `impacts`, to a repository.
pub mod impacts;
// KAIROS-T-0321: the teams of an initiative or a strategy.
pub mod item_teams;
pub mod metadata;
pub mod relationships;
pub mod restore;
pub mod templates;

use axum::Router;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types_meta as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::enums::{FieldType, OrgRole};
use kairos_db::models::templates::{ItemMetadata, MetadataDefinition};
use serde_json::json;
use uuid::Uuid;

use super::{Liveness, resolve_short_code, short_code_not_found};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

/// All six T-0020 family routers, merged.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(relationships::router())
        .merge(impacts::router())
        .merge(item_teams::router())
        .merge(restore::router())
        .merge(metadata::router())
        .merge(definitions::router())
        .merge(templates::router())
        .merge(history::router())
        .merge(activity::router())
}

/// The A-0006 gate for tenant-wide configuration (metadata definitions,
/// templates): org admins only. The role was resolved from
/// `public.organization_members` by the tenant middleware.
///
/// Relationships left this gate with COLLIERY-T-0228: an edge takes the
/// link rule ([`crate::api::require_edge_write`]).
pub fn require_org_admin(tenant: &TenantContext) -> Result<(), ApiError> {
    if tenant.role == OrgRole::Admin {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "This action requires the organization admin role. Ask an admin of the \
             organization to do it, or to make you an admin.",
        )
        .with_details(json!({ "required_role": "admin" })))
    }
}

/// Map a plural `{entity_type}` path segment (the S-0005 family names, as
/// used by every `/api/{family}` route) to its [`ItemType`].
pub fn item_type_of_family(family: &str) -> Option<ItemType> {
    match family {
        "strategies" => Some(ItemType::Strategy),
        "initiatives" => Some(ItemType::Initiative),
        "tasks" => Some(ItemType::Task),
        "documents" => Some(ItemType::Document),
        "adrs" => Some(ItemType::Adr),
        _ => None,
    }
}

/// The A-0006 manage capability for an entity type. The table is in
/// [`kairos_core::abac::manage_capability`], which the cascade of an
/// archive reads too (COLLIERY-T-0234).
pub fn manage_capability(item_type: ItemType) -> &'static str {
    kairos_core::abac::manage_capability(item_type)
}

/// Resolve an `{entity_type}/{short_code}` path pair to an item: the family
/// must be one of the five plural names (404 otherwise — it is a path
/// segment) and the short code must name an item of exactly that type (404
/// otherwise, same as the per-family routes).
///
/// `liveness` decides whether archived work resolves (KAIROS-A-0020). The
/// read paths that exist to answer "what did this say?" — history above all
/// — pass [`Liveness::IncludeArchived`]; everything that writes passes
/// [`Liveness::LiveOnly`].
pub fn resolve_family_item(
    conn: &mut PgConnection,
    family: &str,
    short_code: &str,
    liveness: Liveness,
) -> Result<(Uuid, ItemType), ApiError> {
    let item_type = item_type_of_family(family).ok_or_else(|| {
        ApiError::not_found(format!(
            "{family:?} is not an entity family. The entity families are: strategies, \
             initiatives, tasks, documents, adrs."
        ))
    })?;
    let found = resolve_short_code(conn, short_code, liveness)?
        .filter(|(_, resolved)| *resolved == item_type);
    // COLLIERY-T-3100: a read follows a retired code; a write is refused.
    // A rename keeps the type letter, so the family stays the same.
    crate::api::found_or_follow_retired(
        conn,
        found,
        item_type.entity_type(),
        short_code,
        liveness,
        |conn, current, liveness| {
            resolve_short_code(conn, current, liveness)?
                .filter(|(_, resolved)| *resolved == item_type)
                .ok_or_else(|| short_code_not_found(item_type.entity_type(), current))
        },
    )
}

/// A metadata definition's option values, in display order (empty for
/// non-enum definitions).
pub fn enum_option_values(
    conn: &mut PgConnection,
    definition_id: Uuid,
) -> Result<Vec<String>, ApiError> {
    use kairos_db::schema::metadata_enum_options as options;
    options::table
        .filter(options::metadata_definition_id.eq(definition_id))
        .order(options::position.asc())
        .select(options::value)
        .load(conn)
        .map_err(ApiError::internal)
}

/// The KAIROS-A-0003 value check: `string` passes through, `date` must
/// parse as `YYYY-MM-DD`, `enum` must be a member of the definition's
/// `metadata_enum_options`. Failures are 422 `VALIDATION` naming the
/// definition and (for enums) the allowed values.
pub fn validate_metadata_value(
    conn: &mut PgConnection,
    definition: &MetadataDefinition,
    value: &str,
) -> Result<(), ApiError> {
    match definition.field_type {
        FieldType::String => Ok(()),
        FieldType::Date => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(|_| ())
            .map_err(|_| {
                ApiError::validation(format!(
                    "The metadata {:?} is a date. The value {value:?} is not a date. Send \
                     the date as YYYY-MM-DD.",
                    definition.slug
                ))
            }),
        FieldType::Enum => {
            let allowed = enum_option_values(conn, definition.id)?;
            if allowed.iter().any(|option| option == value) {
                Ok(())
            } else {
                Err(ApiError::validation(format!(
                    "The value {value:?} is not a value of the metadata {:?}. The values \
                     are: {}.",
                    definition.slug,
                    allowed.join(", ")
                )))
            }
        }
    }
}

/// Resolve and validate every entry of a metadata write, writing nothing.
///
/// Phase 1 of both the REST `PATCH .../metadata` handler and the MCP
/// `set_metadata` tool. **It is shared on purpose** (KAIROS-T-0096): the two
/// loops used to be near-identical copies, and the KAIROS-T-0078 entity-type
/// scoping guard landed on the REST copy only. So an agent could stamp a
/// documents-only definition like `document_type` onto a task through MCP,
/// while the GUI refused the same write with a 422 and never offered the field.
///
/// That is the shape of bug worth designing against rather than fixing twice.
/// Agents are a first-class writer here (KAIROS-A-0011), so MCP is not a side
/// door with relaxed rules — and any future rule added to one path would have
/// gone the same way. There is now one path to add it to.
///
/// A bad entry rejects the whole write: nothing is applied until every entry
/// validates (KAIROS-A-0003).
pub fn validated_metadata_ops(
    conn: &mut PgConnection,
    item_type: ItemType,
    values: &std::collections::BTreeMap<String, Option<String>>,
) -> Result<Vec<(Uuid, Option<String>)>, ApiError> {
    use kairos_db::schema::metadata_definitions as definitions;

    let mut ops: Vec<(Uuid, Option<String>)> = Vec::with_capacity(values.len());
    for (definition_slug, value) in values {
        let definition: MetadataDefinition = definitions::table
            .filter(definitions::slug.eq(definition_slug))
            .select(MetadataDefinition::as_select())
            .first(conn)
            .optional()
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                ApiError::validation(format!(
                    "No metadata definition has the slug {definition_slug:?}."
                ))
            })?;

        // KAIROS-T-0078: entity-type scoping is enforced on the write path, not
        // merely hidden in pickers. Clearing an out-of-scope value stays
        // allowed, deliberately — that is how an item gets rid of a value some
        // earlier version let it acquire, and refusing the cleanup would strand
        // exactly the rows this guard is meant to prevent.
        if value.is_some()
            && !kairos_db::items::definition_applies_to(
                conn,
                definition.id,
                item_type.entity_type(),
            )
            .map_err(ApiError::internal)?
        {
            return Err(ApiError::validation(format!(
                "The metadata definition {definition_slug:?} does not apply to an item of \
                 the type {}.",
                item_type.entity_type()
            )));
        }

        if let Some(value) = value {
            validate_metadata_value(conn, &definition, value)?;
        }
        ops.push((definition.id, value.clone()));
    }
    Ok(ops)
}

/// An item's metadata values hydrated with their definitions, ordered by
/// definition slug — the shared payload of the metadata GET and PATCH.
pub fn item_metadata_response(
    conn: &mut PgConnection,
    item_id: Uuid,
    short_code: &str,
) -> Result<dto::ItemMetadataResponse, ApiError> {
    use kairos_db::schema::{item_metadata, metadata_definitions};
    let rows: Vec<(ItemMetadata, MetadataDefinition)> = item_metadata::table
        .inner_join(metadata_definitions::table)
        .filter(item_metadata::item_id.eq(item_id))
        .order(metadata_definitions::slug.asc())
        .select((ItemMetadata::as_select(), MetadataDefinition::as_select()))
        .load(conn)
        .map_err(ApiError::internal)?;
    Ok(dto::ItemMetadataResponse {
        short_code: short_code.to_string(),
        values: rows
            .into_iter()
            .map(|(value, definition)| dto::MetadataValue {
                definition_id: definition.id.to_string(),
                slug: definition.slug,
                name: definition.name,
                field_type: definition.field_type.to_string(),
                value: value.value,
            })
            .collect(),
    })
}
