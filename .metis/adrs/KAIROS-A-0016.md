---
id: 001-identity-as-an-external-concern
level: adr
title: "Identity as an External Concern - BYO OIDC plus SCIM Provisioning"
number: 1
short_code: "KAIROS-A-0016"
created_at: 2026-07-10T08:58:37.823197+00:00
updated_at: 2026-07-10T08:59:44.663567+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: Identity as an External Concern - BYO OIDC plus SCIM Provisioning

## Context

KAIROS-A-0010 and the vision named Keycloak as the OIDC provider, shipped it in the reference deployment, and made a realm export a product artifact. During M2 implementation Dylan redirected (2026-07-10): **Kairos should not hold an identity opinion.** Companies adopting Kairos already run an IdP (Okta, Entra ID, Auth0, Keycloak, Dex, …); bundling one adds operational weight (A-0013 already flagged Keycloak's footprint as an adoption objection), a second admin surface, and an implicit endorsement Kairos doesn't need. The implementation evidence supports this: the T-0017 middleware is already provider-agnostic (OIDC discovery + JWKS by kid; config is issuer URL + audience only) and passes against Dex, which was never more than a test fixture.

What identity actually requires from Kairos: (1) verify who a request is from — OIDC does this; (2) get users and their lifecycle into Kairos — enterprise IdPs speak **SCIM 2.0** (RFC 7643/7644) for exactly this, including the deprovisioning case JIT can't cover (an offboarded employee's tokens expire, but their memberships should be revoked proactively).

## Decision

**Kairos ships no identity provider. Authentication is any spec-compliant OIDC issuer (bring your own). User/group lifecycle is inbound SCIM 2.0 served by Kairos, with JIT + manual membership as the non-SCIM path.**

> **Note on the "state and identity are the operator's" posture
> (KAIROS-A-0021 rule 2, KAIROS-T-0188).** The Helm chart used to justify having
> no Postgres subchart by quoting this ADR. It now bundles one, disableable, for
> evaluation — because `pgvector` became a requirement. **Identity is unchanged
> and remains wholly external**; it is the *database* half of that posture that
> softened from an absolute to a default, and [[KAIROS-A-0013]] carries the
> amendment. Nothing in this ADR is reversed.


> **Amendment: an absolute becomes a default (KAIROS-I-0018, 2026-09-26).**
>
> The sentence above — *Kairos ships no identity provider* — was written as an
> absolute. It is now a **default**, in exactly the way [[KAIROS-A-0021]] rule 2 turned
> [[KAIROS-A-0013]]'s "bring your own PostgreSQL" into one.
>
> **This ADR's own review trigger fired.** It listed *"evaluator friction data showing
> the no-bundled-IdP quickstart is a real adoption barrier"*, and that is what
> happened: evaluating Kairos meant running Dex by hand or standing up a cloud OAuth
> client, and for one person or a ten-person team both are more setup than the product
> deserves. The ADR named this gap itself — *"no turnkey identity for evaluators
> without any IdP"* — and proposed a documented Dex quickstart as the mitigation.
> **That mitigation was not enough.** A documented quickstart is still a second system
> for the evaluator to stand up, understand and debug before they have seen a board.
>
> **What is unchanged, and it is most of this ADR.** An enterprise deployment still
> brings its own issuer. Kairos still owns no identity *for it*. SCIM is still how
> lifecycle arrives. `OIDC_ISSUER_URL` + `OIDC_AUDIENCE` are still the whole
> authentication configuration when an issuer is named, still validated by discovery +
> JWKS, and still provider-agnostic. Deployment-admin authority is still the
> `KAIROS_DEPLOYMENT_ADMINS` sub list. Nothing here is reversed for anyone who has an
> IdP.
>
> **What changed.** Two things, both off by default:
>
> 1. **The Helm chart can bundle a Dex** (`dex.enabled`, KAIROS-T-0200) —
>    evaluation-only, loudly so, in the same shape as the bundled PostgreSQL: one
>    replica, in-memory storage, static users, a password hash in your values file.
>    Dex remains dev/test tooling; the chart now makes standing it up a switch rather
>    than a homework assignment.
> 2. **Kairos can authenticate people itself** (`KAIROS_LOCAL_AUTH`, KAIROS-T-0203):
>    email and password, against accounts an org admin creates. Additive — a
>    deployment may have both an issuer and local accounts — and off unless asked for,
>    so an existing deployment's surface does not change. With it on, the OIDC
>    variables become optional (KAIROS-T-0208), which is what lets a small team run
>    with no identity provider at all.
>
> **What Kairos now owns that it deliberately did not, stated plainly.** Password
> storage, and every security question that comes with it:
>
> - **A password database.** `users.password_hash` holds argon2id PHC strings at OWASP
>   server parameters (19 MiB, t=2, p=1). If that column leaks, the cost of cracking it
>   is a parameter choice this project now maintains.
> - **A brute-force surface.** A password endpoint is guessable in a way a 32-byte API
>   key is not, which is why KAIROS-T-0202 exists. Kairos now owns a throttle, its
>   decay arithmetic, and the question of what to trust for a client address.
> - **An account-enumeration surface.** Every failure on the login path must be
>   indistinguishable — wrong password, unknown email, an OIDC-only account, a revoked
>   session — in body *and* in timing. That is a property to keep true forever, not a
>   feature that was shipped.
> - **Credential lifecycle, with no email subsystem.** No self-service sign-up, no
>   reset link. An org admin resets a password; an operator with database access does
>   it when nobody can log in. Session revocation on password change is enforced in the
>   storage layer so it cannot be forgotten.
> - **A bootstrap credential.** `KAIROS_BOOTSTRAP_ADMIN` is single-use precisely
>   because an environment variable in a values file is a permanent credential in a
>   release history.
>
> None of that was free, and a future reader deserves to see it acknowledged here
> rather than discover it in `local_auth.rs`. The advice that follows from it is in
> *Choosing how people log in* (`docs/src/explanation/`): local accounts are for small
> teams and break-glass access, and an IdP is the right answer the moment there is a
> security team to answer to.

### Authentication (amends KAIROS-A-0010's realm strategy)
- Configuration remains exactly `OIDC_ISSUER_URL` + `OIDC_AUDIENCE`; validation via discovery + JWKS as built in T-0017. No realm export, no bundled IdP, no Keycloak-specific anything in the product or reference deployment.
- A-0010's mechanics stand unchanged: local JWKS validation, JIT user provisioning, PKCE (GUI), device grant (CLI), client-credentials service accounts *where the customer's IdP supports them* — flow availability is the IdP's capability matrix, and the docs state per-flow IdP requirements instead of assuming one vendor.
- **Dex remains dev/test tooling only** (A-0012): the compose dev profile's issuer, never a production recommendation.
- Deployment-admin authority stays the `KAIROS_DEPLOYMENT_ADMINS` sub list — deliberately IdP-neutral (no assumption that the customer's IdP can mint custom roles/claims).

### Provisioning (new capability)
- Kairos serves **SCIM 2.0** per tenant: `/scim/v2/Users` (create/get/list/filter/patch/replace/deactivate) and `/scim/v2/Groups` (mapped to org membership and, optionally, team membership). IdPs push lifecycle events; `active: false` / delete revokes `organization_members` (and team memberships) immediately — the deprovisioning path JIT lacks.
- SCIM auth: per-tenant long-lived bearer token, generated by an org admin via the API, hashed at rest, revocable. SCIM traffic is tenant-scoped by the token, not by subdomain.
- Identity join key: SCIM `userName`/`externalId` maps to `public.users.external_id` (the OIDC sub) with email as the fallback join — the exact mapping contract lives in the implementing task and the operations docs, per-IdP notes included.
- SCIM is additive: JIT-on-first-login + `/api/members` add-by-email (S-0005, 2026-07-10 addendum) remain fully supported for small orgs without IdP admin access.

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| BYO OIDC + SCIM server (chosen) | Zero identity footprint; fits any enterprise IdP; standard deprovisioning; smaller deployment | Kairos must implement/maintain a SCIM subset; per-IdP quirks in the wild | Low | Medium |
| Bundle Keycloak (prior A-0010/A-0013 stance) | Turnkey identity for greenfield installs | Heavy footprint; second admin surface; vendor opinion; still needs federation to the real corp IdP in practice | Medium | Medium |
| OIDC only, no SCIM | Least work | No proactive deprovisioning; membership forever manual; fails enterprise IdP-integration checklists | Medium | Low |
| LDAP sync | Familiar to legacy shops | Wrong direction of travel; polling; SCIM is the modern standard IdPs actually ship | High | High |

## Rationale

1. **Adopting companies have identity solved.** Kairos's value is Flight Levels, not auth — every bundled-IdP hour is stolen from the product, and every bundled-IdP opinion is an integration argument with a customer's security team.
2. **The code already votes for this.** T-0017 needed zero provider-specific logic; deleting the Keycloak commitment deletes weight (compose service, realm export, backup surface) while changing no code paths.
3. **SCIM is the missing half.** OIDC answers "who is this?"; SCIM answers "who should exist here, and who no longer should" — the offboarding story enterprises will require before rollout.
4. **The env-list deployment admin gets stronger** under IdP neutrality: it assumes nothing about the customer's ability to author roles or claims.

## Consequences

### Positive
- Reference deployment shrinks to Caddy + Kairos + Postgres (A-0013 amended) — the footprint objection dies with the bundle
- Enterprise onboarding follows the customer's existing IdP runbooks (SSO + SCIM app registration), not ours
- Proactive deprovisioning via SCIM closes the JIT gap

### Negative
- Kairos owns a SCIM implementation (bounded: Users/Groups subset, one new task) and its per-IdP quirk handling
- ~~No turnkey identity for evaluators without any IdP — mitigated by documenting a one-container Dex quickstart as *example, not product*~~ **This was the cost that came due (KAIROS-I-0018).** The quickstart mitigation was not enough; see the amendment above. The replacement cost is that Kairos now owns password storage, a brute-force surface, an enumeration surface and a credential lifecycle — enumerated in the amendment.

### Neutral
- A-0010's flows/validation/JIT survive intact; only its realm-strategy section and Keycloak framing are superseded by this ADR
- A-0012/A-0013's Keycloak references are amended to Dex-as-fixture and BYO-issuer respectively
- Per-IdP audience/claim notes (e.g., audience mappers) move to operations documentation

## Review Schedule

### Review Triggers
- SCIM interop issues with a major IdP that the subset can't accommodate
- ~~Evaluator friction data showing the no-bundled-IdP quickstart is a real adoption barrier~~ — **fired 2026-09-26, see the amendment in Decision**
- A security defect in the local-auth surface that the throttle, the uniform failure or
  the storage-layer revocation did not prevent. Local accounts are a small-team
  convenience; if keeping them safe starts costing more than an IdP integration would,
  the trade has inverted and this should be revisited rather than patched.