---
id: task-repo-binding-create-time
level: task
title: "Task repo binding: create-time routing, board-consistency rule, set-repository endpoint, repo filter on board items and search"
short_code: "KAIROS-T-0104"
created_at: 2026-09-22T03:04:40.000000+00:00
updated_at: 2026-09-22T03:04:40.000000+00:00
parent: KAIROS-I-0010
blocked_by: ["KAIROS-T-0103"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Task repo binding: create-time routing, board-consistency rule, set-repository endpoint, repo filter on board items and search

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Implements **§D2** plus the read-side filters from §D4. Decision: [[KAIROS-A-0019]] §2.

## Objective

A task can be created *against a repository* and Kairos routes it: repo → owning team → that team's delivery board. The board-consistency rule is enforced on every write that sets a repo. Board items and search can be narrowed to one repo.

## Implementation Notes

- `crates/kairos-server/src/api/tasks.rs::create_task` — resolution order per §D2 (1: repo only → route; 2: repo + board → must match, `team_id` must match; 3: no repo → today's path). Resolution happens **before** `require_capability`, because the capability target is the resolved board. Error text: `repository <slug> belongs to team <team>, whose delivery board is <board>` (422).
- `dto::CreateTaskRequest.board_id` → `Option<String>`; neither `board_id` nor `repository_id` → 422. `dto::CreateTaskRequest.repository_id: Option<String>` (slug or UUID; resolve via `repositories::load_by_slug` then UUID).
- `dto::Task.repository: Option<dto::RepositoryRef { id, slug, forge, repo_full_name, team_id }>` populated by `into_dto` (one join, batched for list endpoints the way `team` is today — check `convert.rs`).
- `PUT /api/tasks/{short_code}/repository { repository_id: Option<String> }` — mirrors `set_work_class`; re-checks the rule against the task's current board; requires `manage_tasks` on that board; `null` clears.
- `GET /api/boards/{id}/items?repository=` and `SearchFilter.repository_id` (db `search.rs` + server `api/search.rs` + `dto::SearchFilter`). Slug or UUID on the wire, UUID in the db layer.
- OpenAPI (`api/openapi.rs`) and `kairos-client` types/methods for all of the above (`create_task` accepting optional board, `set_task_repository`, `repository` filter args). CLI flags are T-0107's.

### Dependencies
T-0103. Blocks T-0105 (which changes which capability is asked for on this path), T-0107, T-0109.

### Risk Considerations
Making `board_id` optional touches every existing caller of `POST /api/tasks` (CLI, web, MCP `create_item_impl`, e2e helpers, soak). All still pass `board_id` so behaviour is unchanged — grep and confirm rather than assume.

## Acceptance Criteria

- [ ] Server integration tests: repo-only create lands on the owning team's delivery board with `team_id` set; repo + wrong board → 422 with the specified message; repo + wrong `team_id` → 422; neither board nor repo → 422; repo-less create unchanged.
- [ ] `PUT …/repository` sets, re-homes (to a repo of the same team), rejects a repo of another team, clears with `null`; `activity_log` carries `task_repository_set`; `version` unchanged.
- [ ] Board items and search filter by repository (slug and UUID); unknown slug → 404/422 consistently with existing filters.
- [ ] `dto::Task.repository` present on get/list/board-items/search responses.
- [ ] OpenAPI regenerated; kairos-client compiles with the new surface; existing client integration suite green.
- [ ] fmt/clippy/unit/integration green.

## Status Updates

*To be added during implementation*
