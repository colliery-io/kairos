---
id: 001-service-account-principals-with
level: adr
title: "Service-account principals with native API keys"
number: 1
short_code: "KAIROS-A-0017"
created_at: 2026-07-17T22:27:41.692965+00:00
updated_at: 2026-07-17T22:30:29.542162+00:00
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

# ADR-1: Service-account principals with native API keys

## Context **[REQUIRED]**

Kairos needs **non-interactive machine access** — CI jobs, scripts, and agents
(including the day-1 skills plugin) that call `/api` and `/mcp` without a human
at a browser. A-0010 anticipated "client-credentials service accounts *where the
customer's IdP supports them*", and A-0016 reaffirmed identity is external. In
practice that path is painful: Dex has no client-credentials grant (the dev
stack resorts to a password-grant workaround), and on Google Workspace — now a
first-class target (KAIROS-T-0054/T-0056) — machine identity means service-account
JWT assertions with per-client audiences, which would additionally require the
multi-audience work (KAIROS-T-0055). Every IdP does headless auth differently,
and none of it is under Kairos's control.

Meanwhile Kairos **already mints and validates one class of native credential**:
the SCIM per-tenant bearer token (A-0016). It is org-admin-issued, stored only as
a hex SHA-256 hash in the tenant schema (`org_<slug>.scim_tokens`), carries its
tenant in the token string (`kairos_scim_<slug>_<secret>`), is validated by an
O(1) hash lookup per request, and is revocable. So a Kairos-issued, hashed,
revocable credential is not a new trust model here — it is an established one,
currently scoped to the SCIM surface.

What's missing is a **machine principal for the API/MCP surface**: a named,
org-scoped identity that holds ABAC capabilities of its own and authenticates
with a key, so automation is first-class and survives the humans who set it up.

Grounding (verified 2026-07-17):
- Human identity: `public.users` (`id`, `external_id` = OIDC sub UNIQUE, `email`,
  `display_name`); JIT-upserted in `require_auth`
  (`crates/kairos-server/src/middleware/auth.rs`), producing `AuthContext
  { user_id, external_id, email, display_name }`.
- ABAC: `org_<slug>.board_member_capabilities (board_id, user_id, capability)`
  with glob matching (`kairos-db/src/abac.rs`); org-admin bypass via
  `public.organization_members.role`. Server gate `require_capability(conn, slug,
  board_id, user, cap)` — `board_id: None` = org-admin-only.
- `/api`, `/mcp`, and `/ws` all sit behind the **same** `require_auth` →
  `require_tenant` stack (`app.rs`), consuming `AuthContext`. Anything
  `require_auth` accepts authenticates all three surfaces unchanged.
- SCIM token precedent: `scim/auth.rs` (`generate_token`, `hash_token` = SHA-256,
  `parse_token` splits `<slug>_<secret>`, `require_scim_token`), `scim/tokens.rs`
  (org-admin `manage_scim_tokens` CRUD, raw token returned once), `kairos-db/src/scim.rs`.

## Decision **[REQUIRED]**

**Kairos gains a first-class, org-scoped `service_account` principal that
authenticates with a native, opaque API key — hashed at rest and revocable —
extending the SCIM-token precedent (A-0016) from the SCIM surface to the whole
API/MCP surface. Human authentication remains external OIDC (A-0010/A-0016);
this ADR adds machine identity, it does not change how people log in.**

### The principal
- A service account is a **`public.users` row** with a new `kind` column
  (`'human'` default | `'service_account'`), a synthetic `external_id`
  (`svc:<uuid>`, which can never collide with an OIDC `sub`), a display name, and
  no interactive login. Representing it as a user row means the **entire
  downstream stack is reused unchanged**: org membership
  (`organization_members`), ABAC (`board_member_capabilities.user_id`), activity
  attribution, tenant resolution, and MCP tool auth all already key on a
  `user_id`.
- A service account belongs to **exactly one org** (one `organization_members`
  row, `role = 'member'`; it never gets org-admin). Its capabilities are ordinary
  board-scoped ABAC grants — a service account can be granted strictly less than
  a human, and only what an org admin gives it.

### The key
- Format **`kairos_sk_<slug>_<secret>`** — the `kairos_sk_` prefix distinguishes
  it from a JWT at the bearer boundary; `<slug>` carries the tenant (mirroring
  SCIM's tenant-from-token); `<secret>` is 32 bytes of OS randomness, hex.
- Stored **only as a hex SHA-256 hash** in the **tenant schema**
  (`org_<slug>.api_keys`), never in plaintext, alongside the owning
  service-account `user_id`, a display prefix (first chars, for listing), `name`,
  `created_by`, `created_at`, `expires_at` (nullable — **API keys support expiry**,
  unlike SCIM tokens), `last_used_at` (nullable), `revoked_at` (nullable). SHA-256
  (not argon2) is correct for a high-entropy random secret and keeps validation a
  single O(1) lookup with no cache — matching `scim_tokens`.
- The **raw key is returned exactly once** at creation and never again
  (`api_keys` never exposes hash or secret on read).

### Validation
- In `require_auth`, before OIDC verification, detect the `kairos_sk_` prefix.
  If present: parse `<slug>`, resolve the org (`public.organizations`), hash the
  presented key, look it up in `org_<slug>.api_keys` by `token_hash`, reject if
  expired/revoked, then build the **same `AuthContext`** for the service-account
  `user_id` and pin the tenant to `<slug>` (the key resolves its own tenant,
  exactly as SCIM does). Downstream `require_tenant` membership + ABAC apply with
  zero changes. A uniform 401 on every failure avoids tenant/key enumeration.
- No key ⇒ the existing OIDC JWKS path runs untouched. `last_used_at` is updated
  best-effort (non-blocking).

### Management
- Org-admin-only CRUD behind the normal auth→tenant stack, gated by a new
  `manage_service_accounts` capability (`board_id: None` = org-admin), mirroring
  `manage_scim_tokens`: create/list/delete service accounts; mint/list/revoke
  their keys (raw key shown once). Exposed on `/api` + the CLI (`kairos
  service-accounts …`, `kairos keys …`). ABAC grants to a service account reuse
  the existing capability-grant endpoints.

### Scope boundaries
- Service accounts are **not** deployment admins: `KAIROS_DEPLOYMENT_ADMINS`
  stays a list of human OIDC `sub`s (A-0016). A `svc:<uuid>` external_id is
  ineligible by construction.
- SCIM tokens are unchanged; the two token systems stay separate (SCIM =
  provisioning surface, no user principal; API keys = a user principal on the
  API/MCP surface).

## Alternatives Analysis **[CONDITIONAL: Complex Decision]**

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| **Native service-account principal + opaque API key (chosen)** | First-class machine identity with its own ABAC; revocable; IdP-independent; reuses users/ABAC/MCP wholesale; extends the SCIM-token precedent | Kairos validates a native credential on the request path (a DB lookup); a small `users.kind` schema addition | Low | Medium |
| Personal access token (key acts *as* the issuing human) | Least schema change; no new principal | Automation dies/needs re-issue when the person leaves or loses access; over-broad (inherits all the human's grants); poor least-privilege | Medium | Low |
| Force machine auth through each customer IdP (A-0010 original) | No native credential; identity stays 100% external | Dex has no client-credentials; Google needs SA-JWT + T-0055 multi-audience; per-IdP toil; worst onboarding | Medium | High |
| Kairos-signed JWTs for service accounts | Reuses the JWKS validation path statelessly | Kairos becomes a token *issuer* (signing keys, rotation); revocation needs a denylist; more machinery than a hashed key | Medium | Medium-High |

## Rationale **[REQUIRED]**

1. **The precedent already exists.** A-0016's SCIM token is a Kairos-issued,
   hashed, tenant-scoped, revocable bearer. This ADR generalizes that exact,
   proven pattern to a user-bearing principal on the API/MCP surface — not a new
   trust model, a wider application of an accepted one.
2. **Machine identity should be first-class and outlive people.** A named
   service account with its own least-privilege ABAC grants is safer and more
   durable than a human's personal token, and clearer to audit.
3. **Reuse beats invention.** Modeling the principal as a `public.users` row
   means membership, ABAC, activity attribution, tenant isolation, and MCP tool
   auth need no new concepts — the key resolves to a `user_id` and everything
   downstream already works.
4. **It removes an IdP dependency for the hardest case.** Headless auth is where
   BYO-OIDC is weakest (Dex can't; Google needs extra work). A native key sidesteps
   the per-IdP client-credentials matrix entirely and means T-0055 is not required
   for service accounts.
5. **Opaque + SHA-256 + revocable is the right primitive.** High-entropy random
   secret ⇒ a fast hash is sufficient; a per-request O(1) lookup gives instant
   revocation without Kairos holding signing keys — the SCIM path already proves
   this in production.

## Consequences **[REQUIRED]**

### Positive
- CI, scripts, and agents (incl. the skills plugin) get non-interactive access
  with least-privilege, per-key revocation, and clean audit — on any deployment,
  regardless of IdP.
- Works on `/api`, `/mcp`, and `/ws` with no per-surface changes (single
  `require_auth` branch point).
- T-0055 (multi-audience) is no longer needed for the service-account use case.

### Negative
- Kairos now validates a native credential on the API request path (a DB lookup),
  a deliberate carve-out from the "external identity only / stateless JWKS"
  framing of A-0010/A-0016 — bounded and mirroring the existing SCIM path.
- A small shared-schema change (`public.users.kind`) and a new per-tenant table
  (`api_keys`) with its migration for existing tenants.
- Secret hygiene surface: raw keys are shown once; hashes only at rest; uniform
  401s; keys must never be logged or echoed.

### Neutral
- A-0010/A-0016 stand: human SSO is unchanged and external. This ADR **amends**
  A-0010's "service accounts via the IdP's client-credentials" note to "native
  service-account API keys (A-0017), with IdP client-credentials still allowed
  where a customer prefers it."
- SCIM tokens and API keys coexist as two separate, similarly-shaped credentials
  for two different surfaces.
- Deployment-admin authority is untouched (human subs only).

## Review Schedule **[CONDITIONAL: Temporary Decision]**

### Review Triggers
- A need for cross-org or deployment-level service accounts (this ADR is
  deliberately single-org).
- Demand for finer key scoping than board-ABAC (e.g. per-key capability subsets
  narrower than the owning service account).
- Key-validation throughput becoming hot enough to warrant a short-TTL cache
  (currently unnecessary, matching SCIM).