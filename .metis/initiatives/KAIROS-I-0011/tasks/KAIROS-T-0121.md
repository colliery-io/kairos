---
id: uat-j4-cross-team-carol-files-into
level: task
title: "UAT J4 cross-team: carol files into platform's Backlog over MCP, refused a transition, links blocks; bob triages in the GUI; badge clears live"
short_code: "KAIROS-T-0121"
created_at: 2026-09-22T11:15:28.143152+00:00
updated_at: 2026-09-22T12:05:13.347450+00:00
parent: KAIROS-I-0011
blocked_by: [KAIROS-T-0117]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] Green under compose (hand-run) and in the full `angreal test uat --server …` run (5/5 journeys); report shows "platform-delivery / column: Backlog", `FORBIDDEN: this action requires capability "transition_items" on board …`, "blocked by 1" then "cleared; reloaded: false".
- [x] `kairos search --query uat-` and `kairos repos list` show nothing after the run.

## Status Updates

**2026-09-22** — Completed in `d46da4d`.

- Story adjustment vs D4: a `blocks` badge counts LIVE edges regardless of the blocker's column (kairos-db `graph.rs` rollup), so "bob moves it to Done → carol's badge clears" cannot happen by itself. The journey has carol remove the satisfied edge (`unlink_items`, allowed because she authored the source) and asserts the live clear — which is what the product actually promises.
- **Finding:** `get_repository` prints `- delivery board: <UUID>` while every other MCP tool prints slugs; an agent following the skills gets a UUID it then has to pass to `board_items` (works, but inconsistent). Candidate hygiene ticket.
- Finding: the refusal text for a `file_backlog`-only caller is the generic `FORBIDDEN … requires capability "transition_items"` — correct, but it does not mention the Backlog-only rule the recipe describes. Candidate: friendlier refusal for cross-team filers.
- The Todo→Active drag needs the retry every run (~5s): the first HTML5 drop after a navigation is swallowed. Journey-level dragCard retries once; if it starts needing two, that is a GUI bug to chase.