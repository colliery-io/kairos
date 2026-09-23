---
id: live-only-board-guard-move-task-in
level: task
title: "Live-only board guard + move_task in kairos-db, POST /api/tasks/{code}/move with two-sided ABAC, client method, ItemMoved event"
short_code: "KAIROS-T-0127"
created_at: 2026-09-23T01:50:41.169488+00:00
updated_at: 2026-09-23T02:09:36.956168+00:00
parent: KAIROS-I-0012
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0012
---

# Live-only board guard + move_task in kairos-db, POST /api/tasks/{code}/move with two-sided ABAC, client method, ItemMoved event

## Parent Initiative

[[KAIROS-I-0012]]

## Objective

I-0012 D1 + D2 (db/API/client) + D4 (db/server tests): a team or board can be deleted once every card on the board is deleted or moved; a task can be moved to another delivery board over the API.

## Implementation Notes

### Technical Approach

- `crates/kairos-server/src/api/org/mod.rs`: rename `count_board_items` → `count_live_board_items` with `deleted_at IS NULL` on all four tables; add `live_board_item_codes(conn, board_id, limit)` for the refusal. Doc comment: why soft-deleted rows no longer count (no restore path exists — verified 2026-09-22; Dylan's rule "all cards must be archived or moved"), and that a future restore must refuse when the board is gone. Update `teams.rs::delete_team` and `boards.rs::delete_board` messages: `team "X"'s delivery board still holds N live card(s): [A, B, …]; move them to another board or delete them, then retry` with `details.items` (first 20) + `details.item_count`. Leave `remove_column`'s rule alone (comment the asymmetry). Check `scim/groups.rs::count_board_items` (its own copy) — align it too so SCIM group deletion follows the same rule.
- `crates/kairos-db/src/boards.rs`: `pub fn move_task(conn, task_id, to_board_id, actor) -> Result<TaskMove, BoardError>` in one transaction: load the live task (`ItemNotFound`), `SameBoard`, target live + `BoardLevel::Delivery` (`NotDeliveryBoard`), if `repository_id` is set the target must equal `repositories::delivery_board_for_team(owner)` (`RepositoryOwnerMismatch { repository_slug, owner_board_id }`), then set `board_id`, `column_id = entry_column(target)` (`NoEntryColumn` if none), `team_id = target.team_id`, `updated_at`; `log_activity(action "board_move", details {from_board, to_board, from_column, to_column})`; emit `EventKind::ItemMoved { from_board_id, to_board_id }` (add the variant in `kairos-db/src/events.rs`, the client `types_events.rs`, and the server WS fan-out — grep `ItemTransitioned` for every site). `TaskMove { board_id, column_id, team_id }`.
- API `crates/kairos-server/src/api/tasks.rs`: `POST /api/tasks/{short_code}/move` body `MoveTaskRequest { board: String }` (slug|UUID via `board_by_ref`-style resolve; put a shared resolver in `api/mod.rs` if the MCP one is private), auth: `require_capability(manage_tasks)` on the CURRENT board then on the TARGET board (org admin bypass inherent), then `boards::move_task`; respond 200 with the full `Task` DTO (reuse the GET rendering incl. embedded repository). Map errors: 404 board; 422 `SAME_BOARD`, `NOT_DELIVERY_BOARD`, `REPOSITORY_OWNER_MISMATCH` (message: `"<code> is bound to <repo>, owned by <team>; move it to <owner board slug> or unbind it first (PUT /api/tasks/<code>/repository)"`), `NO_ENTRY_COLUMN`. utoipa annotations; `tests/openapi.rs` set-equality will demand the path.
- Client: `KairosClient::move_task(code, board) -> Task`; `MoveTaskRequest` in `types.rs`.
- Tests: `crates/kairos-db/tests/boards_move.rs` (or extend an existing boards test) — happy path (entry column, team_id follows, activity row, event), SameBoard, NotDeliveryBoard, RepositoryOwnerMismatch. `crates/kairos-server/tests/task_move.rs`: two-sided ABAC (bob with manage_tasks on platform only → 403 targeting web; a member granted on both → 200; alice admin → 200), the repository rule (422 + message), team delete: refused listing the live code → delete the task → 200; board delete parity; refused-then-move-then-delete path. Update `tests/org_endpoints.rs` / any test that pinned "soft-deleted rows still block".

### Dependencies

None.

## Acceptance Criteria

- [x] Both succeed once the board holds no live cards; the 422 names them ("still holds N live card(s): [CODE, …]") with `details.items`. SCIM group delete follows the same rule.
- [x] All of it, proven in `tests/task_move.rs` (ABAC ladder: no grants → 403, source-only → 403, both → 200, admin by slug → 200) and `tests/board_move.rs`; the `item_moved` pair asserted over a real socket in `ws_events.rs`.
- [x] `angreal test lint` clean, `angreal test unit` green, `angreal test integration` 40/40 (two new targets); the OpenAPI set-equality test passes with the new path registered.

## Status Updates

**2026-09-22** — Completed in `fb6eb45`.

- `move_task` emits TWO `item_moved` events (source board then target) rather than one: a thin event carries a single `board_id`, and both boards' subscribers must refetch. Documented on `EventKind::ItemMoved`.
- No migration needed: `activity_log.action` is plain TEXT with no CHECK (only a comment listing the actions, which I extended). The `assert_text_enum!` unit test pins the vocabulary and needed `board_move` added.
- The repository guard (T-0112, 409) fires before the board guard (422) on team delete — a disbanding team re-homes or retires its repositories first. Pinned in the test.
- Events go over NOTIFY, not a table, so the db-layer test asserts placement/activity and leaves the event assertions to `ws_events.rs`.