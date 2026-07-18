---
id: m2-boards-teams-streams-members
level: task
title: "M2: Boards, teams, streams, members, admin endpoints"
short_code: "KAIROS-T-0019"
created_at: 2026-07-10T01:08:27.428717+00:00
updated_at: 2026-07-10T09:19:43.117300+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0017, KAIROS-T-0011]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: Boards, teams, streams, members, admin endpoints

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Organizational + admin endpoint families per S-0005: boards (CRUD + columns + transitions config), teams (+members), delivery streams (+teams), board member capabilities, and admin tenant provisioning.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Boards family incl. column/transition config with T-0010 rules (non-empty column removal → 422 etc.); board items view GET /api/boards/{id}/items grouped by column
- [x] Teams/members, delivery-streams/teams, board-members capability endpoints (grant/revoke semantics from T-0011, org-admin-only where A-0006 says so); team creation creates the team's delivery board (per A-0002/T-0010 deferred-delivery-board decision)
- [x] POST/GET/DELETE /api/admin/tenants wired to T-0008 provisioning (org-admin/deployment-admin gated — document the chosen authority model for deployment-level admin in the task doc)
- [x] Integration tests: config rule violations, capability grant/revoke via API, tenant lifecycle via API
- [x] SCOPE ADDITION (orchestrator/Dylan day-zero review, 2026-07-10; S-0005 amendment pending): POST /api/admin/tenants accepts optional `initial_admin_external_id` (defaults to the caller's external_id); after provisioning, insert the org-admin `organization_members` row; unknown user → 422 "authenticate once first"; response includes the created admin membership
- [x] SCOPE ADDITION (same review): organization membership family — GET /api/members (paginated), POST /api/members {email, role} (unknown email → 404 with log-in-once guidance; duplicate → 409), PATCH /api/members/{user_id} {role} and DELETE /api/members/{user_id}, both guarded by 422 LAST_ADMIN; writes org-admin gated
- [x] SCOPE ADDITION integration coverage: tenant create → caller is admin and can immediately act in the new tenant (grant a capability); member add-by-email lifecycle incl. unknown-email 404; last-admin guard on both PATCH and DELETE

## Implementation Notes

References S-0005, A-0002, A-0006. The deployment-admin question (who may create tenants when no org exists yet) must be resolved with a documented, non-improvised answer consistent with A-0010's service accounts — surface options in Status Updates if genuinely ambiguous.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-10: Active. Read S-0005 (Boards/Teams/Delivery Streams/Board Authorization/Tenant Provisioning), A-0002/A-0006/A-0010, kairos-db boards/abac/tenant services, T-0018 handler pattern. Plan: new modules `kairos-server/src/api/org/{mod,boards,teams,streams,members,admin}.rs` + `api/convert_org.rs`; DTOs in new `kairos-client/src/types_org.rs`; new test `kairos-server/tests/org_endpoints.rs` (scratch DB `kairos_org_t0019_test`).
- 2026-07-10: **DEPLOYMENT-ADMIN DECISION** (documented, consistent with A-0010 service accounts; addendum candidate for A-0010): `/api/admin/tenants` routes bypass tenant middleware (they are cross-tenant) but sit behind the auth middleware (valid bearer token + JIT user provisioning). Authorization: the authenticated principal's `external_id` (OIDC `sub` — a human user OR an A-0010 service-account sub, both land in `public.users.external_id`) must be listed in `KAIROS_DEPLOYMENT_ADMINS` (comma-separated subs; A-0013 env conventions, parsed into `AppConfig.deployment_admins`). Empty/unset → the routes always return 403 FORBIDDEN. No org membership is consulted (a fresh deployment has no orgs yet). **This is the entire authority model — nothing further invented.**
- 2026-07-10: Other documented interpretations: (a) team creation creates the team's delivery board from `system_board_defaults` (T-0010 deferred decision) in the same transaction; board slug `{team_slug}-delivery`, response carries `delivery_board_id`; team DELETE requires the delivery board to be empty (422 BOARD_NOT_EMPTY) and soft-deletes team + board together. (b) Board create/delete and teams/streams/org-members writes are org-admin gated via the A-0006 tenant-config fallback (403 names the pseudo-capability, `board_id: null`); board config writes (PATCH board, columns, transitions) gated by `configure_boards` on that board; board-members writes by `manage_members`. Reads stay open tenant-wide per A-0006 (incl. GET /api/members — the amendment gates the family's writes). (c) T-0010 rule errors → 422 codes: COLUMN_NOT_EMPTY, DUPLICATE_COLUMN_NAME, DUPLICATE_COLUMN_POSITION, DUPLICATE_TRANSITION, BOARD_NOT_EMPTY (board delete); malformed refs → 422 VALIDATION. (d) Capability vocabulary validated at the API layer (A-0006 fixed set + `*`/`manage_*`/`configure_*`/`transition_*` globs); unknown values → 422 VALIDATION. (e) Membership activity rows: action `create`/`delete`, `entity_type='membership'`; role changes log action `create` with details `membership_role:{old}->{new} user:{id}` (existing ActivityAction vocabulary has no generic update action).
- 2026-07-10: IMPLEMENTED. New files: `crates/kairos-client/src/types_org.rs` (all T-0019 DTOs), `crates/kairos-server/src/api/convert_org.rs`, `crates/kairos-server/src/api/org/{mod,boards,teams,streams,members,admin}.rs`, `crates/kairos-server/tests/org_endpoints.rs`. Existing-file touches: one `pub mod types_org;` in kairos-client lib.rs; `pub mod convert_org;`+`pub mod org;` in api/mod.rs; two app.rs insertions (org router after entities merge inside the protected stack; admin router after `.merge(protected)`, auth-only); `AppConfig.deployment_admins` + `KAIROS_DEPLOYMENT_ADMINS` parsing in config.rs; one-line `deployment_admins: vec![]` compile fixes in middleware/tenant.rs tests and tests/common base_config. Admin handlers use a per-request sync connection (`run_admin` in api/org/admin.rs) rather than the tenant-pinned blocking pool. `delete_tenant` clears `organization_members` rows in the same transaction as `drop_tenant` (the org row FK has no ON DELETE CASCADE; without this the drop 500s now that creation seeds a membership).
- 2026-07-10: SCOPE ADDITION delivered as specified: `initial_admin_external_id` (defaults to caller; 422 `"...must authenticate once first"` for unknown subs; membership insert shares the provisioning transaction; response carries `initial_admin`), and the `/api/members` family with 404 log-in-once guidance, 409 duplicates, 422 `LAST_ADMIN` on PATCH and DELETE.
- 2026-07-10: EVIDENCE (all on the live shared stack, scratch DB `kairos_org_t0019_test`):
  - `cargo test -p kairos-server --test org_endpoints` → `test org_and_admin_endpoints_against_live_stack ... ok. 1 passed; 0 failed` — single lifecycle test covering: deployment-admin gate (empty `KAIROS_DEPLOYMENT_ADMINS` → 403 always; non-listed caller → 403; listed caller → 201), tenant create → caller is org admin (whoami role=admin) and immediately grants capabilities via API; duplicate slug 409, invalid slug 422, unknown initial admin 422; member add-by-email (201), unknown email 404 with "log in once", duplicate 409, non-admin 403 naming `manage_org_members`, LAST_ADMIN 422 on PATCH and DELETE of the sole admin, remove→MEMBERSHIP_REQUIRED; board config (column add 201, DUPLICATE_COLUMN_NAME/DUPLICATE_COLUMN_POSITION/DUPLICATE_TRANSITION 422s, COLUMN_NOT_EMPTY 422 with `details.item_count`, PATCH rename+reorder, transition delete by id + 404, board create-from-defaults 201, BOARD_NOT_EMPTY 422, empty-board delete 200→404); team create → delivery board exists with the seeded delivery defaults (Backlog/Todo/Blocked/Active/Completed, 7 transitions) and `team_id` set; team members add/dup-409/unknown-422/remove/404; stream CRUD + team membership add/dup/list/remove/404; capability admin via API (bob 403 → grant `manage_strategies`+`transition_items` → 201; PATCH replace revokes `transition_items` → transition 403 naming it; DELETE revokes all → create 403 again; PATCH non-member 404; unknown capability 422); board items view grouped by column across all four entity arrays (strategy board: 2 strategies in Draft, all other groups empty; delivery board: task in Backlog); tenant teardown (`DELETE` without confirm → 422 CONFIRMATION_REQUIRED, non-admin → 403, with `?confirm=true` → 200 dropped, repeat → 404, list reflects).
  - `cargo fmt --check` → clean (exit 0). `cargo clippy --workspace --all-targets -- -D warnings` → `Finished` with zero warnings. `cargo test -p kairos-server` → 19 unit + 4 bin + entities ok + meta ok (T-0020's) + middleware ok + org_endpoints ok, 0 failed. `cargo build -p kairos-client` → Finished; `cargo test -p kairos-client` → 4 passed.
  - Services left UP (shared with the concurrent agent). Full `angreal test unit|integration` gate deliberately deferred to the orchestrator per shared-services instructions.
- 2026-07-10: ADDENDUM CANDIDATE for KAIROS-A-0010/A-0016: record the deployment-admin model (`KAIROS_DEPLOYMENT_ADMINS` = comma-separated OIDC subs, human or service-account, gating the cross-tenant `/api/admin/tenants` routes; empty → routes disabled with 403).
- 2026-07-10: Lane deviations (necessary, minimal): `config.rs` gains the `deployment_admins` field + `KAIROS_DEPLOYMENT_ADMINS` parsing (mandated by the task spec); the AppConfig struct literals in `middleware/tenant.rs` tests and `tests/common/mod.rs::base_config` each need the one-line `deployment_admins: vec![]` to keep compiling. app.rs needs TWO insertions (org router inside the protected stack after the entities merge; admin router merged after `.merge(protected)` since it must bypass tenant middleware) — distinct anchors from the concurrent agent's.