---
id: fix-repository-ownership
level: task
title: "Fix: repository ownership invariants — two-sided re-home gate, team delete guard, team_id sync on bind, routing helpers into kairos-db"
short_code: "KAIROS-T-0112"
created_at: 2026-09-22T09:53:00.631400+00:00
updated_at: 2026-09-22T10:16:29.179930+00:00
parent: KAIROS-I-0010
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0010
---

# Fix: repository ownership invariants — two-sided re-home gate, team delete guard, team_id sync on bind, routing helpers into kairos-db

## Parent Initiative

[[KAIROS-I-0010]] — Repository-Scoped Work. Fix ticket from the 2026-09-22 four-agent deep-dive review (server/db, web, plugin/e2e/docs, security/hygiene) after T-0103…T-0110 landed. Decision: [[KAIROS-A-0019]].

## Objective

Hold the repo → team → board invariant on every write path, not just task create. Today a member of the owning team can re-home a repo onto a team that never consented; deleting a team orphans its repos; binding a repo to a task leaves `tasks.team_id` stale; and the routing helpers live in an HTTP handler module and are reached as `crate::api::tasks::…` from MCP and boards.

## Implementation Notes

1. **Re-home gate** (`api/org/repositories.rs` `update_repository`): when `team` changes, require manage on BOTH the current owner's and the new owner's delivery board (org admin bypasses). Test: bob (platform) re-homing to web → 403; admin → 200; a member of both → 200.
2. **Team delete guard** (`api/org/teams.rs` `delete_team` ~517-580): refuse (409) while live repositories are owned by the team, naming them — symmetric with `repositories::soft_delete`'s in-use refusal. Also filter `teams.deleted_at` in `render`'s team join and `list`.
3. **`team_id` sync**: `items::set_task_repository` sets `tasks.team_id = repo.team_id` when binding (and leaves it when clearing); `resolve_routing` for PUT already checks the board. Document that re-homing a repo does NOT rewrite bound tasks (ADR §2) — but surface it: `GET /api/repositories/{slug}` gains `stale_tasks: i64` (tasks bound to it whose `team_id`/`board_id` no longer match the owner), and the CLI/MCP detail prints it.
4. **Move routing into kairos-db**: `TaskRoute`, `resolve_routing` → `kairos_db::repositories::route_task(conn, board_id, team_id, reference) -> Result<TaskRoute, RepositoryError>` with typed errors (`RepositoryError::BoardMismatch{..}`, `TeamMismatch{..}`); the server keeps only error mapping + `require_task_create_capability` (which should use `kairos_db::boards` for the position-0 lookup rather than `board_columns` directly, and compare against `resolve_column`'s "first column" — see review finding on position-0 vs min-position).
5. **One delivery-board helper**: delete the two `delivery_board_of` copies (`org/teams.rs:75`, `org/repositories.rs:111`) in favour of `kairos_db::repositories::delivery_board_for_team` (exactly-one semantics) — or an `Option`-returning sibling — so directory, gate and routing agree.
6. `attach_repositories` in `org/boards.rs` board_items: collect all task DTOs, attach once, redistribute (the comment currently claims one query; make it true).
7. `soft_delete` reference check inside the transaction (TOCTOU).

## Acceptance Criteria

- [x] Re-home: bob (platform only) → web = 403; admin = 200 (`repositories_api.rs`).
- [x] `DELETE /api/teams/{id}` → 409 naming the owned slugs.
- [x] Bind sets `team_id` (asserted on create-by-repo); after the re-home `stale_tasks` = 1, re-binding that task → 422 (rule re-checked against ITS board), unbinding → 0.
- [x] `kairos_db::repositories::route_task` + `TaskRoute` + typed errors; the server's `resolve_routing` is a one-line mapping. MCP and boards still call `crate::api::tasks::{resolve_routing, require_task_create_capability, map_repository_error}` — those are now thin server-layer mappings/ABAC (correctly server-side), so the cross-module reach is to the right kind of thing; not moved further.
- [x] fmt, clippy, unit, integration 37/37.

## Status Updates

- 2026-09-22: Done, `5e10c18`. Also: one exactly-one `delivery_board_of` behind directory/gate/routing (a team with two delivery boards now shows no board rather than an arbitrary one); board-items attaches repo refs in ONE query; `soft_delete` checks references inside the transaction; `file_backlog` compares against `boards::entry_column` rather than `position == 0`; MCP `create_item` board+repository disagreement tested.