---
id: graph-e2e-fixture-wave-graph-spec
level: task
title: "Graph e2e + fixture wave: graph.spec, badge coverage, fallout sweep"
short_code: "KAIROS-T-0092"
created_at: 2026-08-29T14:25:06.694874+00:00
updated_at: 2026-08-29T15:38:01.277269+00:00
parent: KAIROS-I-0008
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0008
---

# Graph e2e + fixture wave: graph.spec, badge coverage, fallout sweep

## Parent Initiative

[[KAIROS-I-0008]] — Focal Flight-Level Graph View. Final wave; depends on KAIROS-T-0088..0091.

## Objective

Make the graph demonstrable and regression-proof: fixture check, a graph e2e spec covering the whole surface (canvas, expand, refocus, tab, WS live, badges), and the fallout sweep — full ladder at the end.

## Implementation Notes

- **Fixtures**: the stock seed already carries a real blocks web (auth → provisioning, spike → webhook handler, cross-team). Verify it exercises depth-2 + a cross-initiative crossing on the canvas; add at most ONE edge if a visual case is missing (then update seed_demo counts — the recurring lesson: grep ALL count assertions at once).
- **graph.spec.ts** (login as bob, in-app navigation only): board → card shows blocked-by badge → badge click lands on the graph → canvas asserts (three columns present, focal node highlighted, a blocks arrow path exists, NO force layout ⇒ positions stable across a reload) → `+N` expand adds nodes without losing view → refocus on a neighbor + browser back returns → side panel lists the PRD under the signup initiative (supports) and suppresses when absent → item detail Graph tab renders the same canvas → WS live: API writer (mintToken) transitions a visible task; the open graph updates → traverse switch on /search reads as scoping.
- **Fallout sweep**: smoke.spec / team-lens.spec touch `/items/` details (tab addition — check selectors that assume the old single-stack layout); any spec hitting `/search/relationships` (the five-panel assertions die with the panels); seed_demo counts if fixtures moved; openapi/client_roundtrip gates re-run via the ladder.
- **Determinism check in e2e**: reload the graph page and assert two sampled node positions are identical — the cheap browser-level proof of the layout AC.

## Acceptance Criteria

## Acceptance Criteria

- [x] graph.spec covers: badge click-through, canvas render, deterministic reload, expand-in-place, refocus + back, side panel presence/suppression, detail tab, WS live update, traverse relabel — green without retries.
- [x] Seeded demo shows visible badges and a depth-2-worthy blocks web; any fixture additions asserted in seed_demo.
- [x] Existing specs updated for the detail tab and the relationships-page replacement; no dead selectors.
- [x] Full ladder green: unit, integration, e2e.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0008 decomposition (design PO-approved).
- 2026-08-29: Fixture decision: NO seed additions — the stock blocks web (T-0002→T-0003 within sign-up, T-0005→T-0006) already renders badges, arrows, lanes, +N, and the PRD side-panel case; seed_demo counts untouched. `graph.spec.ts` written (runs 2nd alphabetically, after drag which only touches web-delivery): badges on both directions → badge click → `/items/DEMO-T-0002?view=graph` → canvas asserts (3 headers, focus code, edge, lane) → deterministic RELOAD (two sampled node x-positions identical — T-0071 silent restore makes reload legal) → +N expand grows the node count without navigating → refocus to DEMO-T-0003 (`?trail=` URL + trail chip) → browser back returns to the tab → PRD in Supporting material and NOT on the canvas → WS live proven TWICE with a reversible Active→Blocked→Active round-trip (later specs pin T-0002 in Active — the revert keeps the suite order-safe) → traverse relabel on /search. Fallout swept: lifecycle.spec's "Relationships" panel is the item-detail one (untouched); progress.spec's `.kairos-progress` absence check unaffected by the tab pills; smoke/team-lens pins on Active/Todo/Backlog stay true because the only mutation reverts. Full e2e running.
- 2026-08-29: The spec caught a REAL `+N` semantics bug (run 1: expand added 0 nodes): hidden-neighbor counts were `degree − canvas edges`, so side-panel material (the signup initiative's supports→PRD edge) looked "hidden" and its badge promised an expansion that fetched nothing new. Fixed: `+N` now means NOT YET FETCHED — computed in the component as `degree − every incident edge in the full merged response` (side-panel edges count as fetched); the layout takes `hidden_neighbors` as input instead of computing it. With the fix the stock depth-2 view has exactly one badge (the strategy, +3: billing + the two buckets) and expanding it grows the canvas. Layout tests updated; re-running e2e.
- 2026-08-29: COMPLETE. Run 2: `angreal test e2e` 9/9 specs passed in 10.7s, zero retries (graph.spec green end to end — badges, canvas, deterministic reload, expand, refocus/back, side panel, WS round-trip, relabel). Full ladder: `angreal test unit` clean, `angreal test integration` 32/32 (run under T-0091 — the only later change was web-tier, covered by unit + e2e), e2e 9/9.