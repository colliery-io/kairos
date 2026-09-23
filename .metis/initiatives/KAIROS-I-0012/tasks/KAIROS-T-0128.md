---
id: mcp-move-item-cli-tasks-move-to
level: task
title: "MCP move_item, CLI tasks move --to-board, plugin skill lines for moving tasks"
short_code: "KAIROS-T-0128"
created_at: 2026-09-23T01:50:44.017809+00:00
updated_at: 2026-09-23T02:22:52.870703+00:00
parent: KAIROS-I-0012
blocked_by: [KAIROS-T-0127]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0012
---

# MCP move_item, CLI tasks move --to-board, plugin skill lines for moving tasks

## Parent Initiative

[[KAIROS-I-0012]]

## Objective

I-0012 D2 (MCP/CLI) + D3: the move reaches agents and the terminal, and the skills say when to use it.

## Implementation Notes

### Technical Approach

- `crates/kairos-server/src/mcp/tools.rs`: `move_item { short_code, to_board }` (board slug|UUID) — tasks only (a non-task → validation error pointing at `transition_item` for columns). Auth via `require_capability_explained` on the current board then on the target (so a cross-team filer is told the Backlog rule). Calls `boards::move_task`; returns `Moved <code>: <from board> / <from column> -> <to board> / <entry column>.` Add to S-0006's tool inventory table if the spec lists tools (`.metis/specifications/KAIROS-S-0006*`). `tests/mcp.rs`: happy path, non-task refusal, two-sided ABAC refusal text.
- `crates/kairos-cli/src/commands/entities.rs`: `kairos tasks move <code> --to-board <slug|uuid>` (`--json` prints the Task). Unit test for parsing alongside the transition one.
- `plugin/skills/workflow/triage/SKILL.md` and `implement/SKILL.md`: one sentence each — a task sitting on the wrong team's board is *moved* with `move_item` (needs `manage_tasks` on both boards), never recreated; a task bound to a repository can only move to that repository's owning team's board (unbind first otherwise). Sync the `/kairos` router bullet if it enumerates tools.

### Dependencies

T-0127.

## Acceptance Criteria

- [x] `move_item` returns "Moved <code>: <from> -> <to> / <entry column>."; a non-task is refused and pointed at `transition_item`; bob (file_backlog only) gets the Backlog explanation. Probed in tests/mcp.rs.
- [x] The verb exists on tasks only (`kairos initiatives move` fails to parse), `--to-board` is required, and `--json` prints the Task DTO. CLI parse tests in main.rs.
- [x] `triage` gained a "Wrong team" outcome and `implement` a "moved, not recreated" paragraph, both with the repository-binding caveat; `angreal test lint`, `angreal test unit`, `angreal test integration` 40/40.

## Status Updates

**2026-09-22** — Completed in `06c5347`.

- The CLI verb rides a new optional `board_move(Move)` clause on `entity_family_cli!`, mirroring `transition(Transition)`; only the tasks invocation passes it. `emit_moved` takes `&Task` directly rather than widening `EntityView` with a `board_id()` accessor for four families that never move.
- `tests/mcp.rs` pins the tool surface, so `move_item` had to be added there AND to the S-0006 table; adding a second delivery board to that test also forced the existing bare `create_item` probes to name a board (the "multiple delivery boards" validation), which is the product behaving correctly.
- The `/kairos` router skill enumerates recipes, not tools, so it needed no change.