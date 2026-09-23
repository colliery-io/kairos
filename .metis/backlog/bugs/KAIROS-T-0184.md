---
id: scim-refuses-every-put-an-okta
level: task
title: "SCIM refuses every PUT an Okta-shaped IdP sends, and burns a team slug forever"
short_code: "KAIROS-T-0184"
created_at: 2026-09-23T23:49:43.113098+00:00
updated_at: 2026-09-23T23:49:43.113098+00:00
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

# SCIM refuses every PUT an Okta-shaped IdP sends, and burns a team slug forever

## Objective

Two independent SCIM defects, filed together because both were found the same
way and both break a real IdP integration rather than an edge case.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (the first breaks a common IdP outright)

### 1. `userName` is immutable, so an Okta-shaped deployment fails every PUT

`crates/kairos-server/src/scim/discovery.rs:51-56,:89-94` advertise `userName`
and group `displayName` as `"mutability": "immutable"`, and the PUT handler
refuses a change to either with `400 mutability`.

That is self-consistent. The problem is the common configuration it collides
with: where `userName` is the login email and the OIDC `sub` is an opaque id,
the IdP has a legitimate reason to send `userName` on every PUT, and gets 400
**every time, forever**. The sync never converges and the admin has no
indication which field is at fault beyond the word "mutability".

- **Decide**: is `userName` immutability the intended contract? If it is, the
  refusal needs to name the field and say what to change. If it is not, PUT
  should accept a `userName` change and re-derive the join.
- Note the join key is `users.external_id`, which is what makes a `userName`
  change fraught — see item 3 below for the related trap.

### 2. A soft-deleted team permanently burns its slug

`scim/groups.rs:621-628` creates a team for a `kairos-team-<slug>` group;
`DELETE` soft-deletes it. `teams.slug` is `UNIQUE` **with no `deleted_at`
predicate**, so the slug is taken forever and re-creating the same group is
refused `409 uniqueness` permanently.

An IdP that deletes and re-adds a group — a routine reorganisation — can never
recover that team name.

**This is the same landmine [[KAIROS-T-0161]] hit**: a soft delete under a
plain `UNIQUE` constraint. The fix there was a **partial unique index**
`WHERE deleted_at IS NULL`, and the same applies here. Worth auditing every
`UNIQUE` on a soft-deletable table in one pass rather than one ticket at a time.

### 3. Two silent traps worth deciding on, now documented

Both are in `reference/scim.md` as behaviour; neither is obviously intended:

- **A no-path PATCH matches keys case-sensitively.** `{"Active": false}`
  returns **200** and leaves the user provisioned — a deprovision that is
  swallowed. (`scim/users.rs`, the no-path value-object arm.)
- **A `userName` filter searches `users.external_id`.** An IdP filtering by
  login email against opaque `sub` values finds nothing, including during its
  own reconciliation sweeps — so it may conclude a user is absent and re-create
  them.

## Acceptance Criteria

- [ ] A decision on `userName` mutability, and either acceptance of the change
      or a refusal message naming the field and the remedy.
- [ ] `teams.slug`'s UNIQUE becomes a partial index on `deleted_at IS NULL`,
      and every other UNIQUE on a soft-deletable table is audited with it.
- [ ] The case-sensitive PATCH either matches case-insensitively or refuses
      unknown keys rather than returning 200.
- [ ] A `userName` filter searches what an IdP means by `userName`, or the
      discovery document stops advertising it as filterable.

## Status Updates

**2026-09-23 — filed.** All four found by the independent Diátaxis review in
[[KAIROS-T-0175]] while checking `reference/scim.md` and the SCIM how-to
against `crates/kairos-server/src/scim/` — R4 completeness forced an
enumeration of every refusal, and these are what the enumeration turned up.

The documentation now describes all four accurately, so an operator is warned.
The product behaviour is this ticket.
