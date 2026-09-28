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

/// What an archive by one principal reaches (COLLIERY-T-0234): the
/// result of [`cascade_reach`]. Each descendant of the root is in exactly
/// one of the three lists.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CascadeReach {
    /// The descendants that the archive takes, in the order of
    /// [`cascade_descendants`].
    pub reached: Vec<Uuid>,
    /// The descendants where the archive stops: the principal cannot edit
    /// them.
    pub stopped: Vec<Uuid>,
    /// The descendants below a stop: `(item, stop)`. The archive does not
    /// take them, whether or not the principal can edit them.
    pub below: Vec<(Uuid, Uuid)>,
}

/// The cascade set of an archive by ONE principal (COLLIERY-T-0234): the
/// descendants of `root` that the archive takes, and those that it leaves.
///
/// THE ATTACK that this function stops. A principal creates an initiative,
/// so it can edit the initiative. The link rule lets it write a `parent`
/// edge from its initiative to a task of a different team. It then
/// archives the initiative. Until COLLIERY-T-0234 the cascade took each
/// descendant and asked nothing, so the task of the other team was
/// archived.
///
/// THE RULE. The archive takes a descendant when the principal may edit
/// it (`may_edit`, the edit rule) AND may edit each item between it and
/// the root, on each path. The archive stops at a descendant that the
/// principal cannot edit, and takes nothing below it.
///
/// WHY nothing below a stop. Archive is an operation on a subtree. An
/// archived task below a live initiative is hidden from the board of that
/// initiative with no archived parent to explain it, and a restore of the
/// root does not bring it back. So a subtree that the principal cannot
/// take is left whole.
///
/// WHY each path. An item can have two parents. If one of them is a stop,
/// the item is below a stop, and the rule above applies. The function
/// archives less when the graph gives two answers.
///
/// `may_edit` is asked one time for each descendant. The root is not
/// asked: the caller checks the edit rule for the root before the archive.
/// Cycle-safe, as [`cascade_descendants`] is.
pub fn cascade_reach(
    root: Uuid,
    edges: &[ParentEdge],
    may_edit: impl Fn(Uuid) -> bool,
) -> CascadeReach {
    use std::collections::HashSet;

    let descendants = cascade_descendants(root, edges);
    let cannot_edit: Vec<Uuid> = descendants
        .iter()
        .copied()
        .filter(|id| !may_edit(*id))
        .collect();

    // Not taken: each item that the principal cannot edit, and each item
    // below one.
    let mut left: HashSet<Uuid> = cannot_edit.iter().copied().collect();
    for id in &cannot_edit {
        left.extend(
            cascade_descendants(*id, edges)
                .into_iter()
                .filter(|below| *below != root),
        );
    }

    let reached: Vec<Uuid> = descendants
        .iter()
        .copied()
        .filter(|id| !left.contains(id))
        .collect();
    let taken: HashSet<Uuid> = reached.iter().copied().chain([root]).collect();

    // A stop is an item that the principal cannot edit, with a parent that
    // the archive takes. Each other item that is left is below a stop.
    let mut stopped: Vec<Uuid> = cannot_edit
        .iter()
        .copied()
        .filter(|id| {
            edges
                .iter()
                .any(|e| e.child_id == *id && taken.contains(&e.parent_id))
        })
        .collect();
    let mut named: HashSet<Uuid> = stopped.iter().copied().collect();
    let mut below: Vec<(Uuid, Uuid)> = Vec::new();
    let mut name_below = |stop: Uuid, named: &mut HashSet<Uuid>| {
        for id in cascade_descendants(stop, edges) {
            if left.contains(&id) && id != root && named.insert(id) {
                below.push((id, stop));
            }
        }
    };
    for stop in &stopped {
        name_below(*stop, &mut named);
    }
    // Only a graph with a cycle leaves an item with no name. Such an item
    // that the principal cannot edit is a stop too, so that each item that
    // is left has a reason.
    for id in &cannot_edit {
        if named.insert(*id) {
            stopped.push(*id);
            name_below(*id, &mut named);
        }
    }

    CascadeReach {
        reached,
        stopped,
        below,
    }
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

    fn edge(parent_id: Uuid, child_id: Uuid) -> ParentEdge {
        ParentEdge {
            parent_id,
            child_id,
        }
    }

    /// COLLIERY-T-0234: with each descendant editable, the reach is the
    /// cascade set of KAIROS-A-0001, in the same order.
    #[test]
    fn reach_is_the_cascade_set_when_the_principal_edits_each_item() {
        let (s, i, t, u) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let edges = [edge(s, i), edge(i, t), edge(i, u)];
        let reach = cascade_reach(s, &edges, |_| true);
        assert_eq!(reach.reached, cascade_descendants(s, &edges));
        assert!(reach.stopped.is_empty() && reach.below.is_empty());
    }

    /// COLLIERY-T-0234, the attack: a child that the principal cannot
    /// edit is not taken, and its siblings are.
    #[test]
    fn reach_leaves_a_child_that_the_principal_cannot_edit() {
        let (i, mine, theirs) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let edges = [edge(i, mine), edge(i, theirs)];
        let reach = cascade_reach(i, &edges, |id| id != theirs);
        assert_eq!(reach.reached, vec![mine]);
        assert_eq!(reach.stopped, vec![theirs]);
        assert!(reach.below.is_empty());
    }

    /// COLLIERY-T-0234: the archive stops at the initiative, and the task
    /// below it is left although the principal can edit it.
    #[test]
    fn reach_takes_nothing_below_a_stop() {
        let (s, i, t) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let edges = [edge(s, i), edge(i, t)];
        let reach = cascade_reach(s, &edges, |id| id != i);
        assert!(reach.reached.is_empty());
        assert_eq!(reach.stopped, vec![i]);
        assert_eq!(reach.below, vec![(t, i)]);
    }

    /// COLLIERY-T-0234: an item with two parents is left when ONE parent
    /// is a stop. A stop below a stop keeps its own reason.
    #[test]
    fn reach_archives_less_when_two_paths_disagree() {
        let (root, open, shut, shared, deep) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let edges = [
            edge(root, open),
            edge(root, shut),
            edge(open, shared),
            edge(shut, shared),
            edge(shared, deep),
        ];
        let reach = cascade_reach(root, &edges, |id| id != shut);
        assert_eq!(reach.reached, vec![open]);
        assert_eq!(reach.stopped, vec![shut]);
        let mut below = reach.below.clone();
        below.sort();
        let mut expected = vec![(shared, shut), (deep, shut)];
        expected.sort();
        assert_eq!(below, expected);

        // `shared` cannot be edited either: it has a parent that the
        // archive takes, so it is a stop with its own reason.
        let reach = cascade_reach(root, &edges, |id| id != shut && id != shared);
        assert_eq!(reach.reached, vec![open]);
        let mut stopped = reach.stopped.clone();
        stopped.sort();
        let mut expected = vec![shut, shared];
        expected.sort();
        assert_eq!(stopped, expected);
        assert_eq!(reach.below.len(), 1);
        assert_eq!(reach.below[0].0, deep);
    }

    /// COLLIERY-T-0234: each descendant is in exactly one list, for each
    /// answer of the edit rule, and the root is in none. A cycle does not
    /// change that.
    #[test]
    fn reach_puts_each_descendant_in_one_list() {
        let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        let edges = [
            edge(ids[0], ids[1]),
            edge(ids[1], ids[2]),
            edge(ids[2], ids[1]), // a cycle below the root
            edge(ids[0], ids[3]),
            edge(ids[3], ids[4]),
            edge(ids[4], ids[0]), // a cycle through the root
        ];
        for mask in 0u8..16 {
            let may_edit = |id: Uuid| {
                let index = ids.iter().position(|x| *x == id).expect("known id");
                mask & (1 << (index - 1)) != 0
            };
            let reach = cascade_reach(ids[0], &edges, may_edit);
            let mut all: Vec<Uuid> = reach
                .reached
                .iter()
                .chain(&reach.stopped)
                .copied()
                .chain(reach.below.iter().map(|(id, _)| *id))
                .collect();
            all.sort();
            let mut expected = cascade_descendants(ids[0], &edges);
            expected.sort();
            assert_eq!(all, expected, "mask {mask:04b}: {reach:?}");
            assert!(
                reach.reached.iter().all(|id| may_edit(*id)),
                "mask {mask:04b}: the archive takes an item that the principal cannot edit"
            );
            assert!(reach.stopped.iter().all(|id| !may_edit(*id)));
        }
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
