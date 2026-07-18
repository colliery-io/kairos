---
id: m1-board-rules-engine-and-defaults
level: task
title: "M1: Board rules engine and defaults seeding"
short_code: "KAIROS-T-0010"
created_at: 2026-07-08T15:06:13.109017+00:00
updated_at: 2026-07-10T00:29:57.713023+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0009]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: Board rules engine and defaults seeding

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

kairos-core board service per A-0002: transition validation, column management rules, and default-board seeding.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Transition allowed iff `(board_id, from_column_id, to_column_id)` exists in `board_transitions`; invalid attempts return a typed error carrying the allowed target columns (feeds S-0006 REQ-1.4 later)
- [x] Column rules enforced: no removal of non-empty columns, position reordering, add with unique name/position
- [x] New boards seed the A-0002 defaults per level exactly (strategy/initiative/delivery incl. Blocked bidirectionals/adr) — integration-tested against a fresh tenant
- [x] Transitions and board config changes write `activity_log` rows

## Implementation Notes

References A-0002 (decided). Pure rule logic lives in kairos-core with unit tests; DB checks in integration tier.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Active. Plan: (1) kairos-core new `board` module — pure types (Column/Transition/ColumnRef), `can_transition` returning `TransitionError::NotAllowed{allowed_targets}` (feeds S-0006 REQ-1.4), column rule checks (add unique name+position, rename, remove-only-when-empty via item_count param, reorder validity), `check_add_transition` (mirrors DDL CHECK/UNIQUE), `dead_end_columns` warning check, and `parse_default_config` (moves T-0008's defaults parsing into core). (2) kairos-db new `boards` module (tenant.rs style, sync PgConnection): `create_board` seeding from `system_board_defaults` (T-0008's `create_board_from_defaults` moves here; tenant.rs provisioning calls it), macro-generated `transition_strategy/initiative/task` + hand-written `transition_adr` (nullable board columns) over one shared validate helper, column add/rename/remove/reorder + transition add/remove with core-rule enforcement and activity logging. (3) Interpretation: `activity_log.action` gains `board_config` for board configuration changes (the documented S-0004 action list is a comment, not a CHECK; A-0004 already extends it with `retention_sweep`) — item transitions use action='transition' details='column:<From>-><To>' per spec; board creation logs action='create' entity_type='board'. Provision-time board creation passes actor=None → no activity rows (no user exists yet; actor_id is NOT NULL). (4) Integration test tests/board_rules.rs on a scratch DB.
- 2026-07-09: Implemented and verified. Files: `crates/kairos-core/src/board.rs` (new, +12 unit tests), `crates/kairos-db/src/boards.rs` (new), `crates/kairos-db/tests/board_rules.rs` (new), refactors in `kairos-db/src/tenant.rs` (board seeding moved out), `models/enums.rs` (ActivityAction::BoardConfig), lib.rs/Cargo.toml wiring. Seeding refactor: T-0008's `create_board_from_defaults` + `parse_transition` deleted from tenant.rs; defaults parsing/validation now `kairos_core::board::parse_default_config`, persistence now `kairos_db::boards::create_board` (typed diesel, optional actor for activity logging); `provision_tenant` calls it via typed `PROVISION_BOARDS: [(BoardLevel, &str, &str); 3]`; `TenantError::BoardDefaults(String)` replaced by `TenantError::Board(#[from] BoardError)`. VERIFICATION GATE (recorded outputs):
  - `cargo fmt --check` → exit 0, no diff (PASS).
  - `cargo clippy --workspace --all-targets -- -D warnings` → "Finished `dev` profile ... target(s)" with zero warnings (PASS).
  - `angreal test unit` → all workspace lib/bin targets ok, incl. kairos-core "test result: ok. 12 passed; 0 failed" (board::tests::*: transition_allowed_iff_edge_exists, invalid_transition_carries_allowed_targets_in_position_order, transition_from_dead_end_reports_empty_allowed_targets, transition_with_unknown_columns_is_typed, add_column_requires_unique_name_and_position, rename/remove/reorder rules, add_transition_mirrors_ddl_constraints, dead_end_detection..., parse_default_config x2) and kairos-db "ok. 11 passed" (PASS).
  - `angreal test integration` → "Running 4 integration test target(s): kairos-db::board_rules, kairos-db::models_roundtrip, kairos-db::public_migrations, kairos-db::tenant_provisioning"; board_rules: "test board_rules_lifecycle ... ok / test result: ok. 1 passed"; models_roundtrip: "ok. 2 passed"; public_migrations: "ok. 1 passed"; tenant_provisioning: "ok. 1 passed" — all four targets green incl. the pre-existing three; compose stack torn down ("Docker services stopped successfully", `docker ps` shows no kairos containers) (PASS).
  - Criterion 1 evidence: board_rules_lifecycle asserts Draft->Review succeeds (edge exists), Review->Draft returns `BoardError::Transition(TransitionError::NotAllowed{allowed_targets})` with allowed_targets == ["Active"], unknown target returns UnknownToColumn, and the rejected move changes nothing.
  - Criterion 2 evidence: remove of non-empty Review column → `ColumnNotEmpty{item_count:1}`; duplicate name/position on add rejected; rename-to-existing rejected; reorder validated (length mismatch typed error) and persisted 0..n; empty-column removal succeeds with transition cascade.
  - Criterion 3 evidence: fresh-tenant assertions of all four A-0002 graphs — strategy/initiative/adr forward-only chains and delivery board (via `create_board` for a team) with exactly Backlog->Todo->Active->Completed + Todo<->Blocked + Active<->Blocked.
  - Criterion 4 evidence: activity rows asserted — 'transition' rows with details 'column:Draft->Review', 'column:Backlog->Todo', 'column:Todo->Blocked', 'column:Blocked->Todo', 'column:Discovery->Design', 'column:Draft->Discussion'; 'board_config' rows in order [column_add:Spike@5, column_rename:Spike->Parking Lot, transition_add:Parking Lot->Draft, transition_remove:Monitoring->Completed, columns_reorder:..., column_remove:Parking Lot]; 'create' row for user-created board.
  - Dead-end detection: flagged ["Completed"] initially, ["Completed","Parking Lot"] after adding an unwired column, back to ["Completed"] once wired, ["Monitoring","Completed"] after removing Monitoring->Completed.