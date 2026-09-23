---
id: abac-resolves-an-authorization
level: task
title: "ABAC resolves an authorization board for archived items"
short_code: "KAIROS-T-0153"
created_at: 2026-09-23T11:29:41.623149+00:00
updated_at: 2026-09-23T11:29:41.623149+00:00
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

- [ ] `resolve_authorization_board` returns the owning board for an archived
      strategy, initiative, task, document and ADR.
- [ ] A test proves a non-admin team member resolves the *same* capability
      set for a card before and after it is archived.
- [ ] A test proves a member of another team resolves *no* capability for it
      — archiving must not widen access either.
- [ ] `angreal test` green; no surface behaviour changes yet.

## Status Updates

*To be added during implementation*
