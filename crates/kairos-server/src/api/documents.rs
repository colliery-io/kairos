//! `/api/documents` (KAIROS-S-0005) — see [`super`] for the shared T-0018
//! handler pattern. Documents do not live on boards and have no transition
//! route; authorization inherits from the parent workflow item's board via
//! the `supports` edge (KAIROS-A-0006).
//!
//! # The parent contract (recorded in KAIROS-T-0018)
//!
//! `POST /api/documents` REQUIRES `parent_short_code` naming a live
//! strategy, initiative, or task: the document is created and the
//! `supports` edge (parent = source, document = target, S-0004
//! orientation) is written by the T-0013 graph service, and
//! `manage_documents` is checked against the parent's board. A missing or
//! unknown parent, or a non-workflow parent, is 422 `VALIDATION`. When the
//! parent resolves to no board (off-board ADR ancestry cannot happen for
//! workflow parents, but defense-in-depth), the org-admin-only fallback
//! applies. Create + link are two service transactions; the parent
//! pre-checks make a link failure after create unreachable in practice.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_client::types as dto;
use kairos_core::short_code::ItemType;
use kairos_db::models::enums::RelationshipType;
use kairos_db::models::items::Document;
use kairos_db::{abac, graph, items};
use serde_json::json;

use super::convert::IntoDto;
use super::{
    clamp_pagination, map_abac_error, map_graph_error, map_item_error, parse_enum,
    parse_opt_uuid, require_capability, resolve_short_code, short_code_not_found,
};
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

/// The A-0006 manage capability for this family.
const MANAGE: &str = "manage_documents";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/documents", get(list_documents).post(create_document))
        .route(
            "/api/documents/{short_code}",
            get(get_document)
                .patch(update_document)
                .delete(delete_document),
        )
        .route(
            "/api/documents/{short_code}/lifecycle",
            axum::routing::patch(set_lifecycle),
        )
}

/// Set a document's editorial lifecycle (KAIROS-T-0078): a free-transition
/// label — draft | review | published | archived — gated like every other
/// document write (`manage_documents` on the authorization board). Not a
/// content edit: no version bump, no history row; activity-logged and
/// announced via the existing `item_updated` thin event.
#[utoipa::path(
    patch,
    path = "/api/documents/{short_code}/lifecycle",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    request_body = dto::SetLifecycleRequest,
    responses(
        (status = 200, description = "Lifecycle updated", body = dto::Document),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 422, description = "Bad lifecycle value", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn set_lifecycle(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::SetLifecycleRequest>,
) -> Result<Json<dto::Document>, ApiError> {
    let lifecycle = parse_enum(
        &body.lifecycle,
        "lifecycle",
        kairos_db::models::enums::DocumentLifecycle::ALL,
    )?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code)?;
            let board = authorization_board(conn, document.id)?;
            require_capability(conn, &slug, board, user, MANAGE)?;
            let updated = items::set_document_lifecycle(conn, document.id, lifecycle, user)
                .map_err(map_item_error)?;
            Ok(updated.into_dto())
        })
        .await?;
    Ok(Json(updated))
}

/// Load the live document with this short code, or 404.
fn load(conn: &mut PgConnection, short_code: &str) -> Result<Document, ApiError> {
    use kairos_db::schema::documents::dsl;
    dsl::documents
        .filter(dsl::short_code.eq(short_code))
        .filter(dsl::deleted_at.is_null())
        .select(Document::as_select())
        .first(conn)
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| short_code_not_found("document", short_code))
}

/// The board that authorizes writes to this document: its parent workflow
/// item's board via the `supports` edge (A-0006 inheritance); `None` = no
/// board context → org-admin-only fallback.
fn authorization_board(
    conn: &mut PgConnection,
    document_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, ApiError> {
    abac::resolve_authorization_board(conn, document_id).map_err(map_abac_error)
}

/// List documents (open tenant-wide, S-0005 list envelope).
#[utoipa::path(
    get,
    path = "/api/documents",
    tag = "documents",
    params(dto::Pagination),
    responses(
        (status = 200, description = "Page of documents", body = dto::ListEnvelope<dto::Document>),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn list_documents(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(pagination): Query<dto::Pagination>,
) -> Result<Json<dto::ListEnvelope<dto::Document>>, ApiError> {
    let (limit, offset) = clamp_pagination(&pagination);
    let envelope = state
        .blocking
        .run(&tenant.slug, move |conn| {
            use kairos_db::schema::documents::dsl;
            let total: i64 = dsl::documents
                .filter(dsl::deleted_at.is_null())
                .count()
                .get_result(conn)
                .map_err(ApiError::internal)?;
            let rows: Vec<Document> = dsl::documents
                .filter(dsl::deleted_at.is_null())
                .order(dsl::short_code.asc())
                .limit(limit)
                .offset(offset)
                .select(Document::as_select())
                .load(conn)
                .map_err(ApiError::internal)?;
            Ok(dto::ListEnvelope {
                items: rows.into_iter().map(IntoDto::into_dto).collect(),
                total,
                limit,
                offset,
            })
        })
        .await?;
    Ok(Json(envelope))
}

/// Get one document by short code (open tenant-wide).
#[utoipa::path(
    get,
    path = "/api/documents/{short_code}",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    responses(
        (status = 200, description = "The document", body = dto::Document),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_document(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::Document>, ApiError> {
    let document = state
        .blocking
        .run(&tenant.slug, move |conn| {
            Ok(load(conn, &short_code)?.into_dto())
        })
        .await?;
    Ok(Json(document))
}

/// Create a document attached to a workflow item (`parent_short_code`
/// REQUIRED — see the module docs). Requires `manage_documents` on the
/// parent's board. With `template_id`, the template's content and metadata
/// defaults are stamped (KAIROS-A-0003).
#[utoipa::path(
    post,
    path = "/api/documents",
    tag = "documents",
    request_body = dto::CreateDocumentRequest,
    responses(
        (status = 201, description = "Created (supports edge written)", body = dto::Document),
        (status = 403, description = "Missing capability on the parent's board", body = dto::ErrorEnvelope),
        (status = 422, description = "Missing/unknown parent, non-workflow parent, or unknown template", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn create_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<dto::CreateDocumentRequest>,
) -> Result<(StatusCode, Json<dto::Document>), ApiError> {
    let parent_short_code = body.parent_short_code.clone().ok_or_else(|| {
        ApiError::validation(
            "parent_short_code is required: documents attach to a strategy, initiative, \
             or task via a supports edge (KAIROS-A-0006)",
        )
    })?;
    let template_id = parse_opt_uuid(body.template_id.as_deref(), "template_id")?;
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let created = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (parent_id, parent_type) = resolve_short_code(conn, &parent_short_code)?
                .ok_or_else(|| {
                    ApiError::validation(format!(
                        "parent_short_code {parent_short_code:?} does not name a live item"
                    ))
                })?;
            if !matches!(
                parent_type,
                ItemType::Strategy | ItemType::Initiative | ItemType::Task
            ) {
                return Err(ApiError::validation(format!(
                    "parent_short_code {parent_short_code:?} is a {parent_type}; documents \
                     attach to a strategy, initiative, or task"
                )));
            }
            let board =
                abac::resolve_authorization_board(conn, parent_id).map_err(map_abac_error)?;
            require_capability(conn, &slug, board, user, MANAGE)?;
            let created = items::create_document(
                conn,
                items::CreateDocument {
                    title: &body.title,
                    content: body.content.as_deref(),
                    template_id,
                },
                user,
            )
            .map_err(map_item_error)?;
            graph::link_items(
                conn,
                parent_id,
                created.id,
                RelationshipType::Supports,
                user,
            )
            .map_err(map_graph_error)?;
            Ok(created.into_dto())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Update document content (KAIROS-A-0004 optimistic concurrency; requires
/// `manage_documents` on the parent's board — A-0006 inheritance).
#[utoipa::path(
    patch,
    path = "/api/documents/{short_code}",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    request_body = dto::UpdateContentRequest,
    responses(
        (status = 200, description = "Updated (new version)", body = dto::Document),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
        (status = 409, description = "Stale version; details.current carries the current entity", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn update_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
    Json(body): Json<dto::UpdateContentRequest>,
) -> Result<Json<dto::Document>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let updated = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code)?;
            let board = authorization_board(conn, document.id)?;
            require_capability(conn, &slug, board, user, MANAGE)?;
            let update = items::ContentUpdate {
                new_title: body.title.as_deref(),
                new_content: &body.content,
                expected_version: body.version,
            };
            match items::update_item_content(conn, ItemType::Document, document.id, update, user) {
                Ok(_) => Ok(load(conn, &short_code)?.into_dto()),
                Err(items::ItemError::VersionConflict {
                    expected_version,
                    current_version,
                    ..
                }) => {
                    let current = load(conn, &short_code)?.into_dto();
                    Err(ApiError::conflict(format!(
                        "version mismatch: expected {expected_version}, current is {current_version}"
                    ))
                    .with_details(json!({ "current": current })))
                }
                Err(e) => Err(map_item_error(e)),
            }
        })
        .await?;
    Ok(Json(updated))
}

/// Soft-delete a document (requires `manage_documents` on the parent's
/// board — A-0006 inheritance).
#[utoipa::path(
    delete,
    path = "/api/documents/{short_code}",
    tag = "documents",
    params(("short_code" = String, Path, description = "Document short code")),
    responses(
        (status = 200, description = "Soft-deleted; notes the cascade", body = dto::DeleteResponse),
        (status = 403, description = "Missing capability", body = dto::ErrorEnvelope),
        (status = 404, description = "Unknown short code", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn delete_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<dto::DeleteResponse>, ApiError> {
    let user = auth.user_id;
    let slug = tenant.slug.clone();
    let outcome = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let document = load(conn, &short_code)?;
            let board = authorization_board(conn, document.id)?;
            require_capability(conn, &slug, board, user, MANAGE)?;
            let outcome = items::soft_delete_item(conn, ItemType::Document, document.id, user)
                .map_err(map_item_error)?;
            Ok(dto::DeleteResponse {
                short_code: outcome.root_short_code,
                cascade_count: outcome.cascaded_short_codes.len() as i64,
                cascaded_short_codes: outcome.cascaded_short_codes,
            })
        })
        .await?;
    Ok(Json(outcome))
}
