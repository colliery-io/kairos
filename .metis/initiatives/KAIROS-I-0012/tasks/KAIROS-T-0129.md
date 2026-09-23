---
id: gui-board-select-on-the-item-page
level: task
title: "GUI: Board select on the item page's Board panel, ItemMoved live refetch on board views; e2e move step"
short_code: "KAIROS-T-0129"
created_at: 2026-09-23T01:50:47.503279+00:00
updated_at: 2026-09-23T02:23:35.071379+00:00
parent: KAIROS-I-0012
blocked_by: [KAIROS-T-0127]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0012
---

# GUI: Board select on the item page's Board panel, ItemMoved live refetch on board views; e2e move step

## Parent Initiative

[[KAIROS-I-0012]]

## Objective

I-0012 D2 (GUI) + D4 (e2e): a person moves a task to another delivery board from the item page and both boards update live.

## Implementation Notes

### Technical Approach

- `crates/kairos-web/src/pages/item.rs` `BoardPanel` (the "Move to" column control ~line 800): add, for tasks, a `Board` `<Select>` of the OTHER live delivery boards the caller can manage (derive from whoami capabilities + `board_powers`; org admin sees all delivery boards; list from `/api/boards?limit=100` filtered `board_level == delivery`), default "(this board)", and a "Move board" button (`data-testid="move-board"`) calling the new client mirror `move_task(code, board_slug)` (`pages/item/api.rs`). On success: the panel re-renders with the new board link and entry column, a feedback line "Moved to <board>". Errors (422 repository rule) shown inline.
- `pages/boards/live.rs`: handle `ItemMoved` like `ItemTransitioned` — refetch when `from_board_id` or `to_board_id` is this board. Events mirror in `pages/boards/data.rs` or wherever the WS event enum is decoded.
- e2e: extend `team-lens.spec` (bob, platform member) or `repositories.spec` (alice) with: open a task, Board select → `web-delivery`, Move board; the card appears on the web board (second page/context, no reload) and disappears from platform; a repo-bound task offers only the owner board (assert the select's options or the 422 message inline).
- Gates: `angreal test lint`, `cargo test -p kairos-web --lib`, `angreal web lint/build`, `angreal test e2e`.

### Dependencies

T-0127.

## Acceptance Criteria

## Acceptance Criteria

- [x] Item page offers a Board select to callers with `manage_tasks` on ≥2 delivery boards; moving updates the panel without reload.
- [x] Both boards reflect an `ItemMoved` event live.
- [x] e2e 11/11 (+ the new step).

## Status Updates

- 2026-09-23: Implemented (commit `1927749`).

  **GUI.** `pages/item.rs` gains `MoveBoardControl`, rendered for tasks in
  the Board panel next to the column picker: a Board `<select>` wrapped in
  `data-testid="move-board"`, defaulting to `(this board)`, with a "Move
  board" button. It renders only when the shared `board_power`
  (`manage_tasks`) mirror grants the SOURCE board, and its options come
  from a new pure helper `pages/boards.rs::movable_delivery_boards(me,
  boards, here_slug)` — built on the existing `board_powers` derivation,
  not a second path — which returns the other live delivery boards the
  caller may manage (org admin: all of them; the board list is the
  existing `/api/boards?limit=100` call, filtered `board_level ==
  "delivery"`). So the picker never offers a move the two-sided server
  rule would 403. Success routes through the panel's existing `on_moved`:
  the page notice reads "Moved to <board>." and the refetched panel shows
  the new board link and entry column; failures render inline in an
  `Alert` (the T-0104 `REPOSITORY_OWNER_MISMATCH` message names the board
  to move to). `pages/item/api.rs::move_task` is the client mirror
  (`POST /api/tasks/{code}/move` with `{board}`).

  **Live.** No special case was needed for `item_moved`: the socket is
  server-filtered to one board and `pages/boards/live.rs` never inspects
  the event kind, so the source board's copy (no column) and the target's
  (entry column) each drive their own side's refetch, exactly like
  `item_transitioned`. Both modules now document that, and the `ThinEvent`
  mirror test decodes both halves.

  **e2e.** `repositories.spec` step 4b (alice, org admin): the picker
  omits the board the task is on and offers Web Delivery; the
  payments-api-bound cross-team task is refused inline with
  `REPOSITORY_OWNER_MISMATCH`; an unbound task moves through the GUI,
  lands in the web board's Backlog, shows on `/boards/web-delivery` and is
  gone from `/boards/platform-delivery`. Both live halves are asserted
  with the smoke spec's page-scoped no-reload marker — watching platform a
  moved-away card disappears, watching web a moved-in card appears — with
  a second writer through the new `helpers/api.ts::moveTask`.

  **Gates (verbatim result lines).**
  - `angreal test lint` → `Finished \`dev\` profile [unoptimized +
    debuginfo] target(s) in 0.15s` (EXIT=0)
  - `cargo test -p kairos-web --lib` → `test result: ok. 74 passed; 0
    failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`
  - `angreal web lint` → `kairos-web token rule: clean (no raw color
    literals)`
  - `angreal web build` → `GUI bundle written to
    /Users/dstorey/Desktop/kairos/crates/kairos-web/dist`
  - `angreal test e2e` → `11 passed (17.0s)` /
    `E2E PASSED (API golden path + MCP + GUI smoke).`

  **Decisions the task left open.** The picker's value is the board slug
  and its label the board name (so the notice reads "Moved to Web
  Delivery."); the control hides itself entirely when there is no eligible
  target rather than rendering a dead select; the Move board button stays
  disabled on `(this board)`, making a board move — which re-homes the
  card's team — a deliberate second act next to the column picker, which
  defaults to a real target.