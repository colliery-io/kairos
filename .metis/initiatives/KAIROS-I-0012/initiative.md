---
id: team-lifecycle-delete-a-team-once
level: initiative
title: "Team Lifecycle - Delete a Team Once Its Board Is Clear, Move Tasks Between Delivery Boards"
short_code: "KAIROS-I-0012"
created_at: 2026-09-23T01:49:01.122765+00:00
updated_at: 2026-09-23T01:49:58.650287+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/design"


exit_criteria_met: false
estimated_complexity: M
initiative_id: team-lifecycle-delete-a-team-once
---

# Team Lifecycle - Delete a Team Once Its Board Is Clear, Move Tasks Between Delivery Boards Initiative

## Context **[REQUIRED]**

UAT finding #7 (KAIROS-I-0011, 2026-09-22): a team is deletable only while
its delivery board has *never* held an item. `DELETE /api/teams/{id}` (and
`DELETE /api/boards/{id}`) refuse with `422 BOARD_NOT_EMPTY` while any
strategy/initiative/task/ADR row references the board — **soft-deleted rows
included** (`count_board_items`, `api/org/mod.rs`), on the T-0010 argument
that a deleted item still holds the FK and "would orphan on restore". The
refusal says "move or delete them", but neither escapes it: delete is a soft
delete (still counted), no API changes an item's `board_id` after creation,
and the retention sweeper prunes history/activity tables only. Verified
alongside: there is **no undelete/restore path** for items anywhere in the
code, so the orphan argument is theoretical; and team delete already
soft-deletes the team's board with it.

Dylan's rule (2026-09-22): **"all cards must be archived or moved to delete a
team."** Decisions taken via AskUserQuestion:

- **Archived = today's delete.** A soft-deleted card no longer blocks team or
  board deletion; no new state. History stays intact. If a restore feature
  ever lands it must refuse when the board is gone.
- **Move scope: tasks between delivery boards.** The case that arises (a team
  disbands, its live work goes to another team). Strategies/initiatives sit
  on org-level singleton boards and need no move.
- **Move ABAC: `manage_tasks` on BOTH boards, or org admin** — two-sided like
  edges (T-0111) and re-homing (T-0112); cross-team requests still go through
  `file_backlog`.

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- A team (and a board) can be deleted once every card on its board is
  deleted or moved away; the refusal, when it still applies, lists what is
  live and says exactly that.
- A task can be moved to another delivery board on every surface (API, MCP,
  CLI, GUI), landing in the target's entry column, respecting the T-0104
  repository rule (a task bound to a repository sits on that repository's
  owning team's board), with an activity row and a live WS update.
- The UAT agent journey (J3) runs on a fresh team again and cleans up fully.

**Non-Goals:**
- No Archived state, no restore/undelete, no hard delete.
- No moving strategies, initiatives, ADRs or documents between boards.
- No change to A-0004 history semantics; soft-deleted items keep their FK to
  the (now soft-deleted) board.

## Detailed Design **[REQUIRED]**

### D1. The guard counts live cards only

`count_board_items` → `count_live_board_items`: every table filtered by
`deleted_at IS NULL`. Both callers (team delete, board delete) use it. The
422 keeps its code but the message becomes *"team "X"'s delivery board still
holds N live card(s): [CODE, CODE, …]; move them to another board or delete
them, then retry"*, with the codes in `details.items` (cap 20 + count). The
doc comment records why soft-deleted rows no longer count (no restore path;
Dylan 2026-09-22) and the rule a future restore must honour.

Board delete's `remove_column` rule (T-0010) keeps counting soft-deleted rows
— a column's removal re-parents rows, which is a different invariant; note
the asymmetry in the comment.

### D2. Move a task to another delivery board

- **kairos-db** `boards::move_task(conn, task_id, to_board_id, actor) ->
  Result<TaskMove, BoardError>`: one transaction — load the live task; refuse
  `SameBoard`; target must be a live **delivery** board (`NotDeliveryBoard`);
  if the task has `repository_id`, the target must be that repository's
  owner's delivery board (`RepositoryOwnerMismatch { repository, owner_board
  }` — reuse `repositories::delivery_board_for_team`); set `board_id`,
  `column_id = boards::entry_column(target)`, `team_id = target.team_id`;
  bump `updated_at`; `log_activity(action = "board_move", details: {from_board,
  to_board, from_column, to_column})`; emit `EventKind::ItemMoved { from_board,
  to_board }` (new variant; kairos-web boards treat it like
  `ItemTransitioned` → refetch on both boards). Returns the new placement.
- **API** `POST /api/tasks/{short_code}/move` body `{ board: <slug|uuid> }`
  → 200 `Task` DTO (with the new placement). ABAC: `require_capability
  (manage_tasks)` on the source board **and** on the target board; org admin
  bypass as everywhere. Errors: 404 board, 422 `SAME_BOARD` /
  `NOT_DELIVERY_BOARD` / `REPOSITORY_OWNER_MISMATCH` (message names the
  owner board and says "unbind with PUT /repository first, or move it
  there"). OpenAPI via utoipa. Client: `KairosClient::move_task(code, board)`.
- **MCP** `move_item { short_code, to_board }` (tasks only; the tool text
  says so and points at `transition_item` for columns). Refusal text goes
  through `require_capability_explained` (T-0123) so a cross-team filer is
  told the rule.
- **CLI** `kairos tasks move <code> --to-board <slug|uuid>`.
- **GUI** item page Board panel: beside "Move to" (column) add a **Board**
  select of the other delivery boards the caller can manage (from whoami
  capabilities / `board_powers`), with a "Move board" button; on success
  navigate stays, the panel re-renders with the new board and entry column.
  Board view: an `ItemMoved` event refetches (a card leaves one board and
  appears on another without reload).

### D3. Plugin

`triage` and `implement` skills: one sentence each — a task on the wrong
board is *moved* (`move_item`), not recreated; a task bound to a repository
can only move to the owner's board.

### D4. Tests

- kairos-db unit/integration: `move_task` happy path (column = entry,
  team_id follows), `SameBoard`, `NotDeliveryBoard` (initiatives board),
  `RepositoryOwnerMismatch`, activity row, event emitted.
- kairos-server integration (`tests/task_move.rs`): two-sided ABAC (bob on
  platform only → 403 for web target; alice admin → 200; a member holding
  `manage_tasks` on both → 200), the T-0104 rule, team delete now succeeds
  after the last card is deleted or moved, refusal lists live codes, board
  delete parity. `tests/mcp.rs`: `move_item` + refusal text. CLI unit test
  for arg parsing.
- e2e `team-lens.spec` or a new step in `repositories.spec`: move a task from
  platform to a new team's board in the GUI, watch both boards.
- UAT: J3 back on a fresh team (`setupTeamRepoAgent`), teardown deletes the
  task then the team; J1 gains a step where alice moves a task onto the new
  team's board and later the team delete is refused naming that live card
  until it is deleted. `uat/README.md` and the I-0011 findings list updated
  (#7 closed).

### D5. Docs

README "Teams" (or admin section): deleting a team — what must be true, the
move command; A-0006 unchanged (two-sided rule cited). No ADR: this reverses
one implementation choice in T-0010, recorded in the initiative and in the
`count_live_board_items` doc comment.

## Alternatives Considered **[REQUIRED]**

- **A distinct Archived state on items** (`archived_at`, GUI filter, Archive
  action). Rejected by Dylan for now: the soft-delete already is the archive;
  history survives; nothing restores.
- **Hard-delete soft-deleted rows on team delete.** Rejected: destroys
  history contrary to A-0004; unnecessary once the guard counts live rows.
- **Archive the team instead of deleting it** (`teams.archived_at`). Not
  chosen; delete-when-clear is what Dylan asked for and the board is
  soft-deleted with the team anyway, which is archival in effect.
- **Move any workflow item between boards of the same level.** Rejected:
  only tasks live on per-team boards.

## Implementation Plan **[REQUIRED]**

1. **Live-only guard + move in kairos-db + API + client** (D1, D2 db/API,
   D4 db/server tests).
2. **MCP `move_item`, CLI `tasks move`, plugin skill lines** (D2, D3; MCP
   and CLI tests).
3. **GUI board move + `ItemMoved` live refetch; e2e** (D2 GUI, D4 e2e).
4. **UAT: J3 on a fresh team, J1 move + refusal step; README; I-0011
   findings closed** (D4 UAT, D5).

Gates per task as usual (fmt, workspace clippy, unit, integration, e2e /
UAT where touched).

## Progress Log

- 2026-09-22: Created from UAT finding #7 after Dylan's rule ("all cards
  must be archived or moved to delete a team"); three decisions taken via
  AskUserQuestion; design D1–D5 written against the code (no restore path
  exists; team delete already soft-deletes the board; no board-move API on
  any surface).