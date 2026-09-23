---
id: a-deleted-item-s-metadata-value
level: task
title: "A deleted item's metadata value pins its definition forever"
short_code: "KAIROS-T-0152"
created_at: 2026-09-23T10:43:56.420174+00:00
updated_at: 2026-09-23T10:43:56.420174+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---


# A deleted item's metadata value pins its definition forever

## Objective

Make a metadata definition retirable once the only work carrying it is gone.
Today a soft-deleted item keeps its `item_metadata` rows, those rows are
counted by the `DEFINITION_IN_USE` guard, and nothing can reach them — so
deleting a card permanently blocks the definition it carried.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [ ] P0 - Critical (blocks users/revenue)
- [x] P1 - High (important for user experience)

### Impact Assessment

- **Affected Users**: any org that stamps a custom field on work and later
  deletes that work — i.e. every org that uses metadata for longer than a
  quarter. One deleted card is enough to make a field immortal.
- **Reproduction Steps** (measured on `main`, 2026-09-23, with a throwaway
  UAT probe against the compose stack):
  1. `POST /api/metadata-definitions` — a `string` field.
  2. Create a task, `set_metadata` the field on it.
  3. `DELETE /api/tasks/{code}` — the ordinary soft delete.
  4. Try to release the value: `set_metadata {field: null}` answers
     `NOT_FOUND: no live item with short code "…"`. No REST route clears it
     either — `PUT /api/tasks/{code}/metadata` is `405`.
  5. `DELETE /api/metadata-definitions/{id}` → `409 DEFINITION_IN_USE`,
     `details.item_values: 1`, `template_fields: 0`.
  6. `?include_deleted=true` → still `409`. `?force=true` → still `409`.
- **Expected vs Actual**: retiring a field should be blocked by *live* work
  that carries it, the way every other guard in the product counts live
  rows (`count_live_board_items` for teams, the repository guard for tasks).
  Instead the guard counts rows belonging to an item that no surface shows,
  no call can edit and no user can find. The only thing that ever frees the
  definition is the retention sweeper, on its own schedule.

## Acceptance Criteria

- [ ] The `DEFINITION_IN_USE` count ignores values on soft-deleted items, so
      a field whose only carriers are archived can be retired.
- [ ] Deleting an item releases — or stops counting — its metadata values by
      the same rule the board, team and repository guards already use.
- [ ] The refusal names live carriers rather than a bare count, so an admin
      who is refused can find the work that is actually blocking them.
- [ ] `uat/journeys/board-setup.journey.ts` drops its defensive
      `set_metadata … null` and still tears down clean.

## Implementation Notes

### Technical Approach

`count_live_board_items` in `crates/kairos-server/src/api/org/mod.rs` is the
precedent: it added `deleted_at.is_null()` to a count that had been
over-counting for exactly this reason. The definition guard needs the same
join against the item's `deleted_at`. Whether the rows are then deleted
eagerly or simply stop counting is the real design call — leaving them is
consistent with copy-forward history (KAIROS-A-0004) and costs nothing once
they are not counted.

### Dependencies

Sits next to [[KAIROS-T-0151]]: both are the same underlying shape, a
soft-deleted item's rows surviving in the database with no surface serving
them. T-0151 is about not being able to *read* them; this is about them
still being *binding*. Worth deciding together — what does archiving mean,
and which guards are allowed to count archived rows?

## Status Updates

**2026-09-23 — filed.** Found while closing out KAIROS-I-0014. The full UAT
run passed while leaving a `uat-…-risk` metadata definition behind in the
tenant on every run, because `board-setup` stamps a field on a card and then
retires the card as part of its story. The journey now clears the stamp
before the delete — the only order that works — with a comment pointing
here. The teardown ledger also stopped reporting a 404 as residue
(`uat/run/ledger.ts`); that noise had hidden this real leak in plain sight
across two full runs.
