---
id: j7-board-setup-a-review-column-and
level: task
title: "J7 board-setup: a Review column and its transitions, a metadata field stamped and searched, a delivery stream, delete_item"
short_code: "KAIROS-T-0135"
created_at: 2026-09-23T02:59:20.090100+00:00
updated_at: 2026-09-23T03:19:29.815841+00:00
parent: KAIROS-I-0013
blocked_by: [KAIROS-T-0131]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0013
---

# J7 board-setup: a Review column and its transitions, a metadata field stamped and searched, a delivery stream, delete_item

## Parent Initiative

[[KAIROS-I-0013]]

## Objective

I-0013 D5: a new journey `uat/journeys/board-setup.journey.ts` — "An admin shapes a new team's board". The `/admin/*` surfaces have no journey.

## Implementation Notes

### Technical Approach

Runs on its own fresh team via `fixtures/team.ts::teamFixture` (suffix `platform-eng` or similar — NOT `mobile`/`ios`, which J1 and J3 use, or the slugs collide in one run). Cast: alice (admin), bob (a member she adds).

1. **alice (CLI)** `kairos boards show <board-id>` — the scaffolded default columns.
2. **alice (GUI `/admin/boards/:board`)** adds a `Review` column positioned between Active and Completed, then the transitions Active → Review and Review → Completed. Read `crates/kairos-web/src/pages/admin/` for selectors.
3. **alice (GUI `/admin/metadata`)** defines a metadata field scoped to tasks (A-0003 entity scoping, KAIROS-T-0078); **bob (MCP)** `set_metadata` stamps it on a card he creates on the new board; **alice (CLI)** `kairos search --metadata key=value` finds it.
4. **alice (GUI `/admin/streams`)** creates a delivery stream and attaches the team; assert it shows on the team page (or via `kairos streams list`).
5. **bob (GUI)** drags his card Active → Review — the transition alice created minutes earlier, which is the whole point: configuration reaches the board. (Use `surfaces/gui.ts::dragCard`, the scroll-safe one.)
6. **bob (MCP)** `delete_item` retires the card; the ledger then unwinds stream → metadata definition → repository-less team (its board is clear, KAIROS-I-0012).
- Delete `mcp:set_metadata`, `mcp:delete_item`, `cli:boards`, `cli:streams` from ALLOW.

### Dependencies

T-0131.

## Acceptance Criteria

## Acceptance Criteria

- [x] Green (hand-run; `--server` parity in T-0136's full runs): 9 steps — columns "…, Completed, Review" with 9 transitions, the field stamped and found by `--metadata`, the stream attached, and the Active → Review drag.
- [x] `mcp:set_metadata`, `mcp:delete_item`, `cli:boards`, `cli:streams` deleted from ALLOW.
- [x] Card deleted in-journey (live_cards_left: 0), then the ledger unwinds stream → metadata definition → team.

## Status Updates

**2026-09-22** — Completed in `1160a3d`.

- Ran green on the first complete attempt; the one fix was a selector: `hasText` is a substring match and several captions on the transitions panel mention "To", so From/To are selected by exact label.
- Suffix `infra` (not `mobile`/`ios`, which J1 and J3 use) so the team slugs never collide inside one run.
- The final step deletes the card deliberately: it leaves the board clear, which is what lets the ledger delete the team at all (KAIROS-I-0012's rule) — the two initiatives' stories join up here.