---
id: password-verification-core-argon2
level: task
title: "Password verification core: argon2, the schema, and no endpoints yet"
short_code: "KAIROS-T-0201"
created_at: 2026-09-26T12:43:06.442887+00:00
updated_at: 2026-09-26T13:18:23.272253+00:00
parent: KAIROS-I-0018
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

- [x] Migration adds `users.password_hash` (nullable) and `local_sessions`; `schema.rs`
      regenerated via `angreal db schema-sync`
- [x] argon2 hash + verify, with the chosen parameters in a named const and the
      reasoning recorded
- [x] Session tokens mint, hash, and parse; `is_session` and `is_api_key` cannot
      both claim one token, with a test that says so
- [x] Wrong password, unknown user and a malformed hash are all distinguishable **in
      code** and identical **to a caller** — no endpoint yet, but the core must not
      make a leaky answer easy
- [x] Unit tests only; no endpoint, no route, no GUI
- [x] `angreal test lint` and `angreal test unit` green

## Status Updates

*To be added during implementation*
## Status Updates

### 2026-09-26 — the foundation, and no endpoint

`crates/kairos-server/src/local_auth.rs` plus two migrations. Nothing is routed and
nothing can log in yet, which is what this task was scoped to.

### argon2id parameters, and why these

**19 MiB, 2 iterations, 1 lane** — the OWASP Argon2id recommendation, and
deliberately the variant tuned for a *server handling concurrent logins* rather than
the higher-memory settings meant for disk encryption.

The trade-off runs both ways and that is the point: too cheap is crackable from a
stolen dump, too expensive is a denial-of-service against your own login endpoint,
because **every attempt including every wrong one** costs the server that memory and
time. At 19 MiB, ten concurrent attempts cost ~190 MiB transiently — which is why
this decision and [[KAIROS-T-0202]]'s rate limiter are the same decision seen from
two sides and should move together.

Stored as a full PHC string rather than a bare digest, so the parameters travel with
each hash. Raising the cost later can re-hash on next successful login instead of
invalidating every password at once.

### Two hashes that must stay different

The module leads with a table saying so, because the inconsistency looks like a bug
and is not:

| Secret | Entropy | Hash |
|---|---|---|
| session token | 32 random bytes | SHA-256 |
| password | whatever a person chose | argon2id |

Making them consistent would be wrong in one direction or the other — argon2 on
every authenticated request is self-inflicted denial of service, SHA-256 on a
password is a dictionary attack waiting for a leak. `kairos_ss_` mirrors
`service_accounts::auth`'s `kairos_sk_` exactly because that machinery is right for
a high-entropy token.

### The timing oracle, closed deliberately

`verify_against_dummy` spends real argon2 work and returns false. Without it, "no
such account" returns in microseconds while a wrong password takes ~50ms, and that
gap **is** an enumeration oracle no matter how careful the response body is. The
same applies to an OIDC-only user whose `password_hash` is `None`.

Its test asserts the dummy is a **parseable** PHC string, which matters more than it
looks: a malformed dummy would return `Err` and skip the work, silently reopening the
oracle it exists to close.

### `Ok(false)` is not `Err`

A wrong password is `Ok(false)`; a stored hash that will not parse is
`Err(MalformedStoredHash)`. Conflating them would let corruption read as a failed
login and lock a user out with nobody told. There is a test per junk input.

### Four API surprises in argon2 0.6 / password-hash 0.6

Each cost a compile error and none were guessable from the older API everybody has
memorised:

- `PasswordHash` is at `password_hash::phc::PasswordHash`, not `password_hash::`
- `SaltString` lives in the separate `phc` crate
- `SaltString::generate()` takes no RNG
- **`hash_password(&self, password)` generates the salt itself** — so `SaltString`
  is not needed at all, and the import went away again. That is the better API: there
  is no way to reuse a salt by accident.

### Schema

- `public.users.password_hash TEXT NULL` — nullable is the design, not a
  convenience. An OIDC user has no password and must not have a column implying one.
- `local_sessions` in the **tenant** schema, mirroring `api_keys`, because the token
  embeds its tenant and carries its own scope rather than needing a Host header.
- `schema.rs` regenerated: 34 tenant tables, up from 33.

`tenant_provisioning`'s `EXPECTED_TABLES` needed the new name and the count — which
is the test doing its job, and the T-0093 schema comparison stayed green because it
compares two tenants and both have it.

### Gates

lint clean, **412 unit tests** (9 new), integration **47/47**.

### One process note

An `apply.pl` edit reported success and did not apply — `"item_relationships",`
appears more than once in that file and the first occurrence was elsewhere. Caught
because the compiler said the array had 33 elements when I had declared 34. Worth
remembering that "applied" from that helper means "a match was replaced", not "the
match you meant".