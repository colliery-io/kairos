---
id: webhook-endpoint-tenant-routing
level: task
title: "Webhook endpoint: tenant routing, signature verification, ordering-safe upsert"
short_code: "KAIROS-T-0099"
created_at: 2026-09-01T23:12:31.882520+00:00
updated_at: 2026-09-01T23:12:31.882520+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] `POST /webhooks/{forge}/{tenant}/{connection_id}` ingests GitHub and GitLab branch + PR/MR deliveries and creates/updates `item_links`.
- [ ] Signature verified over raw bytes in constant time on both forges; a bad signature, unknown connection, unknown tenant, and malformed slug are indistinguishable in the response.
- [ ] Deliveries matching no live short code return 2xx and change nothing; unknown event types (pings) likewise.
- [ ] **Redelivery and out-of-order safety proven by test**: applying a merge event then replaying an earlier open leaves the link merged.
- [ ] A PR edited to add a short code links the new item; the recorded decision on code removal is implemented and tested.
- [ ] Thin `item_links_changed` WS event emitted for affected items.
- [ ] Server integration test drives synthetic signed deliveries for both forges end to end; `angreal test unit` + `angreal test integration` green.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition.
