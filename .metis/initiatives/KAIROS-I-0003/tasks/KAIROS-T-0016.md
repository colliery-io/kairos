---
id: m1-tenant-isolation-negative-test
level: task
title: "M1: Tenant isolation negative test suite"
short_code: "KAIROS-T-0016"
created_at: 2026-07-08T15:06:24.591187+00:00
updated_at: 2026-07-10T08:47:54.996486+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0011, KAIROS-T-0014]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Tenant isolation negative test suite

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Adversarial tenant-isolation test suite — the standing proof of the product's hardest promise (A-0001), run in every integration pass.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Provisions ≥2 tenants whose items share identical short codes; asserts zero cross-tenant visibility through every core service: CRUD reads, search (q/filter/traverse), relationship queries, history, metadata, capability checks — `cross_read_visibility_sweep` + `short_code_collision`
- [x] Cross-tenant write attempts fail; nothing in tenant B changes when acting in tenant A (asserted by full-table checksums or row counts) — `cross_write_battery` (8 attacks, per-table md5 fingerprints of BOTH tenants unchanged)
- [x] Pool-reuse stress: interleaved operations across tenants on the shared pool show no search_path leakage under concurrency — `pool_reuse_stress` (60 ops, 10 barrier-synced rounds, 2-conn pool)
- [x] Suite is part of `angreal test integration` and identifiable in output (named module), so CI failure attribution is instant — file `crates/kairos-db/tests/isolation.rs`, header names it the standing A-0001 proof; four named test fns

## Implementation Notes

References A-0001 (isolation boundary is the schema), A-0012 (negative tests mandated). Write these tests as an attacker, not a QA engineer.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-10: Transitioned to active. Read A-0001 (schema-per-tenant isolation boundary), A-0012 (negative tests mandated). Studied attack surfaces: pool.rs (search_path pinning + reset-on-return), items.rs (ItemError::ItemNotFound), graph.rs (GraphError::ItemNotFound), abac.rs (check_capability per-schema), search.rs (execute_search q/filter/traverse, TraverseRootNotFound), retention.rs (sweep_tenant pins to org schema), boards.rs (transition_* → BoardError::ItemNotFound). Scratch-DB pattern from write_path.rs/models_roundtrip.rs. SHARED-SERVICES MODE: writing crates/kairos-db/tests/isolation.rs; unique scratch DBs per test (kairos_isolation_*_test), parallel-safe. Plan: cross_read_visibility_sweep, cross_write_battery (full-table md5 checksums), pool_reuse_stress (async, ≥50 ops / ≥8 rounds), short_code_collision. acme/zenith seeded via real services (secret-acme/secret-zenith markers) plus direct-inserted identical-short-code collision pair.
- 2026-07-10: COMPLETE. Delivered `crates/kairos-db/tests/isolation.rs` (sole product file; no src/ changed). Four adversarial tests, all green. NO real leak found — the schema-per-tenant boundary (A-0001) held on every surface; suite is fully green, not stop-the-line.
  - SELF-CHECK evidence (run against shared compose Postgres; a Docker outage mid-run caused a transient connection-refused failure, re-run after orchestrator restored services):
    - `cargo fmt --check -p kairos-db` → clean
    - `cargo clippy --workspace --all-targets -- -D warnings` → clean (Finished, 0 warnings)
    - `cargo test -p kairos-db --test isolation` → `test result: ok. 4 passed; 0 failed`
    - stdout markers: `cross_write_battery: 8 cross-tenant write attempts, all repelled` · `pool_reuse_stress: 60 operations across 10 rounds, no leakage`
  - What each test proves (all acting as tenant `acme` against tenant `zenith` ids/codes unless noted):
    - `cross_read_visibility_sweep`: zenith invisible via entity_directory get-by-short-code, item_history, item_metadata, relationships_for, check_capability (a grant on zenith's board conveys nothing in acme), execute_search q/filter/traverse (traverse from a zenith root → typed TraverseRootNotFound), and retention (discard-mode sweep of acme pruned acme history but zenith's item_history fingerprint was byte-identical before/after). Positive controls confirm acme sees its own data.
    - `cross_write_battery`: 8 write attempts at zenith UUIDs (update_item_content ×3, transition_task, link_items, grant_capability, soft_delete_item ×2) each failed typed (ItemError::ItemNotFound / BoardError::ItemNotFound / GraphError::ItemNotFound / AbacError::Database FK-reject); per-table md5 fingerprints across 10 tables of BOTH tenants identical before/after.
    - `pool_reuse_stress`: one 2-connection TenantPool, 10 barrier-synchronized rounds × 2 tenants × (create+search+read) = 60 ops; every searchable/read row carried the pinned tenant's marker, never the other's — no search_path leakage under concurrency.
    - `short_code_collision`: byte-identical `SHARED-T-0001` in both tenants (distinct UUIDs) resolves only within its own tenant via entity_directory and search traverse-root resolution.
  - NOTE (deferred full gate): per SHARED-SERVICES lane I ran only `cargo test -p kairos-db --test isolation` (not `angreal test integration|e2e`); the orchestrator runs the full gate afterward. Services left UP.