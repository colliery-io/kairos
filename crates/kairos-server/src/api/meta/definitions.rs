//! `/api/metadata-definitions` (KAIROS-S-0005, model per KAIROS-A-0003):
//! CRUD over the tenant's reusable typed metadata fields.
//!
//! Reads are open tenant-wide; writes are org-admin only (A-0006). Enum
//! definitions carry their option list (`metadata_enum_options`); DELETE
//! is refused with 409 `DEFINITION_IN_USE` while any `item_metadata`
//! value or `template_metadata` association references the definition —
//! the FKs are `ON DELETE CASCADE`, so the application check is the
//! enforcement (S-0005 "fails if in use").
//!
//! That refusal **names** its blockers (KAIROS-T-0162): the short codes
//! of the work carrying a value, archived carriers marked as such, and
//! the slugs of the templates that collect the field. Counting an
//! archived carrier is legitimate under ADR-20 rule 6 — archived work is
//! still content — but only because rule 1 makes it reachable again
//! (KAIROS-T-0154/0155). A count alone left the admin nowhere to go,
//! which is what made KAIROS-T-0152 a trap.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use kairos_client::types as dto_base;
use kairos_client::types_meta as dto;
use kairos_db::models::enums::FieldType;
use kairos_db::models::templates::{
    MetadataDefinition, MetadataDefinitionChangeset, MetadataDefinitionScope,
    NewMetadataDefinition, NewMetadataEnumOption,
};
use serde_json::json;
use uuid::Uuid;

use super::{enum_option_values, require_org_admin};
use crate::api::convert_meta::definition_dto;
use crate::api::{clamp_pagination, parse_enum, parse_uuid};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/metadata-definitions",
            get(list_definitions).post(create_definition),
        )
        .route(
            "/api/metadata-definitions/{id}",
            get(get_definition)
                .patch(update_definition)
                .delete(delete_definition),
        )
}

/// Load a definition by id, or 404.
fn load(conn: &mut PgConnection, id: Uuid) -> Result<MetadataDefinition, ApiError> {
    use kairos_db::schema::metadata_definitions as definitions;
    definitions::table
        .filter(definitions::id.eq(id))
        .select(MetadataDefinition::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("no metadata definition {id} exists")))
}

/// The entity-type vocabulary for scope rows (KAIROS-T-0078) — matches
/// the DDL CHECK and `kairos_core::short_code::ItemType::entity_type()`.
const ENTITY_TYPES: &[&str] = &["strategy", "initiative", "task", "document", "adr"];

/// Load a definition's scope rows in vocabulary order (empty = applies
/// to every entity type).
fn scopes_of(conn: &mut PgConnection, definition_id: Uuid) -> Result<Vec<String>, ApiError> {
    use kairos_db::schema::metadata_definition_scopes as scopes;
    let mut rows: Vec<String> = scopes::table
        .filter(scopes::metadata_definition_id.eq(definition_id))
        .select(scopes::entity_type)
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.sort_by_key(|t| ENTITY_TYPES.iter().position(|v| v == t));
    Ok(rows)
}

/// Validate an entity_types list: known values, no duplicates.
fn check_entity_types(entity_types: &[String]) -> Result<(), ApiError> {
    for entity_type in entity_types {
        if !ENTITY_TYPES.contains(&entity_type.as_str()) {
            return Err(ApiError::validation(format!(
                "entity_types must be drawn from [{}], got {entity_type:?}",
                ENTITY_TYPES.join(", ")
            )));
        }
    }
    let mut deduped = entity_types.to_vec();
    deduped.sort();
    deduped.dedup();
    if deduped.len() != entity_types.len() {
        return Err(ApiError::validation("entity_types contains duplicates"));
    }
    Ok(())
}

/// Replace a definition's scope rows (delete + insert, caller's
/// transaction). An empty list clears the scopes — applies-to-all.
fn replace_scopes(
    conn: &mut PgConnection,
    definition_id: Uuid,
    entity_types: &[String],
) -> Result<(), DieselError> {
    use kairos_db::schema::metadata_definition_scopes as scopes;
    diesel::delete(scopes::table.filter(scopes::metadata_definition_id.eq(definition_id)))
        .execute(conn)?;
    let rows: Vec<MetadataDefinitionScope> = entity_types
        .iter()
        .map(|entity_type| MetadataDefinitionScope {
            metadata_definition_id: definition_id,
            entity_type: entity_type.clone(),
        })
        .collect();
    if !rows.is_empty() {
        diesel::insert_into(scopes::table)
            .values(&rows)
            .execute(conn)?;
    }
    Ok(())
}

/// Hydrate a definition row with its option values and scopes.
fn hydrate(
    conn: &mut PgConnection,
    definition: MetadataDefinition,
) -> Result<dto::MetadataDefinition, ApiError> {
    let options = enum_option_values(conn, definition.id)?;
    let entity_types = scopes_of(conn, definition.id)?;
    Ok(definition_dto(definition, options, entity_types))
}

/// The option-list rules shared by create and update: enum definitions
/// need at least one option; other types must not carry any.
fn check_option_rules(field_type: FieldType, options: &[String]) -> Result<(), ApiError> {
    match field_type {
        FieldType::Enum if options.is_empty() => Err(ApiError::validation(
            "an enum metadata definition needs at least one enum_options value",
        )),
        FieldType::String | FieldType::Date if !options.is_empty() => Err(ApiError::validation(
            format!("enum_options only apply to enum definitions (field_type is {field_type})"),
        )),
        _ => Ok(()),
    }
}

/// Replace a definition's option list (delete + insert, caller's
/// transaction).
fn replace_options(
    conn: &mut PgConnection,
    definition_id: Uuid,
    options: &[String],
) -> Result<(), DieselError> {
    use kairos_db::schema::metadata_enum_options as option_rows;
    diesel::delete(
        option_rows::table.filter(option_rows::metadata_definition_id.eq(definition_id)),
    )
    .execute(conn)?;
    let rows: Vec<NewMetadataEnumOption> = options
        .iter()
        .enumerate()
        .map(|(position, value)| NewMetadataEnumOption {
            metadata_definition_id: definition_id,
            value: value.clone(),
            position: position as i32,
        })
        .collect();
    if !rows.is_empty() {
        diesel::insert_into(option_rows::table)
            .values(&rows)
            .execute(conn)?;
    }
    Ok(())
}

/// Map the unique violations a definition write can hit (`slug` UNIQUE,
/// duplicate option values) to 422; everything else is internal.
fn map_write_error(e: DieselError) -> ApiError {
    match e {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => {
            ApiError::validation(format!(
                "metadata definition conflicts with an existing row: {}",
                info.message()
            ))
        }
        e => ApiError::internal(e),
    }
}

/// How many blockers a `DEFINITION_IN_USE` refusal names before it falls
/// back to "and N more" — the `BOARD_NOT_EMPTY` guard's precedent
/// (`api/org/mod.rs`), so a field stamped on 4,000 items does not answer
/// with 4,000 short codes.
const NAMED_BLOCKER_LIMIT: i64 = 20;

/// One work item carrying a definition's value: its short code, and
/// whether it is archived (`deleted_at` set).
struct Carrier {
    short_code: String,
    archived: bool,
}

impl Carrier {
    /// `ACME-T-0007`, or `ACME-T-0007 (archived)` — the marking is the
    /// point: it tells the admin the value is there but the item is
    /// hidden by default, so they know to go looking past the default
    /// listings (ADR-20 rule 3).
    fn label(&self) -> String {
        if self.archived {
            format!("{} (archived)", self.short_code)
        } else {
            self.short_code.clone()
        }
    }
}

/// The work items carrying `definition_id`'s values (at most `limit`),
/// live and archived alike, for a refusal that names what to clear.
///
/// `item_metadata.item_id` carries no FK — item ids span the five entity
/// tables in one shared UUID space — so this is one query per table, the
/// same shape as `live_board_item_codes`. Unlike that guard this one does
/// **not** filter on `deleted_at`: it asks "does anything still refer to
/// this definition?", not "is there still live work here?" (ADR-20
/// rules 5 and 6 are different questions).
fn carrying_items(
    conn: &mut PgConnection,
    definition_id: Uuid,
    limit: i64,
) -> Result<Vec<Carrier>, ApiError> {
    use kairos_db::schema::{adrs, documents, initiatives, item_metadata, strategies, tasks};

    /// The carriers in one entity table, cheapest form: the id subquery
    /// is repeated per table because it borrows nothing reusable.
    macro_rules! carriers_in {
        ($table:ident) => {{
            $table::table
                .filter(
                    $table::id.eq_any(
                        item_metadata::table
                            .select(item_metadata::item_id)
                            .filter(item_metadata::metadata_definition_id.eq(definition_id)),
                    ),
                )
                .order($table::short_code.asc())
                .limit(limit)
                .select(($table::short_code, $table::deleted_at))
                .load::<(String, Option<chrono::DateTime<chrono::Utc>>)>(conn)
                .map_err(ApiError::internal)?
                .into_iter()
                .map(|(short_code, deleted_at)| Carrier {
                    short_code,
                    archived: deleted_at.is_some(),
                })
        }};
    }

    let mut carriers: Vec<Carrier> = carriers_in!(strategies).collect();
    carriers.extend(carriers_in!(initiatives));
    carriers.extend(carriers_in!(tasks));
    carriers.extend(carriers_in!(documents));
    carriers.extend(carriers_in!(adrs));
    carriers.truncate(limit.max(0) as usize);
    Ok(carriers)
}

/// The slugs of the templates that collect `definition_id` (at most
/// `limit`). Templates are hard-deleted, so there is nothing to mark.
fn carrying_templates(
    conn: &mut PgConnection,
    definition_id: Uuid,
    limit: i64,
) -> Result<Vec<String>, ApiError> {
    use kairos_db::schema::{template_metadata, templates};
    templates::table
        .filter(
            templates::id.eq_any(
                template_metadata::table
                    .select(template_metadata::template_id)
                    .filter(template_metadata::metadata_definition_id.eq(definition_id)),
            ),
        )
        .order(templates::slug.asc())
        .limit(limit.max(0))
        .select(templates::slug)
        .load::<String>(conn)
        .map_err(ApiError::internal)
}

/// `[a, b, and 3 more]` — the named blockers plus however many were left
/// unnamed, so the admin knows the list is not the whole story.
fn blocker_list(labels: &[String], total: i64) -> String {
    let unnamed = total - labels.len() as i64;
    let mut rendered = labels.join(", ");
    if unnamed > 0 {
        if !rendered.is_empty() {
            rendered.push_str(", ");
        }
        rendered.push_str(&format!("and {unnamed} more"));
    }
    format!("[{rendered}]")
}

/// Query of [`list_definitions`]: pagination plus the KAIROS-T-0078
/// entity-type filter. (A local struct rather than `#[serde(flatten)]`
/// over [`dto_base::Pagination`] — serde_urlencoded does not flatten.)
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct DefinitionListQuery {
    /// Page size (default 50, max 200).
    #[serde(default)]
    limit: Option<i64>,
    /// Rows to skip (default 0).
    #[serde(default)]
    offset: Option<i64>,
    /// Only definitions applying to this entity type
    /// (`strategy|initiative|task|document|adr`): unscoped definitions
    /// plus those whose scopes include it.
    #[serde(default)]
    entity_type: Option<String>,
}

/// List metadata definitions with their enum options and scopes (open
/// tenant-wide), optionally filtered to one entity type's catalog.
#[utoipa::path(
    get,
    path = "/api/metadata-definitions",
    tag = "metadata-definitions",
    params(DefinitionListQuery),
    responses(
        (status = 200, description = "Page of definitions", body = dto_base::ListEnvelope<dto::MetadataDefinition>),
        (status = 422, description = "Bad entity_type value", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_definitions(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<DefinitionListQuery>,
) -> Result<Json<dto_base::ListEnvelope<dto::MetadataDefinition>>, ApiError> {
    let pagination = dto_base::Pagination {
        limit: query.limit,
        offset: query.offset,
    };
    let (limit, offset) = clamp_pagination(&pagination);
    if let Some(entity_type) = &query.entity_type
        && !ENTITY_TYPES.contains(&entity_type.as_str())
    {
        return Err(ApiError::validation(format!(
            "entity_type must be one of [{}], got {entity_type:?}",
            ENTITY_TYPES.join(", ")
        )));
    }
    let entity_type = query.entity_type;
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::metadata_definition_scopes as scopes;
            use kairos_db::schema::metadata_definitions as definitions;
            let mut base = definitions::table.into_boxed();
            let mut count = definitions::table.into_boxed();
            if let Some(entity_type) = &entity_type {
                // In scope = unscoped (no rows at all) OR a scope row for
                // this type exists.
                base = base.filter(
                    definitions::id
                        .eq_any(
                            scopes::table
                                .select(scopes::metadata_definition_id)
                                .filter(scopes::entity_type.eq(entity_type.clone())),
                        )
                        .or(definitions::id
                            .ne_all(scopes::table.select(scopes::metadata_definition_id))),
                );
                count = count.filter(
                    definitions::id
                        .eq_any(
                            scopes::table
                                .select(scopes::metadata_definition_id)
                                .filter(scopes::entity_type.eq(entity_type.clone())),
                        )
                        .or(definitions::id
                            .ne_all(scopes::table.select(scopes::metadata_definition_id))),
                );
            }
            let total: i64 = count.count().get_result(conn).map_err(ApiError::internal)?;
            let rows: Vec<MetadataDefinition> = base
                .order(definitions::slug.asc())
                .limit(limit)
                .offset(offset)
                .select(MetadataDefinition::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            let items = rows
                .into_iter()
                .map(|row| hydrate(conn, row))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(dto_base::ListEnvelope {
                items,
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Create a metadata definition (org admin only, A-0006).
#[utoipa::path(
    post,
    path = "/api/metadata-definitions",
    tag = "metadata-definitions",
    request_body = dto::CreateMetadataDefinitionRequest,
    responses(
        (status = 201, description = "Created", body = dto::MetadataDefinition),
        (status = 403, description = "Caller is not an org admin", body = dto_base::ErrorEnvelope),
        (status = 422, description = "Bad field_type, option rules, or duplicate slug", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_definition(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateMetadataDefinitionRequest>,
) -> Result<(StatusCode, Json<dto::MetadataDefinition>), ApiError> {
    require_org_admin(&tenant)?;
    let field_type = parse_enum(&body.field_type, "field_type", FieldType::ALL)?;
    check_option_rules(field_type, &body.enum_options)?;
    check_entity_types(&body.entity_types)?;
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::metadata_definitions as definitions;
            let created = conn
                .transaction::<MetadataDefinition, DieselError, _>(|conn| {
                    let created: MetadataDefinition = diesel::insert_into(definitions::table)
                        .values(NewMetadataDefinition {
                            name: body.name.clone(),
                            slug: body.slug.clone(),
                            field_type,
                            is_system_default: false,
                        })
                        .returning(MetadataDefinition::as_returning())
                        .get_result(conn)?;
                    replace_options(conn, created.id, &body.enum_options)?;
                    replace_scopes(conn, created.id, &body.entity_types)?;
                    Ok(created)
                })
                .map_err(map_write_error)?;
            hydrate(conn, created)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Get one definition with its enum options (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/metadata-definitions/{id}",
    tag = "metadata-definitions",
    params(("id" = String, Path, description = "Definition id (UUID)")),
    responses(
        (status = 200, description = "The definition", body = dto::MetadataDefinition),
        (status = 404, description = "Unknown id", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_definition(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::MetadataDefinition>, ApiError> {
    let id = parse_uuid(&id, "definition id")?;
    let definition = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let definition = load(conn, id)?;
            hydrate(conn, definition)
        })
        .await?;
    Ok(Json(definition))
}

/// Update a definition (org admin only): rename, re-slug, and/or replace
/// the enum option list. The field type is immutable — existing
/// `item_metadata` values were validated against it (A-0003).
#[utoipa::path(
    patch,
    path = "/api/metadata-definitions/{id}",
    tag = "metadata-definitions",
    params(("id" = String, Path, description = "Definition id (UUID)")),
    request_body = dto::UpdateMetadataDefinitionRequest,
    responses(
        (status = 200, description = "Updated", body = dto::MetadataDefinition),
        (status = 403, description = "Caller is not an org admin", body = dto_base::ErrorEnvelope),
        (status = 404, description = "Unknown id", body = dto_base::ErrorEnvelope),
        (status = 422, description = "Option rules or duplicate slug", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_definition(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::UpdateMetadataDefinitionRequest>,
) -> Result<Json<dto::MetadataDefinition>, ApiError> {
    require_org_admin(&tenant)?;
    let id = parse_uuid(&id, "definition id")?;
    if let Some(entity_types) = &body.entity_types {
        check_entity_types(entity_types)?;
    }
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::metadata_definitions as definitions;
            let existing = load(conn, id)?;
            if let Some(options) = &body.enum_options {
                if existing.field_type != FieldType::Enum {
                    return Err(ApiError::validation(format!(
                        "enum_options only apply to enum definitions \
                         (field_type is {})",
                        existing.field_type
                    )));
                }
                check_option_rules(FieldType::Enum, options)?;
            }
            let updated = conn
                .transaction::<MetadataDefinition, DieselError, _>(|conn| {
                    let updated: MetadataDefinition =
                        diesel::update(definitions::table.filter(definitions::id.eq(id)))
                            .set(MetadataDefinitionChangeset {
                                name: body.name.clone(),
                                slug: body.slug.clone(),
                                field_type: None,
                                is_system_default: None,
                                updated_at: Some(chrono::Utc::now()),
                            })
                            .returning(MetadataDefinition::as_returning())
                            .get_result(conn)?;
                    if let Some(options) = &body.enum_options {
                        replace_options(conn, id, options)?;
                    }
                    if let Some(entity_types) = &body.entity_types {
                        replace_scopes(conn, id, entity_types)?;
                    }
                    Ok(updated)
                })
                .map_err(map_write_error)?;
            hydrate(conn, updated)
        })
        .await?;
    Ok(Json(updated))
}

/// Delete a definition (org admin only). Refused with 409
/// `DEFINITION_IN_USE` while any item value or template association
/// references it (S-0005 "fails if in use") — and the refusal names the
/// carrying work and templates, archived carriers marked (KAIROS-T-0162).
#[utoipa::path(
    delete,
    path = "/api/metadata-definitions/{id}",
    tag = "metadata-definitions",
    params(("id" = String, Path, description = "Definition id (UUID)")),
    responses(
        (status = 200, description = "Deleted", body = dto::DeletedResponse),
        (status = 403, description = "Caller is not an org admin", body = dto_base::ErrorEnvelope),
        (status = 404, description = "Unknown id", body = dto_base::ErrorEnvelope),
        (status = 409, description = "Definition is in use; the message names the carrying items and templates", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_definition(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::DeletedResponse>, ApiError> {
    require_org_admin(&tenant)?;
    let id = parse_uuid(&id, "definition id")?;
    let deleted = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::{item_metadata, metadata_definitions, template_metadata};
            let definition = load(conn, id)?;
            let item_values: i64 = item_metadata::table
                .filter(item_metadata::metadata_definition_id.eq(id))
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let template_fields: i64 = template_metadata::table
                .filter(template_metadata::metadata_definition_id.eq(id))
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            if item_values + template_fields > 0 {
                let carriers = carrying_items(conn, id, NAMED_BLOCKER_LIMIT)?;
                let templates = carrying_templates(conn, id, NAMED_BLOCKER_LIMIT)?;
                let has_archived = carriers.iter().any(|carrier| carrier.archived);
                let mut blockers: Vec<String> = Vec::new();
                if item_values > 0 {
                    let labels: Vec<String> =
                        carriers.iter().map(|carrier| carrier.label()).collect();
                    blockers.push(format!("carried by {}", blocker_list(&labels, item_values)));
                }
                if template_fields > 0 {
                    blockers.push(format!(
                        "collected by template(s) {}",
                        blocker_list(&templates, template_fields)
                    ));
                }
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "DEFINITION_IN_USE",
                    format!(
                        "metadata definition {:?} is in use ({item_values} item \
                         value(s), {template_fields} template field(s)): {}; \
                         clear those references first{}",
                        definition.slug,
                        blockers.join("; "),
                        if has_archived {
                            " — an archived carrier is still readable by short \
                             code, but must be restored before its value can be \
                             cleared"
                        } else {
                            ""
                        }
                    ),
                )
                .with_details(json!({
                    "item_values": item_values,
                    "template_fields": template_fields,
                    "items": carriers
                        .iter()
                        .map(|carrier| json!({
                            "short_code": carrier.short_code,
                            "archived": carrier.archived,
                        }))
                        .collect::<Vec<_>>(),
                    "templates": templates,
                })));
            }
            diesel::delete(metadata_definitions::table.filter(metadata_definitions::id.eq(id)))
                .execute(conn)
                .map_err(ApiError::internal)?;
            Ok(dto::DeletedResponse { id: id.to_string() })
        })
        .await?;
    Ok(Json(deleted))
}

#[cfg(test)]
mod tests {
    use super::blocker_list;

    /// The cap is what makes naming blockers affordable, so the remainder
    /// has to be visible: a field stamped on thousands of items must
    /// still answer with something readable, and the admin must know the
    /// list is partial. (`tests/meta.rs` covers the query itself against
    /// a live stack.)
    #[test]
    fn blocker_list_names_what_it_can_and_counts_the_rest() {
        assert_eq!(blocker_list(&["ACME-T-0001".into()], 1), "[ACME-T-0001]");
        assert_eq!(
            blocker_list(&["ACME-T-0001".into(), "ACME-T-0002 (archived)".into()], 5),
            "[ACME-T-0001, ACME-T-0002 (archived), and 3 more]"
        );
        // A carrier id that resolves to none of the five entity tables
        // cannot be named; the count still has to own up to it.
        assert_eq!(blocker_list(&[], 4), "[and 4 more]");
    }
}
