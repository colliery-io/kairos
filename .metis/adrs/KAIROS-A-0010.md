---
id: 001-oidc-authentication-integration
level: adr
title: "OIDC Authentication Integration - Keycloak"
number: 1
short_code: "KAIROS-A-0010"
created_at: 2026-07-08T11:28:34.373549+00:00
updated_at: 2026-07-08T15:00:36.056327+00:00
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

# ADR-1: OIDC Authentication Integration - Keycloak

## Context

The vision fixes Keycloak as the OIDC provider (swappable for any OIDC-compliant IdP) and KAIROS-A-0006 fixes that Keycloak does authentication only — all authorization lives in Kairos. What remains undecided is the concrete integration: realm strategy, token validation, user provisioning, and the auth flow for each client type (Leptos GUI in the browser, `kairos` CLI, and AI agents connecting to the remote MCP endpoint per KAIROS-A-0011).

## Decision

**Single realm per deployment, local JWKS validation, JIT user provisioning, Authorization Code + PKCE for the GUI, Device Authorization Grant for CLI/agents, client credentials for service accounts.**

### Realm strategy — SUPERSEDED by KAIROS-A-0016 (2026-07-10)
> Kairos ships **no identity provider**. Any spec-compliant OIDC issuer works (`OIDC_ISSUER_URL` + `OIDC_AUDIENCE` are the entire coupling); user lifecycle arrives via SCIM 2.0 (A-0016) or JIT + manual membership. Keycloak is neither bundled nor recommended over alternatives; Dex remains dev/test tooling. Everything below this section (validation, JIT, per-client flows) stands unchanged. Tenant access remains governed by Kairos (`organization_members`), never by IdP groups.

### Token validation
- The server validates JWTs locally: fetch and cache the realm JWKS, select key by `kid`, refresh cache on unknown `kid`; validate signature, `iss`, `aud`, `exp`
- A tower middleware layer produces `AuthContext { user_id, external_id, email }` as a request extension; 401 on missing/invalid token
- Runs before tenant middleware, which then requires an `organization_members` row for the resolved tenant → 403 otherwise
- No token introspection round-trips to Keycloak on the request path — the server stays stateless (vision constraint)

### User provisioning (JIT) — amended by KAIROS-T-0197 (2026-09-25)
First authenticated request upserts `public.users` from token claims: `sub` → `external_id`, plus `email`, `name` → `display_name`. Org membership is **never** auto-granted: an org admin invites users (`organization_members` row). A user who authenticates but belongs to no org gets 403 with a "request access" error code.

> **Amendment (KAIROS-T-0197): one identity claim, and its trust boundary.**
>
> Matching on `sub` alone was not enough. A user SCIM provisioned without an
> `externalId` has an `external_id` taken from their `userName`, so it is not the
> `sub` they later present — and their first login found no row, JIT-created a
> **second** one, and left them with none of the access provisioned for them. The
> inbound SCIM join had an email fallback for exactly this reason (A-0016); this
> direction had none, and the asymmetry was the bug.
>
> The login path now falls back to matching on `users.email`, **only when the token
> asserts `email_verified` as literally true.** On adopting a row it re-keys
> `external_id` to the presented `sub`, so the fallback runs once per person and
> never again.
>
> **What Kairos trusts, stated so it can be challenged:** the issuer's
> `email_verified` claim, and nothing else. The argument that makes this safe is
> that a deployment has exactly **one** issuer (`OIDC_ISSUER_URL` is a single
> value, A-0016), and an issuer will not verify one address for two accounts — so
> within one Kairos a verified email identifies one person. **That argument does
> not survive multiple issuers**, and this fallback would have to change alongside
> any decision to support them.
>
> Failing closed is deliberate at every step. An absent claim, a `false`, a `1`, a
> string that is not `"true"`, or a malformed value all mean *not verified*, and
> the fallback does not fire — the deployment simply behaves as it did before.
> `email_verified` is parsed leniently (bool or the strings `"true"`/`"false"`)
> because a strict `Option<bool>` would reject the entire token over one IdP's
> spelling, turning an interop quirk into a total outage; the leniency is about
> the *type*, never the value.
>
> The cost of being wrong is asymmetric and that is what set the boundary: too
> strict costs a duplicate user row, too loose costs one person binding to
> another's identity.

### Flows per client
| Client | Flow | Token handling |
|---|---|---|
| Leptos GUI | Authorization Code + PKCE | Access token in memory, silent refresh via refresh token; no long-lived cookies |
| `kairos` CLI | Device Authorization Grant (`kairos login` prints URL + code, polls) | Tokens cached in `~/.config/kairos/credentials.json` (0600) with refresh |
| MCP (agents) | OAuth per MCP spec: `/mcp` advertises OAuth protected-resource metadata (RFC 9728) pointing at Keycloak; the MCP client (e.g. Claude Code) drives the browser flow | Client-managed; server sees a bearer token, validated identically to `/api` |
| Service accounts (CI, headless agents) | Client credentials grant against a Keycloak client | Mapped to a Kairos user row via `external_id` = client service-account sub |

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| Local JWKS validation (chosen) | Stateless, no per-request IdP dependency, standard | Revocation lag up to token TTL | Low | Low |
| Token introspection per request | Immediate revocation | IdP on the hot path, latency, violates stateless principle | Medium | Low |
| Server-managed cookie sessions | Simple browser story | Server-side session state, poor fit for CLI/MCP, violates stateless constraint | Medium | Medium |
| Realm per tenant (v1) | Hard IdP isolation, per-tenant SSO now | Heavy provisioning (realm per org), complicates JWKS caching and MCP resource metadata; premature | Medium | High |

## Rationale

1. **Stateless server is a vision constraint** — local JWT validation is the only pattern that honors it.
2. **Kairos owns tenant membership already** (A-0006's whitelist model); duplicating org structure into Keycloak groups would create two sources of truth.
3. **Device flow is the established pattern for CLIs and agent hosts** — no local callback server, works over SSH.
4. **The MCP OAuth story requires a remote-capable flow** — protected-resource metadata + Keycloak satisfies MCP-spec authorization without Kairos implementing an authorization server.
5. **JIT provisioning with explicit membership** keeps onboarding cheap (no user pre-creation) without any auto-access risk.

## Consequences

### Positive
- One validation code path for GUI, CLI, and MCP — a single middleware to build and test
- Keycloak is swappable for any spec-compliant OIDC provider (only issuer URL + audience are config)
- Revocation semantics are simple: short access-token TTL (default 5 min) + refresh tokens

### Negative
- Access remains valid up to TTL after revocation — TTLs are the customer IdP's configuration; docs recommend values
- ~~Keycloak becomes a hard deployment dependency~~ — superseded by KAIROS-A-0016: no bundled IdP
- Device-flow (and client-credentials) availability depends on the customer's IdP — documented per-flow requirements (A-0016)

### Neutral
- **Dev and test stacks use Dex, not Keycloak** (per Dylan at ratification, 2026-07-08): Dex is a lightweight OIDC issuer seeded from a static config file (test users, `kairos` clients, device grant enabled) with near-instant startup — used by `angreal services up` dev profile, integration tests, Playwright, and soak runs (KAIROS-A-0012). This doubles as a standing proof of the "swappable for any OIDC provider" constraint: the server is only ever configured with issuer URL + audience. Keycloak remains the production reference deployment (KAIROS-A-0013)

## Review Schedule

### Review Triggers
- ~~A customer requires their own IdP~~ — resolved by KAIROS-A-0016: every deployment brings its own IdP
- MCP spec authorization requirements change materially