---
id: m2-entity-crud-and-transition
level: task
title: "M2: Entity CRUD and transition endpoints"
short_code: "KAIROS-T-0018"
created_at: 2026-07-10T01:08:19.960794+00:00
updated_at: 2026-07-10T08:47:47.657662+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0017, KAIROS-T-0012, KAIROS-T-0010]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: Entity CRUD and transition endpoints

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

The five entity endpoint families per S-0005 (strategies/initiatives/tasks/documents/adrs): list/get/create/PATCH(version)/DELETE(soft cascade)/transition, wired to the T-0012 write services and T-0010 transition services, with ABAC enforcement and utoipa annotations.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] All S-0005 entity routes implemented with the documented status codes: 200/201, 409 VersionConflict (returns current entity state), 422 invalid transition (returns allowed targets), 403 capability denied, 404
- [x] ABAC enforced per A-0006 on every write (manage_* per type, transition_items for transitions); reads open tenant-wide
- [x] DTOs live in the shared client/types crate with utoipa derives; every handler annotated
- [x] Integration tests via HTTP against the booted app: happy paths + the four error classes per family

## Implementation Notes

References S-0005, A-0004/A-0006 (decided), T-0012/T-0010 services. Establish the handler/module pattern the remaining endpoint tasks copy.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Contract decisions (recorded during implementation)

- **Document create requires `parent_short_code`** (a strategy/initiative/task short code). The server creates the document, then the `supports` edge (parent = source, document = target, per S-0004 orientation), and authorizes `manage_documents` against the parent's board (A-0006 inheritance). Missing/unknown parent or a non-workflow parent → 422 VALIDATION. If the parent resolves to no board, the org-admin-only fallback applies (A-0006 "items not on boards").
- **Off-board ADR create** (no `board_id`): no board exists to authorize against → org-admin-only fallback (A-0006). Transitioning an off-board ADR → 422 `ITEM_NOT_ON_BOARD`.
- **DTOs in `kairos_client::types`** stay dependency-light (serde/serde_json/utoipa only): UUIDs and timestamps are strings on the wire (RFC3339 for timestamps, `YYYY-MM-DD` for ADR decision dates); the server parses and 422s (`VALIDATION`) on malformed ids. Conversions `From<kairos_db model>` live in `kairos-server/src/api/convert.rs`.
- **Sync services bridge**: T-0012/T-0010/T-0011 services take sync `PgConnection`; the server runs them via a small r2d2 pool + `spawn_blocking` (`kairos-server/src/blocking.rs`) with per-checkout `search_path` pinning mirroring `TenantPool`.
- **409 envelope**: code `CONFLICT`, `details.current` = the FULL current entity DTO (reloaded on conflict), per S-0005 "409 with current entity state".
- **422 invalid transition**: code `INVALID_TRANSITION`, `details.allowed_targets` = `[{id,name}]` from core's `TransitionError::NotAllowed`.
- **403**: code `FORBIDDEN`, `details.required_capability` names the missing capability (+ `board_id` when board-scoped).

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-10: Active. Read S-0005/A-0004/A-0005/A-0006, T-0017 server code, T-0010/T-0011/T-0012 services. Plan: DTOs in kairos-client `types` module; server `api/{strategies,initiatives,tasks,documents,adrs}.rs` + `api/convert.rs` + `blocking.rs`; integration test `tests/entities.rs` on scratch DB `kairos_entities_t0018_test` with shared helpers refactored to `tests/common/mod.rs`. Shared-services mode: only `cargo test -p kairos-server`; services left up.
- 2026-07-10: Implemented. New: `kairos-client/src/types.rs` (DTOs + request bodies + ListEnvelope/DeleteResponse/ErrorEnvelope, serde+utoipa derives); `kairos-server/src/api/{mod,convert,strategies,initiatives,tasks,documents,adrs}.rs` (all S-0005 entity routes, utoipa::path on every handler); `kairos-server/src/blocking.rs` (r2d2 sync pool + spawn_blocking bridge to the sync kairos-db services, per-checkout search_path pinning); `error.rs` gained `conflict`/`unprocessable`/`validation`/`capability_required`. `tests/common/mod.rs` (shared helpers extracted from T-0017's middleware test) + `tests/entities.rs` (scratch DB `kairos_entities_t0018_test`, real Dex tokens; per-family happy paths and the error matrix: 409 w/ full current DTO, 422 INVALID_TRANSITION w/ allowed_targets, 422 doc-create without parent, 422 ITEM_NOT_ON_BOARD, 403 naming the capability + 201 after grant, 404s across all verbs). Deps: kairos-client += serde_json+utoipa; kairos-server += kairos-client, utoipa, chrono, diesel `r2d2` feature. Workspace-root Cargo.toml untouched (utoipa already present).
- 2026-07-10: VERIFICATION (deferred full `angreal` gate runs at the orchestrator; note: compose stack had gone down mid-session — restarted with `angreal services up`, left UP):
  - `cargo fmt --check` → clean ("FMT: clean").
  - `cargo clippy --workspace --all-targets -- -D warnings` → "Finished `dev` profile" with zero warnings.
  - `cargo test -p kairos-server` → 19 lib + 4 bin unit tests ok; `tests/entities.rs` 1 passed (entity_endpoints_against_live_stack, live Dex + Postgres); `tests/middleware.rs` 1 passed (still green after the common-helpers refactor).
  - `cargo build -p kairos-client` clean; `cargo test -p kairos-client` → 2 passed (envelope round-trip).
  - Criterion 1 (routes + status codes), 2 (ABAC per A-0006 incl. document board inheritance and org-admin fallback), 3 (DTOs in kairos-client with ToSchema/IntoParams; every handler `#[utoipa::path]`-annotated), 4 (HTTP integration tests, happy paths + error classes per family) all demonstrated by the entities test run above.