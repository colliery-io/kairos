---
id: task-repo-binding-create-time
level: task
title: "Task repo binding: create-time routing, board-consistency rule, set-repository endpoint, repo filter on board items and search"
short_code: "KAIROS-T-0104"
created_at: 2026-09-22T03:04:40+00:00
updated_at: 2026-09-22T03:47:54.465721+00:00
parent: KAIROS-I-0010
blocked_by: [KAIROS-T-0103]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] Server integration test `tests/task_repositories.rs`: repo-only create lands on the owner's delivery board with `team_id` set (slug and UUID); repo + wrong board → 422 naming repo, team and the right board; repo + wrong `team_id` → 422; neither → 422 "board_id is required unless repository_id is given"; repo-less create unchanged; unknown repo → 422.
- [x] `PUT …/repository` sets, re-homes within the team, rejects another team's repo, clears with `null`; three `activity_log` rows with action `repository` (the enum variant, not a free-text `task_repository_set`); `version` unchanged; unknown code → 404.
- [x] Board items `?repository=` (slug and UUID; unknown → 422) narrows tasks only; search `filter.repository_id` (UUID, like every sibling id filter — slug resolution for search lands on the MCP side in T-0107 where a conn is in hand); non-UUID → 400 like `filter.team_id`.
- [x] `dto::Task.repository_id` on every carrier; embedded `dto::Task.repository` (`RepositoryRef`) on create/get/list/update/work-class/transition/set-repository/board-items/search via one batched `attach_repositories` query per response. WS event payloads carry `repository_id` only (pure conversion).
- [x] `set_repository` registered in OpenAPI; kairos-client has `set_task_repository`, `board_items_for_repository`, `put_ok`, `types_repositories::{RepositoryRef, SetTaskRepositoryRequest}`; CLI `tasks create --repo` (board optional), `search --repo`.
- [x] fmt, clippy `-D warnings` on core/db/client/server/cli/soak, `angreal test unit`, `angreal test integration` 35/35 green.

## Status Updates

- 2026-09-22: Done and committed (`cd206fb`). Notes for downstream:
  - `crate::api::tasks::{resolve_routing, TaskRoute, map_repository_error}` are `pub(crate)` — T-0105 wraps the capability choice around `resolve_routing`; T-0107's `create_item_impl` MUST call it rather than re-implement.
  - `convert::{attach_repositories, attach_repository, repository_ref}` are the enrichment helpers; anything new that renders tasks should call them.
  - `kairos-web` keeps its own local `CreateTaskRequest` mirror sending `board_id` — untouched and wire-compatible; T-0109 adds `repository_id` there.
  - All existing callers of `POST /api/tasks` (CLI, web, MCP, soak, e2e helpers) still send `board_id` — verified by grep, behaviour unchanged.