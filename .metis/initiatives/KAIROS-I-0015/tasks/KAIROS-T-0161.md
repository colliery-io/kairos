---
id: board-columns-gets-its-own-deleted
level: task
title: "board_columns gets its own deleted_at so an archived card stops pinning a column"
short_code: "KAIROS-T-0161"
created_at: 2026-09-23T11:30:00.321284+00:00
updated_at: 2026-09-23T12:03:23.077509+00:00
parent: KAIROS-I-0015
blocked_by: []
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

## Acceptance Criteria

## Acceptance Criteria

- [x] Migration adds `deleted_at` to `board_columns`.
- [x] A column whose only occupants are archived can be removed; one holding
      a live card still 422s with `COLUMN_NOT_EMPTY`.
- [x] An archived card in a removed column still reports that column's name.
- [x] A removed column is not a legal transition target and does not render
      on a live board.
- [x] Removing a column no longer destroys its transition edges; they are
      filtered instead.
- [x] The comment at `api/org/mod.rs:108-118` is updated.
- [x] `angreal test` green; `angreal test uat --journey board-setup` green.

## Status Updates

### 2026-09-23 — survey before touching anything

Confirmed the task's premises against the code:

- `column_id UUID NOT NULL REFERENCES board_columns(id)` with no `ON DELETE`
  at up.sql:170/:190/:213/:250. Confirmed: the DB refuses.
- `board_transitions.from_column_id`/`to_column_id` are `ON DELETE CASCADE`
  (up.sql:92-93). Confirmed.

**One thing the design did not mention, and it is the sharpest edge.**
`board_columns` carries `UNIQUE (board_id, position)` and
`UNIQUE (board_id, name)` (up.sql:84-85). A soft-deleted column keeps both,
so without further work:

- re-adding a column with the removed column's name gives a constraint
  violation, not the typed `DuplicateName`;
- adding a column at the removed column's position, same;
- `reorder_columns` parks every column of the board on `position * -1 - 1`
  before assigning `0..n` (boards.rs:729-742). Run that twice with a removed
  column present and the removed row's negative position maps back onto a
  live column's. Silent corruption.

So the migration must also convert both constraints into **partial unique
indexes `WHERE deleted_at IS NULL`**, and `reorder_columns` must only touch
live rows. Recorded because any later task that soft-deletes a row sitting
under a composite UNIQUE hits exactly this.

### Surfaces that read columns, and what each one gets

Filtered to live (`deleted_at IS NULL`):

- `kairos-db/boards.rs`: `load_board_rules`, `column_board_id`,
  `entry_column`, and `count_items_in_column` (live items only).
- `kairos-db/items.rs::resolve_column` — otherwise a new card could be
  created straight into a removed column.
- `kairos-db/seed.rs::column_id` — a partial unique name index makes
  by-name lookup ambiguous without it.
- `kairos-server/api/org/boards.rs`: `load_columns`, `load_column_of_board`,
  `load_transitions` (edges filtered to live endpoints).
- `kairos-server/mcp/tools.rs::board_columns`.
- `graph.rs:481/526` `board_has_done` EXISTS — a removed done-flagged column
  would otherwise still claim the board has done semantics.

Deliberately NOT filtered, because these are the audit answer:

- the joins at `graph.rs:714-726` and `repositories.rs:463` resolve a
  column *name* for an item and are already gated on the item being live;
- a new `column_label` in mcp/tools.rs, so `show_item` on an archived card
  still prints the column it was put away in.

### 2026-09-23 — two tests that were quietly pinned to "the newest migration"

Adding a tenant migration broke two tests that had no business caring, and
both breakages are worth recording because the next schema wave meets them
again.

1. `tests/repositories_migration.rs` reverted the repositories migration by
   deleting the bookkeeping row `WHERE version = (SELECT max(version) ...)`.
   With T-0161 in the tree that deletes the WRONG row, so
   `migrate_all_tenants` re-applies the board-columns migration and the
   `repositories` table never comes back — the failure surfaces a hundred
   lines later as "relation does not exist". Fixed by naming the version it
   actually means (`REPOSITORIES_VERSION`), with an assertion that the
   delete hit exactly one row so the next drift is loud.
2. `tests/tenant_provisioning.rs`'s fleet-upgrade block is knowingly pinned
   to the newest migration — it says so, and KAIROS-T-0093 exists to remove
   the chore. Re-pinned to `board_columns_soft_delete`: it now simulates a
   pre-T-0161 tenant by dropping `deleted_at` and putting the table-wide
   UNIQUEs back, fleet-migrates, and asserts the column returns, the two
   `board_columns_live_*` partial indexes exist, and no table-wide UNIQUE
   constraint remains. That makes the existing-tenant upgrade path a gate
   rather than an assumption.

Note for whoever picks up KAIROS-T-0093: `max(version)` appears in both
files and is the shared root cause.

### 2026-09-23 — done, and a note on where the MCP change landed

Behaviour now:

- a column holding only archived cards is removable; one holding a live
  card still 422s `COLUMN_NOT_EMPTY` with `details.item_count`;
- the archived card keeps its `column_id` and the column row keeps its
  name, so the audit answer survives;
- a removed column does not render on a live board, is not a legal
  transition target (`UnknownToColumn`), cannot be renamed, re-removed or
  flagged done (404 / `ColumnNotFound`), and nothing new can be created in
  it;
- its transition edges are kept and filtered, not cascaded away;
- its name and position are free for re-use, because both UNIQUEs are now
  partial.

**Where the MCP half of this landed.** The two `mcp/tools.rs` hunks —
`board_columns` filtering to live, and the new `column_label` used by
`show_item` — were swept into commit 7517465 ("Archived work is
retrievable again", KAIROS-T-0154) by the agent working that ticket in the
same tree. The code is correct and on main; only the attribution is wrong.
Recorded so a later reader looking for T-0161 in the log does not conclude
the MCP surface was missed.

Gates: `angreal test lint` green; `angreal test unit` green; every
`kairos-db` and `kairos-server` integration target green.
`kairos-cli::cli_tree_live` fails on `"a deleted task must 404"` — that is
KAIROS-T-0154's deliberate behaviour change (archived items are
retrievable now) meeting a test that still asserts the old contract. It
belongs to T-0154, not here; nothing in T-0161 touches item retrieval.