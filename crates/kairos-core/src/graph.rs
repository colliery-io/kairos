//! Pure relationship-graph rules (KAIROS-A-0001, KAIROS-T-0013): the
//! type-rule matrix for the five edge types and cycle detection over
//! already-loaded edges.
//!
//! Per KAIROS-A-0009 everything here is a function over in-memory data — no
//! diesel, no I/O. `kairos-db::graph` resolves UUIDs to entity types (via
//! the `entity_directory` view), calls these decisions, and persists the
//! outcome (the same layering as `board`/`abac`/`items`).
//!
//! # Edge semantics (KAIROS-A-0001 / S-0004 DDL comments)
//!
//! | relationship | source | target |
//! |---|---|---|
//! | `parent` | strategy | initiative |
//! | `parent` | initiative | task |
//! | `supports` | strategy/initiative/task (the SUPPORTED item) | document/adr |
//! | `informs` | document/adr | strategy/initiative/task |
//! | `supersedes` | adr | adr |
//! | `blocks` | strategy/initiative/task | strategy/initiative/task |
//!
//! `parent` is exactly the workflow hierarchy (S-0004: "strategy ->
//! initiative, initiative -> task"); documents and ADRs attach via
//! `supports`, never `parent`. `supports` is stored with the supported
//! workflow item as SOURCE and the supporting document/ADR as TARGET
//! (S-0004: "target supports source — document supports initiative"), the
//! same orientation `kairos-db::abac::resolve_authorization_board` reads.
//! `informs` is restricted in v1 to document/adr → workflow item: A-0001's
//! "company vision informs the strategy board" names a BOARD as the
//! target, but boards are not in the shared UUID item space the
//! relationship table spans, so board-level references are out of scope
//! here (recorded in KAIROS-T-0013). `blocks` allows same-type and
//! cross-type dependencies between workflow items.

use uuid::Uuid;

use crate::short_code::ItemType;

/// The five KAIROS-A-0001 edge types. This is the pure mirror of
/// `kairos-db`'s TEXT-backed `RelationshipType`; it exists so the rule
/// matrix stays free of diesel types (KAIROS-A-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relationship {
    /// Workflow hierarchy: source is parent of target.
    Parent,
    /// Attachment: target (document/adr) supports source (workflow item).
    Supports,
    /// Reference: source (document/adr) informs target (workflow item).
    Informs,
    /// ADR replacement chain: source supersedes target.
    Supersedes,
    /// Dependency: source blocks target.
    Blocks,
}

impl Relationship {
    /// Every variant, in declaration order.
    pub const ALL: &'static [Relationship] = &[
        Relationship::Parent,
        Relationship::Supports,
        Relationship::Informs,
        Relationship::Supersedes,
        Relationship::Blocks,
    ];

    /// The TEXT value stored in `item_relationships.relationship`.
    pub fn as_str(self) -> &'static str {
        match self {
            Relationship::Parent => "parent",
            Relationship::Supports => "supports",
            Relationship::Informs => "informs",
            Relationship::Supersedes => "supersedes",
            Relationship::Blocks => "blocks",
        }
    }

    /// Whether this edge type must stay acyclic (KAIROS-A-0001: "cycle
    /// prevention (e.g., A blocks B blocks A) is enforced at the
    /// application level"). `parent` cycles are additionally precluded by
    /// the type matrix (the hierarchy only descends), so the check there is
    /// defense-in-depth against malformed pre-existing edges.
    pub fn requires_acyclicity(self) -> bool {
        matches!(self, Relationship::Parent | Relationship::Blocks)
    }
}

impl std::fmt::Display for Relationship {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `(relationship, source_type, target_type)` combination outside the
/// KAIROS-A-0001 matrix (see module docs). `rule` restates the allowed
/// shape for the relationship, for error messages.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{relationship} edge {source_type} -> {target_type} is not allowed ({rule})")]
pub struct GraphRuleError {
    /// The rejected edge type.
    pub relationship: Relationship,
    /// The source item's entity type.
    pub source_type: ItemType,
    /// The target item's entity type.
    pub target_type: ItemType,
    /// The allowed shape for this relationship.
    pub rule: &'static str,
}

/// The allowed shape of each relationship, as prose (used in
/// [`GraphRuleError`]).
fn rule_text(relationship: Relationship) -> &'static str {
    match relationship {
        Relationship::Parent => "parent runs strategy -> initiative or initiative -> task",
        Relationship::Supports => {
            "supports runs workflow item (strategy/initiative/task) -> document/adr"
        }
        Relationship::Informs => {
            "informs runs document/adr -> workflow item (strategy/initiative/task)"
        }
        Relationship::Supersedes => "supersedes runs adr -> adr",
        Relationship::Blocks => {
            "blocks runs workflow item -> workflow item (strategy/initiative/task)"
        }
    }
}

/// Whether this entity type is a workflow item (lives in the flight-level
/// hierarchy, as opposed to a supporting document or ADR).
fn is_workflow(item_type: ItemType) -> bool {
    matches!(
        item_type,
        ItemType::Strategy | ItemType::Initiative | ItemType::Task
    )
}

/// The KAIROS-A-0001 type-rule matrix: whether a `relationship` edge may
/// run from a `source_type` item to a `target_type` item (see module docs
/// for the full table). Pure; the DB layer resolves the UUIDs to types and
/// calls this before every insert.
pub fn check_link(
    relationship: Relationship,
    source_type: ItemType,
    target_type: ItemType,
) -> Result<(), GraphRuleError> {
    use ItemType::{Adr, Document, Initiative, Strategy, Task};

    let allowed = match relationship {
        Relationship::Parent => matches!(
            (source_type, target_type),
            (Strategy, Initiative) | (Initiative, Task)
        ),
        Relationship::Supports => is_workflow(source_type) && matches!(target_type, Document | Adr),
        Relationship::Informs => matches!(source_type, Document | Adr) && is_workflow(target_type),
        Relationship::Supersedes => matches!((source_type, target_type), (Adr, Adr)),
        Relationship::Blocks => is_workflow(source_type) && is_workflow(target_type),
    };

    if allowed {
        Ok(())
    } else {
        Err(GraphRuleError {
            relationship,
            source_type,
            target_type,
            rule: rule_text(relationship),
        })
    }
}

/// A directed edge of ONE relationship type from `item_relationships`
/// (source → target).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// `item_relationships.source_id`.
    pub source_id: Uuid,
    /// `item_relationships.target_id`.
    pub target_id: Uuid,
}

/// Whether adding `source → target` to `edges` (all existing edges of the
/// SAME relationship type) would create a directed cycle: true iff
/// `source` is already reachable from `target` (or `source == target`).
/// Pure BFS over the loaded edges; tolerant of pre-existing cycles in a
/// malformed graph (visited-set bounded).
pub fn would_create_cycle(edges: &[Edge], source: Uuid, target: Uuid) -> bool {
    if source == target {
        return true;
    }
    let mut seen = std::collections::HashSet::from([target]);
    let mut frontier = vec![target];
    while let Some(node) = frontier.pop() {
        for edge in edges.iter().filter(|e| e.source_id == node) {
            if edge.target_id == source {
                return true;
            }
            if seen.insert(edge.target_id) {
                frontier.push(edge.target_id);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ItemType::{Adr, Document, Initiative, Strategy, Task};

    /// The COMPLETE allowed set: every `(relationship, source, target)`
    /// combination not listed here must be rejected. 5 relationships × 5 × 5
    /// types = 125 combinations checked exhaustively.
    fn is_allowed(relationship: Relationship, source: ItemType, target: ItemType) -> bool {
        let workflow = [Strategy, Initiative, Task];
        let attachment = [Document, Adr];
        match relationship {
            Relationship::Parent => {
                (source, target) == (Strategy, Initiative) || (source, target) == (Initiative, Task)
            }
            Relationship::Supports => workflow.contains(&source) && attachment.contains(&target),
            Relationship::Informs => attachment.contains(&source) && workflow.contains(&target),
            Relationship::Supersedes => (source, target) == (Adr, Adr),
            Relationship::Blocks => workflow.contains(&source) && workflow.contains(&target),
        }
    }

    #[test]
    fn full_matrix_exhaustive() {
        for &relationship in Relationship::ALL {
            for &source in ItemType::ALL {
                for &target in ItemType::ALL {
                    let result = check_link(relationship, source, target);
                    if is_allowed(relationship, source, target) {
                        assert!(
                            result.is_ok(),
                            "{relationship} {source} -> {target} must be allowed: {result:?}"
                        );
                    } else {
                        let err = match result {
                            Err(err) => err,
                            Ok(()) => {
                                panic!("{relationship} {source} -> {target} must be rejected")
                            }
                        };
                        assert_eq!(
                            (err.relationship, err.source_type, err.target_type),
                            (relationship, source, target),
                            "error carries the rejected combination"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn matrix_spot_checks_match_a0001() {
        // The A-0001 examples verbatim.
        assert!(check_link(Relationship::Parent, Strategy, Initiative).is_ok());
        assert!(check_link(Relationship::Parent, Initiative, Task).is_ok());
        assert!(check_link(Relationship::Supports, Initiative, Document).is_ok());
        assert!(check_link(Relationship::Supports, Strategy, Adr).is_ok());
        assert!(check_link(Relationship::Supersedes, Adr, Adr).is_ok());
        assert!(check_link(Relationship::Blocks, Task, Task).is_ok());
        // Cross-level blocks are allowed (A-0001 restricts blocks to
        // workflow items, not to same-type pairs).
        assert!(check_link(Relationship::Blocks, Initiative, Task).is_ok());

        // Documents/ADRs attach via supports, never parent.
        assert!(check_link(Relationship::Parent, Initiative, Document).is_err());
        assert!(check_link(Relationship::Parent, Strategy, Adr).is_err());
        // parent never skips a level or runs upward.
        assert!(check_link(Relationship::Parent, Strategy, Task).is_err());
        assert!(check_link(Relationship::Parent, Task, Initiative).is_err());
        // supports is stored workflow-item -> document, not the reverse.
        assert!(check_link(Relationship::Supports, Document, Initiative).is_err());
        // informs is document/adr -> workflow item only (v1 restriction).
        assert!(check_link(Relationship::Informs, Document, Strategy).is_ok());
        assert!(check_link(Relationship::Informs, Adr, Task).is_ok());
        assert!(check_link(Relationship::Informs, Strategy, Document).is_err());
        assert!(check_link(Relationship::Informs, Document, Document).is_err());
        // supersedes is ADR-only, both ends.
        assert!(check_link(Relationship::Supersedes, Adr, Document).is_err());
        assert!(check_link(Relationship::Supersedes, Strategy, Strategy).is_err());
        // Documents never block or get blocked.
        assert!(check_link(Relationship::Blocks, Document, Task).is_err());
        assert!(check_link(Relationship::Blocks, Task, Adr).is_err());
    }

    #[test]
    fn rule_error_displays_the_shape() {
        let err = check_link(Relationship::Supersedes, Strategy, Task).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("supersedes") && message.contains("adr -> adr"),
            "message restates the allowed shape: {message}"
        );
    }

    #[test]
    fn acyclicity_applies_to_parent_and_blocks_only() {
        let acyclic: Vec<Relationship> = Relationship::ALL
            .iter()
            .copied()
            .filter(|r| r.requires_acyclicity())
            .collect();
        assert_eq!(acyclic, [Relationship::Parent, Relationship::Blocks]);
    }

    fn edge(source_id: Uuid, target_id: Uuid) -> Edge {
        Edge {
            source_id,
            target_id,
        }
    }

    #[test]
    fn direct_cycle_detected() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        // a -> b exists; b -> a closes the 2-cycle.
        let edges = [edge(a, b)];
        assert!(would_create_cycle(&edges, b, a));
        // The same edge again is not a cycle (it is a duplicate, handled by
        // the UNIQUE constraint, not by this check).
        assert!(!would_create_cycle(&edges, a, b));
        // Self-links are always cycles.
        assert!(would_create_cycle(&[], a, a));
    }

    #[test]
    fn transitive_cycle_detected() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        // a -> b -> c exists; c -> a closes the 3-cycle.
        let edges = [edge(a, b), edge(b, c)];
        assert!(would_create_cycle(&edges, c, a));
        // ... but extending the chain is fine.
        assert!(!would_create_cycle(&edges, c, Uuid::new_v4()));
        assert!(!would_create_cycle(&edges, Uuid::new_v4(), a));
    }

    #[test]
    fn diamond_is_not_a_cycle() {
        let top = Uuid::new_v4();
        let left = Uuid::new_v4();
        let right = Uuid::new_v4();
        let bottom = Uuid::new_v4();
        // top -> left -> bottom and top -> right; right -> bottom closes a
        // DIAMOND (two paths to bottom), which is allowed — no directed
        // cycle exists.
        let edges = [edge(top, left), edge(top, right), edge(left, bottom)];
        assert!(!would_create_cycle(&edges, right, bottom));
        // With the diamond complete, a back-edge bottom -> top WOULD close
        // a directed cycle (top -> left -> bottom -> top) and is rejected.
        let with_diamond = [
            edge(top, left),
            edge(top, right),
            edge(left, bottom),
            edge(right, bottom),
        ];
        assert!(would_create_cycle(&with_diamond, bottom, top));
    }

    #[test]
    fn cycle_check_tolerates_malformed_existing_cycles() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        // A pre-existing (malformed) cycle a <-> b must not hang the BFS.
        let edges = [edge(a, b), edge(b, a)];
        assert!(!would_create_cycle(&edges, c, Uuid::new_v4()));
        assert!(would_create_cycle(&edges, b, a));
    }
}
