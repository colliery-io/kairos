//! Edge proposals over REST (KAIROS-A-0021 rule 6, KAIROS-T-0192).
//!
//! Three endpoints, and the asymmetry between them is the point: an agent
//! **proposes** through MCP, and a holder of the grant **decides** here.
//! [`kairos_db::proposals`] itself makes the check of who decides, rather
//! than a check in this file, so a future fourth surface inherits the rule
//! instead of having to remember it.
//!
//! # Who may decide (KAIROS-T-0659)
//!
//! A holder of `approve_proposals` on the board of the source or of the
//! target confirms or rejects. An org admin decides each proposal. The
//! holder may use an agent key, may be a service account, and may be the
//! proposer. A refusal is 403 `FORBIDDEN`. `details.required_capability` is
//! `approve_proposals`, and `details.any_of` names the board of each end.
//!
//! The grant replaces two older rules. KAIROS-T-0538 refused a service
//! account, an agent key and the proposer (403 `PERSON_DECIDES`).
//! COLLIERY-T-0234 gave the confirm the link rule: the caller could edit the
//! item at one end. The grant on the board of one end is that rule for a
//! proposal now. A member with no grant cannot confirm an edge that they
//! proposed, so a proposal is still no way around the link rule.
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
        P::NotFound(id) => ApiError::not_found(format!("The edge proposal {id} does not exist."))
            .with_details(serde_json::json!({ "field": "id" })),
        // Not a failure the caller should retry around: the work was done.
        P::AlreadyPending => ApiError::conflict(e.to_string()),
        P::AlreadyDecided { .. } => ApiError::conflict(e.to_string()),
        P::TooManyPending { .. } => ApiError::conflict(e.to_string()),
        P::NotApprover { ref boards } => not_approver(&e.to_string(), boards),
        P::NotProposable(_) => ApiError::validation(e.to_string())
            .with_details(serde_json::json!({ "field": "relationship" })),
        // Only a confirm links an edge: the proposal of `id` is refused.
        P::Graph(inner) => ApiError::validation(inner.to_string())
            .with_details(serde_json::json!({ "field": "id" })),
        P::Database(inner) => ApiError::internal(inner),
        P::Abac(inner) => ApiError::internal(inner),
    }
}

/// 403 `FORBIDDEN` for a decision without the grant (KAIROS-T-0659). The
/// details have the shape of each other edge refusal: one entry in `any_of`
/// for each end, with its board.
///
/// KAIROS-T-0536: when the scope of the agent key leaves out the boards of
/// both ends, the refusal is `KEY_SCOPE` for the board of the source.
fn not_approver(message: &str, boards: &kairos_db::proposals::ApprovalBoards) -> ApiError {
    let capability = kairos_core::abac::APPROVE_PROPOSALS;
    let source = boards.source.as_ref().map(|b| b.id);
    let target = boards.target.as_ref().map(|b| b.id);
    if let (Some(refusal), Some(_)) = (
        ApiError::outside_key_scope(capability, source),
        ApiError::outside_key_scope(capability, target),
    ) {
        return refusal;
    }
    let end = |name: &str, board: &Option<kairos_db::proposals::EndBoard>| {
        serde_json::json!({
            "end": name,
            "required_capability": capability,
            "board_id": board.as_ref().map(|b| b.id),
            "board": board.as_ref().map(|b| b.slug.clone()),
        })
    };
    ApiError::forbidden(message.to_string()).with_details(serde_json::json!({
        "required_capability": capability,
        "any_of": [end("source", &boards.source), end("target", &boards.target)],
    }))
}

/// Map a stored proposal to its DTO, resolving both ends to short codes.
///
/// Short codes rather than ids because this is read by a person deciding
/// something, and a UUID pair tells them nothing about what they are agreeing to.
pub(crate) fn to_dto(
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
            let item =
                kairos_db::embeddings::item_by_short_code(conn, &short_code).map_err(|e| {
                    ApiError::not_found(e.to_string())
                        .with_details(serde_json::json!({ "field": "short_code" }))
                })?;
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
/// The caller holds `approve_proposals` on the board of one end, or is an
/// organization admin (KAIROS-T-0659). A refused confirm leaves the
/// proposal pending.
#[utoipa::path(
    post,
    path = "/api/proposals/{id}/confirm",
    tag = "search",
    params(("id" = String, Path, description = "The proposal")),
    responses(
        (status = 200, description = "Confirmed; the relationship now exists", body = dto_search::EdgeProposalDto),
        (status = 400, description = "The edge was refused by the graph's own rules — a cycle, or a shape the rule matrix forbids", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "FORBIDDEN: the caller holds approve_proposals on neither board and is not an organization admin; details.any_of names the board of each end", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No such proposal", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "CONFLICT: the proposal is decided already, also by a decision at the same time", body = kairos_client::types::ErrorEnvelope),
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
///
/// The rule of who decides is the rule of the confirm (KAIROS-T-0659).
#[utoipa::path(
    post,
    path = "/api/proposals/{id}/reject",
    tag = "search",
    params(("id" = String, Path, description = "The proposal")),
    responses(
        (status = 200, description = "Rejected, and kept — a repeatedly rejected pair is the clearest signal that retrieval is wrong about something", body = dto_search::EdgeProposalDto),
        (status = 403, description = "FORBIDDEN: the caller holds approve_proposals on neither board and is not an organization admin; details.any_of names the board of each end", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "No such proposal", body = kairos_client::types::ErrorEnvelope),
        (status = 409, description = "CONFLICT: the proposal is decided already, also by a decision at the same time", body = kairos_client::types::ErrorEnvelope),
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
    // The role comes from the context of the request (KAIROS-T-0625).
    let decider = kairos_db::proposals::Decider {
        user_id: actor,
        is_org_admin: tenant.role == kairos_db::models::OrgRole::Admin,
    };
    let out = state
        .blocking
        .run(&tenant.slug, move |conn| {
            // One transaction for the rule and the write (COLLIERY-T-0234):
            // a refusal leaves the proposal pending and writes no edge.
            crate::api::atomically(conn, |conn| {
                let decided = if confirming {
                    kairos_db::proposals::confirm(conn, id, decider)
                } else {
                    kairos_db::proposals::reject(conn, id, decider)
                }
                .map_err(map_proposal_error)?;
                to_dto(conn, decided)
            })
        })
        .await?;
    Ok(Json(out))
}
