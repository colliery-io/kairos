---
id: abac-computed-file-backlog
level: task
title: "ABAC: computed file_backlog capability for cross-team Backlog filing (A-0006 amendment)"
short_code: "KAIROS-T-0105"
created_at: 2026-09-22T03:04:41.000000+00:00
updated_at: 2026-09-22T03:04:41.000000+00:00
parent: KAIROS-I-0010
blocked_by: ["KAIROS-T-0104"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] `check_capability` unit tests for the new arm (member/non-member × delivery/non-delivery board × `file_backlog`/other cap).
- [ ] Granting or revoking `file_backlog` via the capability endpoints is refused.
- [ ] Full negative suite above passes; positive case (member of team B files against team A's repo, task appears in A's Backlog with `created_by` = B's user) passes over HTTP and MCP.
- [ ] `whoami` shows `implicit: ["file_backlog"]`.
- [ ] fmt/clippy/unit/integration green.

## Status Updates

*To be added during implementation*
