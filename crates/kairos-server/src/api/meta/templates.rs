//! `/api/templates` (KAIROS-S-0005, model per KAIROS-A-0003): CRUD over
//! the tenant's document templates and their metadata-field associations.
//!
//! Reads are open tenant-wide; writes are org-admin only (A-0006). The
//! detail response carries the template content plus its associated
//! metadata definitions with defaults/required flags (what create-from-
//! template stamps). DELETE is a hard delete: templates have no ongoing
//! relationship to documents (A-0003 — `documents.template_id` is `ON
//! DELETE SET NULL`, associations cascade).

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use kairos_client::types as dto_base;
use kairos_client::types_meta as dto;
use kairos_db::models::templates::{
    MetadataDefinition, NewTemplate, NewTemplateMetadata, Template, TemplateChangeset,
    TemplateMetadata,
};
use uuid::Uuid;

use super::{enum_option_values, require_org_admin, validate_metadata_value};
use crate::api::convert::IntoDto;
use crate::api::convert_meta::template_detail_dto;
use crate::api::{clamp_pagination, parse_uuid};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/templates", get(list_templates).post(create_template))
        .route(
            "/api/templates/{id}",
            get(get_template)
                .patch(update_template)
                .delete(delete_template),
        )
}

/// Load a template by id, or 404.
fn load(conn: &mut PgConnection, id: Uuid) -> Result<Template, ApiError> {
    use kairos_db::schema::templates;
    templates::table
        .filter(templates::id.eq(id))
        .select(Template::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("no template {id} exists")))
}

/// The template's associated metadata fields, hydrated with their
/// definitions (ordered by definition slug).
fn metadata_fields(
    conn: &mut PgConnection,
    template_id: Uuid,
) -> Result<Vec<dto::TemplateMetadataField>, ApiError> {
    use kairos_db::schema::{metadata_definitions, template_metadata};
    let rows: Vec<(TemplateMetadata, MetadataDefinition)> = template_metadata::table
        .inner_join(metadata_definitions::table)
        .filter(template_metadata::template_id.eq(template_id))
        .order(metadata_definitions::slug.asc())
        .select((
            TemplateMetadata::as_select(),
            MetadataDefinition::as_select(),
        ))
        .load(conn)
        .map_err(ApiError::internal)?;
    rows.into_iter()
        .map(|(association, definition)| {
            let enum_options = enum_option_values(conn, definition.id)?;
            Ok(dto::TemplateMetadataField {
                definition_id: definition.id.to_string(),
                slug: definition.slug,
                name: definition.name,
                field_type: definition.field_type.to_string(),
                enum_options,
                default_value: association.default_value,
                required: association.required,
            })
        })
        .collect()
}

/// The detail payload (template + hydrated metadata fields).
fn detail(conn: &mut PgConnection, template: Template) -> Result<dto::TemplateDetail, ApiError> {
    let metadata = metadata_fields(conn, template.id)?;
    Ok(template_detail_dto(template, metadata))
}

/// Resolve + validate a template write's metadata entries (KAIROS-A-0003:
/// slugs must name existing definitions, defaults must satisfy the
/// definition's type, no duplicate slugs). Returns `(definition_id,
/// default_value, required)` triples ready to insert.
fn resolve_entries(
    conn: &mut PgConnection,
    entries: &[dto::TemplateMetadataEntry],
) -> Result<Vec<(Uuid, Option<String>, bool)>, ApiError> {
    use kairos_db::schema::metadata_definitions as definitions;
    let mut resolved: Vec<(Uuid, Option<String>, bool)> = Vec::with_capacity(entries.len());
    let mut seen: Vec<&str> = Vec::with_capacity(entries.len());
    for entry in entries {
        if seen.contains(&entry.definition_slug.as_str()) {
            return Err(ApiError::validation(format!(
                "duplicate metadata entry for definition slug {:?}",
                entry.definition_slug
            )));
        }
        seen.push(&entry.definition_slug);
        let definition: MetadataDefinition = definitions::table
            .filter(definitions::slug.eq(&entry.definition_slug))
            .select(MetadataDefinition::as_select())
            .first(conn)
            .optional()
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                ApiError::validation(format!(
                    "unknown metadata definition slug {:?}",
                    entry.definition_slug
                ))
            })?;
        if let Some(default_value) = &entry.default_value {
            validate_metadata_value(conn, &definition, default_value)?;
        }
        resolved.push((definition.id, entry.default_value.clone(), entry.required));
    }
    Ok(resolved)
}

/// Replace a template's metadata associations (caller's transaction).
fn replace_associations(
    conn: &mut PgConnection,
    template_id: Uuid,
    entries: &[(Uuid, Option<String>, bool)],
) -> Result<(), DieselError> {
    use kairos_db::schema::template_metadata;
    diesel::delete(template_metadata::table.filter(template_metadata::template_id.eq(template_id)))
        .execute(conn)?;
    let rows: Vec<NewTemplateMetadata> = entries
        .iter()
        .map(
            |(metadata_definition_id, default_value, required)| NewTemplateMetadata {
                template_id,
                metadata_definition_id: *metadata_definition_id,
                default_value: default_value.clone(),
                required: *required,
            },
        )
        .collect();
    if !rows.is_empty() {
        diesel::insert_into(template_metadata::table)
            .values(&rows)
            .execute(conn)?;
    }
    Ok(())
}

/// Map the unique violation a template write can hit (`slug` UNIQUE) to
/// 422; everything else is internal.
fn map_write_error(e: DieselError) -> ApiError {
    match e {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => {
            ApiError::validation(format!(
                "template conflicts with an existing row: {}",
                info.message()
            ))
        }
        e => ApiError::internal(e),
    }
}

/// List templates (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/templates",
    tag = "templates",
    params(dto_base::Pagination),
    responses(
        (status = 200, description = "Page of templates", body = dto_base::ListEnvelope<dto::Template>),
    ),
)]
pub(crate) async fn list_templates(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(pagination): Query<dto_base::Pagination>,
) -> Result<Json<dto_base::ListEnvelope<dto::Template>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::templates;
            let total: i64 = templates::table
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Template> = templates::table
                .order(templates::slug.asc())
                .limit(limit)
                .offset(offset)
                .select(Template::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            Ok(dto_base::ListEnvelope {
                items: rows.into_iter().map(IntoDto::into_dto).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Get one template: content plus its associated metadata definitions
/// with defaults/required flags (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/templates/{id}",
    tag = "templates",
    params(("id" = String, Path, description = "Template id (UUID)")),
    responses(
        (status = 200, description = "The template with its metadata fields", body = dto::TemplateDetail),
        (status = 404, description = "Unknown id", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_template(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::TemplateDetail>, ApiError> {
    let id = parse_uuid(&id, "template id")?;
    let template = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let template = load(conn, id)?;
            detail(conn, template)
        })
        .await?;
    Ok(Json(template))
}

/// Create a template (org admin only, A-0006). Metadata entries reference
/// existing definitions by slug; defaults are validated against the
/// definition's type (A-0003).
#[utoipa::path(
    post,
    path = "/api/templates",
    tag = "templates",
    request_body = dto::CreateTemplateRequest,
    responses(
        (status = 201, description = "Created", body = dto::TemplateDetail),
        (status = 403, description = "Caller is not an org admin", body = dto_base::ErrorEnvelope),
        (status = 422, description = "Unknown definition slug, invalid default, or duplicate slug", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_template(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateTemplateRequest>,
) -> Result<(StatusCode, Json<dto::TemplateDetail>), ApiError> {
    require_org_admin(&tenant)?;
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::templates;
            let entries = resolve_entries(conn, &body.metadata)?;
            let created = conn
                .transaction::<Template, DieselError, _>(|conn| {
                    let created: Template = diesel::insert_into(templates::table)
                        .values(NewTemplate {
                            name: body.name.clone(),
                            slug: body.slug.clone(),
                            content: body.content.clone(),
                            is_system_default: false,
                        })
                        .returning(Template::as_returning())
                        .get_result(conn)?;
                    replace_associations(conn, created.id, &entries)?;
                    Ok(created)
                })
                .map_err(map_write_error)?;
            detail(conn, created)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Update a template (org admin only): omitted fields are unchanged;
/// `metadata` replaces the full association list. Existing documents are
/// unaffected (A-0003: no ongoing relationship after stamping).
#[utoipa::path(
    patch,
    path = "/api/templates/{id}",
    tag = "templates",
    params(("id" = String, Path, description = "Template id (UUID)")),
    request_body = dto::UpdateTemplateRequest,
    responses(
        (status = 200, description = "Updated", body = dto::TemplateDetail),
        (status = 403, description = "Caller is not an org admin", body = dto_base::ErrorEnvelope),
        (status = 404, description = "Unknown id", body = dto_base::ErrorEnvelope),
        (status = 422, description = "Unknown definition slug, invalid default, or duplicate slug", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_template(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::UpdateTemplateRequest>,
) -> Result<Json<dto::TemplateDetail>, ApiError> {
    require_org_admin(&tenant)?;
    let id = parse_uuid(&id, "template id")?;
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::templates;
            load(conn, id)?;
            let entries = body
                .metadata
                .as_deref()
                .map(|metadata| resolve_entries(conn, metadata))
                .transpose()?;
            let updated = conn
                .transaction::<Template, DieselError, _>(|conn| {
                    let updated: Template =
                        diesel::update(templates::table.filter(templates::id.eq(id)))
                            .set(TemplateChangeset {
                                name: body.name.clone(),
                                slug: body.slug.clone(),
                                content: body.content.clone(),
                                is_system_default: None,
                                updated_at: Some(chrono::Utc::now()),
                            })
                            .returning(Template::as_returning())
                            .get_result(conn)?;
                    if let Some(entries) = &entries {
                        replace_associations(conn, id, entries)?;
                    }
                    Ok(updated)
                })
                .map_err(map_write_error)?;
            detail(conn, updated)
        })
        .await?;
    Ok(Json(updated))
}

/// Delete a template (org admin only). Hard delete: associations cascade
/// and existing documents keep their content (`documents.template_id` is
/// set NULL by the DDL).
#[utoipa::path(
    delete,
    path = "/api/templates/{id}",
    tag = "templates",
    params(("id" = String, Path, description = "Template id (UUID)")),
    responses(
        (status = 200, description = "Deleted", body = dto::DeletedResponse),
        (status = 403, description = "Caller is not an org admin", body = dto_base::ErrorEnvelope),
        (status = 404, description = "Unknown id", body = dto_base::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_template(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<dto::DeletedResponse>, ApiError> {
    require_org_admin(&tenant)?;
    let id = parse_uuid(&id, "template id")?;
    let deleted = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::templates;
            load(conn, id)?;
            diesel::delete(templates::table.filter(templates::id.eq(id)))
                .execute(conn)
                .map_err(ApiError::internal)?;
            Ok(dto::DeletedResponse { id: id.to_string() })
        })
        .await?;
    Ok(Json(deleted))
}
