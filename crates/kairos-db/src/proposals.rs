//! Proposed graph edges, awaiting a human (KAIROS-A-0021 rule 6,
//! KAIROS-T-0192).
//!
//! Every function operates in the CURRENT `search_path` tenant schema.
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
//! rather than decaying — but only because a human is in the loop, which is what
//! keeps a confident wrong similarity from quietly restructuring a portfolio.

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
    #[error("edge proposal {0} does not exist")]
    NotFound(Uuid),
    /// The proposal has already been ruled on. Decisions are not revisited here:
    /// the edge either exists and can be unlinked, or it does not and a fresh
    /// proposal can be made.
    #[error("edge proposal {id} was already {state}")]
    AlreadyDecided {
        /// The proposal.
        id: Uuid,
        /// What it was decided as.
        state: String,
    },
    /// An identical proposal is already waiting. Not an error a caller needs to
    /// handle loudly — it means the work was already done.
    #[error("an identical proposal is already pending")]
    AlreadyPending,
    /// This item already carries [`MAX_PENDING_PER_ITEM`] undecided proposals.
    #[error(
        "item already has {count} pending edge proposals (limit {limit}); \
         decide some before proposing more"
    )]
    TooManyPending {
        /// How many are waiting.
        count: i64,
        /// The cap.
        limit: i64,
    },
    /// A service account tried to rule on a proposal.
    #[error(
        "a service account may propose an edge but not confirm or reject one — \
         that is the human in the loop (KAIROS-A-0021 rule 6)"
    )]
    NotHuman,
    /// Only `parent` and `blocks` may be proposed.
    #[error("only parent and blocks edges may be proposed, not {0:?}")]
    NotProposable(String),
    /// Confirming would have created the edge, and the graph refused it.
    #[error(transparent)]
    Graph(#[from] GraphError),
    /// The database refused or failed.
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
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

/// Whether `actor` is a person rather than a service account.
///
/// The check lives here, in the service, rather than in each surface. Rule 6 is
/// the whole point of this table: an agent proposes, a human decides. A rule
/// enforced in two handlers is a rule that a third handler will not have, and
/// the third handler is the one that silently restructures a portfolio.
fn is_human(conn: &mut PgConnection, actor: Uuid) -> Result<bool, ProposalError> {
    #[derive(QueryableByName)]
    struct Kind {
        #[diesel(sql_type = Text)]
        kind: String,
    }
    let kind = sql_query("SELECT kind FROM public.users WHERE id = $1")
        .bind::<SqlUuid, _>(actor)
        .get_result::<Kind>(conn)?;
    Ok(kind.kind != crate::models::public::USER_KIND_SERVICE_ACCOUNT)
}

fn decide(
    conn: &mut PgConnection,
    id: Uuid,
    state: &str,
    actor: Uuid,
) -> Result<EdgeProposal, ProposalError> {
    Ok(sql_query(
        "UPDATE edge_proposals \
            SET state = $2, decided_by = $3, decided_at = now() \
          WHERE id = $1 \
         RETURNING id, source_id, target_id, relationship, state, claim, why, score, \
                   proposed_by, decided_by, decided_at, created_at",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(state)
    .bind::<SqlUuid, _>(actor)
    .get_result::<EdgeProposal>(conn)?)
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
pub fn confirm(
    conn: &mut PgConnection,
    id: Uuid,
    actor: Uuid,
) -> Result<EdgeProposal, ProposalError> {
    if !is_human(conn, actor)? {
        return Err(ProposalError::NotHuman);
    }
    let proposal = load(conn, id)?;
    if proposal.state != "pending" {
        return Err(ProposalError::AlreadyDecided {
            id,
            state: proposal.state,
        });
    }
    let relationship = match proposal.relationship.as_str() {
        "parent" => RelationshipType::Parent,
        "blocks" => RelationshipType::Blocks,
        other => return Err(ProposalError::NotProposable(other.to_string())),
    };

    conn.transaction(|conn| {
        graph::link_items(
            conn,
            proposal.source_id,
            proposal.target_id,
            relationship,
            actor,
        )?;
        decide(conn, id, "confirmed", actor)
    })
}

/// Reject a proposal.
///
/// Recorded, not deleted. A pair that keeps being proposed and keeps being
/// rejected is the clearest signal available that retrieval is wrong about
/// something, and [`stats`] is where that shows up. Deleting rejections would
/// delete the only honest measurement this feature has.
pub fn reject(
    conn: &mut PgConnection,
    id: Uuid,
    actor: Uuid,
) -> Result<EdgeProposal, ProposalError> {
    if !is_human(conn, actor)? {
        return Err(ProposalError::NotHuman);
    }
    let proposal = load(conn, id)?;
    if proposal.state != "pending" {
        return Err(ProposalError::AlreadyDecided {
            id,
            state: proposal.state,
        });
    }
    decide(conn, id, "rejected", actor)
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
