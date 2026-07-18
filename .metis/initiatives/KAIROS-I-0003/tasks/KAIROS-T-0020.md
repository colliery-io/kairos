---
id: m2-relationships-metadata
level: task
title: "M2: Relationships, metadata, templates, history, activity endpoints"
short_code: "KAIROS-T-0020"
created_at: 2026-07-10T01:08:28.211491+00:00
updated_at: 2026-07-10T09:16:31.882629+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0017, KAIROS-T-0012, KAIROS-T-0013]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: Relationships, metadata, templates, history, activity endpoints

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

The remaining item-adjacent families per S-0005: relationships (org-admin), item metadata (typed validation per A-0003), templates + metadata-definitions admin, content history, and the activity log query endpoint.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Relationships GET per item + POST/DELETE (org admin; T-0013 validation errors → 422 with typed reason)
- [x] Metadata GET/PATCH per item validating against definitions (enum membership, date parse); definitions/templates CRUD (org admin)
- [x] GET history per entity (versions list + specific version content); GET /api/activity with combinable filters + pagination
- [x] Integration tests incl. validation failures and org-admin gating

## Implementation Notes

References S-0005, A-0003, A-0004, T-0013 graph service.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-10: Active. Plan: new DTOs in kairos-client/src/types_meta.rs; new handler tree kairos-server/src/api/meta/{mod,relationships,metadata,definitions,templates,history,activity}.rs + api/convert_meta.rs; router merged via ONE insertion in app.rs before the health section (own route_layer stack, distinct anchor from the concurrent agent); integration test tests/meta.rs with scratch DB kairos_meta_t0020_test. Routes: GET /api/{entity_type}/{short_code}/{relationships|metadata|history} (wildcard family segment validated in-handler), POST /api/relationships + DELETE /api/relationships/{id} (org admin; typed 422 codes RELATIONSHIP_RULE/CYCLE_DETECTED/ALREADY_LINKED), PATCH metadata gated by manage_type via abac::resolve_authorization_board, /api/metadata-definitions + /api/templates CRUD (org-admin writes, open reads per A-0006; definition delete in use -> 409 DEFINITION_IN_USE app-side since the FK is CASCADE), GET /api/activity with combinable filters. Template DELETE is a hard delete (documents.template_id is ON DELETE SET NULL; templates have no deleted_at).
- 2026-07-10: Implemented + verified. Files: crates/kairos-client/src/types_meta.rs (+ one `pub mod types_meta;` in kairos-client lib.rs, placed before the tests module rather than the literal final line — a mod decl inside `mod tests` would not compile and clippy's items_after_test_module forbids the end-of-file spot), crates/kairos-server/src/api/convert_meta.rs, crates/kairos-server/src/api/meta/{mod,relationships,metadata,definitions,templates,history,activity}.rs, two mod lines in api/mod.rs, ONE contiguous insertion in app.rs before the health section (meta router with its own auth→tenant route_layer stack; coexists with T-0019's org merge), new test crates/kairos-server/tests/meta.rs (scratch DB kairos_meta_t0020_test; tests/common untouched). Evidence (all run 2026-07-10, services left UP):
  - `cargo fmt --check` → clean (exit 0).
  - `cargo clippy --workspace --all-targets -- -D warnings` → clean ("Finished `dev` profile").
  - `cargo test -p kairos-server` → 19 unit + entities(1) + meta(1) + middleware(1) all ok: "test meta_endpoints_against_live_stack ... ok. 1 passed; 0 failed".
  - `cargo build -p kairos-client` → Finished; `cargo test -p kairos-client` → 4 passed.
  - Criterion 1 (relationships): meta.rs covers org-admin POST 201, non-admin 403 (required_role=admin), ALREADY_LINKED/RELATIONSHIP_RULE/CYCLE_DETECTED 422s, unknown endpoint + bad relationship value 422 VALIDATION, GET grouped both directions with hydrated summaries + edge ids, family-mismatch/unknown-family 404, DELETE 403/200/404.
  - Criterion 2 (metadata/definitions/templates): enum accept + reject-with-allowed-values, unknown slug 422, date parse accept/reject, null clears, manage_tasks/manage_documents gating (bob 403 incl. document-via-parent), definitions CRUD with option rules + lifecycle + in-use delete 409 DEFINITION_IN_USE (item value AND template reference), templates CRUD incl. bad default 422, create-from-template still stamps content+metadata, hard delete leaves stamped doc intact (template_id NULL).
  - Criterion 3 (history/activity): version list [3,2,1] with edited_by/edited_at, pagination limit/offset, ?version=2 snapshot title+content, 404s; activity entity_id (exactly the create row), action filter (relationship_add=4), combined actor+action (svc=2, alice remove=0, svc remove=1), since past/future, limit pagination, malformed filters 422.
  - Criterion 4: all of the above via HTTP with real Dex tokens in tests/meta.rs.
  - Full `angreal test unit`/`integration` gate deliberately deferred to the orchestrator (shared-services mode).