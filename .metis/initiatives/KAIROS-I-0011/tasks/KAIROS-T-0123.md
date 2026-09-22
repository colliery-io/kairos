---
id: fix-mcp-reads-links-on-get-item
level: task
title: "Fix: MCP reads — links on get_item, delivery board slug in get_repository, Backlog-only refusal text for cross-team filers"
short_code: "KAIROS-T-0123"
created_at: 2026-09-22T12:12:41.644199+00:00
updated_at: 2026-09-22T12:12:41.644199+00:00
parent: KAIROS-I-0011
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# Fix: MCP reads — links on get_item, delivery board slug in get_repository, Backlog-only refusal text for cross-team filers

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Close UAT findings #3, #4 and #5 (I-0011 progress log, 2026-09-22) in `crates/kairos-server/src/mcp/tools.rs` so an agent following the plugin skills sees PR state, gets slugs everywhere, and is told the rule it broke.

## Implementation Notes

### Technical Approach

- **#3 links on `get_item`:** after the relationships section, render a `## Development` section from `kairos_db::links` (the same query `GET /api/{family}/{code}/links` uses — T-0100): one line per link `- <kind> <external_id> [<state>] <title> — <url>`; `(none)` when empty. Tasks/initiatives/strategies (whatever the links API supports). Keep the STALE/repository lines as they are.
- **#4 `get_repository`:** `- delivery board: <slug> (<name>)` instead of the UUID (the board row is already loaded for `RepositoryDetail`; if only the id is at hand, load the board by id in the same blocking closure). Grep the plugin skills for any text that says "board id" here and align.
- **#5 refusal text:** in `transition_item` (and `update_item`/`delete` if they share the path), when authorization fails AND the caller holds only the computed `file_backlog` on that board (i.e. `check_file_backlog` would pass but the required capability does not), map the error to: `FORBIDDEN: <code> sits in <team>'s Backlog for their triage; cross-team filers may create and link, not move, edit or delete (file_backlog)`. Do it in the MCP tool layer via the existing `map_*` error helpers — the HTTP API keeps its generic envelope. Add a unit/integration probe in `tests/mcp.rs` (carol on a platform task → the new text) and keep the existing generic-FORBIDDEN probe for a non-member.
- Tests: `tests/mcp.rs` — get_item renders a seeded link (the seed has PR links on DEMO-T-0002); get_repository line asserts the slug; refusal text probe.

### Dependencies

None.

## Acceptance Criteria

- [ ] `get_item DEMO-T-0002` text contains a `## Development` section listing the seeded PR with its state.
- [ ] `get_repository payments-api` prints `- delivery board: platform-delivery (…)`.
- [ ] carol's `transition_item` on a task she filed into platform's Backlog is refused with text naming Backlog/triage and `file_backlog`; a non-member on a board still gets the generic capability refusal.
- [ ] fmt, workspace clippy `-D warnings`, `angreal test unit`, `angreal test integration` green.

## Status Updates

*To be added during implementation*
