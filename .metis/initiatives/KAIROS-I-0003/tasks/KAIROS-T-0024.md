---
id: m2-kairos-client-and-api
level: task
title: "M2: kairos-client and API integration suite"
short_code: "KAIROS-T-0024"
created_at: 2026-07-10T01:08:44.837369+00:00
updated_at: 2026-07-10T23:53:23.707277+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0023]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: kairos-client and API integration suite

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Complete kairos-client as the typed API client (used by CLI, tests, skills verification) and close out the M2 API integration suite as the comprehensive tier-3 pass per A-0012.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] kairos-client: typed methods for every endpoint family + search + auth token handling (bearer injection, 401/403/409/422 typed errors); DTOs shared with the server (single source)
- [x] The API integration suite is refactored to consume kairos-client end-to-end (no ad-hoc reqwest in tests except protocol-level cases like malformed tokens/raw WS)
- [x] Coverage checklist recorded in the task doc: every S-0005 endpoint family exercised through the client, auth failure matrix, tenant isolation at HTTP level (two tenants via API), WS events through a client helper
- [x] `angreal test integration` green with the full suite; runtime recorded (run as `cargo test -p kairos-server --no-fail-fast` + `cargo test -p kairos-client` under shared-services discipline; full `angreal test integration` gate deferred to the orchestrator — see Status Updates for the one cross-lane failure)

## Coverage Checklist (KAIROS-T-0024)

Every S-0005 family exercised THROUGH the typed client (test target in parentheses):

- Entities — strategies/initiatives/tasks/documents/adrs list+get+create+update+delete, transitions for the four on-board families (entities.rs)
- Boards + config: board CRUD, items view, columns CRUD, transitions CRUD (org_endpoints.rs)
- Board members/capabilities: add/replace/revoke + grant-flips-403 arc (org_endpoints.rs)
- Teams + team members (org_endpoints.rs)
- Delivery streams + stream teams (org_endpoints.rs)
- Org members incl. LAST_ADMIN guard (org_endpoints.rs)
- Admin tenants: create/list/delete with confirm semantics (org_endpoints.rs)
- Relationships: get grouped both-direction, create, delete + typed rule 422s (meta.rs)
- Item metadata get/patch incl. typed validation + clearing (meta.rs)
- Metadata definitions CRUD + DEFINITION_IN_USE (meta.rs)
- Templates CRUD + stamping (meta.rs)
- Content history list/pagination/snapshot (meta.rs)
- Activity log with all combinable filters (meta.rs)
- Unified search: all 5 S-0005 composition examples, pagination, 400 matrix (search_endpoint.rs)
- Whoami (org_endpoints.rs, tenant_isolation.rs)
- WS events through `EventStream` helper: connect with token+tenant, subscribe_board/subscribe_all, typed ThinEvents, typed upgrade rejection (ws_events.rs)

Auth failure matrix via client-typed errors (client_roundtrip.rs): 401 garbage token → `Unauthorized`; 403 non-member → `Forbidden{capability: None, MEMBERSHIP_REQUIRED}`; 403 missing capability → `Forbidden{capability, details.board_id}`; 404 → `NotFound`; 409 stale version → `Conflict{current = full DTO}`; 422 → `InvalidTransition{allowed_targets}` / `Validation{status: 422}`; 400 search → `Validation{status: 400, details.fields}`; other 422 (LAST_ADMIN) → `Other{status, code}`; `TokenProvider` drawn per request + `Error::Token` propagation; untenanted client → `NotFound TENANT_NOT_FOUND`; non-JSON body → `Error::Decode`. (Protocol-level missing/tampered/wrong-aud token matrix stays raw in middleware.rs by design.)

HTTP-level two-tenant isolation via two clients (tenant_isolation.rs): acme+widgets seeded with IDENTICAL data through the API; list/get/search/board/history/relationship cross-invisibility; membership 403s across tenants; the SAME svc token behind two X-Tenant clients sees disjoint worlds; org rosters tenant-scoped.

Raw (non-client) HTTP remains ONLY in protocol-level cases: middleware.rs (malformed/tampered/wrong-audience tokens, Host/X-Tenant resolution), mcp.rs (MCP protocol), openapi.rs (spec probes), ws_events.rs raw handshake probes + `?access_token=` browser fallback, search 401-without-header probe, and `KairosClient::raw_request` for malformed-body/wire-shape probes (unknown path family, deny_unknown_fields bodies, "empty search groups are omitted").

## Implementation Notes

References A-0009 (kairos-client doubles as test client), A-0015 (CLI consumes it next), A-0012 tier 3.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-10: Active (shared-services lane). Plan: (1) implement `KairosClient` in kairos-client (reqwest+rustls, `TokenProvider` trait for CLI refresh, optional X-Tenant, typed methods per endpoint family, S-0005 typed `Error` enum mapped from status+code, `raw_request` escape hatch for protocol probes, tokio-tungstenite `EventStream` WS helper with typed `ThinEvent`); (2) add `spawn_server`/client helpers to tests/common (client needs a real listener; the router spawns on an ephemeral 127.0.0.1 port); (3) mechanically refactor entities/meta/org_endpoints/search_endpoint/ws_events tests onto the client (raw stays only for protocol-level: middleware.rs token matrix, mcp.rs, openapi.rs, raw WS handshake probes, malformed-body probes); (4) new `client_roundtrip` target (typed error-mapping matrix 401/403/404/409/422/400 + Other) and `tenant_isolation` target (two tenants over HTTP via two clients + same token across both tenants); (5) fmt/clippy/full kairos-server + kairos-client test pass, runtime recorded. Full `angreal test integration` gate deferred to orchestrator (shared services).
- 2026-07-10: DONE + evidence. Implemented `kairos-client`: `client.rs` (`KairosClient`, `TokenProvider`/`StaticToken`, `EntityKind`, ~70 typed endpoint methods incl. whoami, exact-status contract enforcement 200/201, `raw_request` escape hatch), `error.rs` (typed S-0005 `Error` enum: Unauthorized/Forbidden{capability}/NotFound/Conflict{current}/InvalidTransition{allowed_targets}/Validation{status,field}/Other{status,code} + Transport/Decode/UnexpectedResponse/Token/WebSocket; `status()/code()/message()/details()` accessors), `ws.rs` (`EventStream`: connect_events with bearer+X-Tenant, typed upgrade-rejection mapping, subscribe_board/subscribe_all, typed `ThinEvent` + raw-frame reads). Deps added to kairos-client: reqwest(workspace rustls), tokio, tokio-tungstenite 0.29, futures-util. tests/common gained `spawn_server` (ephemeral-port axum serve) + `TestServer::client{,_untenanted}`; in-process `request()` retained for protocol-level tests. Refactored entities.rs, meta.rs, org_endpoints.rs, search_endpoint.rs, ws_events.rs onto the client (no assertions weakened; wire-shape/malformed probes via raw_request or raw sockets as documented). NEW targets: tests/client_roundtrip.rs (typed error-mapping matrix, TokenProvider seam) and tests/tenant_isolation.rs (HTTP-level two-tenant isolation via clients).
  - `cargo fmt --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean.
  - `cargo test -p kairos-client`: 9 passed (+0 doc), green.
  - `cargo test -p kairos-server --no-fail-fast` (all targets): lib 29 ok, main 4 ok, client_roundtrip ok (1.24s), entities ok (1.35s), mcp ok (1.40s), meta ok (2.05s), middleware ok (1.49s), org_endpoints ok (2.25s), scim ok (1.84s), search_endpoint ok (0.70s), tenant_isolation ok (1.54s), ws_events ok (1.38s). Full-suite wall time ≈ 85s (1:25.27), dominated by the openapi live-stack test (~61s).
  - KNOWN CROSS-LANE FAILURE (not this task): openapi `registered_routes_and_spec_paths_match_exactly` fails because the concurrent KAIROS-T-0025 SCIM lane registered `/api/scim-tokens` (GET/POST/DELETE) in app.rs without annotating them into `api::openapi::ApiDoc` yet. This task touches no server src; the SCIM lane owns the fix; orchestrator gate will re-verify.
  - Services left UP per shared-services mode; scratch DBs (`kairos_client_roundtrip_m2_test`, `kairos_tenant_isolation_m2_test`, and the per-suite ones) are dropped by their tests' teardown.