---
id: m1-tenant-migrations-and
level: task
title: "M1: Tenant migrations and provisioning service"
short_code: "KAIROS-T-0008"
created_at: 2026-07-08T15:06:10.094280+00:00
updated_at: 2026-07-09T22:48:39.305855+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0007]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Tenant migrations and provisioning service

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

`migrations/tenant/` tree (22 tables + 2 views + 5 sequences per S-0004) and the tenant provisioning service: create org + `org_{slug}` schema + run tenant migrations + seed system defaults.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Provisioning creates the schema with every S-0004 tenant table, view, index, and sequence; provisioning an existing slug fails cleanly with a typed error
- [x] Defaults seeded on provision: 4 board default configs (A-0002), system templates and metadata definitions copied from public (A-0003)
- [x] Fleet operation re-runs pending tenant migrations across all schemas (integration test with 3 tenants); `angreal db create-tenant|drop-tenant|migrate-tenants|list-tenants` wired
- [x] Soft-delete of a tenant (drop-tenant) removes schema + org row behind an explicit confirmation flag

## Implementation Notes

References S-0004, A-0001 (app-level provisioning), A-0002/A-0003 (defaults).

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Started (active). Read S-0004, A-0001/2/3, T-0007 code. Plan: (1) `migrations/tenant/` tree transcribed from S-0004 (DDL unqualified, run with search_path pinned to the tenant schema so `__diesel_schema_migrations` lands per-schema); (2) `kairos-db::tenant` module with `provision_tenant` / `drop_tenant` / `migrate_all_tenants` / `list_tenants`; (3) server subcommands create-tenant/drop-tenant/migrate-tenants/list-tenants following the T-0007 migrate-subcommand pattern; (4) rewire `.angreal/task_db.py` to those subcommands; (5) integration test `tenant_provisioning.rs` on a scratch DB.
- 2026-07-09: Recorded interpretations: (a) S-0004's summary header says "22 tables + 2 views" but the spec DDL and its own summary table enumerate exactly 21 tenant tables + 2 views + 5 sequences — the header count is off by one; tests assert the exact 21-table set from the DDL. (b) Delivery boards are per-team (`boards.team_id` set for delivery boards, A-0002/S-0004), so provisioning creates the strategy board, one initiative board, and the ADR board; delivery boards are created later when teams are created. All FOUR `system_board_defaults` rows (incl. delivery) are seeded so team creation can use them. (c) T-0007's public up.sql seeded no system_* rows, so provisioning seeds `system_board_defaults` (4 rows), the 6 A-0003 system templates, 4 system metadata definitions (+ enum options), and template-metadata associations idempotently (ON CONFLICT DO NOTHING keyed on slug/board_level) before creating the tenant. Template starter markdown content is not specified anywhere — minimal sensible markdown skeletons used. (d) "forward-only" = adjacent forward transitions only. (e) S-0004 orders `documents` (FK -> templates) before `templates`; the migration creates the template/metadata section first — pure ordering change, DDL text unchanged. (f) Transactionality: provision_tenant runs in ONE outer transaction (org insert + CREATE SCHEMA + tenant migrations via savepoints + seeding) with `SET LOCAL search_path`, so failure leaves no partial state.
- 2026-07-09: Implemented. New: `crates/kairos-db/migrations/tenant/2026-07-09-000000_create_tenant_schema/{up,down}.sql` (S-0004 tenant DDL, unqualified names), `crates/kairos-db/src/tenant.rs` (`provision_tenant`, `drop_tenant(confirm)`, `migrate_all_tenants`, `list_tenants`, `seed_system_defaults`, typed `TenantError`), `TENANT_MIGRATIONS` in `migrations.rs`, server subcommands `create-tenant|drop-tenant|migrate-tenants|list-tenants` in `crates/kairos-server/src/main.rs`, rewired `.angreal/task_db.py` tasks to those subcommands (old psql/`02_tenant_template` plumbing removed; drop-tenant is risk_level=destructive and passes --confirm through), and integration test `crates/kairos-db/tests/tenant_provisioning.rs`.
- 2026-07-09: EVIDENCE — acceptance criteria demonstrated against real compose postgres:
  - AC1+AC2+AC3+AC4: `angreal test integration` → `test tenant_provisioning_lifecycle ... ok` and `test public_migrations_from_empty_database ... ok` ("test result: ok. 1 passed" for each of the 2 targets). The lifecycle test asserts: exactly the 21 S-0004 tenant tables + views [entity_directory, searchable_items] + the 5 seq_*_code sequences + all 21 named idx_* indexes in org_acme; re-provisioning `acme` → `TenantError::AlreadyExists("acme")` with org row count still 1 (no partial state); invalid slug → `TenantError::InvalidSlug`; 4 system_board_defaults rows; strategy/initiatives/adrs boards with columns+transitions exactly matching A-0002 defaults and NO delivery board; 6 templates/4 metadata definitions/18 enum options/9 template-metadata rows copied per A-0003; fleet migration across 3 tenants (acme, globex, widgets) all "up to date" with marker row surviving; drop-tenant without confirm → `TenantError::ConfirmationRequired` (nothing removed), with confirm → schema+org row gone, acme untouched; double drop → `TenantError::NotFound`.
  - CLI wiring (manual, real DB): `angreal db create-tenant --slug acme --name "Acme Inc"` → "provisioned tenant 'acme' (schema org_acme): 1 tenant migration(s) applied, boards created: strategy, initiatives, adrs, 6 template(s) and 4 metadata definition(s) copied"; duplicate create → exit=1 `tenant "acme" already exists`; `angreal db list-tenants` → row `acme  Acme Inc  org_acme`; `angreal db migrate-tenants` → "org_acme: up to date ... complete across 1 tenant(s)"; `angreal db drop-tenant --slug acme` → exit=1 "refusing to drop ... requires explicit confirmation"; with `--confirm` → exit=0 "dropped tenant 'acme'", then psql counts for org_acme namespace and organizations row both 0.
  - GATE: `cargo fmt --check` → PASS (no output). `cargo clippy --workspace --all-targets -- -D warnings` → "Finished `dev` profile" (clean). `angreal test unit` → all targets ok (kairos-db 4 passed incl. new slug/transition-parse tests; kairos-server 4 passed incl. new flag-parsing tests). `angreal test integration` → both targets green, services torn down afterwards (docker ps shows 0 kairos containers; services left down).