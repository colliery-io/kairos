---
id: m1-abac-capability-checks
level: task
title: "M1: ABAC capability checks"
short_code: "KAIROS-T-0011"
created_at: 2026-07-08T15:06:13.728822+00:00
updated_at: 2026-07-10T00:41:16.568644+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0009]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M1: ABAC capability checks

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

ABAC capability service per A-0006: board-scoped whitelist checks with glob matching, org-admin bypass, and document-inherits-parent-board resolution.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Unit test matrix for glob semantics: exact match, `*`, `manage_*`, non-matching prefixes, empty/hostile strings — matching the SQL LIKE translation in A-0006
- [x] Grant/revoke operations with `activity_log` entries; UNIQUE constraint honored
- [x] Document authorization resolves through the `supports` edge to the parent's board (integration test); templates/metadata/relationships restricted to org admin; org-admin bypass covered by tests

## Implementation Notes

References A-0006 (decided). The check must be a single indexed query as spec'd — no N+1.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Active. Plan: (1) `kairos-core::abac` — capability vocabulary consts, `capability_matches` mirroring the A-0006 LIKE translation (every `*` in the granted string is a multi-char wildcard; `%`/`_`/`\` in stored values are LIKE-escaped in SQL so they match literally; only trailing `*` is sanctioned vocabulary, documented), `is_authorized` over loaded grants, org-admin-only policy for tenant-wide config. (2) `kairos-db::abac` — `check_capability` as ONE EXISTS query (idx_board_member_cap_board_user) with escaped LIKE + `capability='*'` arm; `is_org_admin` by org SLUG (decision: pass the slug explicitly, same style as boards.rs taking conn+ids — the slug is what the pool pins search_path from, so callers always have it; no new TenantContext type); combined `authorize` checking admin first; `grant_capability`/`revoke_capability` with activity rows (decision: duplicate grant = typed `AlreadyGranted`, missing revoke = typed `GrantNotFound` — auditable, no silent no-ops); `resolve_authorization_board` (board items → own board; documents → `supports` edge where target=document, source=parent per S-0004 DDL comment, parent's board; off-board/unknown → None). (3) Integration test `tests/abac.rs` on scratch DB `kairos_abac_test` per board_rules.rs harness. Gate: fmt/clippy/unit/integration (5 targets).
- 2026-07-09: DONE — all criteria demonstrated. Shipped: `crates/kairos-core/src/abac.rs` (vocabulary consts incl. glob forms, `capability_matches`, `is_authorized`, `TenantConfigResource::org_admin_only()` const-true policy for templates/metadata/relationships) with 12 unit tests; `crates/kairos-db/src/abac.rs` (`check_capability` = ONE EXISTS query over `board_member_capabilities` using `idx_board_member_cap_board_user`, LIKE translation `replace(replace(replace(replace(capability,'\','\\'),'%','\%'),'_','\_'),'*','%')` + `capability='*'` arm; `is_org_admin`/`authorize` by org slug; `grant_capability`/`revoke_capability` transactional with `capability_grant`/`capability_revoke` activity rows); `crates/kairos-db/tests/abac.rs` integration test (scratch DB `kairos_abac_test`).
  - Criterion 1 (glob unit matrix): `angreal test unit` → kairos_core `test result: ok. 24 passed; 0 failed` including abac::tests::{exact_capability_matches_itself_only, bare_star_grants_every_capability, manage_glob_matches_manage_capabilities_only, configure_and_transition_globs, non_matching_prefixes_are_rejected, empty_strings_mirror_sql_like, stored_percent_is_literal_not_wildcard, stored_underscore_is_literal_not_single_char_wildcard, stored_backslash_is_literal, embedded_star_wildcards_like_the_sql_translation, is_authorized_is_a_whitelist_over_grants, tenant_config_resources_are_org_admin_only}. Integration test's `assert_check` additionally asserts the pure matcher and the real SQL agree on every truth-table row.
  - Criteria 2+3 (grant/revoke + activity + UNIQUE; supports-edge inheritance; org-admin bypass): `angreal test integration` → `Running 5 integration test target(s): kairos-db::abac, kairos-db::board_rules, kairos-db::models_roundtrip, kairos-db::public_migrations, kairos-db::tenant_provisioning`; abac.rs `test abac_capability_lifecycle ... ok` (1 passed); all five targets `0 failed`. Covers: grant→activity row `capability:manage_tasks user:{uuid}`; duplicate grant → typed `AlreadyGranted` (composite PK honored, no extra audit row); stored literal `%` grant does NOT wildcard in SQL; revoke removes access + `capability_revoke` row; re-revoke → `GrantNotFound`; org admin authorized with zero grants, plain member denied; document with `supports` edge (source=initiative, target=document) resolves to the initiative's board and `manage_documents` there authorizes editing it; board items resolve to own board; off-board ADR/orphan document/unknown id → None.
  - Gate: `cargo fmt --check` → clean; `cargo clippy --workspace --all-targets -- -D warnings` → `Finished` with zero warnings; `angreal test unit` green (all crates); `angreal test integration` green; compose services torn down (`docker ps` shows 0 kairos containers).
  - Decisions: org identified by SLUG passed explicitly to `is_org_admin`/`authorize` (no TenantContext type — matches boards.rs conn+ids convention; the slug is what the pool pins search_path from). Duplicate grant = typed `AlreadyGranted`; missing revoke = typed `GrantNotFound` (auditable, no silent no-ops). Hostile-glob policy: only trailing `*` is sanctioned vocabulary; `%`/`_`/`\` in stored grants are LIKE-escaped in SQL and literal in the pure matcher; embedded `*` still wildcards in both layers (mirrors A-0006's SQL exactly), documented as outside the vocabulary.