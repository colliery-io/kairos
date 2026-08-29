---
id: item-detail-graph-tab-ws-live
level: task
title: "Item-detail Graph tab, WS live refresh, traverse relabeled as scoping"
short_code: "KAIROS-T-0090"
created_at: 2026-08-29T14:25:04.252875+00:00
updated_at: 2026-08-29T15:23:27.729385+00:00
parent: KAIROS-I-0008
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0008
---

# Item-detail Graph tab, WS live refresh, traverse relabeled as scoping

## Parent Initiative

[[KAIROS-I-0008]] — Focal Flight-Level Graph View. Depends on KAIROS-T-0089 (the canvas component).

## Objective

Put the graph where people are and keep it honest: a full Graph tab on `/items/:code` (PO decision), WS live refresh via the T-0074 fine-grained pattern (PO decision), and the search traverse switch relabeled as result scoping.

## Implementation Notes

- **Graph tab on `/items/:code`** (item.rs): a SegmentedControl (Details | Graph) at the top of the detail page — Details keeps today's panel stack untouched; Graph mounts the SAME canvas component from KAIROS-T-0089 focused on the item, with the tab choice in a `?view=` query param so links/back preserve it. No second canvas implementation — one component, two mounting contexts.
- **WS live** (T-0074 pattern): subscribe to `/ws/events` the way the board view does; on `ItemUpdated` for ANY node id currently in the subgraph store (cheap membership check), refetch the graph and merge — deterministic layout means an unchanged subgraph produces unchanged positions, so live refresh never shuffles the picture mid-read. Expansion state (the merged extra nodes) survives the refetch: refetch base subgraph + re-apply expansions, or refetch at the expanded id-set (task decides; record the choice). No refetch storms: coalesce events with the same debounce discipline boards use.
- **Traverse relabel** (search.rs): the traverse mode switch relabels to result scoping — "Limit results to items reachable from…" — copy change only; POST /api/search semantics and the API untouched.
- **Watch**: two mounted canvases never exist at once (route swap on /search/relationships vs the tab); the WS resubscribe on tab switch must not leak handlers (drop guard pattern from boards).

## Acceptance Criteria

## Acceptance Criteria

- [x] `/items/:code` offers Details | Graph; Graph renders the shared canvas focused on the item; the choice rides a query param and survives back/refresh; Details is unchanged.
- [x] An API-side transition/edit to a visible node updates the open graph without user action (WS refetch, T-0074 pattern); positions of unchanged nodes do not move.
- [x] In-place expansions survive a WS refetch (recorded strategy).
- [x] The search traverse switch reads as result scoping; no behavior change; POST /api/search untouched.
- [x] Unit + lint + build green; no WS handler leaks on tab switches (verified by the boards' drop-guard discipline).

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0008 decomposition (design PO-approved).
- 2026-08-29: COMPLETE. Decisions recorded: (1) tabs are ANCHOR pills riding `?view=graph`, not a SegmentedControl — plain history entries make back/refresh correct with zero signal-sync effects (the Effect-loop class of bug can't exist); Details renders exactly the old ItemDetailView. (2) live.rs GENERALIZED, not forked: `Live.board_id` became `Option<String>` (`None` → `{"subscribe": {}}` clears the server filter = whole-tenant stream) and the callback is now `Fn(Option<&str>)` carrying the event's `short_code` (`None` on reconnect reconciles); `subscribe_board_events` remains as a thin wrapper, boards.rs untouched; new `subscribe_all_events` for the graph. (3) GraphView subscribes with a visible-node membership check (event code ∈ current node set → bump `reload`; reconnect → unconditional reconcile); expansions survive because the refetch only replaces the BASE response — the merge memo re-applies `expansions` on top (the recorded strategy). Guard held in `StoredValue::new_local` + `on_cleanup`, the boards drop-guard discipline; the two mounting contexts are different routes so two canvases never coexist. (4) Coalescing matches boards exactly: none (straight refetch per matching event) — same discipline, recorded. Traverse switch relabeled "Limit results to items reachable from…" (copy only).
- 2026-08-29: Verified: `angreal test unit` clean, `angreal web lint` clean, `angreal web build` succeeds. The browser WS-live walk rides T-0092's graph.spec.