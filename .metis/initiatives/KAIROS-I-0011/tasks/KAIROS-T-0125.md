---
id: fix-uat-journeys-assert-the-real
level: task
title: "Fix: UAT journeys assert the real behaviour — drop the workarounds for the fixed findings, full runs in both modes"
short_code: "KAIROS-T-0125"
created_at: 2026-09-22T12:12:48.100683+00:00
updated_at: 2026-09-22T12:12:48.100683+00:00
parent: KAIROS-I-0011
blocked_by: ["KAIROS-T-0123", "KAIROS-T-0124"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# Fix: UAT journeys assert the real behaviour — drop the workarounds for the fixed findings, full runs in both modes

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Once T-0123 and T-0124 land, make the journeys assert what the product now does instead of working around what it did, and re-record the full runs.

## Implementation Notes

### Technical Approach

- `onboarding`: step 7 also asserts the repository description is visible on the team page (finding #6a).
- `planning`: keep waiting for the item header before clicking Graph (a human does), but add an assertion that the Graph anchor's `href` names the code (finding #2).
- `agent-loop`: step 3 binds the repository in the New task modal's Repository select instead of the item page (#6b — keep one item-page bind elsewhere? no: the picker stays covered by e2e `repositories.spec`); step 8 reads the merged state from `get_item`'s `## Development` section instead of the links API (#3).
- `cross-team`: step 1 reads the delivery board slug straight from `get_repository` (#4); step 4 asserts the refusal names Backlog/triage and `file_backlog` (#5).
- `surfaces/gui.ts`: `dragCard` retry removed (T-0124 #8) — if it is still needed, that is a T-0124 failure, not a journey concern.
- Update `uat/README.md` if any convention changed; update the initiative's findings list with what is now fixed (leave #7 open, marked "Dylan deciding").
- Full runs: `angreal test uat` (compose) and `--server` against the kept stack; paste result lines + run ids into the initiative log.

### Dependencies

T-0123, T-0124.

## Acceptance Criteria

- [ ] No journey reads `/api/*/links`, resolves a board id from `get_repository`, or retries a drag.
- [ ] Both full runs green; run ids in the initiative log; findings list updated.

## Status Updates

*To be added during implementation*
