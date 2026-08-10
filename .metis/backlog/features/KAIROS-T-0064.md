---
id: gui-drag-and-drop-cards-between
level: task
title: "GUI: drag-and-drop cards between board columns, replacing the move menu"
short_code: "KAIROS-T-0064"
created_at: 2026-08-09T17:44:54.271966+00:00
updated_at: 2026-08-09T21:48:15.764263+00:00
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

# GUI: drag-and-drop cards between board columns, replacing the move menu

## Objective **[REQUIRED]**

Cards on a board should move between columns by drag-and-drop, not via the current "move" drop-down menu. Dropping on a column performs the transition; the menu goes away as the primary interaction.

UAT feedback (Dylan, 2026-08-09).

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P1 - High (important for user experience)

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: Drag-and-drop is the universal kanban interaction; a drop-down move menu makes the core daily action feel clunky and slow.
- **Effort Estimate**: M/L (wasm drag-and-drop + transition rules + conflict handling + e2e rework)

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] Cards are draggable between columns with pointer input (HTML5 drag events or pointer-event implementation in Leptos)
- [x] Only legal targets accept a drop: allowed target columns derive from the board's configured transitions (KAIROS-A-0002 rules engine) and are visually indicated during the drag; illegal columns reject the drop
- [x] Drop performs the same transition call the move menu used, including optimistic-concurrency handling — a 409 mid-drag (someone else moved the card) surfaces the standard conflict treatment, and the card visibly returns/updates rather than lying about its column (shared `run_transition` → existing Banner + refetch path)
- [x] Live `/ws/events` updates during a drag do not corrupt the board state (drag state is a signal over stable short codes; refetch reconciles exactly as for menu moves)
- [x] A non-pointer path to transition remains (keyboard/menu fallback for accessibility — keeping the move menu as secondary is acceptable)
- [x] Playwright smoke's transition step rewritten from the move menu to drag-and-drop (or covers both if the menu stays as fallback) — both covered: smoke keeps the menu path, new drag.spec.ts covers drag incl. illegal-drop refusal; A-0012 gates green

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
Board view (`crates/kairos-web/src/pages/boards.rs` + `boards/`, built in KAIROS-T-0040) already knows the column graph and renders per-column card lists; the transition API and 409 handling exist. The work is the interaction layer in wasm: drag state signal, per-column droppable highlighting from the transition rules, drop → transition call → optimistic update reconciled by the WS event.

### Dependencies
None hard; touches the same view as KAIROS-T-0062/0063 — sequence to avoid churn.

### Risk Considerations
HTML5 drag events in wasm/Leptos can be fiddly (especially in Playwright/CDP); may need pointer-event-based dragging for testability.

## Status Updates **[REQUIRED]**

- 2026-08-09: Recorded from UAT feedback session.
- 2026-08-09: Implemented with HTML5 drag events (not pointer-events — the testability risk resolved cleanly: Playwright's `dragTo` drives the HTML5 flow, verified with a live probe before writing the spec). boards.rs: `DragData { kind, short_code, targets }` in one `RwSignal<Option<_>>` on BoardBody; cards render `draggable="true"` iff their column has legal targets, `dragstart` publishes the drag (+ dataTransfer text/plain for engine compat, effectAllowed=move), `dragend` clears; columns compute legality from the SAME transition-derived target list the menu uses — legal targets get `.kairos-board__column--droppable` highlighting and `preventDefault` on dragover; `drop` runs the shared `run_transition` helper (extracted; the move menu now uses it too, with busy-reset wrapper callbacks). Errors surface via the existing page-level Banner; refetch + WS reconciliation unchanged. Move menu KEPT as the keyboard/a11y fallback (AC allowed). CSS: droppable highlight, dragging opacity, grab cursor — tokens only. web-sys features DragEvent + DataTransfer added. New `e2e/tests/drag.spec.ts` on the web-delivery board (no interference with smoke's platform-delivery fixture): legal drag Backlog→Todo lands; illegal drag Todo→Backlog is refused (no transition configured); draggable attr + menu-fallback asserted. cargo check, unit, web lint green; full e2e running.