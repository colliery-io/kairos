---
id: repositories-schema-table-forge
level: task
title: "Repositories schema: table, forge_connections re-parent + backfill, tasks.repository_id, models, query module, seed"
short_code: "KAIROS-T-0103"
created_at: 2026-09-22T03:04:39.409090+00:00
updated_at: 2026-09-22T03:08:20.529154+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/active"


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

## Acceptance Criteria

- [ ] Migration applies to a fresh tenant and to a tenant carrying pre-existing forge connections and links; `down.sql` reverses it cleanly.
- [ ] A team-less live connection fails the migration with its id in the error.
- [ ] `repositories` unit tests: slug/forge-name uniqueness (partial), `soft_delete` refusal while referenced, `delivery_board_for_team` none/one/several.
- [ ] Every existing forge test (`kairos-db` forge module, T-0100/T-0101 server tests) passes unchanged in what it asserts.
- [ ] `seed_demo` produces the fixtures above; `SeedDemoReport.repositories` is non-zero.
- [ ] `cargo fmt --check`, `cargo clippy -- -D warnings`, `angreal test unit && angreal test integration` green.

## Status Updates

*To be added during implementation*