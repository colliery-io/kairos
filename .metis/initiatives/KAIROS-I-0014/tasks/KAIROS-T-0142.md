---
id: j17-incident-an-unplanned-support
level: task
title: "J17 incident: an unplanned support request jumps the queue, becomes a bug, is fixed with a PR"
short_code: "KAIROS-T-0142"
created_at: 2026-09-23T03:45:54.237746+00:00
updated_at: 2026-09-23T10:23:19.695395+00:00
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

## Acceptance Criteria

- [x] Green in compose (hand-run): 12 steps, 2.0s. The report reads as Friday
      afternoon — the refusal, the lane the request is born in, the triage past
      the planned card, `bug [support lane]` in the agent's queue, the rollup,
      and what the weekend cost.
- [x] Clean: team + repo + agent + forge connection + three tasks, all ledgered;
      no teardown failures.
- [x] `npx tsc --noEmit` clean; no new tools or nouns, so the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `41c2242` (`uat/journeys/incident.journey.ts`).

- **Product finding: `task_type` is immutable.** There is no endpoint to retype
  a support request as a bug (only `POST /api/tasks/{code}/work-class` for the
  lane), so "becomes a bug" is a bug RAISED from the request and linked with
  `blocks`. That is arguably the better record anyway — the customer's report
  and the defect are two facts — but the initiative's wording implied a
  mutation that does not exist.
- **Cross-team filing and the Support lane compose correctly**, which was worth
  asserting: `file_backlog` requires a repository (`require_task_create_capability`
  only opens the entry column when `repository_id.is_some()`), so carol filing
  with a `board` and no repository is refused with `manage_tasks` — and when she
  files against the repository, the support-type default still puts it in the
  Support lane on the OTHER team's board.
- **Selector note:** `gui.ts`'s `column()` resolves inside the planned lane by
  design, so the journey carries its own lane-scoped `laneColumn`/`laneCard`/
  `dragInLane` (same hand-driven mouse technique as `dragCard`, for the reason
  in KAIROS-T-0124 #8). Kept local rather than pushed into `surfaces/gui.ts`
  because three agents are writing journeys in this tree concurrently.
- The team page's In flight panel prints BOTH a forge link and an item link per
  row, so `getByRole('link', {name: /CODE/})` is a strict-mode violation; assert
  `a[href="/items/<code>"]`.