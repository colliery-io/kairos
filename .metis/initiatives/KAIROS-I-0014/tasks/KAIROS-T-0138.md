---
id: j8-machine-access-a-ci-service
level: task
title: "J8 machine-access: a CI service account works, its key is rotated mid-story, then revoked"
short_code: "KAIROS-T-0138"
created_at: 2026-09-23T03:45:39.269220+00:00
updated_at: 2026-09-23T03:49:11.595760+00:00
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

- [x] Green in compose (hand-run): 8 steps. Report shows both keys live during rotation, old key 401 after revoke with the new one still 200, and the account delete killing the last key.
- [x] Clean: the service-account ledger entry tolerates the 404 from the journey deleting it deliberately in the last step.
- [x] tsc clean; no new tools or nouns, so the gate is unaffected.

## Status Updates

**2026-09-22** — Completed in `726cdcf`.

- There is no rotate endpoint — rotation is mint-new-then-revoke-old, which is the honest story and makes the overlap window (both keys accepted) assertable.
- `Cast.agent()` widened to a free-text name so one journey can hold two machine identities.
- Attribution is read from the activity RECORD rather than the rendered text: "who deployed that?" is a data question, and the feed's label formatting should not be able to make the test pass or fail.