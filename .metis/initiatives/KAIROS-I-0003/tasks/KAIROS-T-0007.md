---
id: m1-public-schema-migrations-and
level: task
title: "M1: Public schema migrations and startup runner"
short_code: "KAIROS-T-0007"
created_at: 2026-07-08T15:06:09.160578+00:00
updated_at: 2026-07-09T17:27:32.290218+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0004]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Public schema migrations and startup runner

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

`migrations/public/` tree implementing the S-0004 public schema (8 tables) with `embed_migrations!`, plus the startup runner that applies pending public migrations on boot.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] From an empty database, the runner creates all 8 public tables with the exact columns/constraints in KAIROS-S-0004 (org slug CHECK, uniques); re-run is a no-op (idempotent)
- [x] Runner callable as a library function (used by tests) and on server boot; `angreal db migrate` wired to it
- [x] Integration test asserts table existence + a constraint sample (e.g. duplicate org slug rejected)

## Implementation Notes

References S-0004 (DDL is the source of truth — transcribe, don't redesign), A-0009 (embedded migrations), A-0001.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Implementation in place: `crates/kairos-db/migrations/public/2026-07-09-000000_create_public_schema/{up,down}.sql` (S-0004 public schema transcribed verbatim, 8 tables); `kairos-db/src/migrations.rs` with `embed_migrations!("migrations/public")`, `run_public_migrations(&mut PgConnection)` + `establish_migration_connection` (sync connection documented — diesel_migrations is sync, runtime pool stays async per A-0009); `kairos-server` main runs pending public migrations from DATABASE_URL before its placeholder body, fails fast on missing/unreachable URL, and gained a `migrate` subcommand; `angreal db migrate` wired to `cargo run -p kairos-server -- migrate` (single embedded-tree code path, no diesel CLI needed); schema-sync migrations-exist check now rglobs nested trees. Integration test `crates/kairos-db/tests/public_migrations.rs` uses scratch DB `kairos_public_migrations_test`. fmt + clippy -D warnings clean. Next: verify against compose postgres.
- 2026-07-09: Verification gate evidence (all against real compose postgres, no Homebrew, diesel CLI not needed):
  - AC1 (8 tables from empty, exact constraints, idempotent): fresh volume, `angreal db migrate` → `applied public schema migration: 20260709000000` / exit 0; `psql ... information_schema.tables` lists exactly organization_members, organizations, system_board_defaults, system_metadata_definitions, system_metadata_enum_options, system_template_metadata, system_templates, users (+ __diesel_schema_migrations bookkeeping); `\d organizations` shows `organizations_slug_check CHECK (slug ~ '^[a-z][a-z0-9_-]{1,62}$')` and `organizations_slug_key UNIQUE`; re-run `angreal db migrate` → `public schema migrations: up to date (no pending migrations)` / exit 0. PASS
  - AC2 (library fn + server boot + angreal wiring): `kairos_db::run_public_migrations` used by the integration test; `cargo run -p kairos-server` with DATABASE_URL → runs migrations then prints `kairos-server 0.1.0`, exit 0; without DATABASE_URL → `kairos-server: startup failed: DATABASE_URL is not set...` exit 1; unreachable port 5999 → `cannot reach database at DATABASE_URL: ... Connection refused` exit 1; `angreal db migrate` invokes `cargo run -p kairos-server -- migrate` (wiring choice: server subcommand over diesel CLI so tooling/boot/tests share the one embedded tree). PASS
  - AC3 (integration test): `angreal test integration` → discovered `kairos-db::public_migrations`, `test public_migrations_from_empty_database ... ok` (asserts 8 tables via information_schema, idempotent re-run, duplicate slug → UniqueViolation, uppercase slug → CheckViolation), services torn down with volumes (left down). PASS
  - Gate: `cargo fmt --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean (`Finished dev profile`); `angreal test unit` all green; `angreal test integration` green as above. `angreal db schema-sync` migrations-exist check now finds `migrations/public/2026-07-09-000000_create_public_schema/up.sql` via rglob.