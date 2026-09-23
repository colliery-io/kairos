---
id: cli-kairos-tasks-create-board
level: task
title: "CLI: kairos tasks create --board should take a slug like every other board reference"
short_code: "KAIROS-T-0150"
created_at: 2026-09-23T10:23:34.950030+00:00
updated_at: 2026-09-23T10:23:34.950030+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#tech-debt"


exit_criteria_met: false
initiative_id: NULL

# CLI: kairos tasks create --board should take a slug like every other board reference

## Objective

`kairos tasks create --board <BOARD_ID>` accepts a UUID only. Every other board reference a person types takes a slug or a UUID:

- `kairos tasks move <code> --to-board <slug|uuid>` (KAIROS-I-0012)
- `kairos tasks create --repo <slug|uuid>`, `kairos search --repo <slug|uuid>` (KAIROS-A-0019)
- MCP `board_items { board }` and `move_item { to_board }` both resolve slugs

So the one command a person is most likely to type first is the one that makes them go and look up a UUID.

### Type
- [x] Tech Debt - Code improvement or refactoring

### Priority
- [x] P2 - Medium (nice to have)

## Implementation Notes

`crates/kairos-cli/src/commands/entities.rs`: `TaskCreateArgs.board` is passed straight through as `board_id`. The server already resolves slugs for the move endpoint via a board-by-ref helper in `crates/kairos-server/src/api/tasks.rs` (`board_id_by_ref`) — the cheapest fix is for `POST /api/tasks` to accept a slug in `board_id` the same way, which also fixes the MCP and HTTP callers rather than papering over it in the CLI alone. Decide which layer resolves: doing it server-side keeps one rule; doing it CLI-side keeps the wire type strict. My preference is server-side, matching `--repo`.

Also check `strategies|initiatives|adrs create --board` for the same wart while you are there.

## Acceptance Criteria

- [ ] `kairos tasks create --board platform-delivery --title …` works.
- [ ] A slug that does not exist is a 404/422 naming the slug, not a UUID parse error.
- [ ] Integration coverage for the slug form; the UAT `audit-trail` journey drops its "resolve the id first" workaround and its comment.

## Status Updates

**2026-09-23** — Found twice while writing UAT journeys (KAIROS-T-0133, and again in KAIROS-T-0119's `--repo` work). The `audit-trail` journey currently resolves the board id through the API with a comment explaining why; that workaround is the thing to delete when this lands.
