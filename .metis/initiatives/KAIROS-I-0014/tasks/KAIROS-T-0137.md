---
id: j13-first-week-a-newcomer-reads
level: task
title: "J13 first-week: a newcomer reads the boards overview, progress rollups, team in-flight and their repo queue, writing nothing"
short_code: "KAIROS-T-0137"
created_at: 2026-09-23T03:45:36.451332+00:00
updated_at: 2026-09-23T03:46:46.035039+00:00
parent: KAIROS-I-0014
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/active"


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

- [x] Green in compose (hand-run): 6 steps, ~1s. Report reads as an hour of reading.
- [x] Creates nothing — no ledger entries, no teardown.
- [x] tsc clean; gate unaffected (uses tools and nouns already covered).

## Status Updates

**2026-09-22** — Completed in `f279492`.

- The rollup assertion is the one worth keeping: card badge and detail bar are computed by different code paths, and the journey compares them against each other rather than against a constant.
- Selector notes: the app shell has no `<main>` element (read `body` for text-order checks), and the Relationships panel's links carry the title as well as the code — take the code from the href.