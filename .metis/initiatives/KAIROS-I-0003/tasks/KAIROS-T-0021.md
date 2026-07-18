---
id: m2-unified-search-endpoint
level: task
title: "M2: Unified search endpoint"
short_code: "KAIROS-T-0021"
created_at: 2026-07-10T01:08:35.809749+00:00
updated_at: 2026-07-10T09:42:03.166040+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0017, KAIROS-T-0014]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: Unified search endpoint

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

POST /api/search exposing the T-0014 pipeline exactly per A-0007/S-0005: q/filter/traverse composition, typed request validation, grouped fully-typed results, pagination.

## Acceptance Criteria

## Acceptance Criteria

- [x] Request schema validated (at least one of q/filter/traverse; depth required+capped on traverse); 400 with field-level detail otherwise
- [x] Every S-0005 composition example passes as an HTTP-level integration test; results grouped by type, empty groups omitted, total/limit/offset correct
- [x] utoipa-annotated request/response schemas

## Implementation Notes

References A-0007 (decided), T-0014 service. Thin endpoint — the pipeline exists; scope is HTTP shape + validation + tests.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-10: Activated. Read S-0005 Unified Search section, A-0007, kairos-core/src/search.rs + kairos-db/src/search.rs (T-0014 pipeline exists), T-0018 handler patterns. Plan: NEW crates/kairos-client/src/types_search.rs (S-0005-verbatim SearchRequest/SearchResponse DTOs, string-encoded ids/dates per A-0015 discipline, serde+utoipa); NEW crates/kairos-server/src/api/search.rs (POST /api/search behind auth+tenant, no capability check — reads tenant-open; DTO→core conversion with 400 VALIDATION field-level detail; SearchValidationError→400; TraverseRootNotFound→404; execute_search via BlockingTenantPool); router merged in app.rs after org merge; NEW tests/search_endpoint.rs (scratch DB kairos_search_endpoint_t0021_test, real Dex tokens, T-0014-style seeding, all S-0005 composition examples over HTTP, 400 matrix, 401/403, pagination envelope).
- 2026-07-10: Implemented + verified. Files: NEW crates/kairos-client/src/types_search.rs (SearchRequest/SearchFilter/SearchTraverse/SearchTraverseFrom/SearchSort + SearchResponse/SearchResultGroups, all serde+utoipa ToSchema, deny_unknown_fields on request side, empty groups omitted via skip_serializing_if, S-0005 example round-trip unit tests); `pub mod types_search;` in kairos-client lib.rs; NEW crates/kairos-server/src/api/search.rs (POST /api/search, #[utoipa::path] annotated, manual Json<Value>→DTO deserialize so shape errors get the S-0005 envelope, DTO→core conversion with 400 VALIDATION + details.field, SearchValidationError→400 field-level mapping incl. details.depth/cap/limit, TraverseRootNotFound→404, execute_search on the tenant-pinned BlockingTenantPool connection, no capability check — reads tenant-open per A-0006); `pub mod search;` in api/mod.rs; router merged in app.rs after the org merge; NEW crates/kairos-server/tests/search_endpoint.rs (scratch DB kairos_search_endpoint_t0021_test, real Dex tokens).
- 2026-07-10: Evidence (self-check; full angreal gate deferred to orchestrator per shared-services mode): `cargo fmt --check` → exit 0. `cargo clippy --workspace --all-targets -- -D warnings` → "Finished `dev` profile" (clean). `cargo test -p kairos-server --test search_endpoint` → "test search_endpoint_against_live_stack ... ok. 1 passed; 0 failed" (covers all five S-0005 composition examples verbatim over HTTP, empty-group omission asserted via exact results-key sets, fully-typed task/strategy DTO field assertions, pagination: total=3 pre-page with limit=2/offset=1 echoed + defaults 25/0 + past-end offset, 400 matrix: no-capability {} and widening-only filter, missing depth, depth 11 over cap w/ details.cap=10, entity_type "epic", malformed created_after/created_before, blank q, limit 101, unknown field; 401 unauthenticated; 403 non-member; unknown traverse root → 404). `cargo build -p kairos-client` + `cargo test -p kairos-client` → 7 passed. Criteria 1-3 all demonstrated; services left UP.