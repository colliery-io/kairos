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

- [ ] `set_metadata` reaches an archived item, so a stamped value can always
      be cleared by someone who is refused ([[KAIROS-A-0020]] rule 1).
- [ ] The `DEFINITION_IN_USE` refusal **names** the carrying items, archived
      ones marked as such, rather than returning a bare count — the way
      `live_board_item_codes` does for the team guard.
- [ ] A field whose carriers have all been cleared can actually be retired,
      whether or not those carriers were archived.
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
still being *binding*.

**Decided (2026-09-23, [[KAIROS-A-0020]]): archived means hidden by default,
nothing more.** That changes the shape of this fix. The trap here is not
that the guard counts an archived carrier — it is that the carrier is
*unreachable*, so being refused leaves the admin with nowhere to go. Once
T-0151 lands and archived work is retrievable and editable, counting it is
defensible: the admin is refused, told which archived work carries the
field, and can go and clear it.

So the minimum fix is no longer "stop counting archived rows". It is:

1. `set_metadata` (and the equivalent write path) must reach an archived
   item, since archiving is not a write boundary either — it only limits
   default visibility.
2. The `DEFINITION_IN_USE` refusal must **name** the carrying items rather
   than return a bare count, the way `live_board_item_codes` does for the
   team guard. A count the user cannot act on is the actual defect.

Whether archived carriers should additionally stop counting is then a
genuine product choice rather than a forced one, and cheap either way.
Implement alongside T-0151, not after it.

## Status Updates

**2026-09-23 — filed.** Found while closing out KAIROS-I-0014. The full UAT
run passed while leaving a `uat-…-risk` metadata definition behind in the
tenant on every run, because `board-setup` stamps a field on a card and then
retires the card as part of its story. The journey now clears the stamp
before the delete — the only order that works — with a comment pointing
here. The teardown ledger also stopped reporting a 404 as residue
(`uat/run/ledger.ts`); that noise had hidden this real leak in plain sight
across two full runs.
