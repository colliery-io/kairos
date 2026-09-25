---
id: a-scim-created-user-who-logs-in
level: task
title: "A SCIM-created user who logs in gets a second row and no membership"
short_code: "KAIROS-T-0197"
created_at: 2026-09-25T01:08:23.252852+00:00
updated_at: 2026-09-25T01:08:23.252852+00:00
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

# A SCIM-created user who logs in gets a second row and no membership

## Objective

Make a login find the user SCIM already provisioned, instead of creating a
duplicate. Today JIT provisioning joins on `users.external_id` alone, so when
SCIM created the row with an `external_id` that is not the subject the IdP later
presents, the person logs in as a **different user** — with none of the access
that was provisioned for them.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (provisioning silently does not take effect for the user)

### Impact Assessment

- **Affected users**: anyone onboarded by SCIM at an IdP that does not send
  `externalId`, or sends one that differs from the OIDC `sub`. The admin sees a
  provisioned member with the right role; the person logs in and is nobody.
- **Reproduction**: `POST /scim/v2/Users` with `userName` and `emails` but no
  `externalId` — the row is created with `external_id = userName`
  (`scim/users.rs`, the create arm). Grant a role. Then log in with an OIDC token
  whose `sub` is an opaque id. `jit_upsert_user`
  (`crates/kairos-server/src/middleware/auth.rs:355`) filters on
  `users::external_id.eq(&claims.sub)` only, finds nothing, and inserts a second
  row. The new row has no `organization_members` entry, because JIT never grants
  membership — deliberately, per KAIROS-A-0010.
- **Expected vs actual**: the login binds to the provisioned user. Instead there
  are two rows for one person, and the one they are logged in as has no access.

### Why it is worth its own ticket

The **inbound** direction already handles this: SCIM's identity join has an email
fallback specifically so it links to a JIT-created row rather than duplicating it
(`resolve_or_create_user`). The **login** direction has no matching fallback. The
asymmetry is the bug — whichever system sees the person second should find the
row the first one made, and only one of the two does.

[[KAIROS-T-0184]] makes this more tractable rather than fixing it. `userName` now
lives in its own column, so a login has something meaningful to fall back to
beyond email: `external_id`, then `user_name`, then `email`.

### Risk Considerations

This is the authentication path, and a fallback that matches too eagerly is an
account-takeover shape rather than a convenience: matching on `email` means
trusting the IdP's `email` claim to identify a person, and an IdP that lets a user
set an unverified email would then let them bind to someone else's row. Any
fallback needs to state what it trusts and why — and `email_verified`, or
restricting the fallback to rows SCIM created and no one has logged in as yet, are
both worth considering.

That is exactly why this is filed rather than fixed in passing.

## Acceptance Criteria

- [ ] A login by a SCIM-provisioned user binds to the row SCIM created, with its
      membership, rather than creating a second one
- [ ] The fallback's trust assumptions are written down — what claim is believed,
      and what stops it being used to bind to another person's row
- [ ] An integration test: SCIM-create without `externalId`, grant a role, then
      log in with an unrelated `sub` and assert one row and the role intact
- [ ] `reference/scim.md` stops describing the duplicate row as behaviour

## Status Updates

**2026-09-25 — filed while doing [[KAIROS-T-0184]].** Not one of that ticket's
four items. Found by reading `jit_upsert_user` to check that splitting `userName`
out of `external_id` could not break login — it cannot, because login never
touches `user_name`, and that is when the missing fallback became obvious.

Already described accurately in `reference/scim.md` ("is JIT-provisioned as a
**second** user row, without the membership"), so an operator who reads the
reference is warned. Nothing tracked fixing it.

## Decision — 2026-09-25 (Dylan)

**Fall back to email, but only on a verified claim.**

```
match users.external_id == claims.sub
  else if claims.email_verified == true  -> match users.email
  else                                    -> create a new row (today's behaviour)
```

The trust boundary is explicit and narrow: Kairos believes the IdP when it asserts
`email_verified`, and believes nothing otherwise. That matters because the failure
mode of a looser rule is not a duplicate row, it is **one user binding to another
user's identity** — an IdP that lets someone set an unverified email would
otherwise let them claim a provisioned account with that address.

An IdP that omits `email_verified` falls through to creating a row, which is
exactly what happens today, so no deployment gets worse.

Not chosen, and why it is worth recording: gating on "a SCIM-provisioned row nobody
has logged into yet" trusts no new claim at all, but needs a column to track first
login and only ever helps the first login. The verified-email rule is simpler and
also fixes the case where a person's row was created by an earlier JIT login.

To carry into implementation:

- `A-0010` gains the trust boundary in writing — which claim is believed, and what
  stops it being used to bind to someone else's row.
- On adopting a row, `external_id` is UPDATED to the presented `sub`, so the next
  login takes the fast path. That write is the whole point; without it the fallback
  runs for ever.
- The integration test from the acceptance criteria, plus a negative: the same
  shape with `email_verified` absent or false must still create a second row.
