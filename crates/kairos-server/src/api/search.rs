//! `POST /api/search` (KAIROS-T-0021) — the unified search endpoint
//! exposing the KAIROS-T-0014 pipeline exactly per KAIROS-A-0007 / S-0005:
//! q/filter/traverse composition, typed request validation, fully-typed
//! results grouped by entity type (empty groups omitted), pagination.
//!
//! Search is a read: per KAIROS-A-0006 reads are open tenant-wide, so the
//! endpoint sits behind the full auth → tenant middleware stack (401/403
//! for unauthenticated/non-member callers) with no capability check.
//!
//! # Error contract
//!
//! - Structurally invalid requests — including everything
//!   [`kairos_core::search::validate`] rejects (no capability present,
//!   missing/over-cap traverse depth, blank q, inverted date range, bad
//!   limit/offset, ...) and DTO-level conversion failures (malformed
//!   UUIDs/dates, out-of-vocabulary enum values, unknown fields) — are
//!   **400** `VALIDATION` in the S-0005 envelope, with the offending field
//!   named in `details.field` (plus the typed extras the core error
//!   carries, e.g. `details.cap` on an over-cap depth).
//! - A `traverse.from` naming no entity the request may see is 404
//!   `NOT_FOUND` (same convention as unknown short codes elsewhere in the
//!   API). Since KAIROS-T-0157 that means unknown, or archived *without*
//!   `filter.include_deleted` — traversing from archived work is the audit
//!   question, so the flag reaches the root lookup too (KAIROS-A-0020).
//! - Everything else from the pipeline is a 500.
//!
//! # Archived work (KAIROS-A-0020, KAIROS-T-0157)
//!
//! `filter.include_deleted` composes with every other capability, `q`
//! included. On its own it is a complete request ("show me the archived
//! work") rather than a 400. Archived hits are served marked: every entity
//! DTO carries `archived_at`, and it is non-null exactly for those rows.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use kairos_client::types as dto;
use kairos_client::types_search as dto_search;
use kairos_core::search as core_search;
use kairos_core::search::SearchValidationError;
use kairos_db::search::{SearchError, SearchResults, execute_search};
use serde_json::json;
use uuid::Uuid;

use super::convert::IntoDto;
use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/search", post(search))
}

/// Unified search (KAIROS-A-0007): full-text `q`, structured `filter`, and
/// graph `traverse` compose freely; at least one must be present.
#[utoipa::path(
    post,
    path = "/api/search",
    tag = "search",
    request_body = dto_search::SearchRequest,
    responses(
        (status = 200, description = "Matches grouped by entity type (empty groups omitted); total counted before pagination", body = dto_search::SearchResponse),
        (status = 400, description = "Invalid search request; details.field names the offending field", body = dto::ErrorEnvelope),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
        (status = 403, description = "Not a member of the organization", body = dto::ErrorEnvelope),
        (status = 404, description = "traverse.from names no entity the request may see (unknown, or archived without filter.include_deleted)", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn search(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<dto_search::SearchResponse>, ApiError> {
    // Deserialize by hand so shape errors (unknown fields, wrong JSON
    // types) surface as the S-0005 envelope, not axum's default rejection.
    let request: dto_search::SearchRequest = serde_json::from_value(body).map_err(|e| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "VALIDATION",
            format!("malformed search request: {e}"),
        )
    })?;
    let mut core = to_core(&request)?;
    let repository = request.filter.as_ref().and_then(|f| f.repository.clone());
    // Validate before dispatching to the blocking pool: an invalid request
    // never costs a connection checkout. A filter carrying only
    // `repository` becomes constraining once resolved, so that case is
    // validated after resolution instead. `execute_search` re-validates
    // (cheaply) as part of its own contract.
    if repository.is_none() {
        core_search::validate(&core).map_err(map_validation_error)?;
    }

    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            if let Some(reference) = repository.as_deref() {
                let repo = kairos_db::repositories::resolve(conn, reference)
                    .map_err(crate::api::tasks::map_repository_error)?;
                core.filter
                    .get_or_insert_with(Default::default)
                    .repository_id = Some(repo.id);
                core_search::validate(&core).map_err(map_validation_error)?;
            }
            let mut response = execute_search(conn, &core)
                .map(into_response)
                .map_err(map_search_error)?;
            // KAIROS-T-0104: embed the repository ref on task hits.
            super::convert::attach_repositories(conn, &mut response.results.tasks)
                .map_err(ApiError::internal)?;
            Ok(response)
        })
        .await?;
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// DTO → core conversion (wire strings → typed values, 400 on failure)
// ---------------------------------------------------------------------------

/// 400 `VALIDATION` naming the offending field in `details.field`.
fn field_invalid(field: &str, message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "VALIDATION", message)
        .with_details(json!({ "field": field }))
}

/// Parse a UUID-carrying field.
fn uuid_field(value: &str, field: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value)
        .map_err(|_| field_invalid(field, format!("{field} must be a UUID, got {value:?}")))
}

/// Parse an RFC 3339 timestamp field.
fn timestamp_field(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| {
            field_invalid(
                field,
                format!("{field} must be an RFC 3339 timestamp, got {value:?}"),
            )
        })
}

/// Parse a closed-vocabulary field through the core model's serde
/// vocabulary (the single source of truth for the allowed values).
fn enum_field<T: serde::de::DeserializeOwned>(
    value: &str,
    field: &str,
    allowed: &str,
) -> Result<T, ApiError> {
    serde_json::from_value(json!(value)).map_err(|_| {
        field_invalid(
            field,
            format!("{field} must be one of [{allowed}], got {value:?}"),
        )
    })
}

/// Convert the wire request into the typed `kairos_core::search` request.
/// Malformed values are field-level 400s; structural rules (at least one
/// capability, depth required/capped, ...) are `validate`'s job afterwards.
fn to_core(request: &dto_search::SearchRequest) -> Result<core_search::SearchRequest, ApiError> {
    Ok(core_search::SearchRequest {
        q: request.q.clone(),
        filter: request.filter.as_ref().map(filter_to_core).transpose()?,
        traverse: request
            .traverse
            .as_ref()
            .map(traverse_to_core)
            .transpose()?,
        sort: request.sort.as_ref().map(sort_to_core).transpose()?,
        limit: request.limit,
        offset: request.offset,
    })
}

fn filter_to_core(
    filter: &dto_search::SearchFilter,
) -> Result<core_search::SearchFilter, ApiError> {
    let entity_type = filter
        .entity_type
        .as_ref()
        .map(|types| {
            types
                .iter()
                .map(|t| {
                    enum_field(
                        t,
                        "filter.entity_type",
                        "strategy, initiative, task, document, adr",
                    )
                })
                .collect::<Result<Vec<core_search::SearchEntityType>, _>>()
        })
        .transpose()?;
    let task_type = filter
        .task_type
        .as_ref()
        .map(|types| {
            types
                .iter()
                .map(|t| enum_field(t, "filter.task_type", "task, bug, tech_debt, support"))
                .collect::<Result<Vec<core_search::SearchTaskType>, _>>()
        })
        .transpose()?;
    let work_class = filter
        .work_class
        .as_ref()
        .map(|classes| {
            classes
                .iter()
                .map(|c| enum_field(c, "filter.work_class", "planned, support"))
                .collect::<Result<Vec<core_search::SearchWorkClass>, _>>()
        })
        .transpose()?;
    Ok(core_search::SearchFilter {
        entity_type,
        board_id: filter
            .board_id
            .as_deref()
            .map(|v| uuid_field(v, "filter.board_id"))
            .transpose()?,
        column_id: filter
            .column_id
            .as_deref()
            .map(|v| uuid_field(v, "filter.column_id"))
            .transpose()?,
        team_id: filter
            .team_id
            .as_deref()
            .map(|v| uuid_field(v, "filter.team_id"))
            .transpose()?,
        // `filter.repository` is slug-or-UUID and needs a connection to
        // resolve; the handler injects it after conversion (KAIROS-T-0115).
        repository_id: None,
        task_type,
        work_class,
        is_bucket: filter.is_bucket,
        metadata: filter.metadata.clone(),
        created_after: filter
            .created_after
            .as_deref()
            .map(|v| timestamp_field(v, "filter.created_after"))
            .transpose()?,
        created_before: filter
            .created_before
            .as_deref()
            .map(|v| timestamp_field(v, "filter.created_before"))
            .transpose()?,
        include_deleted: filter.include_deleted,
    })
}

fn traverse_to_core(
    traverse: &dto_search::SearchTraverse,
) -> Result<core_search::Traverse, ApiError> {
    Ok(core_search::Traverse {
        from: core_search::TraverseFrom {
            short_code: traverse.from.short_code.clone(),
            id: traverse
                .from
                .id
                .as_deref()
                .map(|v| uuid_field(v, "traverse.from.id"))
                .transpose()?,
        },
        relationships: traverse
            .relationships
            .iter()
            .map(|r| {
                enum_field(
                    r,
                    "traverse.relationships",
                    "parent, supports, informs, supersedes, blocks",
                )
            })
            .collect::<Result<Vec<core_search::SearchRelationship>, _>>()?,
        direction: enum_field(
            &traverse.direction,
            "traverse.direction",
            "outbound, inbound, both",
        )?,
        depth: traverse.depth,
    })
}

fn sort_to_core(sort: &dto_search::SearchSort) -> Result<core_search::Sort, ApiError> {
    Ok(core_search::Sort {
        field: enum_field(&sort.field, "sort.field", "created_at, updated_at, title")?,
        order: enum_field(&sort.order, "sort.order", "asc, desc")?,
    })
}

// ---------------------------------------------------------------------------
// Error mapping (core validation → 400 with field-level detail)
// ---------------------------------------------------------------------------

/// [`SearchValidationError`] → 400 `VALIDATION`. The message is the core
/// error's; `details.field` names the offending field (`details.fields`
/// lists the alternatives for the at-least-one-capability rule), plus the
/// typed extras the variant carries.
fn map_validation_error(e: SearchValidationError) -> ApiError {
    let details = match &e {
        SearchValidationError::NoCapability => json!({"fields": ["q", "filter", "traverse"]}),
        SearchValidationError::BlankQuery => json!({"field": "q"}),
        SearchValidationError::EmptyEntityTypes => json!({"field": "filter.entity_type"}),
        SearchValidationError::EmptyTaskTypes => json!({"field": "filter.task_type"}),
        SearchValidationError::EmptyWorkClasses => json!({"field": "filter.work_class"}),
        SearchValidationError::BlankMetadataKey => json!({"field": "filter.metadata"}),
        SearchValidationError::InvertedDateRange { after, before } => json!({
            "field": "filter.created_after",
            "created_after": after.to_rfc3339(),
            "created_before": before.to_rfc3339(),
        }),
        SearchValidationError::TraverseFromMissing
        | SearchValidationError::TraverseFromAmbiguous => json!({"field": "traverse.from"}),
        SearchValidationError::NoRelationships => json!({"field": "traverse.relationships"}),
        SearchValidationError::TraverseDepthRequired => json!({"field": "traverse.depth"}),
        SearchValidationError::TraverseDepthOutOfRange { depth, cap } => {
            json!({"field": "traverse.depth", "depth": depth, "cap": cap})
        }
        SearchValidationError::LimitOutOfRange { limit, cap } => {
            json!({"field": "limit", "limit": limit, "cap": cap})
        }
        SearchValidationError::NegativeOffset { offset } => {
            json!({"field": "offset", "offset": offset})
        }
    };
    ApiError::new(StatusCode::BAD_REQUEST, "VALIDATION", e.to_string()).with_details(details)
}

/// [`SearchError`] → HTTP (module docs).
fn map_search_error(e: SearchError) -> ApiError {
    match e {
        SearchError::Invalid(e) => map_validation_error(e),
        SearchError::TraverseRootNotFound { reference } => {
            ApiError::not_found(format!("traverse root {reference:?} does not exist"))
        }
        SearchError::Database(e) => ApiError::internal(e),
    }
}

// ---------------------------------------------------------------------------
// Results → response DTO
// ---------------------------------------------------------------------------

/// Convert the pipeline's typed results into the S-0005 response shape
/// (empty groups are omitted by the DTO's serde attributes).
fn into_response(results: SearchResults) -> dto_search::SearchResponse {
    dto_search::SearchResponse {
        results: dto_search::SearchResultGroups {
            strategies: results
                .strategies
                .into_iter()
                .map(IntoDto::into_dto)
                .collect(),
            initiatives: results
                .initiatives
                .into_iter()
                .map(IntoDto::into_dto)
                .collect(),
            tasks: results.tasks.into_iter().map(IntoDto::into_dto).collect(),
            documents: results
                .documents
                .into_iter()
                .map(IntoDto::into_dto)
                .collect(),
            adrs: results.adrs.into_iter().map(IntoDto::into_dto).collect(),
        },
        total: results.total,
        limit: results.limit,
        offset: results.offset,
    }
}
