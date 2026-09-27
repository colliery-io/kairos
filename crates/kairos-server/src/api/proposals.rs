//! Edge proposals over REST (KAIROS-A-0021 rule 6, KAIROS-T-0192).
//!
//! Three endpoints, and the asymmetry between them is the point: an agent
//! **proposes** through MCP, and a human **decides** here. Confirming and
//! rejecting are refused to service accounts by
//! [`kairos_db::proposals`] itself rather than by a check in this file, so a
//! future fourth surface inherits the rule instead of having to remember it.
//!
//! # Who may confirm (COLLIERY-T-0234)
//!
//! A confirm writes an edge, so it takes THE LINK RULE, from the function
//! that each other edge write calls ([`crate::api::require_edge_write`]):
//! the caller may edit the item at one end. Until COLLIERY-T-0234 the
//! confirm asked only for a person. A member who could edit neither end
//! could propose a `parent` edge, confirm it, and so put the task of a
//! different team below an item.
//!
//! A reject writes no edge and changes no item, so it keeps its rule: a
//! person. A proposal is a suggestion, and each member can make one.
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
///
/// The caller is a person. The link rule applies (COLLIERY-T-0234): the
/// caller can edit the item at one end of the edge. A refused confirm
/// leaves the proposal pending.
#[utoipa::path(
    post,
    path = "/api/proposals/{id}/confirm",
    tag = "search",
    params(("id" = String, Path, description = "The proposal")),
    responses(
        (status = 200, description = "Confirmed; the relationship now exists", body = dto_search::EdgeProposalDto),
        (status = 400, description = "The edge was refused by the graph's own rules — a cycle, or a shape the rule matrix forbids", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "A service account may propose but not decide. A person needs the link rule: the caller can edit the item at one end; details.any_of names the two capabilities", body = kairos_client::types::ErrorEnvelope),
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

/// The item type at one end of a proposal, archived or not: the edit rule
/// reads archived items too (KAIROS-A-0020).
fn end_type(
    conn: &mut diesel::pg::PgConnection,
    item_id: Uuid,
) -> Result<kairos_core::short_code::ItemType, ApiError> {
    let item = kairos_db::embeddings::item_by_id(conn, item_id).map_err(ApiError::internal)?;
    kairos_core::short_code::ItemType::ALL
        .iter()
        .copied()
        .find(|item_type| item_type.entity_type() == item.entity_type)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "entity_directory returned unknown entity_type {:?}",
                item.entity_type
            ))
        })
}

/// THE LINK RULE for the edge that a confirm is about to write
/// (COLLIERY-T-0234).
///
/// THE ATTACK that it stops. A member who can edit NEITHER end proposes a
/// `parent` edge, which needs no right, and confirms it. The confirm asked
/// only for a person, so the member wrote an edge that `POST
/// /api/relationships` refuses.
///
/// The order of the refusals is as it was, and the link rule is the last:
/// a service account (403), no such proposal (404), already decided (409).
/// So a caller learns no more from a proposal than before.
fn require_confirm(
    conn: &mut diesel::pg::PgConnection,
    slug: &str,
    actor: Uuid,
    id: Uuid,
) -> Result<(), ApiError> {
    use kairos_db::proposals::{self, ProposalError};

    if !proposals::is_human(conn, actor).map_err(map_proposal_error)? {
        return Err(map_proposal_error(ProposalError::NotHuman));
    }
    let proposal = proposals::get(conn, id).map_err(map_proposal_error)?;
    if proposal.state != "pending" {
        return Err(map_proposal_error(ProposalError::AlreadyDecided {
            id,
            state: proposal.state,
        }));
    }
    let source = (proposal.source_id, end_type(conn, proposal.source_id)?);
    let target = (proposal.target_id, end_type(conn, proposal.target_id)?);
    crate::api::require_edge_write(conn, slug, actor, &proposal.relationship, source, target)
}

async fn decide(
    state: AppState,
    tenant: TenantContext,
    actor: Uuid,
    id: Uuid,
    confirming: bool,
) -> Result<Json<dto_search::EdgeProposalDto>, ApiError> {
    let slug = tenant.slug.clone();
    let out = state
        .blocking
        .run(&tenant.slug, move |conn| {
            // One transaction for the rule and the write (COLLIERY-T-0234):
            // a refusal leaves the proposal pending and writes no edge.
            crate::api::atomically(conn, |conn| {
                let decided = if confirming {
                    require_confirm(conn, &slug, actor, id)?;
                    kairos_db::proposals::confirm(conn, id, actor)
                } else {
                    kairos_db::proposals::reject(conn, id, actor)
                }
                .map_err(map_proposal_error)?;
                to_dto(conn, decided)
            })
        })
        .await?;
    Ok(Json(out))
}
