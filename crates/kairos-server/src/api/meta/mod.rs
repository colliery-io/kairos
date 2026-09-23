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
//!   tenant middleware already resolved). Relationship writes are org-admin
//!   EXCEPT the collaborative `parent`/`blocks` edges, which a member may
//!   write when they manage either end or authored the source
//!   ([`require_edge_capability`], KAIROS-T-0111).
//! - Item-metadata writes are gated by the item's `manage_<type>`
//!   capability on its authorization board
//!   ([`kairos_db::abac::resolve_authorization_board`]: own board for
//!   board items, the parent's board for documents via `supports`, the
//!   org-admin fallback when no board context exists).
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
        .merge(restore::router())
        .merge(metadata::router())
        .merge(definitions::router())
        .merge(templates::router())
        .merge(history::router())
        .merge(activity::router())
}

/// The A-0006 gate for tenant-wide configuration (relationships, metadata
/// definitions, templates): org admins only. The role was resolved from
/// `public.organization_members` by the tenant middleware.
pub fn require_org_admin(tenant: &TenantContext) -> Result<(), ApiError> {
    if tenant.role == OrgRole::Admin {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "this action requires the organization admin role \
             (relationships, metadata definitions, and templates are \
             tenant-wide configuration, KAIROS-A-0006)",
        )
        .with_details(json!({ "required_role": "admin" })))
    }
}

/// Authorize writing (or removing) one relationship edge (KAIROS-T-0111,
/// amending A-0006). Org admins may write any edge. For a COLLABORATIVE
/// type (`parent`, `blocks` — [`kairos_core::abac::is_collaborative_relationship`])
/// a member may write it when they hold `manage_<family>` on the source
/// item's board, or on the target item's board, or they CREATED the source
/// item — which is what lets a `file_backlog` filer (A-0019 §4) hang the
/// task they just filed under their initiative and mark what it blocks.
/// Every other type stays org-admin. One helper, shared by the HTTP
/// relationship routes, MCP `link_items`/`unlink_items`, and the `parent`
/// write inside MCP `create_item` — so the three cannot diverge.
pub fn require_edge_capability(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    relationship: &str,
    (source_id, source_type): (Uuid, ItemType),
    (target_id, target_type): (Uuid, ItemType),
) -> Result<(), ApiError> {
    if tenant.role == OrgRole::Admin {
        return Ok(());
    }
    if !kairos_core::abac::is_collaborative_relationship(relationship) {
        return Err(ApiError::forbidden(format!(
            "{relationship} relationships require the organization admin role; only [{}] \
             may be written by members (KAIROS-T-0111)",
            kairos_core::abac::COLLABORATIVE_RELATIONSHIPS.join(", ")
        ))
        .with_details(json!({ "required_role": "admin", "relationship": relationship })));
    }
    let target_board = kairos_db::abac::resolve_authorization_board(conn, target_id)
        .map_err(super::map_abac_error)?;
    require_edge_capability_on(
        conn,
        tenant,
        user,
        relationship,
        (source_id, source_type),
        (target_board, target_type),
    )
}

/// [`require_edge_capability`] for a target that may not exist yet (MCP
/// `create_item` writes the `parent` edge onto the item it is about to
/// create): the target is identified by the board it WILL sit on and its
/// type. The admin and non-collaborative arms are the caller's
/// ([`require_edge_capability`] handles them; this is its shared core).
pub fn require_edge_capability_on(
    conn: &mut PgConnection,
    tenant: &TenantContext,
    user: Uuid,
    relationship: &str,
    (source_id, source_type): (Uuid, ItemType),
    (target_board, target_type): (Option<Uuid>, ItemType),
) -> Result<(), ApiError> {
    if tenant.role == OrgRole::Admin {
        return Ok(());
    }
    if !kairos_core::abac::is_collaborative_relationship(relationship) {
        return Err(ApiError::forbidden(format!(
            "{relationship} relationships require the organization admin role; only [{}] \
             may be written by members (KAIROS-T-0111)",
            kairos_core::abac::COLLABORATIVE_RELATIONSHIPS.join(", ")
        ))
        .with_details(json!({ "required_role": "admin", "relationship": relationship })));
    }
    let slug = tenant.slug.as_str();
    let source_board = kairos_db::abac::resolve_authorization_board(conn, source_id)
        .map_err(super::map_abac_error)?;
    for (board, item_type) in [(source_board, source_type), (target_board, target_type)] {
        if let Some(board) = board
            && kairos_db::abac::authorize(conn, slug, board, user, manage_capability(item_type))
                .map_err(super::map_abac_error)?
        {
            return Ok(());
        }
    }
    if kairos_db::abac::item_created_by(conn, source_id).map_err(super::map_abac_error)?
        == Some(user)
    {
        return Ok(());
    }
    Err(ApiError::forbidden(format!(
        "a {relationship} edge needs manage_* on the source's or the target's board, or \
         authorship of the source item"
    ))
    .with_details(json!({ "relationship": relationship })))
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

/// The A-0006 manage capability for an entity type.
pub fn manage_capability(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Strategy => "manage_strategies",
        ItemType::Initiative => "manage_initiatives",
        ItemType::Task => "manage_tasks",
        ItemType::Document => "manage_documents",
        ItemType::Adr => "manage_adrs",
    }
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
            "unknown entity family {family:?}; expected one of \
             strategies, initiatives, tasks, documents, adrs"
        ))
    })?;
    resolve_short_code(conn, short_code, liveness)?
        .filter(|(_, resolved)| *resolved == item_type)
        .ok_or_else(|| short_code_not_found(item_type.entity_type(), short_code))
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
                    "metadata {:?} is a date field; {value:?} is not a valid \
                     YYYY-MM-DD date",
                    definition.slug
                ))
            }),
        FieldType::Enum => {
            let allowed = enum_option_values(conn, definition.id)?;
            if allowed.iter().any(|option| option == value) {
                Ok(())
            } else {
                Err(ApiError::validation(format!(
                    "metadata {:?} must be one of [{}], got {value:?}",
                    definition.slug,
                    allowed.join(", ")
                )))
            }
        }
    }
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
