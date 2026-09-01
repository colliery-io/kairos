---
id: forge-connections-item-links
level: task
title: "Forge connections + item_links schema, models, and org-admin setup API"
short_code: "KAIROS-T-0097"
created_at: 2026-09-01T23:12:29.236671+00:00
updated_at: 2026-09-01T23:12:29.236671+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0009
---

# Forge connections + item_links schema, models, and org-admin setup API

## Parent Initiative

[[KAIROS-I-0009]] — Git Forge Integration. First task; everything else stores into or reads from this schema.

## Objective

The persistence and setup half: `forge_connections` + `item_links` tables, models, and the org-admin API that registers a repo and hands back its webhook delivery URL and secret.

## Implementation Notes

- **Follow the team_pages skeleton end to end** (KAIROS-T-0082/0083 is the most recent worked example): migration pair → `schema.rs` (`table!`, `joinable!`, `allow_tables_to_appear_in_same_query!`) → `models/forge.rs` (Row + `New*` + `*Changeset`) → `models/mod.rs` registration → db service module with a typed error enum → server module → client DTOs + methods → openapi paths → `EXPECTED_TABLES`/`EXPECTED_INDEXES` in `crates/kairos-db/tests/tenant_provisioning.rs`.
- **`forge_connections`** (tenant schema): `id`, `forge` (`github|gitlab` — `text_enum!` + round-trip test, the T-0082 `TeamPageKind` precedent), `repo_full_name` (`acme/payments-api`), `repo_url`, `team_id` nullable FK → `teams`, `created_by`, `created_at`, `updated_at`, `deleted_at`. Unique index on `(forge, repo_full_name)` WHERE `deleted_at IS NULL` (the partial-index convention).
- **`item_links`**: `id`, `item_id UUID NOT NULL` (**no FK** — shared UUID space, the `item_metadata` convention), `connection_id` FK → `forge_connections`, `kind` (`branch|pull_request`, `text_enum!`), `external_id` (PR/MR number, or the branch ref), `title`, `url`, `state` (`open|merged|closed|draft`, `text_enum!`), `author`, `forge_updated_at TIMESTAMPTZ` (the ordering guard — see KAIROS-T-0099), `created_at`, `updated_at`. **UNIQUE `(connection_id, kind, external_id)`** — the upsert key. Index `item_id` for the item panel.
- **No `version` column and no history table**: links are derived data mirrored from the forge, so A-0004 optimistic concurrency does not apply (Kairos is not the author). Record that decision in the module docs so the next reader does not "fix" it.
- **Secret derivation** (`kairos-core`, pure + unit-tested): `secret = HMAC-SHA256(server_signing_key, "webhook:" || connection_id)` — hex-encoded. New config value `KAIROS_WEBHOOK_SIGNING_KEY` in `crates/kairos-server/src/config.rs` following the A-0013 env conventions (required only when at least one connection exists — decide and record whether absence is a startup error or a per-request 503). **Nothing secret is persisted**; the secret is displayed exactly once on create, like an API key.
- **Setup API** (`crates/kairos-server/src/api/org/forge.rs`), org-admin-gated with the `MANAGE` pseudo-capability and `board_id: None` — the same gate `service_accounts/routes.rs` and the teams/streams families use:
  - `GET /api/forge-connections` (list; secrets never returned)
  - `POST /api/forge-connections` → 201 carrying `webhook_url` + `webhook_secret` **once**
  - `GET /api/forge-connections/{id}` / `DELETE` (soft delete)
  - `POST /api/forge-connections/{id}/rotate` — mints a new connection id (per the design: rotation is a new id, since the secret is derived from it), returning the new URL + secret.
- `webhook_url` is rendered from the deployment's public base URL — check what config already knows about its own external URL; if nothing does, this needs a config value rather than guessing from the request Host header (record which).
- Reads follow A-0006's open-tenant-wide rule; only writes are admin-gated.

## Acceptance Criteria

- [ ] Migration pair creates both tables with the partial unique/lookup indexes; `down.sql` drops in FK order; `tenant_provisioning` EXPECTED_TABLES/INDEXES updated (count constant bumped).
- [ ] `text_enum!` types for `forge`, `kind`, and `state` with round-trip tests; models follow the Row/New/Changeset convention.
- [ ] Org-admin CRUD + rotate exist, registered in openapi, with kairos-client DTOs and methods; non-admins get the standard 403 naming the required capability; reads open tenant-wide.
- [ ] Creating a connection returns `webhook_url` and `webhook_secret`; the secret is **never** stored and never returned again by any endpoint; rotate produces a different connection id and a different secret.
- [ ] Secret derivation is a pure unit-tested function; config value documented alongside the other A-0013 env vars.
- [ ] Server integration test: create → list (no secret) → duplicate `(forge, repo)` rejected → rotate changes both → delete → 404. `angreal test unit` + `angreal test integration` green.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition (PO decisions: webhooks, links-only, both forges).
