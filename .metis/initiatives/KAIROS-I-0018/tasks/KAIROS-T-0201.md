---
id: password-verification-core-argon2
level: task
title: "Password verification core: argon2, the schema, and no endpoints yet"
short_code: "KAIROS-T-0201"
created_at: 2026-09-26T12:43:06.442887+00:00
updated_at: 2026-09-26T12:43:06.442887+00:00
parent: KAIROS-I-0018
blocked_by: []
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

The storage and the pure logic for local passwords and sessions. **No endpoints** —
this task is the foundation the next three sit on, and it ends with unit tests rather
than a login you can use.

## Implementation Notes

### Schema

- `public.users.password_hash TEXT NULL`. **Nullable is the whole point**: an
  OIDC-only user has no password, and a local-only user has no meaningful
  `external_id` from an issuer. Both kinds live in one table because they are both
  people, and [[KAIROS-T-0197]] already established that one person should be one row.
- `public.local_sessions` — `id`, `user_id`, `token_hash`, `created_at`,
  `expires_at`, `revoked_at`, `last_used_at`. Deliberately the same shape as
  `api_keys`, because the validation path is going to be the same shape too.
- `users.external_id` is `NOT NULL UNIQUE` today. A local-only account has no OIDC
  subject, so decide and record how it is filled — a synthetic `local:<uuid>` mirrors
  what service accounts already do (`svc:<uuid>`, see `NewServiceAccountUser`) and
  keeps the uniqueness guarantee without a schema change.

### argon2, and its parameters are a decision

`argon2` is a new dependency. **Do not accept the defaults without recording why.**
The trade-off is real in both directions: too cheap is crackable from a stolen dump,
too expensive is a denial-of-service against your own login endpoint, because every
attempt costs the server that memory and time.

Start from the OWASP recommendation for Argon2id, write the chosen m/t/p into a
const with a comment saying what was chosen and against what, and put the numbers
in the task's Status Updates so the next person can re-examine them rather than
re-derive them.

Store the full PHC string (`$argon2id$v=19$m=…`), not a bare hash — it carries the
parameters, so a future parameter change can re-hash on next successful login instead
of invalidating every password.

### The session token

Mirror `service_accounts::auth`. Read it first: a prefix that identifies the token
kind and embeds the tenant, a high-entropy random secret, and **only the SHA-256
stored** so validation is one indexed lookup. SHA-256 is correct here and wrong for
the password — the token is high-entropy, the password is not. Say so in a comment,
because the two living side by side invites someone to "fix" the inconsistency.

`is_session(token)` has to be unambiguous against `is_api_key(token)`; pick a prefix
that cannot collide and unit-test that it does not.

### Keep it pure where it can be

Per [[KAIROS-A-0009]], hashing, verification, prefix parsing and expiry arithmetic
are functions over in-memory data and belong in a module with no I/O, unit-tested
without a database. The storage functions go in `kairos-db`.

## Acceptance Criteria

- [ ] Migration adds `users.password_hash` (nullable) and `local_sessions`; `schema.rs`
      regenerated via `angreal db schema-sync`
- [ ] argon2 hash + verify, with the chosen parameters in a named const and the
      reasoning recorded
- [ ] Session tokens mint, hash, and parse; `is_session` and `is_api_key` cannot
      both claim one token, with a test that says so
- [ ] Wrong password, unknown user and a malformed hash are all distinguishable **in
      code** and identical **to a caller** — no endpoint yet, but the core must not
      make a leaky answer easy
- [ ] Unit tests only; no endpoint, no route, no GUI
- [ ] `angreal test lint` and `angreal test unit` green

## Status Updates

*To be added during implementation*
