---
id: mcp-move-item-cli-tasks-move-to
level: task
title: "MCP move_item, CLI tasks move --to-board, plugin skill lines for moving tasks"
short_code: "KAIROS-T-0128"
created_at: 2026-09-23T01:50:44.017809+00:00
updated_at: 2026-09-23T01:50:44.017809+00:00
parent: KAIROS-I-0012
blocked_by: ["KAIROS-T-0127"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] `move_item` works over MCP with the documented text; a filer without powers on both boards gets the explained refusal.
- [ ] `kairos tasks move DEMO-T-0001 --to-board web-delivery --json` prints the moved task.
- [ ] Skills mention the move; fmt/clippy/unit/integration green.

## Status Updates

*To be added during implementation*
