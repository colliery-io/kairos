---
id: log-in-with-a-password-the
level: task
title: "Log in with a password: the endpoint and the session bearer path"
short_code: "KAIROS-T-0203"
created_at: 2026-09-26T12:44:15.954893+00:00
updated_at: 2026-09-26T14:20:59.297322+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0201, KAIROS-T-0202]
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

## Acceptance Criteria

- [x] `POST /api/login` returns a session bearer for a correct email and password
- [x] `require_auth` accepts it, and `/api/whoami` identifies the right person
- [x] With `KAIROS_LOCAL_AUTH` off, `/api/login` is **not routed** (404, not 401)
- [x] Wrong password, unknown email and an OIDC-only account are indistinguishable
      to a caller, in body and in timing
- [x] `POST /api/logout` revokes; a revoked or expired session is a 401
- [x] Revoking on password change is enforced in the storage layer, with a test
- [x] The rate limiter from [[KAIROS-T-0202]] is applied, with a test that a burst
      is refused
- [x] An OIDC deployment with local auth off behaves exactly as it does today —
      asserted, not assumed
- [x] `angreal test lint`, `unit`, `integration` green

## Status Updates

### 2026-09-26 — done, with one design change to [[KAIROS-T-0201]]

`crates/kairos-server/src/login.rs` holds `/api/login`, `/api/logout` and
`authenticate_session`; `crates/kairos-db/src/local_auth.rs` holds the storage.

#### The change: `local_sessions` is PUBLIC, not tenant-scoped

[[KAIROS-T-0201]] put the table in `migrations/tenant/` and had
`generate_session_token(slug)` embed a tenant, mirroring `api_keys`. Building this
task showed that symmetry was superficial and its cost was real, so the migration
moved to `migrations/public/` and the token is now a bare `kairos_ss_<64-hex>`.

The reasoning: an API key embeds a slug because the **script** presenting it has no
other way to say which org it means. A session has one — the browser is already at
`acme.kairos.example`. And what a session stands in for is an OIDC token, which is
deployment-wide. Two consequences decided it:

1. **One login, not one per org.** A person in two organizations would otherwise have
   logged in twice. Membership is still enforced per request by the tenant
   middleware, so nothing widened.
2. **This task's own acceptance criterion.** "Changing a password revokes that user's
   other sessions" is one `UPDATE` against a public table. Tenant-scoped it would
   have had to fan out across every `org_*` schema the person belongs to — and a
   guarantee that has to iterate is one that will eventually miss a row. The
   migration comment records this so the next reader does not "restore" the symmetry.

Safe to change: `git tag --contains` confirmed the T-0201 commit is in no release.
Follow-on edits were the public-migration drift gate (9 → 10 tables) and the tenant
one (34 → 33).

#### Where each guarantee lives

`kairos_db::local_auth::set_password` writes the hash **and** revokes every session in
one transaction, so "changed the password but forgot to revoke" is unrepresentable
rather than merely discouraged — [[KAIROS-T-0204]]'s admin reset cannot get it wrong
by omission. `clear_password` is the same shape, because taking a password away must
not leave the sessions it minted working.

`find_user_by_email` deliberately does **not** filter on `password_hash IS NOT NULL`,
and a test asserts that. If it did, the caller could no longer make "no such person"
and "that person authenticates through the issuer" identical — they would have
diverged before the handler got the chance. That is an enumeration oracle over exactly
the addresses an attacker wants, and it is the easiest thing here to ship by accident.

#### The uniform failure, and its timing

One `bad_login()` constructor, because the property is that every failure is identical
and identical-by-construction is the only kind that survives editing. The
unknown-email and no-password paths call `verify_against_dummy` so they cost the same
argon2 work. The integration test asserts the bodies are equal AND that the
absent-user path takes at least a quarter as long as a real verification — a loose
bound, because the defect being caught (skipping argon2 entirely) is an order of
magnitude, not a few percent. It also asserts a real verification costs at least 5 ms,
since otherwise the comparison would prove nothing.

#### Other decisions

- **Branch order** in `require_auth`: API key, then session, then OIDC. Both local
  checks are a prefix plus one indexed lookup; OIDC may touch the network for JWKS. An
  unrecognised shape still falls through to OIDC, so nothing that authenticates today
  changed.
- **Not routed when off.** `app::router` merges an EMPTY router rather than an
  `Option`, so there is no second assembly path to keep in step. With local auth off a
  `kairos_ss_` bearer is refused without asking the database — there are no sessions
  to find.
- **argon2 runs inside `run_public`**, a `spawn_blocking` thread. 19 MiB and two
  iterations is tens of milliseconds; on a runtime thread that is tens of milliseconds
  serving nobody.
- **Logout is always 204** — twice, or with something that is not a session. The caller
  has what they wanted either way, and saying otherwise is an oracle. It sits outside
  the auth stack so an expired session can still be logged out.
- **A service account cannot log in with a password**, checked after the password so it
  costs an attacker nothing to learn.
- **14 days** for `KAIROS_SESSION_TTL_SECS`. Forever is a permanent credential handed
  out by a login form; an hour logs people out mid-task. `0` with local auth on is
  refused at boot rather than minting already-expired sessions.

#### One defect found

The live `openapi_endpoint_against_live_stack` probe asserts every documented path is
routed, and built its router with local auth off — so documenting `/api/login`
unconditionally made it fail. The right fix was the probe's router, not the spec: the
spec is the API contract and an operator needs to know the endpoint exists and what
switches it on. The probe now builds the router that has every documented route.

#### Raised, not resolved: does the issuer become optional?

`OIDC_ISSUER_URL` and `OIDC_AUDIENCE` are still `required()`, so a local-auth
deployment **still needs an issuer to boot**. That satisfies this task and the
initiative's "local accounts are additive to any issuer", and the chart's bundled Dex
([[KAIROS-T-0200]]) means `helm install` always has one. But [[KAIROS-I-0018]]'s exit
criterion *"a ten-person team can run Kairos with no IdP at all"* reads as needing the
issuer to be optional, and the compose stack bundles no Dex. No task owns that work,
which is a gap in the decomposition rather than in this task. Put to Dylan before
[[KAIROS-T-0204]], because a first-boot bootstrap admin is where the answer changes
what gets built.

**Files:** `login.rs` (new), `kairos-db/src/local_auth.rs` (new), the sessions
migration (moved to public, rewritten), `local_auth.rs` (token loses its slug),
`middleware/auth.rs` (third branch), `app.rs` (conditional mount), `config.rs`
(`KAIROS_LOCAL_AUTH`, `KAIROS_SESSION_TTL_SECS`), `api/openapi.rs`, two new test files
(`kairos-db/tests/local_auth.rs`, 5 tests; `kairos-server/tests/local_login.rs`, 4
tests), both migration drift gates, the openapi probe, plus compose, `.env.example`,
the chart and `docs/src/reference/configuration.md`.

`angreal test lint`, `unit` and `integration` are green.