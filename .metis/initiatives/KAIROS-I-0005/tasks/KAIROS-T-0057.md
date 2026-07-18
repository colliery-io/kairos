---
id: service-account-api-keys-db-schema
level: task
title: "Service-account API keys: DB schema, models, query module"
short_code: "KAIROS-T-0057"
created_at: 2026-07-17T22:31:27.238198+00:00
updated_at: 2026-07-18T00:27:24.660149+00:00
parent: KAIROS-I-0005
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0005
---

# Service-account API keys: DB schema, models, query module

## Parent Initiative

[[KAIROS-I-0005]] — implements [[KAIROS-A-0017]].

## Objective **[REQUIRED]**

The data layer for service-account API keys: (1) a `kind` discriminator on
`public.users`, and (2) a per-tenant `api_keys` table, with diesel models and a
query module. Mirrors the SCIM-token data layer (`kairos-db/src/scim.rs`,
`migrations/tenant/2026-07-10-000000_scim_tokens`).

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] Public migration adds `public.users.kind TEXT NOT NULL DEFAULT 'human'`
      (existing rows → 'human'); down drops it. `run_public_migrations` applies it.
- [ ] Tenant migration `migrations/tenant/NNNN_api_keys/up.sql` creates
      `api_keys` (unqualified DDL, runs under pinned search_path): `id UUID PK
      default gen_random_uuid()`, `user_id UUID NOT NULL` (the service account,
      references `public.users`, no cross-schema FK — matches
      `board_member_capabilities`), `name TEXT NOT NULL`, `token_hash TEXT NOT
      NULL UNIQUE` (hex SHA-256), `prefix TEXT NOT NULL` (display, e.g.
      `kairos_sk_acme_ab12…`), `created_by UUID`, `created_at TIMESTAMPTZ default
      now()`, `expires_at TIMESTAMPTZ NULL`, `last_used_at TIMESTAMPTZ NULL`,
      `revoked_at TIMESTAMPTZ NULL`; index on `token_hash`. Down drops the table.
- [ ] Migration applies to NEW tenants via `provision_tenant` and to EXISTING via
      `migrate_all_tenants` (verified by the seed-demo / provisioning tests).
- [ ] Diesel `schema.rs` updated (`api_keys` table + `users.kind` column);
      models `ApiKey`, `NewApiKey` (+ a `UserKind`/string handling) in
      `kairos-db/src/models`.
- [ ] Query module `kairos-db/src/api_keys.rs` (mirror `scim.rs`):
      `create_key`, `find_by_hash` (returns the key + owner user_id, respecting
      expiry/revocation semantics left to the caller or via a `valid` filter),
      `list_keys` (by service account, no hash/secret exposed at the type level),
      `revoke_key`, `touch_last_used`. Plus service-account helpers as needed
      (`create_service_account_user`, list/delete) or defer those to T-0059.
- [ ] `cargo test -p kairos-db` green incl. a models round-trip / migration test
      for `api_keys`; full `angreal test integration` green (schema changes don't
      break existing tenant provisioning or isolation tests).

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- Public migration mirrors existing `migrations/public/*`; tenant migration
  mirrors `2026-07-10-000000_scim_tokens` (unqualified names).
- `token_hash` = hex SHA-256 (reuse the SCIM hashing approach in T-0058, not
  here). Store `prefix` for listing (never the secret).
- No DB-level FK from tenant `api_keys.user_id` to `public.users` (cross-schema);
  integrity enforced in code, consistent with `board_member_capabilities`.
- Keep expiry/revocation as columns; the "is this key currently valid" decision
  lives in the auth path (T-0058) so errors stay uniform.

### Dependencies
- Foundation for [[KAIROS-T-0058]] (auth), [[KAIROS-T-0059]] (management API).

### Risk Considerations
- Backfilling `users.kind` on a large table: a defaulted NOT NULL add is cheap in
  PG (metadata-only for the default); fine.
- Tenant migration must be idempotent across the existing-tenant fleet; verify
  `migrate_all_tenants` picks it up.

## Status Updates **[REQUIRED]**

### 2026-07-17 — Complete; full gate green

All ACs met:
- Public migration `2026-07-17-000000_users_kind` adds `users.kind TEXT NOT NULL
  DEFAULT 'human'` + CHECK; down drops it.
- Tenant migration `2026-07-17-000000_api_keys` creates `api_keys` (id, user_id,
  name, token_hash UNIQUE, prefix, created_by, created_at, expires_at,
  last_used_at, revoked_at + user_id index).
- `schema.rs`: `users.kind` + `api_keys` table. Models: `User.kind` +
  `is_service_account()`, `NewServiceAccountUser`, `USER_KIND_*` consts (NewUser
  left unchanged → humans use the DB default, no churn to ~15 call sites).
- Query module `kairos-db/src/api_keys.rs`: `ApiKey` (+ `is_valid_at`),
  `NewApiKey`, `create_key`/`find_by_hash`/`find_key`/`list_keys`/`revoke_key`/
  `touch_last_used`.
- New `kairos-db::api_keys` integration test (round-trip + expiry); updated
  `tenant_provisioning` EXPECTED_TABLES (23) and its upgrade-path simulation to
  drop the now-newest `api_keys` migration.

**Gate:** fmt/clippy(workspace)/web-lint clean; unit all crates green;
**integration all 28 targets pass** (incl. api_keys), clean run. Migrations
verified to apply to new tenants (provision) and existing (fleet migrate).

Unblocks [[KAIROS-T-0058]] (auth branch).