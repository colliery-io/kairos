---
id: m2-websocket-event-channel
level: task
title: "M2: WebSocket event channel"
short_code: "KAIROS-T-0022"
created_at: 2026-07-10T01:08:36.752161+00:00
updated_at: 2026-07-10T09:44:39.993320+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0017, KAIROS-T-0012]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: WebSocket event channel

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

GET /ws/events per A-0005 §5: tenant-scoped WebSocket pushing thin change events, fanned out via PostgreSQL LISTEN/NOTIFY, with NOTIFY emission wired into the mutating services.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Mutating services (create/update/transition/delete, relationship + metadata changes) emit NOTIFY kairos_events post-commit with the tenant-tagged thin payload from S-0005 (event, entity_type, short_code, board_id, column_id?, actor, occurred_at)
- [x] /ws/events upgrades with bearer auth (same middleware), binds to the resolved tenant, supports optional board_id subscription filter; events for other tenants never delivered (integration-tested with two tenants)
- [x] A dedicated LISTEN connection per server process forwards to connected sockets; client disconnect/reconnect is clean (no leaked tasks — assert via tokio task count or timeout-bounded test)
- [x] Best-effort semantics documented: no replay; reconnect → client re-fetches

## Implementation Notes

References A-0005 §5 (decided), S-0005 Event Push section. axum ws + tokio broadcast internally; keep the emission helper in kairos-db so services stay the single write path.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-10: Active. Plan: (1) `kairos-db/src/events.rs` — `emit_event` via `SELECT pg_notify('kairos_events', $payload)`; tenant derived from `current_schema()` (same derivation as `items::next_short_code`) because the sync services never receive the slug — keeps call sites purely additive. pg_notify inside the service transaction is delivered ON COMMIT (documented = A-0005 "post-commit"). Placement helper `item_placement` (raw SQL per entity table) feeds `emit_item_event_by_id` for update/delete/transition/relationship/metadata events. (2) Call sites: items.rs 5 creates + update_item_content + soft_delete_item; boards.rs transition macro + transition_adr; graph.rs link/unlink (both endpoints); server api/meta/metadata.rs PATCH (metadata_changed, post-tx). (3) `kairos-server/src/ws.rs` — EventHub (tokio broadcast, cap 1024), one tokio-postgres LISTEN task per `ws::router()` call with reconnect+backoff (500ms→30s, reconnect counter + logs), `GET /ws/events` behind promote-query-token → require_auth → require_tenant; board_id subscribe filter; best-effort docs. Browser fallback chosen: `?access_token=` query param promoted to the Authorization header pre-auth (browsers cannot set WS headers; tenant comes from Host subdomain there). (4) DTO `kairos-client/src/types_events.rs` (string ids per types_meta discipline). (5) Test `tests/ws_events.rs` — scratch DB `kairos_ws_events_t0022_test`, real Dex tokens, tenants acme+widgets, real TCP listener + tokio-tungstenite client: shape, two-tenant isolation, board filter, query-param auth, 401/403, disconnect/reconnect, all timeout-bounded. Deps: axum `ws` feature, tokio-postgres `=0.7.18` (already in lock via diesel-async), dev tokio-tungstenite + futures-util.
- 2026-07-10: Implemented and verified. Files: NEW `crates/kairos-db/src/events.rs` (emit_event / item_placement / emit_item_event_by_id; post-commit semantics documented — pg_notify inside the tx is delivered on COMMIT, dropped on rollback), emission call sites in `items.rs` (finish_create → item_created for all five creates; update_item_content → item_updated; soft_delete_item → item_deleted for the cascade root), `boards.rs` (transition macro + transition_adr → item_transitioned with the NEW column), `graph.rs` (link/unlink → relationship_changed per live endpoint), `kairos-server/src/api/meta/metadata.rs` (PATCH → metadata_changed, post-tx); NEW `crates/kairos-server/src/ws.rs` (EventHub broadcast cap 1024, dedicated tokio-postgres LISTEN task with 500ms→30s backoff + reconnect counter, `/ws/events` behind ?access_token= promotion → require_auth → require_tenant, per-socket tenant binding + board_id subscribe filter, lagged-receiver drop = best-effort, 3 unit tests); NEW `crates/kairos-client/src/types_events.rs` (ThinEvent/SubscribeRequest DTOs); NEW `crates/kairos-server/tests/ws_events.rs` (scratch DB kairos_ws_events_t0022_test, live Dex + Postgres, tenants acme/widgets). Deps: workspace `tokio-postgres = "=0.7.18"` (same version diesel-async pins — no duplicate in lock), kairos-server axum `ws` feature, dev tokio-tungstenite 0.29 (= axum's own) + futures-util. Evidence (all run 2026-07-10 against the live stack, services left UP): `cargo fmt --check` → clean; `cargo clippy --workspace --all-targets -- -D warnings` → Finished, zero warnings; `cargo test -p kairos-server --test ws_events` → ok 1 passed (asserts: 401 missing/garbage token, 403 non-member, 404 unknown tenant on upgrade; item_created/item_transitioned/item_updated/item_deleted/relationship_changed(both endpoints)/metadata_changed shapes incl. board_id/column_id/actor/occurred_at and NO tenant leak; two-tenant isolation via ordering probe — widgets event emitted before an acme event never reaches the acme socket; board_id filter set→honored→cleared; ?access_token= fallback + two-socket fan-out; timeout-bounded disconnect/reconnect with no replay); `cargo test -p kairos-db` → all 11 targets ok (incl. write_path, board_rules, graph, isolation); `cargo test -p kairos-server --lib` → ok 22 passed; regression: entities/meta/middleware integration tests ok; `cargo test -p kairos-client` → ok 7 passed. Full `angreal test unit|integration` gate deferred to the orchestrator per shared-services discipline. Deviation from spec sketch: emit_event derives the tenant tag from `current_schema()` instead of a tenant_slug parameter (the sync services never receive the slug; same derivation next_short_code already uses) — call sites stay purely additive.