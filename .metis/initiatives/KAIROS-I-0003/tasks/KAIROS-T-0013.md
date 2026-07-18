---
id: m1-relationship-graph-service
level: task
title: "M1: Relationship graph service"
short_code: "KAIROS-T-0013"
created_at: 2026-07-08T15:06:21.134095+00:00
updated_at: 2026-07-10T01:43:17.590840+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0009]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Relationship graph service

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Relationship graph service per A-0001: link/unlink for the five edge types with application-enforced semantics.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Five relationship types supported with type-rule validation (e.g. `parent` only along strategy→initiative→task; `supports` only from documents/ADRs; `supersedes` only ADR→ADR) — unit-tested rule matrix
- [x] Cycle prevention for `parent` and `blocks` (A blocks B blocks A rejected; parent loops rejected) — integration tests
- [x] Link/unlink write `activity_log` rows; UNIQUE(source,target,relationship) honored; both-direction queries use the S-0004 indexes

## Implementation Notes

References A-0001 (decided): relationship integrity is application logic, not DB constraints — this task IS that logic.

### As-built (2026-07-09)

- `crates/kairos-core/src/graph.rs` (new): pure `Relationship` enum, `check_link(rel, source_type, target_type)` type-rule matrix, `would_create_cycle(edges, source, target)` BFS, `Relationship::requires_acyclicity()` (`parent`/`blocks`). Unit tests cover the full 125-combination matrix exhaustively plus direct/transitive/diamond cycle cases.
- `crates/kairos-db/src/graph.rs` (new): `link_items`/`unlink_items`/`relationships_for`. UUIDs resolve to entity types via the `entity_directory` view (soft-deleted/unknown → typed `GraphError::ItemNotFound`); duplicate edge → `AlreadyLinked` (UNIQUE violation mapped); missing unlink → `NotLinked`; self-link → `SelfLink`. Activity rows: `relationship_add`/`relationship_remove` with `details = relationship:{rel}:{source_short}->{target_short}` and NULL `entity_id`/`entity_type` (per S-0004 DDL comment). `relationships_for` returns both directions hydrated (short_code/entity_type/title), ordered by relationship then edge creation.
- Matrix as enforced: `parent` = strategy→initiative, initiative→task ONLY (documents/ADRs attach via `supports`, never `parent`); `supports` = workflow item (strategy/initiative/task) as SOURCE → document/adr as TARGET ("target supports source", same orientation T-0011's `resolve_authorization_board` reads); `informs` = document/adr → strategy/initiative/task; `supersedes` = adr→adr; `blocks` = workflow↔workflow, same- or cross-type.
- **`informs` board-reference limitation (v1)**: A-0001's example "company vision informs the strategy board" targets a BOARD, but boards are not in the shared UUID item space `item_relationships` spans (no board rows in `entity_directory`). v1 therefore restricts `informs` to document/adr → workflow item; board-level informs references need a future modeling decision (e.g. a board-scoped reference table or admitting boards to the directory).
- `parent` cycle note: the type matrix alone makes parent cycles unreachable through the service (the hierarchy only descends), so the cycle check on `parent` is defense-in-depth; the integration test seeds a malformed chain directly to prove it holds.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Started (active). Plan: pure rule matrix + cycle detection in new `kairos-core::graph`; DB orchestration (`link_items`/`unlink_items`/`relationships_for`) in new `kairos-db::graph` resolving entity types via `entity_directory`; integration test `crates/kairos-db/tests/graph.rs` on its own scratch DB (`kairos_graph_test`). Matrix per A-0001/S-0004 DDL comments: `parent` strategy→initiative, initiative→task; `supports` stored source=supported workflow item (strategy/initiative/task), target=document/adr (matches T-0011 `resolve_authorization_board`); `informs` document/adr→strategy/initiative/task (v1 restriction, see note below); `supersedes` adr→adr; `blocks` workflow↔workflow (same- or cross-type).
- 2026-07-09: Implemented and verified. Evidence per criterion:
  - **Rule matrix (criterion 1)**: `cargo test -p kairos-core --lib` → `ok. 40 passed; 0 failed` (includes `graph::tests::full_matrix_exhaustive` checking all 5×5×5 = 125 combinations against the A-0001 allowed set, plus spot checks of every A-0001 example and rejection class). Integration: 12 representative rejected pairs each return `GraphError::Rule` carrying the exact `(relationship, source_type, target_type)` and persist nothing.
  - **Cycle prevention (criterion 2)**: `cargo test -p kairos-db --test graph` → `ok. 1 passed; 0 failed` (0.84s). Covers: `blocks` A→B then B→A rejected (`CycleDetected`); transitive `blocks` t1→t2→t3 then t3→t1 rejected; `blocks` diamond (two paths t1..t4) allowed; `parent` transitive loop rejected (malformed chain i2→t4→s2 seeded directly, then service-level s2→i2 refused); diamond `supports` (one document supporting three items) allowed. Core unit tests additionally cover direct/transitive/diamond/malformed-graph termination.
  - **Activity + UNIQUE + indexes (criterion 3)**: 11 allowed links wrote 11 `relationship_add` rows with `details` in the S-0004 format (e.g. `relationship:parent:ACME-S-0001->ACME-I-0001`); unlink wrote `relationship:blocks:ACME-I-0001->ACME-T-0002` as `relationship_remove` and removed the row; duplicate `parent` link returned typed `AlreadyLinked` with the edge stored exactly once; `NotLinked` on absent unlink wrote no activity row. EXPLAIN evidence (run in-test via `diesel::sql_query("EXPLAIN …")` with a manual `QueryableByName` for the `QUERY PLAN` column; `SET enable_seqscan = off` session-locally because the scratch table holds only a handful of rows, then `RESET`):
    - source direction: `Index Scan using idx_item_relationships_source on item_relationships (cost=0.15..8.17 rows=1 width=32); Index Cond: ((source_id = '…'::uuid) AND (relationship = 'parent'::text))`
    - target direction: `Index Scan using idx_item_relationships_target on item_relationships (cost=0.15..8.17 rows=1 width=32); Index Cond: ((target_id = '…'::uuid) AND (relationship = 'parent'::text))`
    - both assertions (`plan.contains("idx_item_relationships_…")`) are part of the test, so index usage is re-verified on every run.
  - **Gate self-check**: `cargo fmt --check` clean · `cargo clippy --workspace --all-targets -- -D warnings` → `Finished` with no warnings · `cargo test -p kairos-core --lib` 40 passed · `cargo test -p kairos-db --lib` 11 passed · `cargo test -p kairos-db --test graph` 1 passed. NOTE: the full `angreal test unit`/`angreal test integration` gate was deliberately NOT run here — shared-services mode (compose stack shared with a concurrent agent; those targets cycle compose). The orchestrator runs the full gate afterward. Services left UP (kairos-postgres, kairos-dex healthy).