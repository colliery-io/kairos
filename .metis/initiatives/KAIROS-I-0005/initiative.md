---
id: service-account-api-keys
level: initiative
title: "Service-account API keys"
short_code: "KAIROS-I-0005"
created_at: 2026-07-17T22:30:34.581823+00:00
updated_at: 2026-07-18T14:05:39.564317+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: M
initiative_id: service-account-api-keys
---

# Service-account API keys Initiative

Implements **[[KAIROS-A-0017]]** — first-class, org-scoped service-account
principals authenticating with native, opaque, hashed, revocable API keys on the
`/api` + `/mcp` + `/ws` surfaces. Extends the SCIM-token precedent (A-0016).

## Context **[REQUIRED]**

Non-interactive machine access (CI, scripts, agents, the skills plugin) has no
clean path today: Dex has no client-credentials grant, Google needs SA-JWT +
multi-audience (T-0055), and every IdP differs. A-0017 decided (approved by Dylan
2026-07-17) to add a native machine principal + API key, mirroring the existing
SCIM per-tenant hashed-token mechanism. Humans keep logging in via external OIDC.

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- A `service_account` principal (a `public.users` row, `kind='service_account'`)
  that is org-scoped, holds ordinary board ABAC grants, and never gets org-admin.
- An opaque API key `kairos_sk_<slug>_<secret>`, SHA-256-hashed in
  `org_<slug>.api_keys`, with expiry + revocation; raw key shown once.
- Key authentication in `require_auth` producing the same `AuthContext`, so
  `/api`, `/mcp`, `/ws` all work unchanged.
- Org-admin management surface (API + CLI) behind `manage_service_accounts`.

**Non-Goals:**
- Cross-org / deployment-level service accounts (single-org only per A-0017).
- Per-key capability subsets narrower than the owning account's ABAC grants.
- Changing human OIDC login, SCIM tokens, or deployment-admin authority.
- A validation cache (unnecessary; SHA-256 O(1) lookup matches SCIM).

## Requirements

### System Requirements
- **Functional**
  - REQ-001: Create/list/delete a service account (org-admin only), represented
    as a `public.users` row + one `organization_members` row (`role=member`).
  - REQ-002: Mint/list/revoke API keys for a service account; the raw key is
    returned exactly once; list shows a display prefix, never the hash/secret.
  - REQ-003: `require_auth` accepts `kairos_sk_<slug>_<secret>`, validates by
    SHA-256 hash lookup in `org_<slug>.api_keys`, rejects expired/revoked, builds
    `AuthContext` for the service-account user, and pins the tenant to `<slug>`.
  - REQ-004: A key authorizes `/api`, `/mcp`, `/ws` identically, subject to the
    service account's board ABAC grants (reuse `require_capability`).
  - REQ-005: ABAC grants to a service account use the existing capability-grant
    endpoints (no new grant surface).
- **Non-Functional**
  - NFR-001 (security): keys stored only as hex SHA-256; raw key never persisted
    or logged; uniform 401 on every auth failure (no key/tenant enumeration);
    service accounts ineligible for deployment-admin and org-admin.
  - NFR-002 (perf): validation is a single O(1) hash lookup per request, no cache
    (matches `scim_tokens`); `last_used_at` update is best-effort/non-blocking.
  - NFR-003 (compat): no key ⇒ the OIDC JWKS path is byte-for-byte unchanged.

## Detailed Design **[REQUIRED]**

Full rationale + alternatives in [[KAIROS-A-0017]]. Key mechanics:

- **Schema (T-1):** `public.users` gains `kind TEXT NOT NULL DEFAULT 'human'`
  (public migration; existing rows → 'human'). New per-tenant table
  `org_<slug>.api_keys` (tenant migration; applies to new tenants via
  `provision_tenant`, existing via `migrate_all_tenants`): `id`, `user_id`
  (→ public.users, the SA), `name`, `token_hash` UNIQUE (hex SHA-256),
  `prefix` (display), `created_by`, `created_at`, `expires_at` NULL,
  `last_used_at` NULL, `revoked_at` NULL. Diesel models + `api_keys.rs` query
  module (mirror `kairos-db/src/scim.rs`).
- **Auth (T-2):** branch in `require_auth` (`middleware/auth.rs`) before OIDC
  `verify`: prefix-detect `kairos_sk_`, `parse_token` → (slug, secret) (mirror
  `scim/auth.rs`), resolve org, hash + lookup in the tenant `api_keys`, check
  expiry/revocation, build `AuthContext { user_id, external_id: "svc:…", … }`,
  pin tenant to slug for `require_tenant`. New `VerifyError` variants; uniform
  401. This is the one coordination point (key resolves its own tenant) — model
  it on how SCIM resolves tenant-from-token.
- **Management API (T-3):** `crate::api` module mirroring `scim/tokens.rs`:
  `POST/GET/DELETE /api/service-accounts`, `POST/GET/DELETE
  /api/service-accounts/{id}/keys`, gated by `require_capability(conn, slug,
  None, user, "manage_service_accounts")`. Add the capability to the vocabulary
  (`api/org/mod.rs`). OpenAPI doc-stubs. Raw key in the create response only.
- **CLI (T-4):** `kairos service-accounts create|list|delete` and `kairos keys
  create|list|revoke` (new `commands/service_accounts.rs` + `keys.rs`), thin
  veneers over new `kairos-client` methods, following `commands/members.rs`.
- **Docs (T-5):** a "Service accounts & API keys" guide (create SA → grant board
  capabilities → mint key → use with curl/CLI/MCP), security notes, and a note in
  the skills-plugin bootstrap that agents can use a key instead of interactive
  OAuth.

## Alternatives Considered **[REQUIRED]**

Decided in [[KAIROS-A-0017]]: rejected personal-access-tokens (poor
least-privilege, dies with the person), IdP client-credentials (per-IdP toil,
Dex can't, Google needs T-0055), and Kairos-signed JWTs (makes Kairos a token
issuer; weaker revocation). Chosen: native SA principal + opaque hashed key.

## Implementation Plan **[REQUIRED]**

Five tasks, sequential (each through the full gate: fmt/clippy/web-lint + unit +
integration):

1. **T-1 DB** — `users.kind` + `api_keys` migrations, diesel models, query module.
2. **T-2 Auth** — API-key branch in `require_auth`; validation + AuthContext +
   tenant pin; unit + integration tests (valid/expired/revoked/wrong-tenant).
3. **T-3 Management API** — service-account + key CRUD, `manage_service_accounts`
   capability, org-admin gating, OpenAPI, once-only raw key.
4. **T-4 CLI** — `service-accounts` + `keys` command groups + client methods.
5. **T-5 Docs** — service-account guide + security notes + skills-plugin note.

Exit criteria: an org admin can create a service account, grant it board
capabilities, mint a key, and that key drives `/api` and `/mcp` under ABAC;
revocation and expiry take effect; the OIDC path is unchanged; full gate green.