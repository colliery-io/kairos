---
id: board-columns-gets-its-own-deleted
level: task
title: "board_columns gets its own deleted_at so an archived card stops pinning a column"
short_code: "KAIROS-T-0161"
created_at: 2026-09-23T11:30:00.321284+00:00
updated_at: 2026-09-23T11:30:00.321284+00:00
parent: KAIROS-I-0015
blocked_by: []
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

Let a board column be removed once the only cards in it are archived —
Dylan's call (2026-09-23). Doing that safely means giving `board_columns` a
`deleted_at` of its own: [[KAIROS-A-0020]] applied one level down.

## Implementation Notes

### Why the obvious fix does not work

`count_items_in_column` (`crates/kairos-db/src/boards.rs:868-885`, called
from `boards.rs:695`) counts archived rows, and `remove_column`'s doc
comment states that is deliberate: *"soft-deleted rows included, since they
still reference the column"*.

It is not only policy. `column_id` is `UUID NOT NULL REFERENCES
board_columns(id)` with **no `ON DELETE` clause** (up.sql:170, :190, :213,
:250 for strategies, initiatives, tasks and ADRs). The database itself
refuses. Removing the count filter alone converts a clean 422 into a
foreign-key violation.

### The design

Add `deleted_at` to `board_columns` and make removal a soft delete:

- the FK stays satisfied, so **an archived card still renders with the real
  column name it was put away in** — that is an audit fact worth keeping,
  and it is why re-parenting or denormalising the name were both rejected;
- `count_items_in_column` filters to live rows, so a column holding only
  archived cards is removable;
- live board rendering, the column rules
  (`kairos_core::board::check_remove_column`) and
  `load_board_rules`/`column_board_id` filter to live columns, so nothing on
  a working board changes;
- a restore into a removed column becomes one of [[KAIROS-T-0160]]'s
  refusals — detectable now, rather than an FK error.

### The one non-mechanical part

`board_transitions.from_column_id` / `to_column_id` are
`REFERENCES board_columns(id) ON DELETE CASCADE` (up.sql:92-93). Today
removing a column cascades its edges away. With a soft delete nothing
cascades, so the edges must be **filtered** to live columns wherever the
graph is read or validated — otherwise a removed column reappears as a legal
transition target. Check `boards.rs:310/404/418/516` (transition and move
validation) and the board-rules loader.

Contrast worth preserving in a comment: `count_live_board_items`
(`api/org/mod.rs:119-148`) already counts live rows only, and the rationale
at `org/mod.rs:108-118` explains the distinction that no longer applies once
columns are soft-deletable. Update that note rather than leaving it to
contradict the new behaviour.

## Acceptance Criteria

- [ ] Migration adds `deleted_at` to `board_columns`.
- [ ] A column whose only occupants are archived can be removed; one holding
      a live card still 422s with `COLUMN_NOT_EMPTY`.
- [ ] An archived card in a removed column still reports that column's name.
- [ ] A removed column is not a legal transition target and does not render
      on a live board.
- [ ] Removing a column no longer destroys its transition edges; they are
      filtered instead.
- [ ] The comment at `api/org/mod.rs:108-118` is updated.
- [ ] `angreal test` green; `angreal test uat --journey board-setup` green.

## Status Updates

*To be added during implementation*
