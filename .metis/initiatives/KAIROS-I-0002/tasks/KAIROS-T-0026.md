---
id: m3-kairos-mcp-server-mcp-endpoint
level: task
title: "M3: Kairos MCP server - /mcp endpoint per S-0006"
short_code: "KAIROS-T-0026"
created_at: 2026-07-10T09:08:53.518093+00:00
updated_at: 2026-07-10T22:09:52.248791+00:00
parent: KAIROS-I-0002
blocked_by: [KAIROS-T-0019, KAIROS-T-0020]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0002
---

# M3: Kairos MCP server - /mcp endpoint per S-0006

## Parent Initiative

[[KAIROS-I-0002]]

## Objective

The Kairos MCP server per KAIROS-A-0011 (decided) and KAIROS-S-0006 (the frozen tool surface): streamable HTTP at /mcp on the server binary, OAuth protected resource, tools calling kairos-core/kairos-db services in-process.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] /mcp mounted via the rmcp SDK (streamable HTTP) on the axum router behind the same bearer validation as /api; RFC 9728 protected-resource metadata served pointing at the configured issuer; session acts as the authenticated user under full ABAC (no agent identity)
- [x] Every S-0006 tool implemented with its exact name/inputs/semantics: whoami, my_boards, board_items; get_item, get_history, search; create_item, update_item, edit_item (server-side search/replace with one race retry), transition_item (failure enumerates allowed targets), link_items, unlink_items, set_metadata, delete_item (confirm required; response lists cascade)
- [x] Tools wrap existing services in-process (no HTTP loopback); responses are compact agent-shaped markdown/text per S-0006 REQ-1.6; errors mirror API codes (REQ-1.1); tenant from connection host only (REQ-1.2); short codes as identifiers (REQ-1.3)
- [x] Tool invocations logged to activity_log identically to API calls (NFR-1.3)
- [x] Integration test: MCP session over streamable HTTP with a real Dex token — initialize, list tools (assert the full S-0006 inventory), and a golden path: whoami → create_item → edit_item → transition_item (invalid first, assert allowed-targets in the error; then valid) → search → delete_item with confirm; plus a 409-style conflict via update_item with stale version

## Implementation Notes

References A-0011 + S-0006 (both decided/frozen — implement exactly, surface any necessary deviation instead of improvising). Read crates/kairos-server/src (T-0017/T-0018 structure) and the db service modules first. rmcp is already a pinned workspace dep.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created at I-0002 decompose (todo).
- 2026-07-10: Active. Read S-0006 (frozen surface), A-0011, middleware/blocking bridge, kairos-db services, API handler patterns. rmcp 2.2.0 fetched; enabled `server` + `transport-streamable-http-server` features on kairos-server's dep (workspace pin untouched). Plan: `StreamableHttpService` (stateful, LocalSessionManager, rmcp allowed-hosts check disabled — tenant Host resolution owns Host semantics) mounted at /mcp via `route_service` behind require_auth→require_tenant route_layers; tools read AuthContext/TenantContext from the injected `http::request::Parts` extensions; RFC 9728 metadata at /.well-known/oauth-protected-resource[/mcp]; 401s on /mcp carry WWW-Authenticate resource_metadata. New files: src/mcp/{mod,service,tools}.rs; test tests/mcp.rs (scratch DB kairos_mcp_t0026_test).
- 2026-07-10 (resume): Session limit hit mid-task; resumed. Fixed the 3 rmcp API compile errors (`Content` → `ContentBlock`; non-exhaustive `ServerInfo`/`Implementation` literals → `ServerInfo::new().with_server_info(Implementation::new().with_title()).with_instructions()` builders). Un-quarantined `pub mod mcp;` (lib.rs) and `.merge(crate::mcp::router(...))` (app.rs). `cargo check -p kairos-server` clean; `cargo clippy -p kairos-server --all-targets` exit 0. All 14 S-0006 tools present in src/mcp/tools.rs. Services verified up (kairos-postgres, kairos-dex healthy). Next: tests/mcp.rs integration test (raw streamable-HTTP JSON-RPC over the in-process router, SSE parsing).
- 2026-07-10: COMPLETE. Evidence (all against live compose stack, scratch DB kairos_mcp_t0026_test):
  - `cargo fmt --check` → clean (exit 0).
  - `cargo clippy --workspace --all-targets -- -D warnings` → "Finished `dev` profile", zero warnings.
  - `cargo test -p kairos-server --test mcp` → `test mcp_endpoint_against_live_stack ... ok; 1 passed` — real Dex token, streamable-HTTP session (initialize w/ Mcp-Session-Id + notifications/initialized 202); tools/list asserted EXACTLY the 14 S-0006 names; golden path whoami → my_boards → create_item(initiative, board defaulted) → create_item(task, parent → parent edge) → get_item (content/version/board/parent chain) → edit_item (not-found VALIDATION, then success → v2, 1 replacement) → transition_item invalid (INVALID_TRANSITION + allowed_targets naming the valid column) → valid → board_items (task in target column) → search q=kondensator (compact, finds task, no content) → get_history (v2/v1, editor "alice"; v1 snapshot content) → update_item stale version (CONFLICT + details.current.version=2 + current content) → link_items as member → FORBIDDEN (org-admin gate) → delete_item confirm=false → VALIDATION, confirm=true → cascade lists the task → board_items reflects delete → get_item NOT_FOUND. Auth: no token → 401 with WWW-Authenticate resource_metadata challenge; authenticated non-member → 403 MEMBERSHIP_REQUIRED. RFC 9728 metadata at /.well-known/oauth-protected-resource[/mcp] with authorization_servers=[issuer]. NFR-1.3 asserted from org_acme.activity_log (create=2, transition=1, delete=1, relationship_add=1, all actor=alice).
  - Regression `cargo test -p kairos-server` (all targets) → unit 22+4 ok; entities/mcp/meta/middleware/openapi(5)/org_endpoints/search_endpoint/ws_events ALL ok (openapi route-scanner green with the src/mcp skip).
  - Full `angreal test unit|integration` gate deliberately deferred to the orchestrator (shared-services discipline). Services left UP.
  - Interpretations recorded (no S-0006 deviations): get_history gained an optional `version` input (required by its own Returns column: "optionally a requested version's content"); whoami "teams with roles" rendered as team_type (team_members carries no role column); my_boards "user's delivery boards" = boards of the user's teams ∪ boards where the user holds any capability grant; create_item defaults `board` to the single live board of the matching level, ADRs default onto the ADR board; link/unlink are org-admin-gated exactly like POST/DELETE /api/relationships.