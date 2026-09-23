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

- [x] `DEFINITION_IN_USE` returns the short codes carrying the field,
      archived ones marked, capped with a count of any remainder.
- [x] The GUI admin rejection (`.cl-alert[role="alert"]` on `/admin/metadata`)
      shows them.
- [~] **Deferred to [[KAIROS-T-0160]].** The documented path works end to
      end: refuse → find → restore → clear → re-archive → retire. The
      middle of that path is `restore`, which does not exist yet — T-0160
      builds it. `set_metadata` on an archived item stays refused per D5,
      so there is no way to clear an archived carrier's value today and
      nothing to test end to end. This task closes the *first* half:
      refuse → find. T-0160 closes the rest.
- [x] `template_fields` blockers are named too, not just item values.
- [~] **Deferred to [[KAIROS-T-0160]]** (the journey half).
      [[KAIROS-T-0152]] can be closed — the refusal is no longer a dead
      end. `uat/journeys/board-setup.journey.ts` keeps its defensive
      `set_metadata … null` for now: it clears the stamp *before* deleting
      the card precisely because there is no way to clear it afterwards,
      and that is still true until restore lands. Dropping it is a T-0160
      verification, not this one's.
- [x] `angreal test` green (see the Status Update for which legs ran).

## Status Updates

### 2026-09-23 — the refusal names its blockers

`crates/kairos-server/src/api/meta/definitions.rs` now answers a
`DEFINITION_IN_USE` with names rather than a number. Two helpers do the
work, both modelled on `live_board_item_codes`:

- `carrying_items` — one query per entity table, because
  `item_metadata.item_id` carries no FK (item ids span the five tables in
  one shared UUID space). It selects `(short_code, deleted_at)` and
  returns a `Carrier { short_code, archived }`. Unlike the board guard it
  deliberately does **not** filter on `deleted_at`: that guard asks "is
  there still live work here?" (ADR-20 rule 5); this one asks "does
  anything still refer to this definition?" (rule 6). Different question,
  different filter — the comment says so, because the two sit next to
  each other and the difference is not obvious.
- `carrying_templates` — the `templates.slug`s behind the
  `template_fields` count. Templates are hard-deleted, so there is
  nothing to mark.

The message reads, e.g.:

> metadata definition "due" is in use (2 item value(s), 0 template
> field(s)): carried by [ACME-T-0001, ACME-T-0002 (archived)]; clear those
> references first — an archived carrier is still readable by short code,
> but must be restored before its value can be cleared

The archived clause only appears when a carrier actually is archived.
`details` keeps `item_values` / `template_fields` (the UAT journey reads
both) and gains `items` (`[{short_code, archived}]`) and `templates`.

**Cap:** `NAMED_BLOCKER_LIMIT = 20`, the board guard's number. `blocker_list`
renders `[a, b, and 3 more]` from the named labels plus the true count, so
the remainder is honest even for a carrier id that resolves to none of the
five tables.

**No kairos-web change needed.** `/admin/metadata`'s delete goes through
`run_mutation` → `MutationOutcome` → `ErrorState`, which renders the
server's message and `code:` verbatim. The named blockers surface in
`.cl-alert[role="alert"]` for free; `uat/journeys/new-kind-of-work.journey.ts`
asserts on `in use` / `DEFINITION_IN_USE` / `details.item_values` /
`details.template_fields`, all of which are preserved.

**Tests.** `crates/kairos-server/tests/meta.rs` now stamps `due` on the
second task, archives it, and asserts the refusal names both carriers with
the archived one marked and `details.items` flagging it — i.e. that
archiving does not drop the value and does not hide the carrier from the
refusal. The template leg asserts every slug in `details.templates`
appears in the message. `blocker_list`'s cap branch has a unit test in
`definitions.rs` (the only `#[cfg(test)]` under `api/`, but the function is
pure and the 20-carrier case is not worth a fixture).

**Gates.** `cargo test -p kairos-server` green across all 21 test binaries
plus the 51 lib unit tests. `cargo fmt --all --check` clean for this
task's files. `cargo clippy -p kairos-server --all-targets -D warnings`
clean. (The workspace legs intermittently failed to *compile* mid-run —
`kairos-db` was being edited concurrently for T-0160/T-0161 — which is
build noise from a shared tree, not a result.)

### Scope: two acceptance criteria deferred to KAIROS-T-0160

The end-to-end path and the `board-setup.journey.ts` cleanup both need
`restore`, which does not exist yet. `set_metadata` on an archived item
stays refused (D5), so an archived carrier's value cannot be cleared
today — restore → clear → re-archive is exactly T-0160's job. What this
task delivers is the half that was actually broken: the admin is refused
*and told where to go*. The criteria are marked `[~]` with the reason,
rather than dropped, so T-0160 inherits them.
