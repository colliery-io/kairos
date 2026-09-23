---
id: gui-board-select-on-the-item-page
level: task
title: "GUI: Board select on the item page's Board panel, ItemMoved live refetch on board views; e2e move step"
short_code: "KAIROS-T-0129"
created_at: 2026-09-23T01:50:47.503279+00:00
updated_at: 2026-09-23T02:09:39.672404+00:00
parent: KAIROS-I-0012
blocked_by: [KAIROS-T-0127]
archived: false

tags:
  - "#task"
  - "#phase/active"


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

- [ ] Item page offers a Board select to callers with `manage_tasks` on ≥2 delivery boards; moving updates the panel without reload.
- [ ] Both boards reflect an `ItemMoved` event live.
- [ ] e2e 11/11 (+ the new step).

## Status Updates

*To be added during implementation*