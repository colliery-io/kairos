---
id: cross-team-gains-the-move-surfaces
level: task
title: "cross-team gains the move surfaces: bob re-homes via the GUI Board select, carol moves it back over MCP move_item"
short_code: "KAIROS-T-0132"
created_at: 2026-09-23T02:59:10.381705+00:00
updated_at: 2026-09-23T03:10:57.895341+00:00
parent: KAIROS-I-0013
blocked_by: [KAIROS-T-0131]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0013
---

# cross-team gains the move surfaces: bob re-homes via the GUI Board select, carol moves it back over MCP move_item

## Parent Initiative

[[KAIROS-I-0013]]

## Objective

I-0013 D2: the board move we shipped in KAIROS-I-0012 gets a persona. Three steps appended to `cross-team`, which already has the right cast on both sides of the seam.

## Implementation Notes

### Technical Approach

The journey currently ends with alice's `kairos search --repo`. Insert before that (after bob has triaged the filed card to Todo), keeping the existing repository-rule story intact — note the filed card IS bound to `payments-api`, so it cannot move; use carol's own `portal-web` task or an unbound card for the move steps, and let the refusal be the first beat:

1. **bob (GUI)** opens the filed card and tries the Board select → refused inline with `REPOSITORY_OWNER_MISMATCH` naming `payments-api` (the guard as a person meets it). Assert the message inside `[data-testid="move-board"]`.
2. **bob (GUI)** unbinds the repository on the item page (the existing `[data-testid="repository-control"]`, select `(none)` → Set repository), then moves it to the web board with the Board select → the notice, and the card gone from platform-delivery.
3. **carol (MCP)** `board_items` on her board finds it; she agrees it was platform's after all and `move_item`s it back — then re-binds is NOT needed (leave it unbound; the ledger deletes it).
4. **carol (MCP)** attempts `move_item` to a board she cannot manage (e.g. the initiatives board → NOT_DELIVERY_BOARD, or platform's board if she lacks `manage_tasks` there) and gets the refusal; assert the text. Check what carol actually holds on platform-delivery in the seed before choosing which refusal to assert — `file_backlog` only means the explained Backlog refusal (KAIROS-T-0123).
- Delete `mcp:move_item` from the check's ALLOW map.
- Keep the journey's step count sane: fold assertions rather than adding a step per assertion.

### Dependencies

T-0131 (the ALLOW map to edit).

## Acceptance Criteria

## Acceptance Criteria

- [x] Green (hand-run against the kept stack); the report shows the REPOSITORY_OWNER_MISMATCH text, the GUI move to web-delivery, carol's FORBIDDEN, and "Moved …: web-delivery -> platform-delivery / Backlog."
- [x] `mcp:move_item` deleted from ALLOW (full-run verification in T-0136).
- [x] Teardown unchanged (both tasks ledgered); no leftovers.

## Status Updates

**2026-09-22** — Completed in `c184a89`.

- **Story change forced by the product:** the task said bob would do the GUI move. He cannot — the Board select is hidden for him, correctly, because the two-sided rule leaves him no eligible target (he manages platform only). That became its own step (asserting the control is absent), and alice drives the move. Better story, and it pins a behaviour a reader might otherwise mistake for a bug.
- The four steps run LAST in the journey: unbinding the repository would break the `kairos search --repo` step that precedes them.
- Gotcha: the item page's repository picker uses the sentinel `(none)` (`NO_REPOSITORY` in `pages/repositories/api.rs`), not an empty value, to unbind.