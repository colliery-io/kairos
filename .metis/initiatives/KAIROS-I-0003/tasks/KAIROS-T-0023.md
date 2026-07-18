---
id: m2-openapi-generation-and-dev
level: task
title: "M2: OpenAPI generation and dev Swagger UI"
short_code: "KAIROS-T-0023"
created_at: 2026-07-10T01:08:43.998714+00:00
updated_at: 2026-07-10T19:19:12.905358+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0018, KAIROS-T-0019, KAIROS-T-0020, KAIROS-T-0021, KAIROS-T-0022]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: OpenAPI generation and dev Swagger UI

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

OpenAPI per A-0005 §6: aggregate every annotated handler/DTO into /api/openapi.json, mount Swagger UI in dev builds, and make spec completeness enforceable.

## Acceptance Criteria

## Acceptance Criteria

- [x] GET /api/openapi.json serves a valid OpenAPI 3.x document covering every /api route in the router (assert programmatically: enumerate axum routes vs spec paths — a route missing from the spec fails a test)
- [x] Swagger UI mounted behind a dev-build flag (cfg or env)
- [x] Spec validated with a containerized validator (no Homebrew); CI artifact step added to the workflow (upload openapi.json)
- [x] WS channel documented in an accompanying docs section (OpenAPI covers REST only)

## Implementation Notes

References A-0005 §6 (decided), A-0012 gate rule "handler without annotations fails review" — the route-vs-spec test mechanizes it.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-10 (implementation + evidence):
  - **Spec + endpoint**: `api/openapi.rs` aggregates 90 operations across 46 paths (87 handlers + `/api/whoami` doc-stub + self-describing `/api/openapi.json`); served behind auth → tenant. `KAIROS_DEV_UI=true` (new `AppConfig.dev_ui`) mounts `/api/docs` — a self-contained Swagger UI page (CDN-loaded swagger-ui-dist 5.17.14, no new crate dep); route absent by default.
  - **Completeness test** (`tests/openapi.rs::registered_routes_and_spec_paths_match_exactly`): scans kairos-server `src/` for `.route("...")` registrations → set of `(METHOD, path)` under `/api` (comment lines stripped; `/api/docs` on a documented exclusion list) and asserts set-equality with the spec's operations. Second half (`openapi_endpoint_against_live_stack`): probes every documented path on the REAL production router token-less, asserting non-404 (only the router fallback 404s a token-less request), catching documented-but-unmounted routers. Method drift is owned by the scan half (auth answers 401 before method dispatch).
  - **Drift failure mode DEMONSTRATED**: temporarily removing `crate::api::meta::activity::get_activity` from ApiDoc → `route/spec drift (A-0005 §6). Registered but NOT in the OpenAPI spec ...: [("GET", "/api/activity")]` — test FAILED as designed, then restored (all green).
  - **Test runs recorded**: pure suite `cargo test -p kairos-server --test openapi -- --skip openapi_endpoint_against_live_stack` → 4 passed (drift gate, 3.x + family spot-list + schema presence, operationId uniqueness, artifact writer). Live `cargo test -p kairos-server --test openapi openapi_endpoint_against_live_stack` → 1 passed in 60.5s (401 unauth / 403 non-member / 200 member; served doc == aggregation byte-for-byte; 90-route probe; `/api/docs` 200 text/html with dev_ui, 404 without).
  - **Containerized validation**: `docker run --rm -v "$PWD/target:/spec" python:3-slim sh -c "pip -q install openapi-spec-validator && python -m openapi_spec_validator /spec/openapi.json"` → `/spec/openapi.json: OK` (250,600-byte artifact from `write_spec_artifact`).
  - **CI**: appended "Generate OpenAPI artifact" (runs `write_spec_artifact`, pure/no services) + `actions/upload-artifact@v4` steps to `.github/workflows/ci.yml`.
  - **WS docs**: `docs/api/events.md` documents `GET /ws/events` per A-0005 §5 (auth incl. `?access_token=` browser fallback, ThinEvent shape, subscribe filter, best-effort semantics); spec `info.description` points there.
  - **operationId fix**: `org/teams.rs` list/add/remove member fns collide with `org/members.rs` names → `operation_id = "list_team_members" / "add_team_member" / "remove_team_member"` overrides.
  - **Gates so far**: `cargo fmt --check` exit 0; `cargo clippy --workspace --all-targets -- -D warnings` clean. Full `cargo test -p kairos-server` regression pending: the concurrent T-0026 agent's `pub mod mcp;`/app.rs merge landed before its `src/mcp/` files exist, so the crate is momentarily uncompilable — waiting for their module, then re-running.
  - **Lane deviations (mechanical, forced by the `dev_ui` config field)**: `config.rs` (+field, +env parse, +unit-test coverage), `tests/common/mod.rs` (`dev_ui: false` in `base_config`), `middleware/tenant.rs` (ONE line: `dev_ui: false` in a cfg(test) config literal — compile fix only). Handlers across api/ became `pub(crate)` (visibility-only, so ApiDoc can reference the generated `__path_*` structs); `admin.rs` `ConfirmParams` likewise `pub(crate)` (private_interfaces warning).
- 2026-07-10 (ORCHESTRATOR CLOSE-OUT): the implementing agent completed all work but terminated while waiting on the concurrent T-0026 agent's partial `src/mcp/` module (which broke crate compilation when that agent died on a session limit). Orchestrator quarantined the partial module (commented `pub mod mcp;` + app.rs merge, files preserved), added a documented mcp-directory skip to the route scanner (T-0026 must revisit on un-quarantine), and ran the final regression: **openapi suite 5/5 passed** (incl. the live-stack probe), full `cargo test -p kairos-server` all targets green, `cargo fmt --check` + workspace clippy clean. All four criteria verified; task completed by orchestrator on the agent's evidence plus this regression.
- 2026-07-10: Active. Audit: every handler in api/ already carries `#[utoipa::path]` (T-0018..T-0021); `/api/whoami` (app.rs) is the only unannotated /api route. Plan: (1) new `api/openapi.rs` — `ApiDoc` derive aggregating all ~87 handler paths + a doc-stub path for `/api/whoami` (its handler lives in app.rs, outside this task's file lane) + self-describing `/api/openapi.json`; (2) handlers become `pub(crate)` so the generated `__path_*` structs are referenceable from the aggregator (visibility-only edits, no logic); (3) `operation_id` overrides in org/teams.rs for the three fn names colliding with org/members.rs (list_members/add_member/remove_member — OpenAPI requires unique operationIds); (4) **auth decision: `/api/openapi.json` and `/api/docs` sit behind the standard auth → tenant stack**, consistent with every other /api route (mounted inside the protected chain in app.rs); the CI artifact is the unauthenticated distribution channel; (5) Swagger UI at `/api/docs` gated by new `KAIROS_DEV_UI` env flag (AppConfig field, A-0013 style) — self-contained HTML page loading swagger-ui-dist from CDN, no new crate dependency; (6) completeness mechanism: tests/openapi.rs scans kairos-server src for `.route("...")` registrations (method+path), asserts set-equality with spec paths, PLUS probes every spec path on the production router asserting non-404; (7) CI: artifact-generating test writes target/openapi.json, upload step appended; (8) docs/api/events.md documents /ws/events (OpenAPI covers REST only).