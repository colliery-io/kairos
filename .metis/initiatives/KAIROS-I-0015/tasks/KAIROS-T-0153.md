---
id: abac-resolves-an-authorization
level: task
title: "ABAC resolves an authorization board for archived items"
short_code: "KAIROS-T-0153"
created_at: 2026-09-23T11:29:41.623149+00:00
updated_at: 2026-09-23T11:38:11.044935+00:00
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

Make `resolve_authorization_board` work for an archived item, so that when
later tasks start serving archived work it is governed by the same
capabilities as live work. Alone, this task changes no surface behaviour —
nothing serves archived items yet. It exists first because getting the order
wrong ships a security-shaped bug.

## Implementation Notes

`crates/kairos-db/src/abac.rs` resolves through live-only lookups:

- `abac.rs:339` — `board_of_workflow_item`
- `abac.rs:355`, `:394` — the document lookup
- `abac.rs:425` — `item_created_by`

An archived item therefore yields `None`, and capability resolution falls
back to the tenant-wide policy — i.e. **org admins only**. Serving archived
items on top of that would make archived work *more* restricted than live
work, which inverts [[KAIROS-A-0020]] rule 4 ("archiving is not a permission
boundary"). It would also fail quietly and look like a permissions config
problem rather than a bug.

Drop the liveness filter from these four lookups specifically. They answer
"which board governs this row?", and the answer does not change when the row
is put away.

Do not touch the *mutating* paths' liveness checks (`items.rs:325/650/702/792`)
— those are what make archived work read-only by construction (D5).

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [ ] `resolve_authorization_board` returns the owning board for an archived
      strategy, initiative, task, document and ADR.
- [ ] A test proves a non-admin team member resolves the *same* capability
      set for a card before and after it is archived.
- [ ] A test proves a member of another team resolves *no* capability for it
      — archiving must not widen access either.
- [ ] `angreal test` green; no surface behaviour changes yet.

## Status Updates

*To be added during implementation*
**2026-09-23 — done.** Commit `a50d82c`.

Removed the `deleted_at` filter from four lookups in
`crates/kairos-db/src/abac.rs`: the `try_table!` macro in
`board_of_workflow_item`, its ADR arm, the document check in
`resolve_authorization_board`, and the macro in `item_created_by`. Doc
comments rewritten to say why, so the filters do not get restored by someone
tidying up.

No surface behaviour changes yet — nothing serves archived work until
[[KAIROS-T-0154]].

Test: `archived_items_resolve_the_same_capabilities_as_live_ones` in
`crates/kairos-db/tests/abac.rs`. It records what a plain team member may do
while a task is live, archives it, and asserts the capability set is
**identical** — then asserts the converse, since a lookup that stops asking
about liveness could just as easily widen access: an outsider still holds
nothing. `cargo test -p kairos-db --test abac` → 3 passed.

Gotcha for later tasks in this initiative: a hand-inserted board has **no
columns**, so `first_column` panics with `board has columns: NotFound`. Test
setup must insert a `NewBoardColumn` itself, unlike the provisioned default
boards which come seeded.