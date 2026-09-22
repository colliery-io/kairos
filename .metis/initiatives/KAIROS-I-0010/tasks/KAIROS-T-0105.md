---
id: abac-computed-file-backlog
level: task
title: "ABAC: computed file_backlog capability for cross-team Backlog filing (A-0006 amendment)"
short_code: "KAIROS-T-0105"
created_at: 2026-09-22T03:04:41+00:00
updated_at: 2026-09-22T04:00:52.121116+00:00
parent: KAIROS-I-0010
blocked_by: [KAIROS-T-0104]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# ABAC: computed file_backlog capability for cross-team Backlog filing (A-0006 amendment)

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Implements **§D3**. Decision: [[KAIROS-A-0019]] §4, amending [[KAIROS-A-0006]] (as T-0072 did).

## Objective

Any authenticated tenant member — human or service account — can create a task against another team's repository, and it lands in that team's delivery-board Backlog. Nothing else opens up: not other columns, not transitions, not edits.

## Implementation Notes

- `crates/kairos-core/src/abac.rs`: `pub const FILE_BACKLOG: &str = "file_backlog"`; it is **not** in `TEAM_IMPLIED_CAPABILITIES` and never appears in `board_member_capabilities`. Add `is_computed(cap)` or equivalent so grant/revoke endpoints refuse to store it (400).
- `crates/kairos-db/src/abac.rs::check_capability`: third arm per §D3 — bind `$5 = required == FILE_BACKLOG`; satisfied when the board is `level = 'delivery'` and the caller is a member of the org (`public.organization_members`, see `is_org_admin` for the join). Keep it one query.
- `api/tasks.rs::create_task` and `mcp/tools.rs::create_item_impl`: after §D2 resolution, decide the capability to require:
  `MANAGE_TASKS` unless (task ∧ `repository_id` set ∧ resolved column is the board's position-0 column ∧ caller lacks `MANAGE_TASKS` there) → require `FILE_BACKLOG`. Do it as one helper in `kairos-server` so both entry points share it. Explicitly-requested non-Backlog column by a non-member → 403, not silently re-routed.
- `whoami` (API + MCP): add `implicit: ["file_backlog"]` (list, so later computed caps slot in) — the agent should be able to see it.
- Negative test suite in `crates/kairos-server/tests` following the tenant-isolation style: non-member cannot file into position ≥ 1; cannot file without a repo; cannot file with a repo owned by a different team than the target board; cannot `transition`, `update`, `edit`, `delete`, `set_metadata`, `set_work_class`, `set repository` on what they filed; a service-account key (A-0017) *can* file; an org member of a **different tenant** cannot (isolation unchanged).
- Docs: one paragraph in the ABAC section of the operator docs; the ADR already records the rationale.

### Dependencies
T-0104 (routing must exist so the target board/column are resolved before the check).

### Risk Considerations
This is the one deliberate widening of A-0006. The test suite is the contract; make it exhaustive over the write surface rather than sampling.

## Acceptance Criteria

- [x] The arm lives in `kairos_db::abac::authorize` (which has the org slug for the membership check) as `check_file_backlog`, not inside `check_capability`'s single SQL statement — keeps that query single-purpose. Covered end to end by the integration suite (member × delivery board = allowed; non-member tenant = refused; the non-delivery-board case cannot arise because `resolve_routing` only ever routes to a delivery board). Core unit test: `file_backlog_is_computed_never_grantable_never_team_implied`.
- [x] Granting `file_backlog` via `POST /api/boards/{id}/members` → 422 (not in `CAPABILITY_VOCABULARY`; also `kairos_core::abac::is_computed`). Nothing to revoke since nothing can be stored.
- [x] `tests/file_backlog.rs`: positive over HTTP (alice/web → platform Backlog, `created_by` = alice; carol on no team; a service-account key; explicit Backlog column) and over MCP (`create_item` with `repository`). Negative: non-Backlog column 403; repo-less create 403; repo/board mismatch 422; on the filed task — transition, edit, delete, work-class, set-repository, metadata all 403; owning team can transition; another tenant cannot reach the repo; MCP repo-less create on the foreign board refused; `repository` on a non-task rejected.
- [x] `whoami` (HTTP DTO `implicit: Vec<String>`; MCP text) shows `file_backlog`.
- [x] fmt, clippy `-D warnings` on the touched crates, `angreal test unit`, `angreal test integration` 36/36 green.

## Status Updates

- 2026-09-22: Done and committed (`4c4dca3`). Notes for downstream:
  - `crate::api::tasks::require_task_create_capability(conn, slug, user, &route, column_id)` is THE gate for task creation; MCP `create_item_impl` now calls `resolve_routing` + this helper for every task (repo or not), and gained the `repository` param (T-0107 has one less thing to do; it still owns `list_repositories`/`get_repository`/filters/whoami repos).
  - The docs paragraph in the AC has no home yet — `docs/` has no ABAC reference page. T-0110 creates it and documents `file_backlog` there.
  - MCP tests can drive the spawned server over HTTP with a ~60-line `McpSession` (initialize → initialized → tools/call, SSE-or-JSON decode); reusable for T-0107.