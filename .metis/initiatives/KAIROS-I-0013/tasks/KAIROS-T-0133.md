---
id: j5-audit-trail-stale-update-item
level: task
title: "J5 audit-trail: stale update_item, edit_item, history diff, copy-forward rollback, get_history, activity feed"
short_code: "KAIROS-T-0133"
created_at: 2026-09-23T02:59:13.216114+00:00
updated_at: 2026-09-23T02:59:13.216114+00:00
parent: KAIROS-I-0013
blocked_by: ["KAIROS-T-0131"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0013
---

# J5 audit-trail: stale update_item, edit_item, history diff, copy-forward rollback, get_history, activity feed

## Parent Initiative

[[KAIROS-I-0013]]

## Objective

I-0013 D3: a new journey `uat/journeys/audit-trail.journey.ts` — "An edit goes wrong and the record puts it right". Covers A-0004's promise, which nothing walks as a user today.

## Implementation Notes

### Technical Approach

Cast: alice (edits), bob (the competing writer). Work on a task alice creates on `platform-delivery` (ledgered), NOT seed data.

1. **alice (MCP)** `update_item` with the current version → ok; then again with the now-stale version → the tool returns CONFLICT carrying the current version and content (assert both are present — that is the contract agents rely on), and she retries with the fresh version and wins.
2. **alice (MCP)** `edit_item` search/replace for a small correction; assert the replaced text is in `get_item`.
3. **alice (GUI)** opens the item's history (`/activity/history/:code` — reachable from the item page's History link; read `crates/kairos-web/src/pages/activity.rs` for the selectors, and `e2e/tests/` for any existing history assertions): the version list, and a diff showing what changed.
4. **alice (GUI)** rolls back to the pre-mistake version. A-0004 rollback is COPY-FORWARD (`pages/activity.rs` docs say so): assert the content is restored AND the version went UP, not back — that distinction is the point of the step.
5. **alice (CLI)** `kairos tasks get <code>` shows the restored content; **alice (MCP)** `get_history` shows the whole chain (assert the version count).
6. **bob (GUI)** opens `/activity` and finds the episode (filter to the item if the page offers it).
- Delete `mcp:update_item`, `mcp:edit_item`, `mcp:get_history` from ALLOW.

### Dependencies

T-0131.

## Acceptance Criteria

- [ ] `angreal test uat --journey audit-trail` green in compose and `--server`; the report shows the conflict's current-version payload, the rollback's version going up, and the activity line.
- [ ] Those three ALLOW entries gone, full run still green.
- [ ] No `uat-` leftovers.

## Status Updates

*To be added during implementation*
