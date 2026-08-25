//! Pure item write-path decisions (KAIROS-A-0004/A-0001, KAIROS-T-0012):
//! the optimistic-concurrency version check and the soft-delete cascade set
//! over already-loaded `parent` edges.
//!
//! Per KAIROS-A-0009 everything here is a function over in-memory data — no
//! diesel, no I/O. `kairos-db::items` loads rows, calls these decisions,
//! and persists the outcome (the same layering as `board`/`abac`).

use uuid::Uuid;

/// A `parent` edge from the `item_relationships` graph: `parent_id` is the
/// source (owner), `child_id` the target (owned) — KAIROS-A-0001 semantics
/// (strategy → initiative, initiative → task).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentEdge {
    /// The owning item (`item_relationships.source_id`).
    pub parent_id: Uuid,
    /// The owned item (`item_relationships.target_id`).
    pub child_id: Uuid,
}

/// The soft-delete cascade set (KAIROS-A-0001 "deleting a parent cascades
/// soft-deletion to children, walked via `parent` relationships"): every
/// descendant of `root` reachable through `parent` edges, deduplicated,
/// `root` itself excluded. Cycle-safe — a malformed graph cannot loop it
/// (cycle *prevention* is API-layer validation; this computation merely
/// tolerates them).
pub fn cascade_descendants(root: Uuid, edges: &[ParentEdge]) -> Vec<Uuid> {
    let mut seen = std::collections::HashSet::from([root]);
    let mut order: Vec<Uuid> = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for edge in edges.iter().filter(|e| e.parent_id == parent) {
            if seen.insert(edge.child_id) {
                order.push(edge.child_id);
                frontier.push(edge.child_id);
            }
        }
    }
    order
}

/// The optimistic-concurrency decision for a content edit (KAIROS-A-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionCheck {
    /// The submitted base version matches: the write may proceed and the
    /// item becomes `next_version`.
    Proceed {
        /// `current_version + 1`.
        next_version: i32,
    },
    /// The submitted base version is stale: reject (HTTP 409), the client
    /// must reload `current_version` and reconcile.
    Conflict {
        /// The item's actual current version.
        current_version: i32,
    },
}

/// Decide whether a content edit based on `expected_version` may be applied
/// to an item currently at `current_version` (KAIROS-A-0004 optimistic
/// concurrency).
///
/// This is the pure mirror of the atomic SQL enforcement in
/// `kairos-db::items::update_item_content` (`UPDATE … WHERE version =
/// expected_version`) — one contract, two layers, so they can never
/// disagree (the same pattern as `abac::capability_matches`).
pub fn check_version(current_version: i32, expected_version: i32) -> VersionCheck {
    if current_version == expected_version {
        VersionCheck::Proceed {
            next_version: current_version + 1,
        }
    } else {
        VersionCheck::Conflict { current_version }
    }
}

/// Sum a children-by-column rollup into `(done, total)` counts
/// (KAIROS-T-0080): each entry is one column's `(is_done, occupant
/// count)`; `done` is the sum over done-flagged columns. Pure — the
/// grouping itself is the single SQL query in
/// `kairos-db::graph::children_progress`.
pub fn children_progress_counts(by_column: &[(bool, i64)]) -> (i64, i64) {
    let total = by_column.iter().map(|(_, count)| count).sum();
    let done = by_column
        .iter()
        .filter(|(is_done, _)| *is_done)
        .map(|(_, count)| count)
        .sum();
    (done, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_check_matches_a0004_contract() {
        assert_eq!(
            check_version(3, 3),
            VersionCheck::Proceed { next_version: 4 }
        );
        assert_eq!(
            check_version(4, 3),
            VersionCheck::Conflict { current_version: 4 }
        );
        // A stale-but-higher submitted version is still a conflict, not a
        // proceed: only an exact match writes.
        assert_eq!(
            check_version(2, 7),
            VersionCheck::Conflict { current_version: 2 }
        );
    }

    #[test]
    fn cascade_walks_chain() {
        let s = Uuid::new_v4();
        let i = Uuid::new_v4();
        let t = Uuid::new_v4();
        let edges = [
            ParentEdge {
                parent_id: s,
                child_id: i,
            },
            ParentEdge {
                parent_id: i,
                child_id: t,
            },
        ];
        assert_eq!(cascade_descendants(s, &edges), vec![i, t]);
        assert_eq!(cascade_descendants(i, &edges), vec![t]);
        assert_eq!(cascade_descendants(t, &edges), Vec::<Uuid>::new());
    }

    #[test]
    fn cascade_ignores_unrelated_edges_and_dedups_diamonds() {
        let root = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let shared = Uuid::new_v4();
        let stranger = Uuid::new_v4();
        let orphan = Uuid::new_v4();
        let edges = [
            ParentEdge {
                parent_id: root,
                child_id: a,
            },
            ParentEdge {
                parent_id: root,
                child_id: b,
            },
            // Diamond: both a and b claim `shared`.
            ParentEdge {
                parent_id: a,
                child_id: shared,
            },
            ParentEdge {
                parent_id: b,
                child_id: shared,
            },
            // Unrelated subtree must not be pulled in.
            ParentEdge {
                parent_id: stranger,
                child_id: orphan,
            },
        ];
        let result = cascade_descendants(root, &edges);
        assert_eq!(result.len(), 3, "shared child appears once: {result:?}");
        assert!(result.contains(&a) && result.contains(&b) && result.contains(&shared));
        assert!(!result.contains(&stranger) && !result.contains(&orphan));
    }

    /// KAIROS-T-0080: the done count is exactly the occupants of
    /// done-flagged columns; no flagged columns means done = 0 (clients
    /// then show composition only, never a percentage).
    #[test]
    fn children_progress_counts_sums_done_columns() {
        assert_eq!(children_progress_counts(&[]), (0, 0));
        assert_eq!(
            children_progress_counts(&[(false, 3), (true, 2), (false, 1), (true, 1)]),
            (3, 7)
        );
        // Zero done-flagged columns: composition only.
        assert_eq!(children_progress_counts(&[(false, 4), (false, 2)]), (0, 6));
    }

    #[test]
    fn cascade_tolerates_cycles_and_excludes_root() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let edges = [
            ParentEdge {
                parent_id: a,
                child_id: b,
            },
            // Malformed back-edge: b claims to be a's parent.
            ParentEdge {
                parent_id: b,
                child_id: a,
            },
        ];
        // Terminates, and the root never appears in its own cascade set.
        assert_eq!(cascade_descendants(a, &edges), vec![b]);
    }
}
