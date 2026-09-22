---
id: repository-api-crud-forge
level: task
title: "Repository API: /api/repositories CRUD, forge-connections re-keyed on repository, DTOs"
short_code: "KAIROS-T-0106"
created_at: 2026-09-22T03:04:42.000000+00:00
updated_at: 2026-09-22T03:04:42.000000+00:00
parent: KAIROS-I-0010
blocked_by: ["KAIROS-T-0103"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] Integration tests per route: list/get shapes, self-serve POST by a team-board `manage_tasks` holder succeeds, POST by an unrelated member → 403, duplicate forge name → 409, slug validation → 422, PATCH re-home, DELETE refused while referenced then succeeds after.
- [ ] Forge connection create/patch/list tests updated; webhook ingestion tests (T-0099) unchanged in what they assert.
- [ ] `GET /api/repositories/{slug}.in_flight` matches the team rollup for a single-repo team.
- [ ] OpenAPI regenerated; kairos-client compiles and its integration suite is green.
- [ ] fmt/clippy/unit/integration green.

## Status Updates

*To be added during implementation*
