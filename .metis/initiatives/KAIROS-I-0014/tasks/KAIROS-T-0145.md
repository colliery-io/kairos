---
id: j16-quarterly-review-the-portfolio
level: task
title: "J16 quarterly-review: the portfolio read across strategy, initiatives, streams and blocked chains"
short_code: "KAIROS-T-0145"
created_at: 2026-09-23T03:46:03.461308+00:00
updated_at: 2026-09-23T10:22:02.469202+00:00
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

- [x] The journey is green in compose mode, and its report reads as the story.
      7 steps, ~2s: the boards overview by flight level, the strategy's
      bets, every initiative badge vs its rollup, the lead initiative
      counted by hand across two delivery boards, the blocked chain into
      the graph, the CLI traverse, and the stream view.
- [x] Nothing `uat-` is left behind — the journey creates nothing at all
      (read-only, like J13), so there is no ledger and no teardown.
- [x] `npx tsc --noEmit` clean in `uat/`; no new tools or nouns (`search`
      and the GUI only), so the drift gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `5b8440f`.

- The rollup claim is asserted THREE ways, not two: the board's bulk
  rollup (`board_children_progress`, what the card badge draws), the
  per-item `children-progress` endpoint (`graph::children_progress`), and
  the ground truth — the cards actually sitting in the actual columns,
  found by a depth-1 `parent` traverse and located on each board's
  `is_done` columns. Those are three distinct code paths over one graph.
- The seed happens to make this a real test: `DEMO-I-0001`'s five children
  span BOTH `platform-delivery` and `web-delivery`, so the step asserts
  the children cross more than one board before counting them. A rollup
  that only ever summarised one board would not be worth checking.
- **Finding (not a defect, worth stating):** a strategy traverse is blind
  to the standing buckets. `search --from <strategy> --relationships
  parent --depth 2` reaches 2 initiatives and 8 tasks; 3 tasks hang off
  the Bugs / Tech Debt buckets, which have no parent bet, so a portfolio
  review that reads only the strategy tree silently drops them. The
  journey observes the count rather than asserting a number.
- Selector notes: SVG `<text>` has no `innerText` — `allTextContents()`,
  not `allInnerTexts()`, for `text.kairos-graph__code` (the first attempt
  got `undefined`). The card's dependency pill is
  `a.kairos-card__blocks` and navigates to `/items/<code>?view=graph`.
  Board bands are `section.kairos-board-band` with `a.kairos-board-tile`
  tiles; the `Initiatives` band is exactly one board by A-0002.
- The board `blocks_summary` only COUNTS; which card is in the way comes
  from `/api/tasks/<code>/relationships` (`incoming` group
  `relationship: "blocks"`). Picking the blocker out of the summary map
  happened to work on the seed and would have been a lie on any other
  data.