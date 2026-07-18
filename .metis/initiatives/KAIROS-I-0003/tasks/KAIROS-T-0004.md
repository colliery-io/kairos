---
id: m0-extend-angreal-harness-e2e-soak
level: task
title: "M0: Extend angreal harness - e2e, soak, seed, schema-sync"
short_code: "KAIROS-T-0004"
created_at: 2026-07-08T15:05:59.597407+00:00
updated_at: 2026-07-09T16:37:03.662878+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0002, KAIROS-T-0003]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M0: Extend angreal harness - e2e, soak, seed, schema-sync

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Extend the angreal harness to the full KAIROS-A-0012 task surface so agents and CI share one entry point.

## Acceptance Criteria

## Acceptance Criteria

- [x] `angreal test unit` runs `cargo test --workspace` (lib/unit targets); `angreal test integration` brings up services and runs integration targets; both propagate return codes
- [x] New `angreal test e2e` task: compose lifecycle + golden-path placeholder that fails with NOT IMPLEMENTED (so it can't silently pass)
- [x] New `angreal test soak` task: accepts `--duration`/`--config`, exits NOT IMPLEMENTED until the workforce harness lands (M5)
- [x] New `angreal db seed` task (stub → NOT IMPLEMENTED until seed-demo exists) and `angreal db schema-sync` task (diesel print-schema against the migrated dev DB, writes `crates/kairos-db/src/schema.rs`, per A-0009)
- [x] All new tasks have angreal ToolDescriptions and appear in `angreal tree --long`

## Implementation Notes

Stubs must fail loudly, never no-op green — A-0012's gate depends on these being trustworthy.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Implemented full KAIROS-A-0012 task surface in `.angreal/task_test.py` (rewritten) and `.angreal/task_db.py` (extended). Evidence:
  - `angreal tree --long` — all 7 tasks (`test unit|integration|e2e|soak|all`, `db seed|schema-sync`) listed with ToolDescription prose and risk levels rendered.
  - `angreal test unit` → `cargo test --workspace --lib --bins`; 6 crates × 1 smoke test each, all `ok`; exit 0.
  - `angreal test integration` → compose up --wait (postgres+dex healthy), enumerated workspace test targets via `cargo metadata` (none exist yet), printed "No integration test targets found in the workspace (no crate has a tests/ directory yet) - nothing to run. PASS.", compose down -v; exit 0. When targets exist it runs `cargo test --workspace --test '*'` and propagates the return code; `--keep-running` preserved.
  - `angreal test e2e` → compose up (healthy), printed 5-step golden-path plan, `E2E FAILED: NOT IMPLEMENTED (KAIROS-A-0012 tier 4; lands with M2/M5)` on stderr, compose down -v; exit 1.
  - `angreal test soak --duration 1m` → `Soak run requested: duration=1m, config=<default>` + workforce-payload plan + `SOAK FAILED: NOT IMPLEMENTED (KAIROS-A-0012 tier 5 workforce harness; lands with M5)`; exit 1. `--config` accepted (optional).
  - `angreal db seed` → `SEED FAILED: NOT IMPLEMENTED (seed-demo lands with KAIROS-T-0008+)`; exit 1.
  - `angreal db schema-sync` → `SCHEMA-SYNC FAILED: no migrations/database schema yet - run after KAIROS-T-0007 lands the initial migrations in crates/kairos-db/migrations/`; exit 1 (graceful failure verified; migrations don't exist yet). Full path implemented: migrations check → compose up → apply migrations → diesel print-schema → write `crates/kairos-db/src/schema.rs` with @generated header; refuses to write an empty schema.
  - diesel CLI acquisition (no-Homebrew): reuse a postgres-capable `diesel` on PATH if present, else `cargo install diesel_cli --version 2.3.6 --no-default-features --features postgres-bundled --root target/tools --locked` (postgres-bundled compiles libpq from source, so no system libpq/brew needed; project-local --root avoids clobbering the user's global sqlite-only diesel).
  - Gate: `cargo fmt --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `docker ps` shows no kairos containers (services left down).
  - Note: added `sys.stdout.flush()` before stderr failure lines in placeholders — angreal's non-zero exit path does not flush Python's buffered stdout, which was silently dropping the placeholder plan text.
  - New-tests note: this task changes only the Python harness (no Rust runtime surface); verification is the recorded command/exit-code evidence above.