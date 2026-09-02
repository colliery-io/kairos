---
id: webhook-endpoint-tenant-routing
level: task
title: "Webhook endpoint: tenant routing, signature verification, ordering-safe upsert"
short_code: "KAIROS-T-0099"
created_at: 2026-09-01T23:12:31.882520+00:00
updated_at: 2026-09-02T10:20:26.364228+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0009
---

# Webhook endpoint: tenant routing, signature verification, ordering-safe upsert

## Parent Initiative

[[KAIROS-I-0009]] — Git Forge Integration. Depends on KAIROS-T-0097 (schema + connections) and KAIROS-T-0098 (normalization).

## Objective

The ingestion endpoint: accept deliveries from GitHub and GitLab, resolve the tenant without an auth stack, verify authenticity, match short codes to live items, and upsert links in a way that redeliveries and out-of-order events cannot corrupt.

## Implementation Notes

- **Route + mounting**: `POST /webhooks/{forge}/{tenant_slug}/{connection_id}`, mounted in `app.rs` **outside** the OIDC auth → tenant middleware, next to the SCIM router (`crate::scim::router`) and the deployment-admin router. Non-`/api` by design: it stays off the S-0005 surface and out of the openapi route-vs-spec scanner (the `/healthz`, `/readyz`, `/metrics` precedent). Add a module doc explaining the mounting, as `scim/mod.rs` does.
- **Tenant resolution mirrors SCIM** (`crates/kairos-server/src/scim/auth.rs`): the slug comes from the URL, is validated with `kairos_db::tenant::is_valid_slug`, and the connection is looked up in that tenant's schema. Copy SCIM's uniform-failure discipline: **malformed slug, unknown tenant, unknown connection, and bad signature all return the same generic response** so the endpoint cannot be used to enumerate tenants or connections.
- **Signature verification before parsing, over raw bytes**: read the body with `axum::body::Bytes`, not a typed `Json<T>` extractor — HMAC must cover exactly what was sent. GitHub: `X-Hub-Signature-256: sha256=<hex>` compared in **constant time**. GitLab: `X-Gitlab-Token` compared in constant time. Secret is recomputed via KAIROS-T-0097's derivation (`HMAC(server_key, "webhook:" || connection_id)`) — never read from storage.
- **Match + resolve**: `kairos_core::forge` extracts codes → resolve each through `entity_directory` (live-only, A-0001). **Unknown or foreign codes are ignored silently** — a branch may legitimately name a code from another deployment. Log at debug, never warn; do not fail the delivery. An event that matches nothing is a successful no-op 200 (a non-2xx makes forges retry forever and eventually disable the hook).
- **Ordering-safe upsert** — the subtle correctness requirement:
  ```sql
  INSERT INTO item_links (...) VALUES (...)
  ON CONFLICT (connection_id, kind, external_id) DO UPDATE
    SET state = EXCLUDED.state, title = ..., forge_updated_at = EXCLUDED.forge_updated_at, ...
    WHERE item_links.forge_updated_at <= EXCLUDED.forge_updated_at
  ```
  Forges retry and do not guarantee order; without the `WHERE` guard a redelivered "opened" silently regresses a merged PR. Test this explicitly by applying a merge then replaying the earlier open.
- **Re-matching**: a PR edited to add a short code must create the new link; a PR edited to remove one should drop that item's link for this PR (otherwise stale associations accumulate). Decide and record whether removal deletes the row or is out of scope for v1 — recommend delete, since links are derived and cheap to rebuild.
- **Events**: emit a thin `item_links_changed` WS event carrying the affected item short codes so open item/team views refresh through the existing T-0074 pattern. Follow `kairos_db::events` payload conventions.
- **Respond fast**: forges time out (GitHub ~10s). The work is small, so synchronous is fine for v1 — but keep the handler free of anything unbounded, and record that decision.
- **Metrics**: count deliveries by `(forge, outcome)` through the existing A-0013 metrics layer if it can be done cheaply — operators need to see rejected signatures.

## Acceptance Criteria

## Acceptance Criteria

- [x] `POST /webhooks/{forge}/{tenant}/{connection_id}` ingests GitHub and GitLab branch + PR/MR deliveries and creates/updates `item_links`.
- [x] Signature verified over raw bytes in constant time on both forges; a bad signature, unknown connection, unknown tenant, and malformed slug are indistinguishable in the response.
- [x] Deliveries matching no live short code return 2xx and change nothing; unknown event types (pings) likewise.
- [x] **Redelivery and out-of-order safety proven by test**: applying a merge event then replaying an earlier open leaves the link merged.
- [x] A PR edited to add a short code links the new item; the recorded decision on code removal is implemented and tested.
- [x] Thin `item_links_changed` WS event emitted for affected items.
- [x] Server integration test drives synthetic signed deliveries for both forges end to end; `angreal test unit` + `angreal test integration` green.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition.
- 2026-09-02: COMPLETE. `kairos-server/src/forge/webhook.rs` mounted in app.rs beside the SCIM router, outside the auth→tenant stack, non-`/api`. Body read as `Bytes` and the HMAC verified over the raw bytes BEFORE any parsing; GitHub `X-Hub-Signature-256`, GitLab `X-Gitlab-Token`, both constant-time. New `EventKind::ItemLinksChanged` emitted per touched item.
- 2026-09-02: Decisions recorded:
  (1) **Code removal deletes the link** (the ticket's recommendation), but ONLY for pull-request events. A branch push carries no authoritative list of the codes it once mentioned, so pruning on a push would delete links the PR still legitimately holds.
  (2) **Unknown connection resolves to the SAME rejection as a bad signature** — the handler returns `Ok(None)` from the tenant closure and the caller maps it to the uniform 401, so connection existence is not observable.
  (3) **Synchronous handling** kept for v1 (the work is one parse plus a few statements, far inside GitHub's ~10s timeout); no queue, and the handler contains nothing unbounded.
  (4) **Metrics deferred** — the existing A-0013 layer is a generic HTTP middleware with no per-outcome hook, so a `(forge, outcome)` counter would mean new plumbing rather than a cheap addition. Noted for a follow-up rather than done badly here.
- 2026-09-02: Two real bugs the tests caught, both would have shipped silently:
  (1) **A syntactically valid but nonexistent tenant slug 500'd** instead of rejecting — it reached the blocking pool and hit a missing schema, which both leaks the difference and is an ugly error. Now resolved in the PUBLIC schema first (the SCIM approach) and mapped to the uniform rejection.
  (2) **`serde_json` was only a dev-dependency of kairos-core**, so `kairos_core::forge` compiled under `cargo test` but the library alone did not — `angreal test unit` passed while `cargo check -p kairos-server` failed. Promoted to a real dependency.
- 2026-09-02: Verified: new `forge_webhook` integration test drives real signed deliveries — create link → merge → **replay the earlier open and assert it stays merged** (the ordering guard, browser-invisible otherwise) → bad signature / unsigned / unknown connection / unknown tenant all 401 and write nothing → ping and foreign-code deliveries are 2xx no-ops → PR edited to drop the code removes the link. `angreal test unit` clean; `angreal test integration` 34/34 targets green.