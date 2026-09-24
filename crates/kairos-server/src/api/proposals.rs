//! Edge proposals over REST (KAIROS-A-0021 rule 6, KAIROS-T-0192).
//!
//! Three endpoints, and the asymmetry between them is the point: an agent
//! **proposes** through MCP, and a human **decides** here. Confirming and
//! rejecting are refused to service accounts by
//! [`kairos_db::proposals`] itself rather than by a check in this file, so a
//! future fourth surface inherits the rule instead of having to remember it.
//!
//! Listing is on the item, not in a queue. A separate review inbox is a second
//! product with its own notifications and its own backlog of things nobody
//! looks at; a proposal shown where the work already is gets seen by the person
//! already looking at it.

use axum::extract::{Extension, Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use kairos_client::types_search as dto_search;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/items/{short_code}/proposals", get(list))
        .route("/api/proposals/{id}/confirm", post(confirm))
        .route("/api/proposals/{id}/reject", post(reject))
}

/// [`kairos_db::proposals::ProposalError`] → HTTP-shaped API error.
pub(crate) fn map_proposal_error(e: kairos_db::proposals::ProposalError) -> ApiError {
    use kairos_db::proposals::ProposalError as P;
    match e {
        P::NotFound(id) => ApiError::not_found(format!("edge proposal {id} does not exist")),
        // Not a failure the caller should retry around: the work was done.
        P::AlreadyPending => ApiError::conflict(e.to_string()),
        P::AlreadyDecided { .. } => ApiError::conflict(e.to_string()),
        P::TooManyPending { .. } => ApiError::conflict(e.to_string()),
        P::NotHuman => ApiError::forbidden(e.to_string()),
        P::NotProposable(_) => ApiError::validation(e.to_string()),
        P::Graph(inner) => ApiError::validation(inner.to_string()),
        P::Database(inner) => ApiError::internal(inner),
    }
}

/// Map a stored proposal to its DTO, resolving both ends to short codes.
///
/// Short codes rather than ids because this is read by a person deciding
/// something, and a UUID pair tells them nothing about what they are agreeing to.
fn to_dto(
    conn: &mut diesel::pg::PgConnection,
    p: kairos_db::proposals::EdgeProposal,
) -> Result<dto_search::EdgeProposalDto, ApiError> {
    let source =
        kairos_db::embeddings::item_by_id(conn, p.source_id).map_err(ApiError::internal)?;
    let target =
        kairos_db::embeddings::item_by_id(conn, p.target_id).map_err(ApiError::internal)?;
    Ok(dto_search::EdgeProposalDto {
        id: p.id.to_string(),
        source: source.short_code,
        target: target.short_code,
        relationship: p.relationship,
        state: p.state,
        claim: p.claim,
        why: p.why,
        created_at: p.created_at.to_rfc3339(),
    })
}

/// Pending edge proposals touching an item.
#[utoipa::path(
    get,
    path = "/api/items/{short_code}/proposals",
    tag = "search",
    params(("short_code" = String, Path, description = "The item")),
    responses(
        (status = 200, description = "Undecided proposals naming this item at either end, best-evidence first", body = Vec<dto_search::EdgeProposalDto>),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No item with that short code", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(short_code): Path<String>,
) -> Result<Json<Vec<dto_search::EdgeProposalDto>>, ApiError> {
    let proposals = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let item = kairos_db::embeddings::item_by_short_code(conn, &short_code)
                .map_err(|e| ApiError::not_found(e.to_string()))?;
            let rows = kairos_db::proposals::pending_for_item(conn, item.id)
                .map_err(ApiError::internal)?;
            rows.into_iter()
                .map(|p| to_dto(conn, p))
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    Ok(Json(proposals))
}

/// Confirm a proposal, creating the edge.
#[utoipa::path(
    post,
    path = "/api/proposals/{id}/confirm",
    tag = "search",
    params(("id" = String, Path, description = "The proposal")),
    responses(
        (status = 200, description = "Confirmed; the relationship now exists", body = dto_search::EdgeProposalDto),
        (status = 400, description = "The edge was refused by the graph's own rules — a cycle, or a shape the rule matrix forbids", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "A service account may propose but not decide", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No such proposal", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already decided", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn confirm(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(auth): Extension<crate::middleware::auth::AuthContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<dto_search::EdgeProposalDto>, ApiError> {
    decide(state, tenant, auth.user_id, id, true).await
}

/// Reject a proposal. Recorded, not erased.
#[utoipa::path(
    post,
    path = "/api/proposals/{id}/reject",
    tag = "search",
    params(("id" = String, Path, description = "The proposal")),
    responses(
        (status = 200, description = "Rejected, and kept — a repeatedly rejected pair is the clearest signal that retrieval is wrong about something", body = dto_search::EdgeProposalDto),
        (status = 403, description = "A service account may propose but not decide", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No such proposal", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "Already decided", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn reject(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(auth): Extension<crate::middleware::auth::AuthContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<dto_search::EdgeProposalDto>, ApiError> {
    decide(state, tenant, auth.user_id, id, false).await
}

async fn decide(
    state: AppState,
    tenant: TenantContext,
    actor: Uuid,
    id: Uuid,
    confirming: bool,
) -> Result<Json<dto_search::EdgeProposalDto>, ApiError> {
    let out = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let decided = if confirming {
                kairos_db::proposals::confirm(conn, id, actor)
            } else {
                kairos_db::proposals::reject(conn, id, actor)
            }
            .map_err(map_proposal_error)?;
            to_dto(conn, decided)
        })
        .await?;
    Ok(Json(out))
}
