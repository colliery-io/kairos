---
id: j19-housekeeping-completed-work
level: task
title: "J19 housekeeping: completed work archived, a repository retired, the guards still hold"
short_code: "KAIROS-T-0148"
created_at: 2026-09-23T03:46:12.151162+00:00
updated_at: 2026-09-23T10:34:27.566234+00:00
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

- [x] Green: 7 steps. Report reads as a quarter being closed out.
- [x] The journey archives and retires everything itself; the ledger tolerates what it already removed.
- [x] tsc clean; no new tools or nouns.

## Status Updates

**2026-09-23** — Completed in `cc52081`.

- **Finding, filed as KAIROS-T-0151:** the I-0012 archiving decision only half holds. Archived cards stop obstructing the guards (good), but a soft-deleted item AND its `/history` both 404, so the CONTENT of put-away work is unreachable; only the activity trail survives, and it records that the work existed rather than what it said. The journey asserts the real behaviour with the reasoning in its narration, rather than asserting the assumption and going red.
- `kairos repos list` takes no `--limit`, unlike `teams list` and `members list`. Minor, not worth its own ticket, but it is the second CLI flag inconsistency the journeys have hit (see KAIROS-T-0150).