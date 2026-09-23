---
id: un-archive-restore-a-put-away-item
level: task
title: "Un-archive: restore a put-away item on API, MCP and CLI"
short_code: "KAIROS-T-0160"
created_at: 2026-09-23T11:29:58.054515+00:00
updated_at: 2026-09-23T11:29:58.054515+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0154]
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

Add the verb that makes read-only archived work tolerable: put it back.
Dylan's call (2026-09-23) was "read-only, plus un-archive" — archived work
never changes while invisible, but anyone who could archive it can restore
it and then edit it normally.

## Implementation Notes

**Blocked by [[KAIROS-T-0154]].**

Restoring clears `deleted_at`. The soft-delete primitive is
`crates/kairos-db/src/items.rs:379-382 soft_delete_*`; the inverse belongs
beside it.

### Surfaces

- **API**: `POST /api/{family}/{short_code}/restore`, all five families.
- **Capability**: the same one that archived it. Restoring is the inverse of
  deleting, not a new privilege — do not invent a capability for it.
- **MCP**: a `restore_item` tool. **This trips the drift gate** (17 tools →
  18) until a journey exercises it. That is the gate working; [[KAIROS-T-0165]]
  covers it.
- **CLI**: a `restore` verb on the entity-family macro in
  `commands/entities.rs` — a verb, not a noun, so the gate's noun count is
  unaffected. Follow the `board_move(Move)` clause added in KAIROS-I-0012 as
  the pattern for adding a verb to that macro.
- **GUI**: [[KAIROS-T-0164]].

### Restore refuses when its home is gone

Do not silently re-home. Refuse and **name what is missing**, the way
`live_board_item_codes` (`api/org/mod.rs:154-185`) names the blockers for a
team delete. Cases:

- the board was deleted (`boards` has `deleted_at`, schema.rs:96);
- the column was removed — after [[KAIROS-T-0161]] it is soft-deleted, so
  this is detectable rather than an FK error;
- the owning team was deleted (schema.rs:371);
- for a task, its repository was retired (schema.rs:251).

The user then moves it: `POST /api/tasks/{code}/move` already exists from
KAIROS-I-0012.

Restoring must write an activity row — `restore` is exactly the kind of
event an audit trail exists for. `log_activity` is used throughout
`kairos-db`; follow `remove_column`'s call in `boards.rs` for shape.

Version history is untouched by restore: the item comes back at the version
it was archived at.

## Acceptance Criteria

- [ ] `POST /api/{family}/{code}/restore` works for all five families and
      returns the live item.
- [ ] It requires the same capability as deleting, and a test proves a user
      who could not delete it cannot restore it.
- [ ] Restore refuses, naming the missing board / column / team /
      repository, rather than re-homing or erroring at the FK.
- [ ] Restoring writes an activity row naming the actor.
- [ ] MCP `restore_item` and `kairos <family> restore <code>` both work.
- [ ] A restored item is live everywhere: boards, lists, default search.
- [ ] `angreal test` green. The UAT drift gate will fail at 17/18 until
      T-0165 — expected, and noted there.

## Status Updates

*To be added during implementation*
