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

use super::convert::IntoCard;
use crate::app::AppState;
use crate::body::ApiJson;
use crate::error::ApiError;
use crate::input::ApiQuery;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/search", post(search)).route(
        "/api/items/{short_code}/related",
        axum::routing::get(related),
    )
}

/// Related work for an item (KAIROS-A-0021 rules 5-7, KAIROS-T-0191).
///
/// A **separate surface**, deliberately. `/api/search` keeps its at-least-one
/// rule, its ≤5-query bound and its deterministic sort; callers relying on those
/// keep them, and nothing here changes what that endpoint does.
///
/// `GET` rather than `POST`, unlike `/api/search`: this takes one identifier and
/// is a read with no body, so it is cacheable and linkable and behaves the way a
/// reader expects.
#[utoipa::path(
    get,
    path = "/api/items/{short_code}/related",
    tag = "search",
    params(
        ("short_code" = String, Path, description = "The item to find related work for"),
        ("limit" = Option<usize>, Query, description = "How many proposals (default 5, max 10)"),
    ),
    responses(
        (status = 200, description = "Bounded proposals, best first. `vector: false` means a degraded, text-only answer rather than a wrong one", body = dto_search::RelatedWorkResponse),
        (status = 401, description = "Missing/invalid token", body = dto::ErrorEnvelope),
        (status = 403, description = "Not a member of the organization", body = dto::ErrorEnvelope),
        (status = 404, description = "No item with that short code", body = dto::ErrorEnvelope),
        (status = 503, description = "Embeddings are not enabled on this deployment", body = dto::ErrorEnvelope),
    ),
)]
pub(crate) async fn related(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    axum::extract::Path(short_code): axum::extract::Path<String>,
    ApiQuery(params): ApiQuery<RelatedQuery>,
) -> Result<Json<dto_search::RelatedWorkResponse>, ApiError> {
    let Some(service) = state.embedding.clone() else {
        // 503 rather than 404 or an empty list: the resource exists, this
        // deployment just has not enabled it, and an empty list would read as
        // "nothing is related" — a wrong answer dressed as a right one.
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "EMBEDDINGS_DISABLED",
            "This deployment does not have the search for related work. Use /api/search.",
        ));
    };
    let mut config = kairos_core::retrieval::RetrievalConfig::default();
    if let Some(limit) = params.limit {
        config.limit = limit.clamp(1, 10);
    }
    let found = state
        .blocking
        .run(&tenant.slug, move |conn| {
            service
                .related_work(conn, &short_code, &config)
                .map_err(related_work_error)
        })
        .await?;

    // The sparse-graph caveat belongs to the response, not to every proposal in
    // it: repeated per row it buries the part of each sentence that differs.
    let mut note = found.sources.note().to_string();
    if let Some(caveat) = kairos_core::retrieval::graph_caveat(&found.proposals) {
        note.push(' ');
        note.push_str(caveat);
    }

    Ok(Json(dto_search::RelatedWorkResponse {
        short_code: found.short_code,
        proposals: found
            .proposals
            .into_iter()
            .map(|p| dto_search::RelatedProposal {
                short_code: p.short_code,
                title: p.title,
                entity_type: p.entity_type,
                claim: match p.claim {
                    kairos_core::retrieval::Claim::ImplicitDependency => "implicit_dependency",
                    kairos_core::retrieval::Claim::NearDuplicate => "near_duplicate",
                    kairos_core::retrieval::Claim::PriorArt => "prior_art",
                }
                .to_string(),
                score: p.score,
                why: p.why,
            })
            .collect(),
        lexical: found.sources.lexical,
        vector: found.sources.vector,
        note,
    }))
}

/// The error of a search for related work (KAIROS-T-0561), for REST and
/// MCP: an unknown short code is `NOT_FOUND`; each other error is
/// `INTERNAL`, so that a fault of the store does not read as a missing item.
pub(crate) fn related_work_error(error: crate::embedding::RefreshError) -> ApiError {
    use crate::embedding::RefreshError;
    use kairos_db::embeddings::EmbeddingError;
    match error {
        RefreshError::Store(EmbeddingError::ItemNotFound(code)) => {
            ApiError::not_found(format!("No item has the short code {code:?}."))
        }
        other => ApiError::internal(other),
    }
}

/// Query parameters for [`related`].
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelatedQuery {
    /// How many proposals to return.
    pub limit: Option<usize>,
}

/// Unified search (KAIROS-A-0007): full-text `q`, structured `filter`, and
/// graph `traverse` compose freely; at least one must be present.
///
/// A hit is a card (KAIROS-T-0602). It has the title and a `snippet` of
/// the content near the match. It has no content. The item route of the
/// hit gives the content.
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
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> Result<Json<dto_search::SearchResponse>, ApiError> {
    // Deserialize by hand: a fault of shape (an unknown field, a wrong
    // JSON type) is a 400 on this route, and a 422 on each other route
    // (COLLIERY-T-0249). The message names an unknown field.
    let request: dto_search::SearchRequest = serde_json::from_value(body)
        .map_err(|e| crate::body::shape_error(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let mut core = to_core(&request)?;
    let refs = FilterRefs::of(request.filter.as_ref())?;
    let (slug, caller) = (tenant.slug.clone(), auth.user_id);
    // Validate before dispatching to the blocking pool: an invalid request
    // never costs a connection checkout. A filter carrying only
    // `repository` becomes constraining once resolved, so that case is
    // validated after resolution instead. `execute_search` re-validates
    // (cheaply) as part of its own contract.
    if refs.is_empty() {
        core_search::validate(&core).map_err(map_validation_error)?;
    }

    let response = state
        .blocking
        .run(&tenant.slug, move |conn| {
            if !refs.is_empty() {
                refs.resolve_into(conn, &slug, caller, &mut core)?;
                core_search::validate(&core).map_err(map_validation_error)?;
            }
            let mut response = execute_search(conn, &core)
                .map(|results| into_response(results, core.q.as_deref()))
                .map_err(map_search_error)?;
            // KAIROS-T-0104: embed the repository ref on task hits.
            super::convert::attach_repositories(conn, &mut response.results.tasks)
                .map_err(ApiError::internal)?;
            // COLLIERY-T-0269: the `impacts` links of each document and of
            // each ADR, one query for each of the two types.
            super::convert::attach_impacts(conn, &mut response.results.documents)?;
            super::convert::attach_impacts(conn, &mut response.results.adrs)?;
            Ok(response)
        })
        .await?;
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Filter references that need a connection (slug or UUID)
// ---------------------------------------------------------------------------

/// The references of a search filter that resolve with a connection: the
/// repository (KAIROS-T-0115), the board and the team by slug or UUID
/// (KAIROS-T-0368), and the persons of `claimed_by`, `owned_by` and
/// `created_by` (KAIROS-T-0488). The REST route and the MCP tool share
/// them.
#[derive(Debug, Clone, Default)]
pub(crate) struct FilterRefs {
    pub repository: Option<String>,
    pub board: Option<String>,
    /// A column by name (needs the board) or by UUID (KAIROS-T-0552).
    pub column: Option<String>,
    pub team: Option<String>,
    pub claimed_by: Option<String>,
    pub owned_by: Option<String>,
    pub created_by: Option<String>,
}

impl FilterRefs {
    /// The references of a wire filter. `board` with `board_id`, or `team`
    /// with `team_id`, is refused.
    pub(crate) fn of(filter: Option<&dto_search::SearchFilter>) -> Result<Self, ApiError> {
        let Some(filter) = filter else {
            return Ok(Self::default());
        };
        Ok(Self::new(
            filter.repository.clone(),
            (filter.board.clone(), filter.board_id.is_some()),
            (filter.team.clone(), filter.team_id.is_some()),
        )?
        .with_column(filter.column.clone(), filter.column_id.is_some())?
        .with_people(
            filter.claimed_by.clone(),
            filter.owned_by.clone(),
            filter.created_by.clone(),
        ))
    }

    /// The persons of `claimed_by`, `owned_by` and `created_by`
    /// (KAIROS-T-0488): each a user id, an email, a user name, or `me`.
    pub(crate) fn with_people(
        mut self,
        claimed_by: Option<String>,
        owned_by: Option<String>,
        created_by: Option<String>,
    ) -> Self {
        self.claimed_by = claimed_by;
        self.owned_by = owned_by;
        self.created_by = created_by;
        self
    }

    /// The column (KAIROS-T-0552): a name, resolved within the board of the
    /// filter, or a UUID. `column` with `column_id` is refused.
    pub(crate) fn with_column(
        mut self,
        column: Option<String>,
        has_column_id: bool,
    ) -> Result<Self, ApiError> {
        if column.is_some() && has_column_id {
            return Err(field_invalid(
                "filter.column",
                "The filter has column and column_id. Send only one of them.",
            ));
        }
        self.column = column;
        Ok(self)
    }

    /// From the values; each pair is the slug-or-UUID reference and
    /// whether the UUID field (`board_id`, `team_id`) is set too.
    pub(crate) fn new(
        repository: Option<String>,
        board: (Option<String>, bool),
        team: (Option<String>, bool),
    ) -> Result<Self, ApiError> {
        if board.0.is_some() && board.1 {
            return Err(field_invalid(
                "filter.board",
                "The filter has board and board_id. Send only one of them.",
            ));
        }
        if team.0.is_some() && team.1 {
            return Err(field_invalid(
                "filter.team",
                "The filter has team and team_id. Send only one of them.",
            ));
        }
        Ok(FilterRefs {
            repository,
            board: board.0,
            team: team.0,
            ..Default::default()
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.repository.is_none()
            && self.board.is_none()
            && self.column.is_none()
            && self.team.is_none()
            && self.claimed_by.is_none()
            && self.owned_by.is_none()
            && self.created_by.is_none()
    }

    /// Resolve each reference and put its id in the core filter. A board
    /// or a team that does not resolve is a `VALIDATION` refusal that
    /// names the field.
    ///
    /// A person is `me` (the caller: `caller`, also for an agent key) or a
    /// member of the organization `slug` by user id, email or user name.
    pub(crate) fn resolve_into(
        &self,
        conn: &mut diesel::pg::PgConnection,
        slug: &str,
        caller: Uuid,
        core: &mut core_search::SearchRequest,
    ) -> Result<(), ApiError> {
        use diesel::prelude::*;
        let filter = core.filter.get_or_insert_with(Default::default);
        if let Some(reference) = self.claimed_by.as_deref() {
            filter.claimed_by = Some(resolve_member(
                conn,
                slug,
                caller,
                reference,
                "filter.claimed_by",
            )?);
        }
        if let Some(reference) = self.owned_by.as_deref() {
            filter.owned_by = Some(resolve_member(
                conn,
                slug,
                caller,
                reference,
                "filter.owned_by",
            )?);
        }
        if let Some(reference) = self.created_by.as_deref() {
            filter.created_by = Some(resolve_member(
                conn,
                slug,
                caller,
                reference,
                "filter.created_by",
            )?);
        }
        if let Some(reference) = self.repository.as_deref() {
            let repo = kairos_db::repositories::resolve(conn, reference)
                .map_err(crate::api::tasks::map_repository_error)?;
            filter.repository_id = Some(repo.id);
        }
        if let Some(reference) = self.board.as_deref() {
            // KAIROS-T-0689: the forms of crate::references::BOARD_FORMS.
            let reference = crate::references::text(reference);
            let id: Uuid = crate::references::board(conn, reference)?
                .map(|board| board.id)
                .ok_or_else(|| {
                    field_invalid(
                        "filter.board",
                        format!(
                            "No live board has the slug or the id {reference:?}. Send the \
                             slug or the id of a board of the organization."
                        ),
                    )
                })?;
            filter.board_id = Some(id);
        }
        if let Some(reference) = self.column.as_deref() {
            use kairos_db::schema::board_columns;
            // KAIROS-T-0689: the forms of crate::references::COLUMN_FORMS.
            let reference = crate::references::text(reference);
            // KAIROS-T-0552: a UUID stands alone; a name needs the board.
            let by_id = Uuid::parse_str(reference).ok();
            let Some(board_id) = filter.board_id else {
                match by_id {
                    Some(id) => {
                        filter.column_id = Some(id);
                        return self.resolve_team(conn, filter);
                    }
                    None => {
                        return Err(field_invalid(
                            "filter.column",
                            "A column by name needs the board: send filter.board with it.",
                        ));
                    }
                }
            };
            let columns: Vec<(Uuid, String)> = board_columns::table
                .filter(board_columns::board_id.eq(board_id))
                .filter(board_columns::deleted_at.is_null())
                .order(board_columns::position.asc())
                .select((board_columns::id, board_columns::name))
                .load(conn)
                .map_err(ApiError::internal)?;
            let id = crate::references::column(
                columns.iter().map(|(id, name)| (*id, name.as_str())),
                reference,
            );
            let Some(id) = id else {
                let names: Vec<&str> = columns.iter().map(|(_, name)| name.as_str()).collect();
                return Err(field_invalid(
                    "filter.column",
                    format!(
                        "The board has no live column {reference:?}. Its columns are: {}.",
                        names.join(", ")
                    ),
                ));
            };
            filter.column_id = Some(id);
        }
        self.resolve_team(conn, filter)
    }

    /// The team of the filter (split out so the column branch can finish
    /// early; KAIROS-T-0552).
    fn resolve_team(
        &self,
        conn: &mut diesel::pg::PgConnection,
        filter: &mut core_search::SearchFilter,
    ) -> Result<(), ApiError> {
        if let Some(reference) = self.team.as_deref() {
            // KAIROS-T-0689: the one lookup of a team, and its refusal with
            // the slugs (KAIROS-T-0685), for the field filter.team.
            let team =
                crate::api::org::repositories::resolve_team(conn, reference).map_err(|e| {
                    if e.code == "VALIDATION" {
                        field_invalid("filter.team", e.message)
                    } else {
                        e
                    }
                })?;
            filter.team_id = Some(team.id);
        }
        Ok(())
    }
}

/// The member that `reference` names (KAIROS-T-0488), in the forms of
/// [`crate::references::PERSON_FORMS`] (KAIROS-T-0689): `me` is the caller.
/// A person and a service account both count. A reference that names no
/// member is a `VALIDATION` refusal that names the field and the
/// reference.
pub(crate) fn resolve_member(
    conn: &mut diesel::pg::PgConnection,
    slug: &str,
    caller: Uuid,
    reference: &str,
    field: &str,
) -> Result<Uuid, ApiError> {
    if crate::references::text(reference).eq_ignore_ascii_case("me") {
        return Ok(caller);
    }
    let mut member = None;
    for user in crate::references::people(conn, caller, reference)? {
        if kairos_db::abac::is_org_member(conn, slug, user.id).map_err(ApiError::internal)? {
            if member.is_some() {
                return Err(field_invalid(
                    field,
                    format!(
                        "More than one member of the organization has the name {reference:?}. \
                         Send the id of the member."
                    ),
                ));
            }
            member = Some(user.id);
        }
    }
    let reference = crate::references::text(reference);
    member.ok_or_else(|| {
        field_invalid(
            field,
            format!(
                "No member of the organization has the id, the email, the user name or the \
                 service-account name {reference:?} of {field}. Send me, or a member of the \
                 organization."
            ),
        )
        .with_details(json!({ "field": field, "person": reference }))
    })
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
    Uuid::parse_str(value).map_err(|_| {
        field_invalid(
            field,
            format!("The value {value:?} of {field} is not a UUID."),
        )
    })
}

/// Parse an RFC 3339 timestamp field.
fn timestamp_field(value: &str, field: &str) -> Result<DateTime<Utc>, ApiError> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| {
            field_invalid(
                field,
                format!(
                    "The value {value:?} of {field} is not a timestamp. Send an RFC 3339 \
                     timestamp."
                ),
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
            format!("The value {value:?} is not a value of {field}. The values are: {allowed}."),
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
        updated_after: filter
            .updated_after
            .as_deref()
            .map(|v| timestamp_field(v, "filter.updated_after"))
            .transpose()?,
        updated_before: filter
            .updated_before
            .as_deref()
            .map(|v| timestamp_field(v, "filter.updated_before"))
            .transpose()?,
        metadata_missing: filter.metadata_missing.clone(),
        // `claimed_by`, `owned_by` and `created_by` name a person and need
        // a connection to resolve; the handler injects them (KAIROS-T-0488).
        claimed_by: None,
        owned_by: None,
        created_by: None,
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
        field: enum_field(
            &sort.field,
            "sort.field",
            "created_at, updated_at, title, relevance",
        )?,
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
        SearchValidationError::InvertedUpdateRange { after, before } => json!({
            "field": "filter.updated_after",
            "updated_after": after.to_rfc3339(),
            "updated_before": before.to_rfc3339(),
        }),
        SearchValidationError::BlankMetadataMissing => json!({"field": "filter.metadata_missing"}),
        SearchValidationError::TraverseFromMissing
        | SearchValidationError::TraverseFromAmbiguous => json!({"field": "traverse.from"}),
        SearchValidationError::NoRelationships => json!({"field": "traverse.relationships"}),
        SearchValidationError::TraverseDepthRequired => json!({"field": "traverse.depth"}),
        SearchValidationError::TraverseDepthOutOfRange { depth, cap } => {
            json!({"field": "traverse.depth", "depth": depth, "cap": cap})
        }
        SearchValidationError::RelevanceWithoutQuery => {
            json!({"field": "sort.field", "requires": "q"})
        }
    };
    ApiError::new(StatusCode::BAD_REQUEST, "VALIDATION", e.to_string()).with_details(details)
}

/// [`SearchError`] → HTTP (module docs).
fn map_search_error(e: SearchError) -> ApiError {
    match e {
        SearchError::Invalid(e) => map_validation_error(e),
        SearchError::TraverseRootNotFound { reference } => ApiError::not_found(format!(
            "The item {reference:?} of traverse.from does not exist."
        )),
        SearchError::UnknownMetadataDefinition(ref slug) => {
            field_invalid("filter.metadata_missing", e.to_string())
                .with_details(json!({"field": "filter.metadata_missing", "slug": slug}))
        }
        SearchError::Database(e) => ApiError::internal(e),
    }
}

// ---------------------------------------------------------------------------
// Results → response DTO
// ---------------------------------------------------------------------------

/// The cards of the hits of one type (KAIROS-T-0602): the full row without
/// `content`, `created_by` and `updated_by`, and with a snippet of the
/// content. Documents do not use it: a document card has none of the
/// three fields.
macro_rules! hits {
    ($rows:expr, $q:expr) => {
        $rows
            .into_iter()
            .map(|row| {
                let snippet = snippet(&row.content, $q);
                let mut card = row.into_card();
                card.content = None;
                card.created_by = None;
                card.updated_by = None;
                card.snippet = snippet;
                card
            })
            .collect()
    };
}

/// The search response of the results: cards with a snippet, not the
/// content (KAIROS-T-0602). `q` places the snippet.
fn into_response(results: SearchResults, q: Option<&str>) -> dto_search::SearchResponse {
    dto_search::SearchResponse {
        results: dto_search::SearchResultGroups {
            strategies: hits!(results.strategies, q),
            initiatives: hits!(results.initiatives, q),
            tasks: hits!(results.tasks, q),
            documents: results
                .documents
                .into_iter()
                .map(|row| {
                    let snippet = snippet(&row.content, q);
                    let mut card: kairos_client::types_cards::DocumentCard = row.into_card();
                    card.snippet = snippet;
                    card
                })
                .collect(),
            adrs: hits!(results.adrs, q),
        },
        total: results.total,
        limit: results.limit,
        offset: results.offset,
    }
}

/// The longest snippet of a search hit, in characters (KAIROS-T-0602).
pub(crate) const SNIPPET_CHARS: usize = 200;

/// The snippet of a search hit (KAIROS-T-0602): about [`SNIPPET_CHARS`]
/// characters of `content`, with the white space collapsed. The window
/// starts a little before the first word of `q` that the content has
/// (case-insensitive). With no `q`, or with no word found, the window is
/// the start of the content. An ellipsis marks a cut. `None` for an empty
/// content. Pure.
pub(crate) fn snippet(content: &str, q: Option<&str>) -> Option<String> {
    let text: Vec<char> = {
        let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
        collapsed.chars().collect()
    };
    if text.is_empty() {
        return None;
    }
    let lower: Vec<char> = text
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let first_match = q
        .into_iter()
        .flat_map(str::split_whitespace)
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
                .chars()
                .collect::<Vec<char>>()
        })
        .filter(|word| !word.is_empty())
        .filter_map(|word| lower.windows(word.len()).position(|w| w == word.as_slice()))
        .min();
    // The window starts a quarter of the way before the match.
    let start = first_match
        .map(|at| at.saturating_sub(SNIPPET_CHARS / 4))
        .unwrap_or(0)
        .min(text.len().saturating_sub(SNIPPET_CHARS));
    let end = (start + SNIPPET_CHARS).min(text.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&text[start..end]);
    if end < text.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod snippet_tests {
    use super::*;

    /// KAIROS-T-0602: the snippet is the start of the content without
    /// `q`, and the window near the first word of `q` with it.
    #[test]
    fn snippet_window() {
        assert_eq!(snippet("", None), None);
        assert_eq!(snippet("  a\n\nb  ", None).as_deref(), Some("a b"));
        let long = format!("{} needle {}", "x ".repeat(300), "y ".repeat(300));
        let start = snippet(&long, None).expect("a snippet");
        assert!(start.starts_with("x x"), "{start}");
        assert!(start.ends_with('…'));
        assert_eq!(start.chars().count(), SNIPPET_CHARS + 1);
        let near = snippet(&long, Some("NEEDLE")).expect("a snippet");
        assert!(near.starts_with('…') && near.ends_with('…'), "{near}");
        assert!(near.contains("needle"), "{near}");
        // A word that the content does not have: the start.
        assert_eq!(snippet(&long, Some("absent")), Some(start));
        // A short content is not cut.
        assert_eq!(
            snippet("Fix the login.", Some("login")).as_deref(),
            Some("Fix the login.")
        );
    }
}
