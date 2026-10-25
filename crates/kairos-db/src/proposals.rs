//! Proposed graph edges, awaiting a decision (KAIROS-A-0021 rule 6,
//! KAIROS-T-0192).
//!
//! Every function operates in the CURRENT `search_path` tenant schema.
//!
//! # Who decides (KAIROS-T-0659)
//!
//! A holder of [`kairos_core::abac::APPROVE_PROPOSALS`] on the board of the
//! source or of the target confirms or rejects. An org admin decides each
//! proposal. Who the caller is does not count: an agent key of a holder, a
//! service account that holds the grant, and the proposer all decide. Until
//! KAIROS-T-0659 a person other than the proposer decided, never through an
//! agent key (KAIROS-T-0538). So nobody could confirm the proposal of an
//! agent in an organization with one person.
//!
//! # Why a proposal rather than an edge
//!
//! Agents write most of the content in this product and humans edit. Edges are
//! the exception, because the blast radius differs in kind: a wrong `parent`
//! re-parents work onto a board that then reports the wrong thing to the wrong
//! people, and nobody looks at a parent edge twice once it exists.
//! KAIROS-T-0190 measured the precision available at the top of the similarity
//! distribution at roughly half. A proposal costs a click; a wrong edge costs a
//! conversation.
//!
//! # This is the part that compounds
//!
//! Graph distance is half the signal the retrieval surface uses, so every
//! confirmation makes the next retrieval better. The feature improves with use
//! rather than decaying — but only because a decider is in the loop, which is
//! what keeps a confident wrong similarity from quietly restructuring a
//! portfolio. The decider holds a grant for it (see "Who decides").

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Float, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use diesel::{QueryableByName, sql_query};
use uuid::Uuid;

use crate::graph::{self, GraphError};
use crate::models::enums::RelationshipType;

/// The most pending proposals one item may carry, in either direction.
///
/// An agent loop that proposes on every run would otherwise bury the signal
/// under its own output, and a human looking at thirty suggestions on one card
/// reads none of them. When this is reached the answer is to decide some, not to
/// make room.
pub const MAX_PENDING_PER_ITEM: i64 = 10;

/// Errors from the proposal services.
#[derive(Debug, thiserror::Error)]
pub enum ProposalError {
    /// No proposal with this id.
    #[error("The edge proposal {0} does not exist.")]
    NotFound(Uuid),
    /// The proposal has already been ruled on. Decisions are not revisited here:
    /// the edge either exists and can be unlinked, or it does not and a fresh
    /// proposal can be made.
    #[error("The edge proposal {id} has the state {state} already. A decision does not change.")]
    AlreadyDecided {
        /// The proposal.
        id: Uuid,
        /// What it was decided as.
        state: String,
    },
    /// An identical proposal is already waiting. Not an error a caller needs to
    /// handle loudly — it means the work was already done.
    #[error("The same proposal waits for a decision already.")]
    AlreadyPending,
    /// This item already carries [`MAX_PENDING_PER_ITEM`] undecided proposals.
    #[error(
        "The item has {count} edge proposals that wait for a decision. The limit is {limit}. \
         Confirm or reject some of them. Then propose the edge again."
    )]
    TooManyPending {
        /// How many are waiting.
        count: i64,
        /// The cap.
        limit: i64,
    },
    /// The caller holds `approve_proposals` on neither board of the
    /// proposal, and is not an org admin (KAIROS-T-0659).
    #[error(
        "You cannot confirm or reject this proposal. {} Ask an organization admin for the grant.",
        boards.deciders()
    )]
    NotApprover {
        /// The boards that decide the proposal.
        boards: ApprovalBoards,
    },
    /// Only `parent` and `blocks` may be proposed.
    #[error("You can propose only a parent edge or a blocks edge. {0:?} is not one of them.")]
    NotProposable(String),
    /// Confirming would have created the edge, and the graph refused it.
    #[error(transparent)]
    Graph(#[from] GraphError),
    /// The database refused or failed.
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    /// The capability check failed.
    #[error(transparent)]
    Abac(#[from] crate::abac::AbacError),
}

/// The board at one end of a proposal (KAIROS-T-0659).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndBoard {
    /// `boards.id`.
    pub id: Uuid,
    /// `boards.slug`, for the text of a refusal.
    pub slug: String,
}

/// The boards that decide a proposal (KAIROS-T-0659): the board of the
/// source and the board of the target. `None` is an end with no board (an
/// ADR off a board). Only an org admin decides for that end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalBoards {
    /// The board of the source.
    pub source: Option<EndBoard>,
    /// The board of the target.
    pub target: Option<EndBoard>,
}

impl ApprovalBoards {
    /// The slugs of the boards, each one once: the source first.
    pub fn slugs(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for end in [&self.source, &self.target].into_iter().flatten() {
            if !out.contains(&end.slug.as_str()) {
                out.push(&end.slug);
            }
        }
        out
    }

    /// Who decides, in one sentence or two: for the reply of
    /// `propose_edge` and the text of a refusal.
    pub fn deciders(&self) -> String {
        let capability = kairos_core::abac::APPROVE_PROPOSALS;
        match self.slugs().as_slice() {
            [] => "Only an organization admin can confirm or reject it.".to_string(),
            [one] => format!(
                "A holder of {capability:?} on the board {one} can confirm or reject it. An \
                 organization admin can too."
            ),
            [source, target, ..] => format!(
                "A holder of {capability:?} on the board {source} or on the board {target} can \
                 confirm or reject it. An organization admin can too."
            ),
        }
    }
}

/// A proposed edge.
#[derive(Debug, Clone, QueryableByName)]
pub struct EdgeProposal {
    /// Its id.
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    /// The proposed edge's source.
    #[diesel(sql_type = SqlUuid)]
    pub source_id: Uuid,
    /// The proposed edge's target.
    #[diesel(sql_type = SqlUuid)]
    pub target_id: Uuid,
    /// `parent` or `blocks`.
    #[diesel(sql_type = Text)]
    pub relationship: String,
    /// `pending`, `confirmed` or `rejected`.
    #[diesel(sql_type = Text)]
    pub state: String,
    /// What the retrieval surface claimed.
    #[diesel(sql_type = Text)]
    pub claim: String,
    /// Its reasoning, verbatim, so a reviewer sees what the agent saw.
    #[diesel(sql_type = Text)]
    pub why: String,
    /// The fused rank score from that retrieval. Comparable only against other
    /// proposals from the same query.
    #[diesel(sql_type = Float)]
    pub score: f32,
    /// Who proposed it.
    #[diesel(sql_type = SqlUuid)]
    pub proposed_by: Uuid,
    /// Who ruled on it, if anyone has.
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub decided_by: Option<Uuid>,
    /// When they did.
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub decided_at: Option<DateTime<Utc>>,
    /// When it was proposed.
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// What to propose.
#[derive(Debug, Clone)]
pub struct NewProposal<'a> {
    /// The proposed edge's source.
    pub source_id: Uuid,
    /// The proposed edge's target.
    pub target_id: Uuid,
    /// `parent` or `blocks`.
    pub relationship: RelationshipType,
    /// The claim from the retrieval surface.
    pub claim: &'a str,
    /// Its reasoning, verbatim.
    pub why: &'a str,
    /// Its fused score.
    pub score: f32,
}

/// Whether a relationship is one an agent may propose.
///
/// `parent` and `blocks` only. They are the two that change what a board reports
/// and what an agent picks up next; `supports`, `informs` and `supersedes` are
/// editorial, cheap to undo, and remain a human's to draw — there is no need to
/// build a review workflow for a decision nobody would get badly wrong.
pub fn is_proposable(relationship: RelationshipType) -> bool {
    matches!(
        relationship,
        RelationshipType::Parent | RelationshipType::Blocks
    )
}

fn relationship_str(relationship: RelationshipType) -> &'static str {
    match relationship {
        RelationshipType::Parent => "parent",
        RelationshipType::Supports => "supports",
        RelationshipType::Informs => "informs",
        RelationshipType::Supersedes => "supersedes",
        RelationshipType::Amends => "amends",
        RelationshipType::Blocks => "blocks",
    }
}

/// Record a proposed edge.
///
/// Refuses a duplicate of something already pending, and refuses to add to an
/// item already carrying [`MAX_PENDING_PER_ITEM`]. A previously **rejected**
/// pair may be proposed again: the partial unique index covers pending rows
/// only, because a rejection is a judgement about the evidence at the time and
/// better evidence deserves another hearing.
pub fn propose(
    conn: &mut PgConnection,
    new: NewProposal<'_>,
    proposed_by: Uuid,
) -> Result<EdgeProposal, ProposalError> {
    if !is_proposable(new.relationship) {
        return Err(ProposalError::NotProposable(
            relationship_str(new.relationship).to_string(),
        ));
    }

    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }
    let pending = sql_query(
        "SELECT count(*)::bigint AS count FROM edge_proposals \
         WHERE state = 'pending' AND (source_id = $1 OR target_id = $1)",
    )
    .bind::<SqlUuid, _>(new.source_id)
    .get_result::<Count>(conn)?
    .count;
    if pending >= MAX_PENDING_PER_ITEM {
        return Err(ProposalError::TooManyPending {
            count: pending,
            limit: MAX_PENDING_PER_ITEM,
        });
    }

    let inserted = sql_query(
        "INSERT INTO edge_proposals \
             (source_id, target_id, relationship, claim, why, score, proposed_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, source_id, target_id, relationship, state, claim, why, score, \
                   proposed_by, decided_by, decided_at, created_at",
    )
    .bind::<SqlUuid, _>(new.source_id)
    .bind::<SqlUuid, _>(new.target_id)
    .bind::<Text, _>(relationship_str(new.relationship))
    .bind::<Text, _>(new.claim)
    .bind::<Text, _>(new.why)
    .bind::<Float, _>(new.score)
    .bind::<SqlUuid, _>(proposed_by)
    .get_result::<EdgeProposal>(conn);

    match inserted {
        Ok(p) => Ok(p),
        // The partial unique index did its job: an agent proposing the same
        // thing twice is not an error worth escalating, it is work already done.
        Err(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => Err(ProposalError::AlreadyPending),
        Err(e) => Err(e.into()),
    }
}

/// Undecided proposals touching an item, either end.
///
/// Both directions, because a proposal concerns both items and a human looking
/// at either one should see it. This is what the GUI asks for on every item it
/// shows.
pub fn pending_for_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<Vec<EdgeProposal>, ProposalError> {
    Ok(sql_query(
        "SELECT id, source_id, target_id, relationship, state, claim, why, score, \
                proposed_by, decided_by, decided_at, created_at \
           FROM edge_proposals \
          WHERE state = 'pending' AND (source_id = $1 OR target_id = $1) \
          ORDER BY score DESC, created_at",
    )
    .bind::<SqlUuid, _>(item_id)
    .load::<EdgeProposal>(conn)?)
}

/// One proposal by id, in any state: [`ProposalError::NotFound`] when
/// there is none.
///
/// Public since COLLIERY-T-0234, for a caller that reads a proposal in any
/// state.
pub fn get(conn: &mut PgConnection, id: Uuid) -> Result<EdgeProposal, ProposalError> {
    load(conn, id)
}

fn load(conn: &mut PgConnection, id: Uuid) -> Result<EdgeProposal, ProposalError> {
    sql_query(
        "SELECT id, source_id, target_id, relationship, state, claim, why, score, \
                proposed_by, decided_by, decided_at, created_at \
           FROM edge_proposals WHERE id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .get_result::<EdgeProposal>(conn)
    .map_err(|e| match e {
        diesel::result::Error::NotFound => ProposalError::NotFound(id),
        other => ProposalError::Database(other),
    })
}

/// The boards that decide a proposal of an edge from `source_id` to
/// `target_id` (KAIROS-T-0659): the board of each end, as
/// [`crate::abac::resolve_authorization_board`] gives it. A document gives
/// its owner board.
pub fn approval_boards(
    conn: &mut PgConnection,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<ApprovalBoards, ProposalError> {
    let mut end = |item_id: Uuid| -> Result<Option<EndBoard>, ProposalError> {
        use crate::schema::boards;
        let Some(id) = crate::abac::resolve_authorization_board(conn, item_id)? else {
            return Ok(None);
        };
        let slug = boards::table
            .filter(boards::id.eq(id))
            .select(boards::slug)
            .first::<String>(conn)?;
        Ok(Some(EndBoard { id, slug }))
    };
    Ok(ApprovalBoards {
        source: end(source_id)?,
        target: end(target_id)?,
    })
}

/// Who decides a proposal (KAIROS-T-0659): the caller, and whether the
/// caller is an org admin. The server reads the role from the context of
/// the request, as the edit rule does (KAIROS-T-0625).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decider {
    /// `public.users.id` of the caller. An agent key authenticates as its
    /// person, so this is the person.
    pub user_id: Uuid,
    /// True when the caller is an org admin of the current tenant.
    pub is_org_admin: bool,
}

/// The rule of who decides (KAIROS-T-0659): an org admin, or a holder of
/// `approve_proposals` on the board of one end
/// ([`kairos_core::abac::may_decide_proposal`]). The check reads the grants
/// of the caller. An agent key authenticates as its person, so the grants
/// of the person count for it.
fn require_approver(
    conn: &mut PgConnection,
    decider: Decider,
    proposal: &EdgeProposal,
) -> Result<(), ProposalError> {
    let (actor, is_org_admin) = (decider.user_id, decider.is_org_admin);
    // KAIROS-T-0536: with an agent key that has a scope, the admin role
    // decides only on a board of one end that the scope allows.
    if is_org_admin && crate::key_scope::scope_of(actor).is_none() {
        return Ok(());
    }
    let boards = approval_boards(conn, proposal.source_id, proposal.target_id)?;
    let in_scope = |end: &Option<EndBoard>| {
        crate::key_scope::allows(
            actor,
            kairos_core::abac::APPROVE_PROPOSALS,
            end.as_ref().map(|board| board.id),
        )
    };
    let is_org_admin = is_org_admin && (in_scope(&boards.source) || in_scope(&boards.target));
    let mut holds = |end: &Option<EndBoard>| -> Result<bool, ProposalError> {
        match end {
            Some(board) => Ok(crate::abac::check_capability(
                conn,
                board.id,
                actor,
                kairos_core::abac::APPROVE_PROPOSALS,
            )?),
            None => Ok(false),
        }
    };
    let on_source = holds(&boards.source)?;
    let on_target = holds(&boards.target)?;
    if kairos_core::abac::may_decide_proposal(is_org_admin, on_source, on_target) {
        Ok(())
    } else {
        Err(ProposalError::NotApprover { boards })
    }
}

/// Load the proposal to decide: it exists and waits.
fn load_pending(conn: &mut PgConnection, id: Uuid) -> Result<EdgeProposal, ProposalError> {
    let proposal = load(conn, id)?;
    if proposal.state != "pending" {
        return Err(ProposalError::AlreadyDecided {
            id,
            state: proposal.state,
        });
    }
    Ok(proposal)
}

/// Write the decision. The `state = 'pending'` condition is in the UPDATE
/// itself (KAIROS-T-0538): of two decisions at the same time, the row lock
/// lets one change the row, and the other changes no row and gets
/// [`ProposalError::AlreadyDecided`]. A check before the UPDATE alone let
/// both succeed.
fn decide(
    conn: &mut PgConnection,
    id: Uuid,
    state: &str,
    actor: Uuid,
) -> Result<EdgeProposal, ProposalError> {
    let decided = sql_query(
        "UPDATE edge_proposals \
            SET state = $2, decided_by = $3, decided_at = now() \
          WHERE id = $1 AND state = 'pending' \
         RETURNING id, source_id, target_id, relationship, state, claim, why, score, \
                   proposed_by, decided_by, decided_at, created_at",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(state)
    .bind::<SqlUuid, _>(actor)
    .get_result::<EdgeProposal>(conn)
    .optional()?;
    match decided {
        Some(p) => Ok(p),
        None => {
            let now = load(conn, id)?;
            Err(ProposalError::AlreadyDecided {
                id,
                state: now.state,
            })
        }
    }
}

/// Confirm a proposal, creating the real edge.
///
/// The edge is created through [`crate::graph::link_items`], which means the
/// cycle check, the rule matrix and the activity log are **the existing ones**
/// rather than a second copy written for this path. A confirmation that would
/// create a cycle is refused by the same code that refuses it anywhere else, and
/// the proposal stays pending — a refused confirmation is not a decision.
///
/// Transactional: the edge and the state change land together or not at all.
///
/// WHO may confirm is decided here (KAIROS-T-0659): an org admin, or a
/// holder of `approve_proposals` on the board of one end. The order of the
/// refusals: no such proposal, a decided proposal, then the grant.
pub fn confirm(
    conn: &mut PgConnection,
    id: Uuid,
    decider: Decider,
) -> Result<EdgeProposal, ProposalError> {
    let proposal = load_pending(conn, id)?;
    require_approver(conn, decider, &proposal)?;
    let actor = decider.user_id;
    let relationship = match proposal.relationship.as_str() {
        "parent" => RelationshipType::Parent,
        "blocks" => RelationshipType::Blocks,
        other => return Err(ProposalError::NotProposable(other.to_string())),
    };

    // The decision first, then the edge: the UPDATE takes the row lock, so a
    // second confirm at the same time waits, changes no row, and gets the
    // 409 before it tries to write the same edge (KAIROS-T-0538).
    conn.transaction(|conn| {
        let decided = decide(conn, id, "confirmed", actor)?;
        graph::link_items(
            conn,
            proposal.source_id,
            proposal.target_id,
            relationship,
            actor,
        )?;
        // KAIROS-T-0558: the proposer learns the outcome.
        crate::notifications::on_proposal_decided(conn, &decided, actor)?;
        Ok(decided)
    })
}

/// Reject a proposal.
///
/// Recorded, not deleted. A pair that keeps being proposed and keeps being
/// rejected is the clearest signal available that retrieval is wrong about
/// something, and [`stats`] is where that shows up. Deleting rejections would
/// delete the only honest measurement this feature has.
///
/// The rule of who may decide is the rule of [`confirm`] (KAIROS-T-0659).
pub fn reject(
    conn: &mut PgConnection,
    id: Uuid,
    decider: Decider,
) -> Result<EdgeProposal, ProposalError> {
    let proposal = load_pending(conn, id)?;
    require_approver(conn, decider, &proposal)?;
    // The decision and the notification of the proposer (KAIROS-T-0558)
    // land together.
    conn.transaction(|conn| {
        let decided = decide(conn, id, "rejected", decider.user_id)?;
        crate::notifications::on_proposal_decided(conn, &decided, decider.user_id)?;
        Ok(decided)
    })
}

/// How the proposals are going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProposalStats {
    /// Undecided.
    pub pending: i64,
    /// Confirmed into real edges.
    pub confirmed: i64,
    /// Rejected, and kept.
    pub rejected: i64,
}

impl ProposalStats {
    /// Confirmed as a fraction of everything decided, or `None` when nothing has
    /// been.
    ///
    /// The only honest measure of whether this feature works. A ratio drifting
    /// toward zero means retrieval is proposing things humans do not recognise,
    /// and that is worth knowing before the proposals start being ignored
    /// wholesale.
    pub fn confirm_rate(&self) -> Option<f32> {
        let decided = self.confirmed + self.rejected;
        (decided > 0).then(|| self.confirmed as f32 / decided as f32)
    }
}

/// Counts by state, for an operator.
pub fn stats(conn: &mut PgConnection) -> Result<ProposalStats, ProposalError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = BigInt)]
        pending: i64,
        #[diesel(sql_type = BigInt)]
        confirmed: i64,
        #[diesel(sql_type = BigInt)]
        rejected: i64,
    }
    let row = sql_query(
        "SELECT count(*) FILTER (WHERE state = 'pending')::bigint AS pending, \
                count(*) FILTER (WHERE state = 'confirmed')::bigint AS confirmed, \
                count(*) FILTER (WHERE state = 'rejected')::bigint AS rejected \
           FROM edge_proposals",
    )
    .get_result::<Row>(conn)?;
    Ok(ProposalStats {
        pending: row.pending,
        confirmed: row.confirmed,
        rejected: row.rejected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_parent_and_blocks_are_proposable() {
        assert!(is_proposable(RelationshipType::Parent));
        assert!(is_proposable(RelationshipType::Blocks));
        // Editorial edges: a wrong one is cheap to undo, so there is nothing for
        // a review workflow to protect.
        assert!(!is_proposable(RelationshipType::Supports));
        assert!(!is_proposable(RelationshipType::Informs));
        assert!(!is_proposable(RelationshipType::Supersedes));
        assert!(!is_proposable(RelationshipType::Amends));
    }

    #[test]
    fn the_confirm_rate_is_none_until_something_is_decided() {
        let none = ProposalStats {
            pending: 5,
            ..Default::default()
        };
        assert_eq!(none.confirm_rate(), None, "five pending decide nothing");

        let half = ProposalStats {
            pending: 1,
            confirmed: 3,
            rejected: 3,
        };
        assert_eq!(half.confirm_rate(), Some(0.5));

        let bad = ProposalStats {
            pending: 0,
            confirmed: 0,
            rejected: 4,
        };
        assert_eq!(
            bad.confirm_rate(),
            Some(0.0),
            "all rejected is a number worth seeing, not a division by zero"
        );
    }
}
