---
id: gui-remove-the-move-menu-from
level: task
title: "GUI: remove the Move menu from board cards — drag-and-drop is the transition UX"
short_code: "KAIROS-T-0075"
created_at: 2026-08-16T14:54:23.375500+00:00
updated_at: 2026-08-17T12:20:05.220702+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# GUI: remove the Move menu from board cards — drag-and-drop is the transition UX

## Objective

UAT feedback: "drop 'move' on cards — we have drag + drop." Remove the per-card Move menu so drag-and-drop (KAIROS-T-0064) is the transition UX, while preserving a keyboard-accessible path and migrating the e2e suite off the menu.

## Backlog Item Details

### Type
Feature (UX cleanup)

### Priority
P1 — direct UAT feedback on the primary board surface.

### Business Justification
- **User Value**: Cards carry a redundant control that duplicates drag-and-drop and adds visual noise on every movable card.
- **Effort Estimate**: S (UI removal is small; the real work is the e2e migration and the a11y decision).

## Current State

- The Move menu renders in `ItemCard` — `<Menu label="Move">` at `crates/kairos-web/src/pages/boards.rs:916`, inside `div.kairos-card__actions` (line 912), with a "Moving…" busy indicator (913–915). It renders only when `movable` is true — `has_targets && powers.get().transition` (line 860, gate at 911).
- Menu items call `run_transition` (boards.rs:164–178), the same path used by the T-0064 drop handler (column `on:drop` at 758–773). Drag legality is gated by column `on:dragover` (748–757).
- The `ItemCard` doc comment (boards.rs:829) designates the menu as **the keyboard/accessibility fallback** for drag-and-drop. The module doc at boards.rs:8–11 is stale (still says drag-and-drop is deferred).
- The fine-grained rendering work (T-0074) exists partly to keep this menu open across WS refetches (comments at boards.rs:364–365, 541, 615–617, 857–859).

### E2E dependencies (will break or hollow out on removal)
- `e2e/tests/smoke.spec.ts` step 5 (96–114): transitions the created card via the Move menu (`getByRole('button', { name: /Move/ })` at 106, `.cl-menu__dropdown` at 108–110). **Hard dependency** — must be rewritten to drag-and-drop or another mechanism.
- `smoke.spec.ts` step 6 (118–149): the T-0074 "open menu survives WS refetch" assertion uses the Move dropdown as its subject. **Hard dependency** — needs a new open-popover subject or the assertion is lost.
- `e2e/tests/drag.spec.ts:48`: asserts the Move button is visible as the a11y fallback. **Hard dependency** — assertion must be replaced/removed.
- `e2e/tests/team-lens.spec.ts:151`: asserts Move-button count 0 for a no-powers user. Passes vacuously after removal — replace with an equivalent capability-gating assertion or it silently loses meaning.

## Acceptance Criteria

## Acceptance Criteria

- [x] The Move menu no longer renders on board cards; drag-and-drop is the only pointer-based transition mechanism.
- [x] A keyboard-accessible transition path still exists — the item detail page's move control, capability-gated exactly as the menu was (`powers.transition` via the shared `board_powers` mirror).
- [x] `smoke.spec.ts` step 5 transitions the card without the Move menu (atomic-retry drag-and-drop).
- [x] The T-0074 regression coverage is preserved with a new subject: the watched card's DOM node is stamped and must survive the WS refetch (element identity — a stronger, menu-free assertion of the same guarantee).
- [x] `team-lens.spec.ts` asserts the detail-page move control does not render for a no-powers user (replacing the vacuous count-0 check).
- [x] Stale module doc at boards.rs:8–11 is corrected.
- [x] Full e2e suite green (drag, smoke incl. new step 7b, team-lens — no retries).

## Resolved Questions

- **A11y replacement**: option (a) implemented — the item detail page's Board panel gains a "Move to" select + Move button (native controls, keyboard-accessible), gated by the same `transition_items` power as the drag. Cards are clean.
- **Sequencing with KAIROS-T-0076**: implemented back-to-back in the same session; the card layout is touched once more by T-0076 with tests updated in step.

## Status Updates

- 2026-08-16: Created from UAT feedback with investigation findings (design workflow, session ffc0d1f9).
- 2026-08-17: Implementation (PO accepted option (a) by ralphing the ticket — detail page gains the keyboard path):
  - boards.rs: Move menu + busy indicator removed from ItemCard; `on_changed`/`on_error` props dropped (only the column drop handler mutates); `Menu`/`MenuItem` imports removed; module doc + T-0073/T-0074 comments rewritten (menu references → DOM-node identity); `mod data`, `BoardPowers`, `board_powers` now `pub(crate)` for reuse.
  - item.rs: new `MoveControl` in the Board panel — legal targets from the board's transitions (A-0002), gated by `board_powers(...).transition` (same KAIROS-T-0072 mirror as drag), native Select + Move button (keyboard-accessible), success → "Moved to {column}." notice + refetch. `BoardPanel` gains `code`/`on_moved` props.
  - item/api.rs: `BoardInfo` mirror extended with `team_id` + `transitions` (+ decode test).
  - e2e: smoke step 5 → atomic-retry drag; step 6 → element-stamp assertion (T-0074 coverage preserved menu-free: the watched card's DOM node must survive the WS refetch); new step 7b exercises the move control (Todo→Active on the created card); drag.spec asserts NO move menu; team-lens 5c asserts the detail-page control is capability-gated for bob (replacing the vacuous count-0 check).
  - Unit tests green (54 passed); WASM bundle builds. E2E run next.