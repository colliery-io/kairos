---
id: un-archive-restore-a-put-away-item
level: task
title: "Un-archive: restore a put-away item on API, MCP and CLI"
short_code: "KAIROS-T-0160"
created_at: 2026-09-23T11:29:58.054515+00:00
updated_at: 2026-09-23T12:20:41.569263+00:00
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

## Acceptance Criteria

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

## Notes carried in from other tasks

**2026-09-23, from [[KAIROS-T-0161]].** "Restore into a removed column" is
now a **detectable** refusal rather than a foreign-key error: the column row
survives with `deleted_at` set, and `kairos_db::items::resolve_column`
already refuses a removed column with `ColumnNotOnBoard`. So the
refuse-and-name guard has a clean signal to read.
**2026-09-23 — done.** Commit `f8b0847`.

`items::restore_item` in `kairos-db` returns
`Result<Result<RestoreOutcome, RestoreBlockers>, ItemError>` — the inner
`Err` is a *refusal to act*, not a failure, and keeping it out of
`ItemError` stops it being mapped to a generic 422 somewhere later.

### Surfaces

- **API**: one wildcard route, `POST /api/{entity_type}/{short_code}/restore`
  in a new `api/meta/restore.rs` — not five per-family routes. The verb is
  identical across families and the segment resolves exactly as
  `/history`'s does. Registered in `api::openapi::ApiDoc` (the
  `registered_routes_and_spec_paths_match_exactly` test catches omissions —
  T-0162's agent hit it before I did).
- **MCP**: `restore_item`. **Tool count 17 → 18**, so the UAT drift gate
  fails until [[KAIROS-T-0165]] covers it. Expected.
- **CLI**: `restore` verb on the entity-family macro (a verb, not a noun —
  the gate's noun count is unaffected), with `emit_restored`.
- New `ActivityAction::Restore` and `EventKind::ItemRestored`. The event is
  deliberately NOT `ItemCreated`: a client treating a restore as a create
  would render a new card carrying an old version number.

### Two decisions that look like omissions

- **No un-cascade.** Restoring a parent leaves its archived descendants
  archived and names them. A cascade delete was an act on a subtree;
  resurrecting it would undo decisions nobody asked to revisit, invisibly.
- **Refuse, never re-home.** If the board, column, owning team or repository
  is gone, 422 `RESTORE_BLOCKED` with `details.missing`. Re-homing would
  destroy the placement the record is evidence of. [[KAIROS-T-0161]] is what
  makes this clean — a removed column survives as a soft-deleted row, so it
  is a check rather than a foreign-key violation.

Restoring an already-live item is refused, since it almost always means the
caller has the wrong short code.

### Tests

- `entities.rs` — restore a leaf, assert it is live and back in the default
  listing; restore a cascade root and assert `still_archived_count == 1`
  naming the child, the child is still archived, and restoring the child
  separately works.
- `org_endpoints.rs` — the case the refusal exists for: a strategy archived,
  then its column removed. `RESTORE_BLOCKED`, `details.missing` names the
  column, the message names the item, **and the item is still readable** —
  which is what makes the refusal humane rather than a dead end.
- `mcp.rs` — `restore_item` added to the exact tool inventory.

Gates: `angreal test lint` green, `angreal test unit` green, 47/47
integration binaries green across kairos-db, kairos-server and kairos-cli.

### For [[KAIROS-T-0162]]

Its deferred criteria are now unblocked: the refuse → find → restore →
clear → re-archive → retire path is reachable, and
`details.items[].archived` is the handle for "which of these need restoring
first".