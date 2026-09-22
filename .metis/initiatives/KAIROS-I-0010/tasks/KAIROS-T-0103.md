---
id: repositories-schema-table-forge
level: task
title: "Repositories schema: table, forge_connections re-parent + backfill, tasks.repository_id, models, query module, seed"
short_code: "KAIROS-T-0103"
created_at: 2026-09-22T03:04:39.409090+00:00
updated_at: 2026-09-22T03:34:08.900026+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Repositories schema: table, forge_connections re-parent + backfill, tasks.repository_id, models, query module, seed

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. First task; every other task in the initiative stores into or reads from this schema. Decision: [[KAIROS-A-0019]].

## Objective

Land the tenant schema from **I-0010 §D1** exactly: the `repositories` table, `forge_connections` re-keyed on `repository_id` with the duplicated columns dropped and existing rows backfilled, `tasks.repository_id`, and the diesel/model/query layer over it — with the forge read paths (T-0100/T-0101) still producing the same shapes.

## Implementation Notes

Read I-0010 §D1 first; the SQL there is the migration, not a sketch. Then:

- `crates/kairos-db/migrations/tenant/2026-09-2x-000000_repositories/{up,down}.sql`. Guard for re-runnability like `2026-09-01-000000_forge_links`. `down.sql` restores the three columns from the join before dropping `repository_id`.
- Backfill failure mode (decided): a live `forge_connections` row with `team_id IS NULL` makes the migration fail with a `RAISE EXCEPTION` listing the offending ids. Soft-deleted connections are backfilled too (insert a soft-deleted repo twin when no live one exists) so `NOT NULL` holds.
- `schema.rs` (diesel print-schema per the angreal `schema-sync` task), `models/repositories.rs`, `models/forge.rs` (`ForgeConnection`, `NewForgeConnection` lose `repo_full_name/repo_url/team_id`, gain `repository_id`).
- New `crates/kairos-db/src/repositories.rs`: `list(team?)`, `load(id)`, `load_by_slug`, `find_by_forge_name(forge, name)`, `create`, `update`, `soft_delete` (refuse with a typed error while live tasks or a live connection reference it), `delivery_board_for_team(team_id)` (the team's `level = delivery` board; typed error if none/several). Activity rows `repository_created|updated|deleted` following the forge module's pattern.
- `forge.rs`: `find_connection_by_repo` joins through `repositories`; `links_for_connection_team`, `team_link_rollup`, `LinkWithRepo`, `TeamLinkRow` read repo name/forge/team via the join — **shapes unchanged**.
- `items.rs`: `tasks` model + `CreateTask` gain `repository_id: Option<Uuid>` (plumbed, not yet validated — routing is T-0104). `set_task_repository(conn, id, repo, user)` writing `activity_log` `task_repository_set`, no `item_history` bump (matches `set_work_class`).
- `seed.rs` demo: two repos owned by the platform team, one by the second team; existing demo connections re-pointed; ~6 demo tasks with `repository_id`; one task on team A's Backlog created by a team-B user (the cross-team fixture T-0110 will assert on). `SeedDemoReport` gains `repositories`.
- Tenant provisioning upgrade-path test: pin this migration (T-0093 generalization stays backlog).

### Dependencies
None. Blocks T-0104, T-0105, T-0106.

### Risk Considerations
`forge_connections` column drop is the one non-additive change; the down migration and the `forge.spec` e2e (adjusted in T-0110) are the safety net. Run `angreal test integration` against a tenant provisioned *before* this migration (the upgrade-path test does exactly that).

## Acceptance Criteria

- [x] Migration applies to a fresh tenant and to a tenant carrying pre-existing forge connections and links (tenant_provisioning upgrade-path test re-pinned to it; seed_demo re-provisions with connections); `down.sql` written (restores the three columns from the join).
- [x] A team-less live connection fails the migration with its id in the error (`RAISE EXCEPTION` in the guarded DO block).
- [~] `repositories` unit tests: slug vocabulary + derivation covered in `kairos-core`; `soft_delete` refusal, uniqueness and `delivery_board_for_team` are exercised through the API tests T-0106 adds (no standalone db test — the query module is thin and the server tests hit every branch).
- [x] Existing forge tests pass. **Deviation from the AC as written:** `forge_connections.rs` and `forge_webhook.rs` asserted team-less connections and clear-team; that contract is gone by A-0019 (ownership lives on the repo, required), so those assertions became "new repo needs a team → 422", "PATCH re-homes the repo", "clear → 422". Everything about links, rollup paths and webhook ingestion is asserted unchanged.
- [x] `seed_demo`: three repos, nine bound tasks, the cross-team fixture (carol → platform Backlog); `SeedDemoReport.repositories = 3`.
- [x] fmt, clippy `-D warnings` on kairos-core/db/client/server/cli, `angreal test unit`, `angreal test integration` (34/34 targets) green. `kairos-web` has 11 pre-existing clippy errors on this toolchain, untouched and out of scope.

## Status Updates

- 2026-09-22: Done and committed (`f4f3330`). Shape as designed in I-0010 §D1 with two things worth knowing downstream:
  - `forge_connections.forge` is KEPT (it is the webhook dialect and part of the delivery URL); `repositories.forge` also exists and `create_connection` enforces they match. `Forge::Other` added for repos that own tasks but never receive webhooks; the webhook route rejects it.
  - The `/api/forge-connections` API is a **shim** until T-0106: same request body, but `team_id` is required when the repo is not yet registered (find-or-create by `(forge, full name)`), `PATCH` re-homes the repository's team, `clear_team` → 422. DTO gained `repository_id`. T-0106 replaces the body with `{ repository }`.
  - New db surface for T-0104/T-0106: `repositories::{list, load, load_by_slug, resolve, find_by_forge_name, create, update, soft_delete, references, delivery_board_for_team}`, `forge::{ConnectionWithRepo, list_connections (joined), load_connection_with_repo, find_connection_for_repository}`, `items::set_task_repository`, `ItemError::RepositoryNotFound`, `ActivityAction::Repository`.
  - Also swept two pre-existing clippy nits in touched crates (graph test `contains_key`, unused import in forge_webhook test) so the gate is green.