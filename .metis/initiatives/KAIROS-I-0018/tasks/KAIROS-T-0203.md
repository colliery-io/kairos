---
id: log-in-with-a-password-the
level: task
title: "Log in with a password: the endpoint and the session bearer path"
short_code: "KAIROS-T-0203"
created_at: 2026-09-26T12:44:15.954893+00:00
updated_at: 2026-09-26T12:44:15.954893+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0201, KAIROS-T-0202]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0018
---

## Parent Initiative

[[KAIROS-I-0018]]

## Objective

`POST /api/login` with an email and password returns a session bearer, and
`require_auth` accepts it. This is the task where local auth becomes usable.

## Dependencies

[[KAIROS-T-0201]] (the hashing and the schema) and [[KAIROS-T-0202]] (the rate
limiter, which this endpoint is the first consumer of).

## Implementation Notes

### The third branch

`require_auth` already reads:

```rust
if crate::service_accounts::auth::is_api_key(&token) { … }
let claims = state.auth.verify(&token).await?;
```

A session token becomes a third branch before the OIDC verify. Keep the branch order
deliberate — cheap local checks first, the network-touching OIDC path last — and make
sure an unrecognised token shape still ends at the OIDC path so nothing that works
today stops working.

### Local auth is OFF unless enabled

`KAIROS_LOCAL_AUTH` (bool, default false). With it off, `/api/login` must not exist
— **not return 401, not exist** — so a deployment that never wanted local accounts
has no password endpoint to attack. Routes are composed in `app.rs`; add it
conditionally there rather than gating inside the handler.

It is additive: a deployment may have both an issuer and local auth, per the
[[KAIROS-I-0018]] decision. Neither path knows about the other.

### The failure response is a security surface

Wrong password, unknown email, an account with `password_hash IS NULL` (an
OIDC-only user), a revoked session — **all one 401 with one message**, and taking
comparable time. The last one matters and is easy to miss: an OIDC user's email
returning a *different* error than a nonexistent one is an account-enumeration
oracle. Verify against a dummy hash when the user is absent, so the timing does not
answer the question either.

### Session lifecycle

- An expiry, with a default that is a decision rather than an accident. Sessions are
  bearer tokens the CLI may store, so "forever" is wrong and "one hour" is hostile.
- `POST /api/logout` revokes the presented session.
- **Changing a password revokes that user's other sessions.** Without it, a password
  change after a suspected compromise leaves the attacker logged in, which is the one
  thing a user changing their password is trying to prevent. The endpoint for changing
  it is [[KAIROS-T-0204]]; the revocation belongs in the storage layer here so it
  cannot be forgotten there.
- `last_used_at` on successful validation — cheap, and it is what makes stale
  sessions visible later.

### Do not write a session row on every request

Validation is a lookup, not a write. `last_used_at` is the exception and should be
best-effort rather than on the critical path.

## Acceptance Criteria

- [ ] `POST /api/login` returns a session bearer for a correct email and password
- [ ] `require_auth` accepts it, and `/api/whoami` identifies the right person
- [ ] With `KAIROS_LOCAL_AUTH` off, `/api/login` is **not routed** (404, not 401)
- [ ] Wrong password, unknown email and an OIDC-only account are indistinguishable
      to a caller, in body and in timing
- [ ] `POST /api/logout` revokes; a revoked or expired session is a 401
- [ ] Revoking on password change is enforced in the storage layer, with a test
- [ ] The rate limiter from [[KAIROS-T-0202]] is applied, with a test that a burst
      is refused
- [ ] An OIDC deployment with local auth off behaves exactly as it does today —
      asserted, not assumed
- [ ] `angreal test lint`, `unit`, `integration` green

## Status Updates

*To be added during implementation*
