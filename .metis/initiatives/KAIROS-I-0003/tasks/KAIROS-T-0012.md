---
id: m1-short-codes-optimistic
level: task
title: "M1: Short codes, optimistic concurrency, item history"
short_code: "KAIROS-T-0012"
created_at: 2026-07-08T15:06:20.149078+00:00
updated_at: 2026-07-10T01:14:20.695052+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0009]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Short codes, optimistic concurrency, item history

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Write-path services per A-0004: short-code generation, optimistic concurrency with item_history snapshots, and soft-delete cascade.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Short codes generated from per-type sequences as `{PREFIX}-{TYPE}-{NNNN}`; unique within tenant; concurrent-create test shows no gaps-panic or dupes
- [x] Content update: version check → write → history snapshot in one transaction; stale version returns typed conflict carrying current version + content; concurrent-writer integration test produces exactly one success and one conflict
- [x] Rollback helper copies a historical snapshot forward as a new version
- [x] Soft-delete cascades via `parent` edges (strategy→initiatives→tasks integration test); soft-deleted items excluded from queries and views; cascade logged to activity_log

## Implementation Notes

References A-0004 (decided, incl. retention amendment — the sweeper itself is KAIROS-T-0015), A-0001 (cascade semantics).

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Active. Read A-0004/A-0001/A-0009, S-0004 short-code section, existing core/db pattern (boards/abac). Plan: kairos-core `short_code` (ItemType, format, default prefix) + `items` (version-check decision, cascade set over loaded parent edges); kairos-db `items` service module (next_short_code via seq_* nextval, create_* services with v1 history baseline + activity 'create', update_item_content with atomic version-checked UPDATE + history snapshot, rollback_item through the same path, soft_delete_item cascading over parent edges computed in core); integration test crates/kairos-db/tests/write_path.rs against compose Postgres. Interpretations to record: (a) short-code PREFIX default = upper-cased tenant slug sanitized to [A-Z0-9], derived from current_schema() (org_{slug}); per-org override column is NOT in the DDL — future API-layer setting. (b) v1 baseline snapshot written to item_history at create time so rollback-to-any-version is coherent (A-0004 snapshots every edit; the baseline makes v1 restorable too). (c) cascade descendant set computed in kairos-core as a pure BFS over loaded `parent` edges (one query loads the tenant's parent edges) rather than a recursive CTE — keeps the decision testable in core per A-0009 layering.
- 2026-07-09: Implemented. New files: `crates/kairos-core/src/short_code.rs` (ItemType S/I/T/D/A, `format_short_code` zero-pad-4 growing past 9999, `default_prefix`), `crates/kairos-core/src/items.rs` (`check_version` pure mirror of the atomic SQL check, `cascade_descendants` BFS over parent edges — cycle-safe, dedups diamonds), `crates/kairos-db/src/items.rs` (next_short_code via seq_*_code nextval + prefix from current_schema(); create_strategy/initiative/task/document/adr each in one transaction with version=1, v1 history baseline, activity 'create', column default = board's first column; create_document copies template content and stamps item_metadata from template_metadata defaults per A-0003; update_item_content = single atomic `UPDATE … SET version=version+1 … WHERE id AND version=expected AND deleted_at IS NULL` + history snapshot, zero rows → typed VersionConflict{current_version,current_title,current_content}; rollback_item copies a snapshot forward as v(N+1) through the same path; soft_delete_item cascades over parent edges via core, one 'delete' activity row `short_code:… cascade:N descendants:…`), `crates/kairos-db/tests/write_path.rs` (integration: 8-thread concurrent creates → distinct sequential codes; 2-thread same-version writers → exactly one success + one VersionConflict carrying winner content; history v1..vN exact; rollback → v4 with v1 content; template stamping; strategy→initiative→task+document cascade excluded from searchable_items/entity_directory).
- 2026-07-09: VERIFICATION GATE (KAIROS-A-0012), all recorded from real runs:
  - `cargo fmt --check` → exit 0. `cargo clippy --workspace --all-targets -- -D warnings` → exit 0.
  - `angreal test unit` → all workspace lib/bin targets pass; kairos-core now `32 passed; 0 failed` including the 8 new short_code/items tests.
  - `angreal test integration` → ALL SIX targets green: `kairos-db::abac ok, board_rules ok, models_roundtrip ok (2 tests), public_migrations ok, tenant_provisioning ok, write_path ok (write_path_lifecycle … ok, 0.81s)`; compose stack torn down by the task (`Container kairos-postgres Removed`), confirmed no kairos containers left via `docker ps`.
  - Criterion 1 (short codes): create services returned ACME-S-0001/ACME-I-0001/ACME-T-0001/ACME-A-0001/ACME-D-0001 on tenant slug `acme`; 8 concurrent create_task threads (std::sync::Barrier, one connection each) produced 8 distinct codes with contiguous sequence numbers 2..=9 — asserted in write_path_lifecycle, passed.
  - Criterion 2 (optimistic concurrency): two barrier-synchronized writers both submitting expected_version=1 → asserted exactly (1 success at v2, 1 VersionConflict{expected 1, current 2, current_content == winner's content}); stale single-threaded writer also got typed conflict carrying v3 content — passed.
  - Criterion 3 (rollback): rollback_item(…, to_version=1) returned 4; live row = v1 title/content at version 4; v4 snapshot appended; rollback to missing version 99 → typed HistoryNotFound — passed.
  - Criterion 4 (cascade): soft_delete_item on the strategy root cascaded to initiative, task, and a document child via parent edges; all four ids afterwards count 0 in BOTH searchable_items and entity_directory (1 before); unrelated items unaffected; single activity row `short_code:ACME-S-0001 cascade:3 descendants:ACME-D-0002,ACME-I-0001,ACME-T-0001`; editing a soft-deleted item → ItemNotFound — passed.
  - Recorded interpretations: prefix default = upper-cased tenant slug sanitized to [A-Z0-9], derived from current_schema() (per-org override = future API-layer setting, no DDL column added); v1 baseline history snapshot at create; cascade computed in kairos-core over loaded parent edges instead of a recursive CTE (documented in module docs).