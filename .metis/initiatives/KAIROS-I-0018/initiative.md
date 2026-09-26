---
id: login-without-an-idp-a-bundled-dex
level: initiative
title: "Login without an IdP — a bundled Dex and native local accounts"
short_code: "KAIROS-I-0018"
created_at: 2026-09-26T12:34:33.435909+00:00
updated_at: 2026-09-26T12:51:02.766699+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/active"


exit_criteria_met: false
estimated_complexity: M
initiative_id: login-without-an-idp-a-bundled-dex
---

# Login without an IdP — a bundled Dex and native local accounts

## Context

Kairos requires an OIDC issuer to start. `OIDC_ISSUER_URL` and `OIDC_AUDIENCE` are
`required()` in `config.rs`; there is no password storage anywhere in the schema,
and `require_auth` accepts exactly two things — a Kairos API key (service accounts)
or an OIDC JWT. A person cannot log in without an IdP.

[[KAIROS-A-0016]] made that deliberately, and its rationale still holds for the
enterprise case: *"every bundled-IdP hour is stolen from the product, and every
bundled-IdP opinion is an integration argument with a customer's security team."*
It also **named this exact gap as a known cost** — "no turnkey identity for
evaluators without any IdP" — with the mitigation being "document a one-container
Dex quickstart as example, not product."

That mitigation has proven insufficient in practice. Evaluating Kairos currently
means either running Dex by hand or standing up a Google Cloud project, and for a
single user or a ten-person team both are more setup than the product deserves.

**There is now a precedent for the fix.** [[KAIROS-A-0021]] rule 2 amended A-0013's
"bring your own PostgreSQL" into "the chart stands one up unless you name your own",
because requiring pgvector made the absolute unreasonable for a first afternoon.
This initiative applies the same reasoning to identity, and goes one step further
with native local accounts.

## Decisions (Dylan, 2026-09-26)

Four shape questions were put and answered; each cheapest coherent option was taken.

| Question | Decision |
|---|---|
| Session mechanism | **Reuse the API-key shape** — opaque bearer, hashed at rest, server-validated |
| Coexistence | **Both, independently** — local accounts are additive to any issuer |
| Account creation | **Admin creates them**, plus a first-boot bootstrap admin |
| Password reset | **Admin resets**, no email subsystem |

### Why the session decision matters most

`require_auth` already branches on `is_api_key(token)` before trying OIDC, and API
keys are opaque tokens hashed at rest with revocation and expiry — exactly the shape
a local session wants. Local auth becomes a **third branch on an existing pattern**
rather than a new subsystem.

The one thing that machinery is NOT reusable for is the password itself. API keys are
high-entropy random strings, so the existing SHA-256 is appropriate; a password is
low-entropy and needs a memory-hard KDF. **argon2 is a new dependency and its
parameters are a decision**, not a default to accept unexamined.

Rejected: making Kairos an OIDC provider. It would unify the auth paths and even make
the CLI device grant work, at the cost of generating, storing and rotating signing
keys and serving discovery and JWKS — i.e. maintaining an IdP, which is the thing
A-0016 spent its entire rationale avoiding.

## Proposed decomposition

Six tasks. The first is independent of the rest and could ship alone.

| # | Task | Shape |
|---|---|---|
| 1 | **Chart bundles an optional Dex** | Mirrors `postgresql.*` almost exactly: tri-state `enabled`, Deployment + Service + ConfigMap of static users, a helper resolving the in-cluster issuer URL, a CI values set, and evaluation-only warnings in every place it appears. No product code. |
| 2 | **Password verification core + schema** | `users.password_hash` (nullable — an OIDC-only user has none), a `local_sessions` table, argon2 with recorded parameters, and the pure verify/mint logic with unit tests. No endpoints yet. |
| 3 | **Login endpoint and the session bearer path** | `POST /api/login`, `POST /api/logout`, the `is_session` branch in `require_auth`, expiry, revoke-on-password-change. Integration tests including the negative paths. |
| 4 | **Admin surfaces and the break-glass CLI** | Create a local user, set a password, revoke sessions; `kairos users set-password` for an operator locked out of the GUI; first-boot bootstrap admin from env. |
| 5 | **GUI login form** | Email/password beside the existing provider button, shown only when the deployment enables local auth — which means `/api/config` has to say so. |
| 6 | **Close-out** | The [[KAIROS-A-0016]] amendment, the book (when to use which, a how-to, reference), a UAT journey, and the surface drift gate. |

## Risk Considerations

These are the things that make local auth different from the rest of Kairos, and
each wants deciding inside its task rather than discovered:

- **Rate limiting is not optional.** A password endpoint with no throttle is a
  brute-force target, and Kairos has no rate limiting anywhere today. This is the
  single largest piece of genuinely new work hiding in task 3.
- **Do not reveal whether an address exists.** Wrong-password and no-such-user must
  return the same 401 and take comparable time.
- **The bootstrap password must be single-use.** An env var that stays set is a
  credential in the deployment manifest for ever; it should be consumed on first
  boot and then refuse to work.
- **Session invalidation on password change**, or a changed password leaves live
  sessions behind it.
- **argon2 parameters are a deployment trade-off** — too cheap is crackable, too
  expensive is a denial-of-service against your own login. Pick, record the
  reasoning, and make it configurable only if there is a reason to.
- **Bundled Dex must be as loudly evaluation-only as the bundled Postgres is.** One
  replica, static users, a password in your values file. The Postgres wording is the
  model.
- **Two auth paths mean two ways to be the same person.** A local account and an
  OIDC identity with one email should resolve to one `users` row — which is the
  [[KAIROS-T-0197]] join question again, and its answer (match on a verified email,
  under one issuer) has to extend to "or a local account".

## Exit Criteria

- [ ] `helm install` with neither an issuer nor a database named brings up a working
      Kairos an evaluator can log into
- [ ] A ten-person team can run Kairos with no IdP at all
- [ ] An OIDC deployment can add a local break-glass admin without touching its issuer
- [ ] Nothing about either path is reachable by accident: both are off unless enabled,
      and the documentation says which to use and why
- [ ] A-0016 is amended rather than contradicted, with the reasoning recorded