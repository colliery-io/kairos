---
id: scim-refuses-every-put-an-okta
level: task
title: "SCIM refuses every PUT an Okta-shaped IdP sends, and burns a team slug forever"
short_code: "KAIROS-T-0184"
created_at: 2026-09-23T23:49:43.113098+00:00
updated_at: 2026-09-25T01:15:56.794609+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A decision on `userName` mutability, and either acceptance of the change
      or a refusal message naming the field and the remedy.
- [x] `teams.slug`'s UNIQUE becomes a partial index on `deleted_at IS NULL`,
      and every other UNIQUE on a soft-deletable table is audited with it.
- [x] The case-sensitive PATCH either matches case-insensitively or refuses
      unknown keys rather than returning 200.
- [x] A `userName` filter searches what an IdP means by `userName`, or the
      discovery document stops advertising it as filterable.

## Status Updates

**2026-09-23 — filed.** All four found by the independent Diátaxis review in
[[KAIROS-T-0175]] while checking `reference/scim.md` and the SCIM how-to
against `crates/kairos-server/src/scim/` — R4 completeness forced an
enumeration of every refusal, and these are what the enumeration turned up.

The documentation now describes all four accurately, so an operator is warned.
The product behaviour is this ticket.
### 2026-09-25 — all four fixed, and the audit found two more

### Item 1: `userName` was never the problem — sharing a column was

The decision the ticket asked for: **`userName` immutability was not the
intended contract, and could not have been made to work as one.**

SCIM served `userName` AND `externalId` from the single `users.external_id`
column — the OIDC `sub` that logins join on. That forced the immutability: a
`userName` change would have re-keyed a live identity and locked the person out.
So the refusal was self-consistent and the integration was still broken, for ever,
at any IdP whose `userName` is a login email and whose subject is opaque.

The ticket's other branch — keep immutability, improve the message — was
considered and rejected. It would have documented a failure rather than fixed one:
the sync still never converges, and there is nothing the admin could have changed.

They are two things in SCIM and are now two columns. `external_id` stays the
subject and stays immutable through SCIM; `user_name` is new, mutable, and what a
`userName` filter searches. Backfilled `user_name = external_id`, so **no existing
deployment observes a change** — a tenant that only ever sent the subject as
`userName` still matches on it, still reads it back, still works.

`externalId` is still refused, and the message now names the stored and received
values, says why (it would lock the user out rather than rename them), and names
the remedy (send `userName`; to re-key, deprovision and re-provision). The old one
said "userName/externalId are immutable" without saying which, to an admin who
could see neither.

Group `displayName` stays immutable, and needed no change: its refusal already
names the field and the remedy (`rename teams via /api/teams`). A group's
displayName encodes the team slug, so a rename there is a team rename with its own
collision semantics.

### Item 4 came free with item 1

A `userName eq` filter searches `user_name` and an `externalId eq` filter searches
`external_id` — different queries now, where they used to be the same one. The old
behaviour is worse than "finds nothing": an IdP that finds nothing during
reconciliation concludes the user is absent, and re-creates them.

### Item 2: the audit found two more of the same landmine

`teams.slug` was UNIQUE with no `deleted_at` predicate while DELETE soft-deletes,
so an IdP reorganisation — remove a group, add it back — got 409 `uniqueness`
permanently and the team name was unrecoverable. Same fix as
[[KAIROS-T-0161]]: a partial unique index.

The ticket asked for every UNIQUE on a soft-deletable table to be audited in one
pass. Done against the live schema rather than by reading migrations, and it found
**two more**, both soft-deleted by handlers that already exist:

| | |
|---|---|
| `teams.slug` | reported |
| `delivery_streams.slug` | found by the audit — `DELETE /api/streams` soft-deletes |
| `team_pages` sibling slug | found by the audit — `team_pages::soft_delete_page` |

And the audit's other half matters as much: **every `short_code` UNIQUE was left
alone deliberately.** A short code must stay unique across live *and* archived
items for ever — [[KAIROS-A-0020]] exists so an archived `ACME-T-0001` stays
readable and unambiguous, and re-issuing it would break exactly that. A blanket
"add the predicate everywhere" pass would have broken five tables. So would
reading the ticket without checking.

Mechanism demonstrated directly in psql rather than assumed: a plain UNIQUE
rejects re-inserting a soft-deleted row's slug, a partial index accepts it.

### Item 3: a swallowed deprovision

`{"Active": false}` in a pathless PATCH returned **200 with the user still
provisioned**. Attribute names are case-insensitive per RFC 7643 §2.1, and the
`path` form already lowercased — but the pathless arm matched exactly, so a
capitalised key fell through to the ignored-for-IdP-compatibility arm.

That is the worst available shape for this bug: the IdP is told the request
succeeded, so it never retries, and the person keeps their access.

The regression test is the existing deprovision assertion with its key
capitalised, so `active: false` in the response and the revoked membership both
have to hold. **Verified by reverting the fix**, which fails it with
`left: Bool(true), right: false` — a requested deprovision that did not happen.

### Filed rather than fixed: [[KAIROS-T-0197]]

Reading `jit_upsert_user` to confirm the column split could not break login — it
cannot; login never touches `user_name` — made a different gap obvious. The
inbound SCIM join has an email fallback specifically so it links to a JIT row
instead of duplicating it. The login direction has no matching fallback, so a
SCIM-created user whose `external_id` is not the subject they later present is
JIT-provisioned as a **second** row, without the membership.

`reference/scim.md` already described that accurately, so operators were warned
and nothing tracked fixing it. It is authentication-path work where a too-eager
fallback is an account-takeover shape rather than a convenience, which is why it
is its own ticket with the trust question written down.

### Gates

lint clean, **397 unit tests**, integration **47/47 exit 0**, `angreal web lint`
clean, `angreal docs build` green, REST reference current.

One self-inflicted detour worth recording: my bulk patcher for the ten `NewUser`
fixtures ran twice over the same files and produced duplicate fields, which
clippy caught immediately. Cheap to undo, and a reminder that a
non-idempotent rewrite over a glob wants a dry run first.