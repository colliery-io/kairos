---
id: service-account-api-keys-auth
level: task
title: "Service-account API keys: auth branch in require_auth"
short_code: "KAIROS-T-0058"
created_at: 2026-07-17T22:31:30.819058+00:00
updated_at: 2026-07-18T00:41:43.778462+00:00
parent: KAIROS-I-0005
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0005
---

# Service-account API keys: auth branch in require_auth

## Parent Initiative

[[KAIROS-I-0005]] — implements [[KAIROS-A-0017]].

## Objective **[REQUIRED]**

Teach `require_auth` (`crates/kairos-server/src/middleware/auth.rs`) to accept a
`kairos_sk_<slug>_<secret>` API key as an alternative to an OIDC JWT: validate it
by hashed lookup, build the same `AuthContext` for the service-account principal,
and pin the tenant to the key's slug — so `/api`, `/mcp`, and `/ws` all authorize
a key identically. Depends on [[KAIROS-T-0057]].

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] `require_auth` detects the `kairos_sk_` prefix before OIDC verification.
      No prefix ⇒ the OIDC JWKS path is byte-for-byte unchanged.
- [ ] Key path: parse `<slug>_<secret>` (mirror `scim/auth.rs::parse_token`),
      resolve org via `public.organizations`, hex-SHA-256 the presented key, look
      it up in `org_<slug>.api_keys`, reject if `revoked_at` set or `expires_at`
      in the past, then build `AuthContext { user_id, external_id: svc:…, email,
      display_name }` for the owning service-account user.
- [ ] Tenant is pinned to the key's slug so downstream `require_tenant` +
      `require_capability` apply the service account's ABAC unchanged (verify a
      key with a board capability can act; without it is 403).
- [ ] Every auth failure (bad prefix parse, unknown org, no hash match, revoked,
      expired) returns a UNIFORM 401 — no key/tenant enumeration. New
      `VerifyError` variants map to 401 via the existing `From` impl.
- [ ] `last_used_at` updated best-effort (must not fail/block the request).
- [ ] Tests: unit (parse + hash + prefix detection) and integration against the
      live stack — valid key authorizes `/api` whoami + an MCP tool; expired and
      revoked keys 401; a key for org A cannot act on org B; a human JWT still
      works. Full gate green.

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- Reuse SHA-256 hashing (factor a shared helper or mirror `scim/auth.rs`).
- The one coordination point (A-0017): the key resolves its own tenant. Model on
  how `require_scim_token` resolves tenant-from-token; here we still emit
  `AuthContext` and let the normal tenant middleware run with the pinned slug.
- Keep the OIDC branch first-class; the key branch is an early alternative that
  returns the same `AuthContext` shape.

### Dependencies
- [[KAIROS-T-0057]] (schema + `find_by_hash`). Unblocks nothing else directly but
  is required for [[KAIROS-T-0059]] to be end-to-end testable.

### Risk Considerations
- Timing/enumeration: uniform 401 + constant-ish work; do the hash before any
  branch that could leak which stage failed.
- Ordering vs `require_tenant`: ensure the pinned tenant is honored and cannot be
  overridden by a mismatched Host/X-Tenant.

## Status Updates **[REQUIRED]**

### 2026-07-17 — Complete; full gate green

- New server module `service_accounts::auth`: `KEY_PREFIX="kairos_sk_"`,
  `generate_key`/`hash_key` (SHA-256)/`parse_key`/`display_prefix`/`is_api_key`,
  `ApiKeyTenant`, and `authenticate_api_key` (mirrors `require_scim_token`: org
  resolve → tenant hash lookup → validity → load SA user → best-effort
  `last_used_at`; uniform 401 `bad_key`).
- `require_auth`: `kairos_sk_` bearer takes the key path (always — invalid = 401,
  never OIDC fallthrough), builds the same `AuthContext`, inserts `ApiKeyTenant`.
  OIDC path byte-for-byte unchanged.
- `require_tenant`: prefers `ApiKeyTenant` slug over Host/X-Tenant; membership +
  ABAC still enforced downstream.
- Tests: 5 unit (parse/hash/prefix/is_api_key) + integration `api_key_auth`
  (7 assertions): tenant-from-key, key beats header, non-member SA → 403
  MEMBERSHIP_REQUIRED, revoked/expired/unknown-tenant/malformed → uniform 401.

**Gate:** fmt/clippy(server all-targets)/web-lint clean; unit all crates green;
**integration all 29 targets pass** (incl. api_key_auth; middleware/OIDC
unaffected). Uniform 401 verified; `last_used_at` best-effort.

Unblocks [[KAIROS-T-0059]] (management API can now mint keys that authenticate).