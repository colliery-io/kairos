---
id: fix-mcp-reads-links-on-get-item
level: task
title: "Fix: MCP reads — links on get_item, delivery board slug in get_repository, Backlog-only refusal text for cross-team filers"
short_code: "KAIROS-T-0123"
created_at: 2026-09-22T12:12:41.644199+00:00
updated_at: 2026-09-22T12:19:14.441247+00:00
parent: KAIROS-I-0011
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] `get_item` renders `## Development` with `- pull_request 77 [merged] …` (probed in tests/mcp.rs on a link inserted for the bound task; same query/order as the links API).
- [x] `get_repository payments-api` prints `- delivery board: platform-delivery (Platform Delivery)`.
- [x] bob (org member, no grant on platform's board) files a task over MCP and his `transition_item` / `update_item` are refused with "… sits in platform's Backlog for their triage; a cross-team filer may create and link it (file_backlog), not move, edit or delete it — that needs \"transition_items\" on their board"; the existing generic-FORBIDDEN probes still pass.
- [x] fmt + workspace clippy clean; `cargo test --test mcp|file_backlog|meta|task_repositories|forge_webhook|repositories_api` green against the running stack. The full `angreal test integration` (which cycles compose) is deferred to T-0125's gate run because the T-0124 agent is using the stack concurrently.

## Status Updates

**2026-09-22** — Completed in `3cba74b`.

- Implemented as a `require_capability_explained` helper in `mcp/tools.rs` used by `transition_item` and `authorize_item_write` (update/delete): on a FORBIDDEN from `require_capability`, if `abac::check_file_backlog` passes for the caller on that board, the message is rewritten with the owning team's slug (boards ⋈ teams). Details carry `held: "file_backlog"`.
- `## Development` is rendered for every non-document item from `kairos_db::forge::links_for_item`.
- Test gotcha: bob's `users` row is JIT-provisioned on his first authenticated `/mcp` call (403 MEMBERSHIP_REQUIRED) — mint + call before `user_id()`; and `conn`'s search_path must be re-pinned to `org_acme` before inserting the link.