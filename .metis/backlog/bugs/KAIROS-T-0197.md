---
id: a-scim-created-user-who-logs-in
level: task
title: "A SCIM-created user who logs in gets a second row and no membership"
short_code: "KAIROS-T-0197"
created_at: 2026-09-25T01:08:23.252852+00:00
updated_at: 2026-09-25T02:44:24.656553+00:00
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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A login by a SCIM-provisioned user binds to the row SCIM created, with its
      membership, rather than creating a second one
- [x] The fallback's trust assumptions are written down — what claim is believed,
      and what stops it being used to bind to another person's row
- [x] An integration test: SCIM-create without `externalId`, grant a role, then
      log in with an unrelated `sub` and assert one row and the role intact
- [x] `reference/scim.md` stops describing the duplicate row as behaviour

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
## Status Updates (continued)

### 2026-09-25 — implemented on the decided trust boundary

The login path falls back to `users.email`, and only on `email_verified == true`.
Adopting a row re-keys `external_id` to the presented `sub`, so it happens once per
person and the fast path serves them afterwards.

### The argument that makes it safe, and its limit

A deployment has exactly **one** issuer (`OIDC_ISSUER_URL` is a single value), and
an issuer will not verify one address for two accounts — so within one Kairos a
verified email identifies one person. That is the whole basis for binding on it,
and it is written into the code and into the [[KAIROS-A-0010]] amendment rather
than left implicit, because **it does not survive multiple issuers**. If Kairos
ever supports more than one, this fallback has to change with it.

The residual case the code cannot distinguish: a row carrying this email and some
other genuine `sub`. Under one issuer that should not arise, since a subject is
stable per person — and there is no field that would let us tell a SCIM-created
placeholder subject from a real one. The row is selected earliest-created-first,
matching the inbound SCIM join's own tie-break, so at least both directions agree
about which row wins. Recorded as a known limit rather than papered over.

### Failing closed, at the cost of an outage that nearly was

`email_verified` is parsed leniently — bool, or the strings `"true"`/`"false"`.
That is not laziness about the value; it is about the TYPE. A plain
`Option<bool>` would fail to deserialize **the whole token** when an IdP sends
`"true"`, so one vendor's spelling of a claim Kairos barely uses would mean nobody
at that company can log in at all. The leniency converts that into "not verified",
which costs a duplicate row.

Anything else — `1`, `0`, `"yes"`, `null`, an object, an array — is `None`, which
means not verified, which means the fallback does not fire. `1` is in the unit test
by name, because a truthy-looking number is the value most likely to slip through a
looser implementation.

### Verified in both directions

- **Integration**: carol is SCIM-provisioned with a `userName` and no
  `externalId`, so her `external_id` is her email rather than her subject. She then
  logs in with a real Dex token (opaque `sub`, `email_verified: true`) and the test
  asserts she is the *same* user SCIM created, her membership survived, there is
  exactly **one** row for her email, and her `external_id` is now Dex's subject.
  Confirmed by disabling the fallback, which fails on "she must log in AS the user
  SCIM provisioned".
- **Unit**: the trust boundary itself, which the integration test cannot reach
  because Dex always sends `true`. Every value that must not verify, plus the two
  that must, plus a token with the claim absent and one with it malformed — both of
  which must still parse, since rejecting them would be the outage described above.

### Gates

lint clean, **400 unit tests**, integration **47/47**, e2e **16**, uat **22
journeys**, docs build green. `reference/scim.md` now documents adoption and the
verified-email condition instead of describing the duplicate row as behaviour.