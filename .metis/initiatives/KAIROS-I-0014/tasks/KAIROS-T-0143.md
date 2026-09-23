---
id: j14-growing-team-a-member-joins
level: task
title: "J14 growing-team: a member joins and is granted capabilities; another leaves and their work is reassigned"
short_code: "KAIROS-T-0143"
created_at: 2026-09-23T03:45:57.785370+00:00
updated_at: 2026-09-23T03:45:57.785370+00:00
parent: KAIROS-I-0014
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0014
---



## Parent Initiative

[[KAIROS-I-0014]]

## Implementation Notes

The journey's story, cast and the thing it would catch are in the
initiative's Detailed Design — read that section first; it is the spec.

Harness conventions (unchanged): `journey(id, title, {humans}, body)` and
`step(persona, narration, fn)` from `uat/run/narrate.ts`; personas from
`uat/personas`; surfaces in `uat/surfaces/` (gui, api, cli, mcp, forge);
`ledger.add` everything created, in dependency order; `named()` for every
created slug or title; `step.composeOnly` for anything needing a fresh
tenant or deployment admin. Read a neighbouring journey before writing —
`uat/journeys/board-setup.journey.ts` is the most recent and shows the
current idioms. Selector notes live in the completed task docs of
KAIROS-I-0013 (T-0133 and T-0134 in particular).

If a journey needs a team of its own, give the fixture a suffix no other
journey uses (`mobile`, `ios`, `infra` are taken).

## Acceptance Criteria

- [ ] The journey is green in compose mode, and its report reads as the story.
- [ ] Nothing `uat-` is left behind (teardown verified).
- [ ] `npx tsc --noEmit` clean in `uat/`; the drift gate still passes on a full run.

## Status Updates

*To be added during implementation*
