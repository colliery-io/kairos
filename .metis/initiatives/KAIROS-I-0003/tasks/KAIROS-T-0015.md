---
id: m1-retention-sweeper
level: task
title: "M1: Retention sweeper"
short_code: "KAIROS-T-0015"
created_at: 2026-07-08T15:06:23.452335+00:00
updated_at: 2026-07-10T08:01:48.422999+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0012]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Retention sweeper

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Retention sweeper per the A-0004 amendment: scheduled in-process task enforcing hot window → monthly boundary compaction (keep latest-N) → NDJSON offload → prune, plus activity_log retention.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Tiering proven with injected-clock tests: hot-window rows untouched; past-window item_history thinned to first+last per item per month; latest-5 per item always retained; pruned rows byte-identical in the NDJSON archive
- [x] `KAIROS_RETENTION_MODE=archive` with no `KAIROS_ARCHIVE_TARGET` → nothing deleted, warning logged and metered; `discard` prunes without archive; `off` disables
- [x] Filesystem archive target implemented; S3-compatible target behind the same trait (implementation may stub with a clear NOT IMPLEMENTED error if out of scope — state which in Status Updates)
- [x] activity_log rows past `KAIROS_ACTIVITY_RETENTION_DAYS` archived-then-deleted; every sweep writes a `retention_sweep` activity row with counts and exports Prometheus counters (counters exposed on SweepReport; Prometheus wiring is the documented M2 seam per A-0013)

## Implementation Notes

References A-0004 (decided, retention section), A-0013 (env config, metrics). Sweeper iterates tenant schemas via the fleet pattern from T-0008.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Started (active). Plan: pure planner + env config in `kairos-core/src/retention.rs` (new dep: chrono); `sweep_tenant`/`sweep_all_tenants`/`spawn_retention_loop` in `kairos-db/src/retention.rs` (new deps: serde, serde_json, tracing; chrono/uuid serde features); integration test `kairos-db/tests/retention.rs` on scratch DB `kairos_retention_test`. Interpretations: `activity_log.action` has no CHECK constraint and `actor_id` has no FK (DDL comment-enforced), so the sweep row is inserted with literal `'retention_sweep'` and the nil UUID as system actor — no change to the `ActivityAction` enum (out of lane). S3 target parses in config but archiving returns a typed `ArchiveTargetNotImplemented` error (v1 ships filesystem only, no S3 SDK dep). `mode=off` is a full no-op (no retention_sweep row). Server wiring of `spawn_retention_loop` + Prometheus counter export are M2 (seam documented on SweepReport).
- 2026-07-09: Implemented and verified. Shipped: `kairos-core/src/retention.rs` (RetentionConfig + env parsing via injected lookup — `KAIROS_HISTORY_HOT_DAYS`/`KAIROS_HISTORY_KEEP_LATEST`/`KAIROS_ACTIVITY_RETENTION_DAYS`/`KAIROS_ARCHIVE_TARGET`/`KAIROS_RETENTION_MODE`; pure `plan_history_compaction` planner), `kairos-db/src/retention.rs` (`sweep_tenant`, `sweep_all_tenants` fleet iteration per T-0008 pattern, `spawn_retention_loop` tokio scheduler with injected tick body; NDJSON offload to `{target}/{tenant}/{table}/{timestamp}.ndjson` with fsync-before-delete; deletes + `retention_sweep` audit row in one transaction), `kairos-db/tests/retention.rs` (scratch DB `kairos_retention_test`). **S3 target: NOT IMPLEMENTED in v1** — parsed/recognized, sweeps return typed `RetentionError::ArchiveTargetNotImplemented`; filesystem target is fully implemented. Evidence (recorded outputs):
  - `rustfmt --edition 2024 --check` on all 5 lane files → clean ("FMT CLEAN: all T-0015 lane files"). Workspace `cargo fmt --check` currently diffs ONLY in T-0014's in-flight `search` files (out of lane).
  - `cargo clippy --workspace --all-targets -- -D warnings` → `Finished 'dev' profile ... in 14.53s`, zero warnings.
  - `cargo test -p kairos-core --lib` → `ok. 71 passed; 0 failed` (16 new retention tests: month boundaries incl. Jan-31/Feb-1 straddle and same-month-different-year, latest-N overriding compaction, hot-cutoff inclusive boundary ±1s, empty history, single-version items, ≤N-version items, multi-item independence + sorted output, config defaults/overrides/typed errors, mode + target parsing).
  - `cargo test -p kairos-db --test retention` → `ok. 2 passed; 0 failed` (lifecycle: hot rows untouched; A thinned Jan/Feb to first+last with v9 guarded by latest-5; archive+no-target pruned nothing with warnings=1 per table and audit row written; S3 → typed error, nothing changed; filesystem sweep archived 5 history + 3 activity rows, NDJSON parsed and field-by-field identical to the pruned rows incl. timestamps; old activity rows archived-then-deleted, recent kept; idempotent re-run pruned 0; `off` full no-op incl. fleet; `sweep_all_tenants` discard over acme+beta pruned beta v2/v3 with no files; `retention_sweep` details strings exact-matched. Plus scheduler loop manual-tick test).
  - Full gate (`angreal test unit`/`integration`, workspace fmt) deferred to the orchestrator per shared-services mode; compose services left UP.