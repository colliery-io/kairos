---
id: j15-reorg-two-teams-become-one
level: task
title: "J15 reorg: two teams become one, repos re-homed, cards moved, the old team wound down"
short_code: "KAIROS-T-0144"
created_at: 2026-09-23T03:46:00.540080+00:00
updated_at: 2026-09-23T10:36:25.281963+00:00
parent: KAIROS-I-0014
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] Green in compose (hand-run): 13 steps, 1.2s, and green again in a
      three-journey run with J17 and J14 (`mudyw9fv`). The report reads as a
      merge: two named refusals, the re-home, the stale routing, the move, the
      machine's half-day, and the wind-down.
- [x] Clean: two teams, a repository, a service account, two memberships and a
      task, all ledgered; no teardown failures (the wound-down team's closures
      tolerate a 404 because the story deletes it).
- [x] `npx tsc --noEmit` clean; no new tools or nouns, so the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `325b8ca` (`uat/journeys/reorg.journey.ts`).

- **The catch, confirmed, and it has two halves.** Re-homing a repository
  retargets everything bound to it: the card sitting on the old board may now
  only move to the NEW owner's board (parking it on a third board is
  `REPOSITORY_OWNER_MISMATCH`, message naming the owner board's UUID), and the
  repo-scoped agent's `board_items` query — which follows the repository, not
  the team — finds its work on the new board while every `transition_item`
  fails with `transition_items`, because its powers came from a team that no
  longer owns the service. An unattended agent would sit there retrying. The
  reorg is only finished when the machines' memberships move across too.
- The I-0012 guards both fire with the detail that makes them usable: 409
  `still owns 1 repository: [slug]`, then 422 `BOARD_NOT_EMPTY` naming the
  live card. The old delivery board is 404 after the delete, so no orphan
  board survives the team.
- **Fixture note:** the wound-down team is built by hand (CLI) rather than via
  `teamFixture`, because the journey deletes it and `teamFixture`'s ledger
  closures are strict — a second DELETE would be reported as a teardown
  failure. The SURVIVING team uses the fixture as normal. Same reason the
  service account and its memberships are registered by this journey rather
  than through `createAgent()`.
- **Harness note for whoever runs these next:** macOS has no `flock(1)`. The
  three agents sharing this stack serialise on a DIRECTORY lock:
  `until mkdir /tmp/kairos-uat.lock 2>/dev/null; do sleep 3; done; …;
  rmdir /tmp/kairos-uat.lock`. A stale regular FILE at that path blocks every
  `mkdir` forever — one was removed during this task.