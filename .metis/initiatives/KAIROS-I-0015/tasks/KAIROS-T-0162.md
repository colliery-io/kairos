---
id: definition-in-use-names-the-work
level: task
title: "DEFINITION_IN_USE names the work carrying the field instead of counting it"
short_code: "KAIROS-T-0162"
created_at: 2026-09-23T11:30:02.612710+00:00
updated_at: 2026-09-23T11:30:02.612710+00:00
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

Close [[KAIROS-T-0152]]: make a `DEFINITION_IN_USE` refusal actionable by
naming the work that carries the field, instead of returning a count the
user cannot act on.

## Implementation Notes

`crates/kairos-server/src/api/meta/definitions.rs:451-494` counts
`item_metadata` rows at `:463` and `template_metadata` at `:468`, and builds
the refusal at `:473-486`. Neither count joins the owning entity table, so
liveness is not considered at all.

Under [[KAIROS-A-0020]] counting an archived carrier is **defensible** — it
is still content — but only because archived work is now reachable. The
defect was never the count; it was that the carrier was invisible, so being
refused left the admin nowhere to go. With [[KAIROS-T-0154]] landed they can
find it.

So: keep counting, and **name them**. Follow `live_board_item_codes`
(`api/org/mod.rs:154-185`), which does exactly this for the team-delete
guard — return short codes, marking which carriers are archived.

The admin's path becomes: refused → told which work carries the field →
restore it ([[KAIROS-T-0160]]) → clear the value → re-archive → retire the
field. `set_metadata` on an archived item stays refused, per D5.

Cap the list (the team guard's precedent) so a field stamped on 4,000 items
does not return 4,000 short codes; say how many more there are.

Note `metadata_definitions`, `templates` and `item_metadata` are all
**hard-deleted** — no `deleted_at` — so none of this initiative's view work
applies to them. The join is to the *owning item's* table.

## Acceptance Criteria

- [ ] `DEFINITION_IN_USE` returns the short codes carrying the field,
      archived ones marked, capped with a count of any remainder.
- [ ] The GUI admin rejection (`.cl-alert[role="alert"]` on `/admin/metadata`)
      shows them.
- [ ] The documented path works end to end: refuse → find → restore → clear
      → re-archive → retire.
- [ ] `template_fields` blockers are named too, not just item values.
- [ ] [[KAIROS-T-0152]] can be closed, and `uat/journeys/board-setup.journey.ts`
      can drop its defensive `set_metadata … null` — verify teardown stays
      clean without it.
- [ ] `angreal test` green.

## Status Updates

*To be added during implementation*
