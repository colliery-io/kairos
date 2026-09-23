---
id: mcp-and-cli-read-archived-work
level: task
title: "MCP and CLI read archived work, marked as archived"
short_code: "KAIROS-T-0155"
created_at: 2026-09-23T11:29:46.726462+00:00
updated_at: 2026-09-23T11:58:10.521261+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0154]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0015
---

## Parent Initiative

[[KAIROS-I-0015]]

## Objective

Give agents and the CLI the same archived reads the API just gained. An
auditor who finds the answer on one surface will assume the others agree;
a partial rollout is worse than none.

## Implementation Notes

**Blocked by [[KAIROS-T-0154]].**

### MCP (`crates/kairos-server/src/mcp/tools.rs`)

- `get_item` — decl `tools.rs:677-679`, body `:679`, loader `load_item`
  `tools.rs:1377-1380` then per-table loads at `:1389/:1420/:1451/:1482/:1513`.
- `get_history` — `tools.rs:793-796`, which calls `load_item` first
  (`tools.rs:804`) and inherits the same refusal.

Both should serve archived items. The rendered text must carry an explicit
marker — a `## Archived` section, or a line in the header stating when it was
put away — consistent with how `get_item` already renders `## Development`.
An agent reading a ticket must be able to tell it is looking at retired work,
because it will otherwise try to act on it and be refused by the write paths.

Leave `board_items`, `list_boards` and the counts alone; those are default
listings and [[KAIROS-T-0159]] gives them their opt-in.

### CLI (`crates/kairos-cli/`)

`kairos {tasks,strategies,initiatives,documents,adrs} get CODE` currently
surfaces the server's 404. With T-0154 landed it will simply work; confirm
and add coverage. The rendered output needs the same archived marker.

`--include-deleted` exists today on `kairos search` only
(`commands/search.rs:62-64`) — leave its spelling alone here so the whole
product can be made consistent in one pass later.

### Drift gate

No new MCP tool and no new CLI noun, so the UAT gate's 17/17 and 16/16 are
unaffected by this task.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [ ] MCP `get_item` and `get_history` return archived work, visibly marked.
- [ ] `kairos <family> get <archived-code>` prints it with the same marker.
- [ ] MCP write tools still refuse archived items with a clear message.
- [ ] `board_items` / `list_boards` unchanged — a test asserts the archived
      card is absent.
- [ ] `angreal test` green.

## Status Updates

*To be added during implementation*
**2026-09-23 — done.** Commit `2582fa3`.

`load_item` (`mcp/tools.rs`) takes a `Liveness`; `get_item` and
`get_history` pass `IncludeArchived`, the other six callers
(`update_item`, `edit_item`, `move_item`, `transition_item`,
`set_metadata`, `delete_item`) keep `LiveOnly`.

**Simplification worth keeping:** the five per-table loads inside
`load_item` no longer carry their own `deleted_at.is_null()`. Resolution
enforces liveness and is authoritative — a `LiveOnly` caller never reaches
those loads for an archived row — so the second filter was only somewhere
for the two to disagree. Same reasoning applies anywhere else in this
initiative that a resolve-then-load pair appears.

### The banner

Both MCP renders lead with `> **ARCHIVED** <timestamp> — …` *before* any
other line. Rationale, since it will look verbose to someone tidying later:
an agent that cannot tell retired work from live work will try to act on it
and be refused by every write path with no idea why. The banner states it
is readable, not on a board, and must be restored before it can move.

CLI: new `EntityView::archived_at()` (five impls), `emit_get` prints
`ARCHIVED: <ts> — put away; readable, but not on a board`. `--json` needed
nothing — `archived_at` is already on the DTOs from T-0154.

### Tests

Two more places the old contract was written down, both rewritten rather
than deleted:

- `crates/kairos-server/tests/mcp.rs` — was *"the deleted item is now
  NOT_FOUND"*. Now asserts readability, the banner, intact history, that
  `transition_item` is still refused, and that an **unknown** code is still
  `NOT_FOUND` (which is what that status means now).
- `crates/kairos-cli/tests/cli_tree_live.rs` — was *"a deleted task must
  404"*. Now asserts exit 0, the banner, and absence from `tasks list`.

That makes **five** stale assertions found across three surfaces
(entities, cascade_preview, mcp, cli_tree_live). Anyone extending this
initiative should expect more: grep for `404`, `NOT_FOUND`, `gone` and
`soft-deleted` near a delete before assuming a suite is clean.

`cargo test -p kairos-server -p kairos-cli` → green.