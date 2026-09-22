---
id: uat-j2-planning-strategy
level: task
title: "UAT J2 planning: strategy → initiative → tasks with a blocks edge; progress bar, graph, live WS move, traverse + repo search, lifecycle"
short_code: "KAIROS-T-0119"
created_at: 2026-09-22T11:15:22.725893+00:00
updated_at: 2026-09-22T11:57:20.081166+00:00
parent: KAIROS-I-0011
blocked_by: [KAIROS-T-0117]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# UAT J2 planning: strategy → initiative → tasks with a blocks edge; progress bar, graph, live WS move, traverse + repo search, lifecycle

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Implement journey J2 (I-0011 D4) — "A strategy is broken down until it is work on a board" — as `uat/journeys/planning.journey.ts`.

## Implementation Notes

### Technical Approach

1. alice (GUI) creates a strategy document `uat-<run> Strategy` and an initiative `uat-<run> Initiative` under it (parent relationship) from the boards view; the initiatives board shows the card. (Check how the GUI creates strategies/initiatives — the global create control from T-0062/T-0063; if the GUI cannot set the parent, create the edge via CLI and narrate "(CLI)".)
2. alice (CLI) creates two tasks on `platform-delivery` under the initiative (`parent`), the first bound `--repo payments-api`, and links `blocks` second → first → observe short codes.
3. alice (GUI) opens the initiative: children progress reads 0/2 (progress.spec shows the selectors' text); the Graph tab shows initiative + both tasks + the blocks arrow (graph.spec shows what is asserted).
4. bob (GUI, its own context) opens `platform-delivery`; alice (CLI) transitions the unblocked task to Active; bob's page shows the card in Active without reload; the blocked task's card carries the blocked-by badge naming the other.
5. alice (CLI) `kairos search --traverse …` from the initiative returns both tasks; `kairos search --repo payments-api` returns the bound task and not the other (filter by the run prefix in the assertion).
6. alice (GUI) moves the strategy document's lifecycle to *review*; the badge updates (lifecycle.spec).
7. Teardown via ledger: tasks → initiative → strategy (soft-delete cascade preview is not needed; delete leaf-first).

### Dependencies

T-0117.

### Risk Considerations

- GUI create flows for strategy/initiative may need the CLI for the parent edge; narrate honestly.
- WS live-update timing: expect-polling only, 15s.

## Acceptance Criteria

- [x] Green under compose (hand-run, `UAT_MODE=compose`) and `angreal test uat --server … --journey planning`; report lists short codes, "0 of 2 done", graph nodes/arrow, traverse children and the repo-filter hit.
- [x] `kairos search --query uat-` returns nothing after the run.

## Status Updates

**2026-09-22** — Completed in `4fdb6a1`.

- Story adjustments vs D4: the strategy→initiative edge is made in the GUI (Graph tab "Manage links" panel) rather than over the API; the editorial lifecycle is a *document* feature (A-0018), so the journey attaches a design note to the initiative and moves that to review instead of "the strategy document".
- **Product bug found and fixed:** `kairos search --repo <slug>` alone was refused ("nothing to search for") — `--repo` was missing from the CLI's has-filter check although README documents the invocation. Fixed in `crates/kairos-cli/src/commands/search.rs` with unit test `repo_alone_is_a_filter`.
- **GUI observation (not fixed):** `/items/:code` renders its Details/Graph tab anchors from the route param, which is empty on the first render (`/items/?view=graph` → router fallback "Nothing here"). Only reachable by a programmatic click within the first frame; the journey waits for the code text first. Candidate hygiene ticket.
- Graph semantics: `parent` is drawn as containment lanes, `blocks` as the only arrow — the assertion is `1` arrow, not 3 edges.
- Tooling gotcha: perl replacements interpolate `${…}` in TS template strings — use the Edit tool for those.