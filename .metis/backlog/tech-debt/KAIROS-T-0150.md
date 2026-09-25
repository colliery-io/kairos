---
id: cli-kairos-tasks-create-board
level: task
title: "CLI: kairos tasks create --board should take a slug like every other board reference"
short_code: "KAIROS-T-0150"
created_at: 2026-09-23T10:23:34.950030+00:00
updated_at: 2026-09-25T02:15:31.353864+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#tech-debt"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] `kairos tasks create --board platform-delivery --title …` works.
- [x] A slug that does not exist is a 404/422 naming the slug, not a UUID parse error.
- [x] Integration coverage for the slug form; the UAT `audit-trail` journey drops its "resolve the id first" workaround and its comment.

## Status Updates

**2026-09-23** — Found twice while writing UAT journeys (KAIROS-T-0133, and again in KAIROS-T-0119's `--repo` work). The `audit-trail` journey currently resolves the board id through the API with a comment explaining why; that workaround is the thing to delete when this lands.
### 2026-09-25 — resolved server-side, on all four families

Done the way the implementation notes preferred: `POST /api/tasks` and the
strategy, initiative and ADR creates all take a board **slug or UUID**, resolved
in the server. One rule, in one place, for the CLI, REST callers and MCP alike.
Resolving in the CLI would have kept the wire type strict at the price of leaving
the REST API with the wart and every other client to reimplement the lookup.

The rule already existed — `board_id_by_ref`, private to `tasks.rs`, serving the
move endpoint. It is now `pub` in `api/mod.rs` with an optional sibling, and the
four create handlers call it. Resolution needs a connection, so it moved from the
top of each handler into the closure.

Also checked `strategies|initiatives|adrs create --board` as the notes asked: all
three had the same wart, all three are fixed. CLI help and `value_name` now read
`BOARD` rather than `BOARD_ID`, matching `--to-board`.

### Two error-shape changes, which the existing tests caught

Neither was in the acceptance criteria, and both are improvements:

- **`board_id: <a valid UUID with no such board>` was a 403, now a 404.** The old
  code parsed the UUID, handed it to the ABAC check, and reported a missing
  capability on a board that does not exist. Resolution now runs first and says
  the true thing. The 404 leaks nothing an authorised reader could not already
  see: [[KAIROS-A-0006]] makes reads open tenant-wide, so board existence is not
  a secret inside a tenant.
- **`board_id: "not-a-uuid"` was a 422 VALIDATION, now a 404.** Which is the
  ticket: an unrecognised reference is a missing board, not a malformed id.

`client_roundtrip`'s 422-mapping assertion used `board_id` for its malformed-UUID
example, so it had to move to `column_id` — still a UUID-only field. That test is
about the `Error::Validation` mapping rather than about boards, so the example
changing is fine; using board_id there was incidental.

### One assertion I wrote wrong

My first version of the 404 test asserted the message must not mention "uuid" at
all, on the theory that mentioning UUIDs is what sends a reader off to look one
up. It failed, correctly: the message is `no live board "no-such-board" (slug or
UUID)`, and that parenthetical is telling the reader **both forms work** — the
opposite of the wart. The assertion now checks for it rather than against it.

### Coverage

`entities.rs`: create by slug resolves to the same board a UUID does; an unknown
reference is a 404 naming what was typed; and the same slug form on strategies.
The `audit-trail` UAT journey drops its `boardBySlug` workaround and the comment
explaining why it needed one — it passes `platform-delivery` the way a person
would type it.

### Gates

lint clean, **397 unit tests**, integration **47/47**, e2e **16**, uat **22
journeys + drift gate**, REST reference regenerated (the `board_id` descriptions
changed).