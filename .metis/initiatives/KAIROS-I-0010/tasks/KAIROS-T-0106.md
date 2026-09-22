---
id: repository-api-api-repositories
level: task
title: "Repository API: /api/repositories CRUD, forge-connections re-keyed on repository, DTOs"
short_code: "KAIROS-T-0106"
created_at: 2026-09-22T03:04:42+00:00
updated_at: 2026-09-22T04:18:35.789696+00:00
parent: KAIROS-I-0010
blocked_by: [KAIROS-T-0103]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Repository API: /api/repositories CRUD, forge-connections re-keyed on repository, DTOs

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Implements **§D4** (the repository routes; the board/search filters are in T-0104). Decision: [[KAIROS-A-0019]] §1.

## Objective

Repositories are readable by any tenant member and manageable through the API; forge connections hang off a repository instead of carrying their own repo fields.

## Implementation Notes

- New `crates/kairos-server/src/api/repositories.rs`, mounted in `api/mod.rs`; routes and capabilities exactly per the §D4 table:
  - `GET /api/repositories?team=` — list with owner team `{id, slug, name}`, delivery board `{id, slug, name}`, `open_tasks` (tasks not in an `is_done` column, T-0077's flag), `has_webhook`.
  - `GET /api/repositories/{slug}` — plus `description`, `default_branch`, `connection: Option<{id}>`, `in_flight` (reuse the T-0101 `team_link_rollup` query narrowed to this repo — add a `repository_id` filter to it in `kairos-db/src/forge.rs`).
  - `POST` — org admin **or** `manage_tasks` on the owning team's delivery board (decided: self-serve registration). Slug validated `^[a-z0-9][a-z0-9-]{1,62}$`; forge-name uniqueness → 409.
  - `PATCH /{slug}` — same gate evaluated on the *current* owner; `team_id` change allowed, does not touch tasks.
  - `DELETE /{slug}` — org admin; 409 while live tasks or a live connection reference it (the db layer's typed error → `ApiError::conflict`).
- `api/org/forge_connections.rs` (wherever `/api/forge-connections` lives — `forge/mod.rs` says "with the other org-admin routes"): `POST` body becomes `{ repository: <slug|uuid> }`; `PATCH` drops `team_id`; responses carry `repository: RepositoryRef`. `dto::ForgeConnection` / `CreateForgeConnectionRequest` updated; `derive_secret` and the webhook path untouched.
- Webhook ingestion (`forge/webhook.rs`, T-0099) resolves the connection by id as today; where it used `connection.repo_full_name` for logging/verification, read it via the repo join.
- `dto::Repository`, `dto::RepositoryRef`, `dto::CreateRepositoryRequest`, `dto::UpdateRepositoryRequest`; OpenAPI; `kairos-client` methods `list_repositories`, `get_repository`, `create_repository`, `update_repository`, `delete_repository`, and the changed forge-connection calls.

### Dependencies
T-0103. Independent of T-0104/T-0105 (can run in parallel with them). Blocks T-0107, T-0109.

### Risk Considerations
The forge-connection body change breaks the e2e `createForgeConnection` helper and `forge.spec`; T-0110 owns that fallout, but keep the old field names out of the new DTO so the break is loud, not silent.

## Acceptance Criteria

- [x] `tests/repositories_api.rs`: list (by slug, `?team=` slug/UUID, unknown team 422) and detail shapes; self-serve POST by a team member (team by slug and by UUID) succeeds, unrelated member → 403, admin anything; bad slug 422, slug taken 409, forge name taken 409, unknown team 422, unknown forge 422; PATCH gated on the *current* owner, re-home + rename by admin flips who may edit; DELETE owner-not-enough 403, referenced 409 (connection, then task), succeeds once unbound, slug/name free again.
- [x] `forge_connections.rs` and `forge_webhook.rs` rewritten to the repository-first flow (`{ repository }` body, `other`-forge repo cannot be connected, ownership edited on the repo); everything about links, rollup paths and webhook ingestion asserts exactly what it did before.
- [x] `GET /api/repositories/{slug}.in_flight` equals the team rollup for the single-repo team after a real signed PR delivery.
- [x] Paths registered in OpenAPI (`tests/openapi.rs` completeness check green); kairos-client has the five repository methods; PATCH forge-connection method removed.
- [x] fmt, clippy `-D warnings` on the touched crates, `angreal test unit`, `angreal test integration` 37/37 green.

## Status Updates

- 2026-09-22: Done and committed (`4231b84`). Notes for downstream:
  - Route module is `api/org/repositories.rs` (with the other org-level families); `map_error` there is `pub(crate)` and the forge API reuses it.
  - Team references on the wire are UUID **or slug** (`team` field) — same as `repository` everywhere else.
  - `GET /api/forge-connections/{id}` PATCH is gone; `UpdateForgeConnectionRequest` deleted from the client. The web GUI never called it (T-0109 builds the admin page fresh).
  - Deliberately left broken for T-0110: `e2e/helpers/api.ts::createForgeConnection` still sends the old body — the break is loud on the next `angreal test e2e`.
  - `Repository.delivery_board_id` is `Option` only for misconfigured tenants; T-0107's `list_repositories` can render it as the board an agent files onto.