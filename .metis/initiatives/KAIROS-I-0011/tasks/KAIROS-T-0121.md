---
id: uat-j4-cross-team-carol-files-into
level: task
title: "UAT J4 cross-team: carol files into platform's Backlog over MCP, refused a transition, links blocks; bob triages in the GUI; badge clears live"
short_code: "KAIROS-T-0121"
created_at: 2026-09-22T11:15:28.143152+00:00
updated_at: 2026-09-22T11:15:28.143152+00:00
parent: KAIROS-I-0011
blocked_by: ["KAIROS-T-0117"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# UAT J4 cross-team: carol files into platform's Backlog over MCP, refused a transition, links blocks; bob triages in the GUI; badge clears live

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Implement journey J4 (I-0011 D4) — "A web engineer needs something from platform and gets it" — as `uat/journeys/cross-team.journey.ts`, mirroring the plugin's `CROSS-TEAM-FILING.md` recipe.

## Implementation Notes

### Technical Approach

1. carol (MCP) `list_repositories` → `get_repository payments-api` (reads the description) → `create_item` task with `repository: payments-api`, no board → the response/`get_item` shows board `platform-delivery`, column Backlog → observe.
2. carol (MCP) `create_item` her own task with `repository: portal-web`; `link_items` `blocks` platform-task → her task succeeds; `transition` of the platform task is refused and the tool text explains the Backlog-only rule (observe the message).
3. bob (GUI) opens `platform-delivery`: the filed card is in Backlog with the `payments-api` chip; drags it to Todo.
4. carol (GUI) opens her task: blocked-by badge names the platform task; her web board card shows the badge.
5. bob (GUI) moves the platform task to Done (drag through the columns the board's transitions allow — read the board's transitions via API and follow them); carol's open page clears the badge without reload.
6. alice (CLI) `kairos search --repo payments-api` lists the filed task with carol as creator.
7. Teardown: edge (delete relationship as alice), both tasks.

### Dependencies

T-0117.

### Risk Considerations

- Column path to Done may require multiple hops; derive from transitions, do not hard-code.
- Badge-clear timing over WS: expect-polling.

## Acceptance Criteria

- [ ] `angreal test uat --journey cross-team` green under compose and `--server`; report shows the Backlog landing, the refusal text, and the badge appearing then clearing.
- [ ] Nothing `uat-<run>-` remains after teardown.

## Status Updates

*To be added during implementation*
