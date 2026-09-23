---
id: list-endpoints-and-board-items
level: task
title: "List endpoints and board items gain an include_deleted opt-in"
short_code: "KAIROS-T-0159"
created_at: 2026-09-23T11:29:55.870120+00:00
updated_at: 2026-09-23T11:29:55.870120+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0156]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0015
---

## Parent Initiative

[[KAIROS-I-0015]]

## Objective

Give "visible when asked for explicitly" somewhere to hang on the list
surfaces. Today the five family lists and the board items endpoint have no
opt-in at all.

## Implementation Notes

**Blocked by [[KAIROS-T-0156]].**

- Family lists: `api/tasks.rs:148/153` and the same pair in
  `strategies.rs:82/87`, `initiatives.rs:83/88`, `documents.rs:151/156`,
  `adrs.rs:79/84` — count and page both filter.
- Board items: `api/org/boards.rs:461-600`, with the per-family filters at
  `:507`, `:520`, `:533`, `:571`. `BoardItemsQuery` (`boards.rs:440-446`)
  carries only `repository`.

Add `include_deleted` (match the existing spelling on `SearchFilter` — a
product-wide rename is explicitly out of scope, see the initiative's
non-goals). **Default false everywhere.**

The count and the page must agree. A list that reports 40 results and
returns 12 is a worse bug than the one being fixed.

MCP `board_items` (`mcp/tools.rs:621-624`, rows at `:1712-1810`) and
`list_boards` counts (`column_item_counts`, `tools.rs:1825-1848`) take the
same argument. Adding an argument to an existing tool does not change the
drift gate's tool count.

CLI: the entity-family list commands in `commands/entities.rs` gain the flag.
A verb-level flag, not a new noun — the gate's noun count is unaffected.

## Acceptance Criteria

- [ ] All five family lists and `/api/boards/{id}/items` accept
      `include_deleted`, default false.
- [ ] Counts and pages agree under both settings.
- [ ] Archived rows are marked in list payloads.
- [ ] MCP `board_items` and the CLI list commands expose it.
- [ ] A test asserts default behaviour is byte-identical to before.
- [ ] `angreal test` green.

## Status Updates

*To be added during implementation*

## Notes carried in from [[KAIROS-T-0156]]

**2026-09-23.** List endpoints **do not read the two views at all** — they
are diesel query-builder queries straight against the base tables — so this
task is independent of T-0156 rather than built on it.

Note the partial `idx_*_board` / `idx_*_column` indexes (up.sql:181/204/225)
are `WHERE deleted_at IS NULL` and so will **not serve a wide listing**
either. The same index question T-0157 faces for full-text applies here for
board and column listings; measure before assuming a wide list is cheap.
