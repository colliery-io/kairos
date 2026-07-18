---
id: service-account-api-keys
level: task
title: "Service-account API keys: management API (CRUD + capability)"
short_code: "KAIROS-T-0059"
created_at: 2026-07-17T22:31:34.308312+00:00
updated_at: 2026-07-18T00:55:01.130128+00:00
parent: KAIROS-I-0005
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0005
---

# Service-account API keys: management API (CRUD + capability)

## Parent Initiative

[[KAIROS-I-0005]] — implements [[KAIROS-A-0017]].

## Objective **[REQUIRED]**

The org-admin management surface for service accounts and their keys, mirroring
`crate::scim::tokens`: create/list/delete service accounts, mint/list/revoke
keys, all gated by a new `manage_service_accounts` capability. Depends on
[[KAIROS-T-0057]]; end-to-end-testable with [[KAIROS-T-0058]].

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] `manage_service_accounts` added to the capability vocabulary
      (`api/org/mod.rs::validate_capabilities`) and enforced via
      `require_capability(conn, slug, None, user, MANAGE)` (org-admin fallback).
- [ ] `POST /api/service-accounts` creates a service account: a
      `public.users` row (`kind='service_account'`, `external_id='svc:<uuid>'`,
      synthetic email/display name) + one `organization_members` row
      (`role='member'`). `GET` lists; `DELETE /{id}` removes (and its keys).
- [ ] `POST /api/service-accounts/{id}/keys` mints a key: returns the RAW
      `kairos_sk_<slug>_<secret>` exactly ONCE; stores only the hash + prefix.
      `GET .../keys` lists (prefix, name, created/expires/last_used/revoked — never
      hash/secret). `DELETE .../keys/{kid}` revokes (sets `revoked_at`).
      Optional `expires_at` accepted on mint.
- [ ] ABAC grants to a service account use the EXISTING capability-grant
      endpoints (a service account id is a valid `user_id`); no new grant surface.
- [ ] OpenAPI doc-stubs for every new route; `registered_routes_and_spec_paths_
      match_exactly` passes. Errors use the S-0005 `ApiError` envelope.
- [ ] Integration tests: non-admin is 403; admin can create SA → grant it a board
      capability → mint key → (with T-0058) that key acts on `/api` + `/mcp`;
      revoke kills it; list never leaks secrets. Full gate green.

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- Model the module on `crates/kairos-server/src/scim/tokens.rs` (router, handler
  signatures `State + Extension<AuthContext> + Extension<TenantContext> + Json`,
  `state.blocking.run(&slug, ...)` with `require_capability`).
- Reuse SA-user creation from T-0057 helpers (or add here) + `abac::grant_*` for
  the membership row. Key mint/list/revoke call `kairos-db::api_keys`.
- Response types: `ServiceAccountView`, `ApiKeyView` (no hash/secret),
  `ApiKeyCreatedResponse { …, key }` (raw, once).

### Dependencies
- [[KAIROS-T-0057]] (DB), [[KAIROS-T-0058]] (auth, for end-to-end tests).

### Risk Considerations
- Deleting a service account must cascade/clean its keys and memberships.
- Never return the hash or raw secret on any read path; the create response is
  the only place the raw key appears.

## Status Updates **[REQUIRED]**

### 2026-07-17 — Complete; full gate green

- `kairos_db::service_accounts`: `create_service_account` (users + membership,
  atomic), `list_service_accounts`, `find_service_account` (org-scoped + kind
  check), `delete_service_account` (keys + capabilities + membership + user,
  atomic).
- `crate::service_accounts::routes`: 6 endpoints — `POST/GET /api/service-accounts`,
  `DELETE /api/service-accounts/{id}`, `POST/GET /api/service-accounts/{id}/keys`,
  `DELETE .../keys/{key_id}`. Org-admin-gated via `manage_service_accounts`
  pseudo-capability (board_id=None; NOT added to the grant vocabulary — it's an
  admin-only label like `manage_scim_tokens`). Raw key returned once; list shows
  prefix only. Activity-logged. Mounted behind auth→tenant in app.rs; OpenAPI
  doc-stubs registered.
- ABAC grants reuse the existing capability endpoints (a SA id is a valid
  user_id) — no new grant surface, per plan.
- Integration test `service_account_mgmt`: admin creates SA → lists → mints key
  → **the key authenticates /api** (T-0058 integration) → list hides secret →
  revoke → 401 → double-revoke 409 → delete → gone → unknown 404; non-admin 403.

**Gate:** fmt/clippy(server all-targets)/web-lint clean; unit all crates green;
**integration all 30 targets pass** (incl. service_account_mgmt + openapi
route↔spec match accepts the 6 routes).

Unblocks [[KAIROS-T-0060]] (CLI over these endpoints).