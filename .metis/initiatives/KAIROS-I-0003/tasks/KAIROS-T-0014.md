---
id: m1-search-and-traverse-pipeline
level: task
title: "M1: Search and traverse pipeline"
short_code: "KAIROS-T-0014"
created_at: 2026-07-08T15:06:22.578073+00:00
updated_at: 2026-07-10T08:01:11.678947+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0012, KAIROS-T-0013]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Search and traverse pipeline

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

The unified search/traverse pipeline per A-0007 as a kairos-core service (consumed later by `POST /api/search` and the MCP `search` tool): traverse → type resolution → bounded hydration → filter → full-text → sort/paginate.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Each capability works alone: `q` (tsquery via searchable_items), `filter` (every S-0005 filter field incl. metadata glob values and date ranges), `traverse` (recursive CTE, required depth, server cap)
- [x] All composition examples from KAIROS-S-0005 pass as integration tests, including q+filter+traverse combined
- [x] Hydration bounded at ≤5 queries regardless of result size (assert query count); results grouped by type, fully typed
- [x] Soft-deleted excluded by default, `include_deleted` honored; wide fan-out pathological graph test completes within a stated time budget

## Implementation Notes

References A-0007 (decided), S-0005. This is sanctioned raw-SQL territory (A-0009) and carries the heaviest integration burden — test accordingly.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Active. Plan: `kairos-core::search` = serde request model (S-0005 shape) + pure validation (typed `SearchValidationError`; depth cap 10, limit cap 100/default 25, typed entity/task-type enums, date-range sanity) + the T-0011-style LIKE translation for metadata globs. `kairos-db::search::execute_search` = A-0007 order: traverse (recursive CTE, UNION dedup + depth bound for cycle safety, root excluded) -> q via `searchable_items` with `websearch_to_tsquery` -> metadata pre-pass id set (one unnest/GROUP BY/HAVING query) -> intersect candidate sets -> type resolution via `entity_directory` (UNION-ALL-without-deleted-filter variant when `include_deleted`) -> partition -> <=5 typed-diesel per-type hydration queries carrying structural filters + deleted_at -> in-memory sort/total/paginate, grouped by type. Query counter (`SearchStats`) instrumented; `execute_search_with_stats` for tests. Integration test on scratch DB `kairos_search_test` (shared services stay up).
- 2026-07-10: Implemented and verified. Files: `crates/kairos-core/src/search.rs` (new, model + validation + glob translation, 17 unit tests incl. the full S-0005 request example parsed verbatim and serde rejection of unknown vocabulary), `crates/kairos-db/src/search.rs` (new, `execute_search`/`execute_search_with_stats`), `crates/kairos-db/tests/search.rs` (new, one end-to-end test on scratch DB `kairos_search_test`), plus one `pub mod search;` line in each lib.rs and chrono/uuid-serde + serde_json dev-dep in kairos-core's Cargo.toml.
- 2026-07-10: Decisions of record: (1) tsquery function = `websearch_to_tsquery('english', $q)` — never errors on arbitrary user input, supports quoted phrases/OR/-negation (documented in module docs). (2) `q` matches through `searchable_items` only, so full-text never matches soft-deleted rows even with `include_deleted` (the view is the S-0004 search surface); `include_deleted` governs filter/traverse/hydration visibility, with type resolution switching to a UNION-ALL-over-base-tables variant so deleted traverse hits keep their type. (3) Traversal walks the raw edge graph (edges through soft-deleted nodes are followed; visibility enforced at resolution/hydration) and excludes the root. (4) Cycle safety = CTE `UNION` dedup of (id, depth) + strict depth bound (validated cap 10); acyclicity of parent/blocks is already enforced at write time (T-0013). (5) Mixed-type sort/total/pagination applied in memory over hydrated rows (A-0007 combined-result-set semantics); hydration stays ≤5 queries regardless.
- 2026-07-10: Evidence (commands + outputs):
  - `cargo test -p kairos-core --lib` → `ok. 71 passed; 0 failed`.
  - `cargo test -p kairos-db --test search -- --nocapture` → `ok. 1 passed; 0 failed` with `fan-out (200 tasks, traverse depth 5): 3.7ms, stats SearchStats { hydration_queries: 1, total_queries: 4 }` — well inside the asserted 5s budget; the test asserts `results.total == 200`, one task-hydration query, default-limit page of 25.
  - Criterion 1: q alone (stems + multiword websearch), every filter field alone (entity_type, task_type, board_id, column_id, team_id, is_bucket, metadata exact/glob/AND/literal-`_`, strict date ranges), traverse alone (parent descendants outbound, inbound blockers, both-direction, by short_code and by id, unknown root = typed `TraverseRootNotFound`) — all asserted in tests/search.rs.
  - Criterion 2: all five S-0005 composition examples run verbatim as JSON (adapted short codes), incl. q+filter+traverse combined ("auth" in tasks under S-0001 → exactly t1); a second combined query over the 200-task fan (q+traverse+filter, limit 100) asserted too.
  - Criterion 3: `SearchStats.hydration_queries` asserted == 4 for a 4-type traversal, == 1 for 200 same-type results, ≤5 for the all-types q; results grouped by type as fully-typed `Strategy`/`Initiative`/`Task`/`Document`/`Adr` rows.
  - Criterion 4: t4 soft-deleted after linking; excluded from filter and traverse paths by default, visible (with `deleted_at` set) under `include_deleted` on both paths; 12-deep blocks chain truncates at depth 10, depth 11 = typed `TraverseDepthOutOfRange`; limit 101 = typed `LimitOutOfRange`.
  - Self-check gate: `cargo fmt --check` clean · `cargo clippy --workspace --all-targets -- -D warnings` clean · `cargo test -p kairos-core --lib` green · `cargo test -p kairos-db --test search` green. Full `angreal test unit`/`integration` gate deliberately DEFERRED to the orchestrator (shared-services mode; services left up).