---
id: gui-fine-grained-board-rendering
level: task
title: "GUI: fine-grained board rendering — open menus survive WS refetches"
short_code: "KAIROS-T-0074"
created_at: 2026-08-11T03:44:23.930582+00:00
updated_at: 2026-08-11T03:49:56.211426+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# GUI: fine-grained board rendering — open menus survive WS refetches

## Objective **[REQUIRED]**

The board view must stop rebuilding its entire DOM on every refetch. Every WS event (any writer touching the board) destroyed and recreated the whole page, killing transient UI state: an open Move menu snapped shut, and typing in an open create-document modal was lost. This is the architectural root cause recorded on KAIROS-T-0073 — that ticket deflaked the test; this one fixes the behavior.

Follow-up authorized by Dylan ("do 0073" → the recorded follow-up).

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (important for user experience)

### Impact Assessment **[CONDITIONAL: Bug]**
- **Affected Users**: Anyone using a board while any other writer (person, agent, CLI) is active — i.e. the product's core multiplayer scenario
- **Reproduction Steps**: 
  1. Open a board, open any card's Move menu
  2. Have a second writer transition any card on the same board (API/CLI)
  3. The WS event refetches → the whole `BoardBody` re-renders → the menu closes under the cursor (same for in-progress typing in the New-document modal)
- **Expected vs Actual**: Expected — the other card moves, everything else (open menu, modal state) stays put. Actual — full DOM rebuild on every refetch.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] Refetches update the mounted board in place: `BoardPage` holds the view in a signal; `BoardBody` is created once per board id and reads through memos; columns and cards render via keyed `<For>`s (column key = id+name+targets fingerprint; card key = content fingerprint) so unchanged nodes keep their DOM
- [x] An open Move menu on an untouched card survives a WS-driven update of another card (a changed/moved card is legitimately rebuilt — its menu closing is correct) — required the powers/movable memoization recorded in Status Updates
- [x] Modal open-state and in-progress input survive refetches; the New-document parent options follow the live item list (reactive, not a creation-time snapshot)
- [x] Column counts and empty-states update reactively without rebuilding the column node
- [x] E2E proof: the smoke live-WS step holds a menu open across the second writer's transition and asserts it is still open after the update lands — passing (and it caught the first implementation's gap)
- [x] All prior board behavior intact (drag T-0064, powers gating T-0072, global create T-0062): all three specs green on the first attempt (4.7s); web lint + unit green

**Completed 2026-08-10.**

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
`boards.rs`: `CardModel`/`ColumnModel` hoisted to module level with `Clone + PartialEq` and fingerprint keys (`column_models()` / `doc_parent_options()` pure builders). `BoardPage`: resource → `model: RwSignal<Option<BoardView>>` via an Effect; `board_key` memo keys one `BoardBody` per board id; powers derive from model + whoami. `BoardBody`: identity fields snapshotted once; `header_text`/`entry_column`/`doc_parents`/`documents_offered`/`columns` memos (PartialEq dedupe — identical refetch results cause zero DOM work); outer `<For>` over columns keyed on config fingerprint, inner `<For>` over cards keyed on content fingerprint, per-column cards memo so the column node persists while its cards diff. Load errors render above the stale board instead of replacing it. `CreateDocumentModal` takes `parents: Memo<…>` and renders its Select reactively.

### Risk Considerations
Card keys fingerprint content, so a changed card is rebuilt (menu closes) — intended. Column keys fingerprint transitions config, so admin config edits rebuild columns — rare and correct.

## Status Updates **[REQUIRED]**

- 2026-08-10: Filed as the KAIROS-T-0073 follow-up and implemented in the same session.
- 2026-08-10: FIRST E2E CAUGHT A REAL GAP — the new menu-survival assertion failed deterministically (both attempts): the keyed-`<For>` structure was sound, but `powers` was a `Signal::derive`, which notifies consumers on EVERY model refetch even when the derived value is identical; each card's actions block (`move || movable().then(|| …Menu…)`) re-ran on that notification and rebuilt the Menu closed. Fix: `powers` is now a `Memo` (PartialEq dedupe — BoardPowers is PartialEq) exposed as `Signal` via `.into()`, and each card's `movable` decision is itself a `Memo<bool>`, so the Menu block re-renders only when the decision actually flips. The e2e assertion did exactly its job — it proved the survival claim false before it shipped. Re-running the gate.