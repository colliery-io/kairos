---
id: forge-connections-item-links
level: task
title: "Forge connections + item_links schema, models, and org-admin setup API"
short_code: "KAIROS-T-0097"
created_at: 2026-09-01T23:12:29.236671+00:00
updated_at: 2026-09-02T10:06:09.610458+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

- [x] Migration pair creates both tables with the partial unique/lookup indexes; `down.sql` drops in FK order; `tenant_provisioning` EXPECTED_TABLES/INDEXES updated (count constant bumped).
- [x] `text_enum!` types for `forge`, `kind`, and `state` with round-trip tests; models follow the Row/New/Changeset convention.
- [x] Org-admin CRUD + rotate exist, registered in openapi, with kairos-client DTOs and methods; non-admins get the standard 403 naming the required capability; reads open tenant-wide.
- [x] Creating a connection returns `webhook_url` and `webhook_secret`; the secret is **never** stored and never returned again by any endpoint; rotate produces a different connection id and a different secret.
- [x] Secret derivation is a pure unit-tested function; config value documented alongside the other A-0013 env vars.
- [x] Server integration test: create → list (no secret) → duplicate `(forge, repo)` rejected → rotate changes both → delete → 404. `angreal test unit` + `angreal test integration` green.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition (PO decisions: webhooks, links-only, both forges).
- 2026-09-02: COMPLETE. Shipped: migration `2026-09-01-000000_forge_links` (both tables, partial unique on `(forge, repo_full_name)` WHERE live, `item_links` upsert index, no FK on `item_id` per the shared-UUID convention); `Forge`/`LinkKind`/`LinkState` text_enums with round-trip tests; `models/forge.rs`; `kairos-db/src/forge.rs` services with typed `ForgeError`; `api/org/forge.rs` (list/get/create/patch/delete/rotate, org-admin writes via the `MANAGE` + `board_id: None` gate, reads open); `types_forge.rs` DTOs + 6 client methods; openapi registered.
- 2026-09-02: Decisions recorded (deviations from the ticket's guesses):
  (1) **Secret derivation lives in `kairos-server/src/forge/auth.rs`, NOT kairos-core** — core is deliberately dependency-light (no sha2/hmac) and both existing credential modules (`scim/auth.rs`, `service_accounts/auth.rs`) live in the server. Added `hmac` to kairos-server. Unit-tested: determinism, key/id sensitivity, 64-hex length, GitHub signature shape, constant-time compare.
  (2) **Missing config is a 501 `FORGE_NOT_CONFIGURED` / `PUBLIC_URL_NOT_CONFIGURED`, not a startup error** — the integration is optional, so a deployment without it should boot fine and only refuse the forge endpoints.
  (3) **Nothing in config knew the deployment's external URL**, so `KAIROS_PUBLIC_URL` is new. Deliberately not inferred from the `Host` header: that is attacker-controlled and the value ends up pasted into a third party.
  (4) **Rotation mints a new connection row** (delete + create in one transaction, same repo) because the secret is derived from the id — so the delivery URL changes too and the operator updates both fields. The partial unique index never sees two live rows for one repo.
  (5) **The guarded upsert is raw SQL** (sanctioned under A-0009, as search.rs/graph.rs do): diesel's builder cannot express `DO UPDATE ... WHERE`, and select-then-write would race concurrent deliveries for the same PR.
- 2026-09-02: Also re-pinned the `tenant_provisioning` upgrade-path simulation from `team_pages` to `forge_links` — the exact recurring chore KAIROS-T-0093 exists to remove; left a comment there naming that ticket. Verified: `angreal test unit` clean, `angreal test integration` 33/33 targets green (new `forge_connections` test covers the permission matrix, derived-secret reproducibility, duplicate-repo 409, team attribution + clear, rotation changing id/URL/secret, and repo reuse after disconnect).