---
id: j9-explorer-carol-asks-where-a
level: task
title: "J9 explorer: carol asks where a task came from and walks search to graph to traverse"
short_code: "KAIROS-T-0139"
created_at: 2026-09-23T03:45:44.788257+00:00
updated_at: 2026-09-23T10:23:15.183759+00:00
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

- [x] Green in compose (hand-run): 7 steps, ~1.3s. The report reads as carol
      asking one question three ways and getting one answer.
- [x] Creates nothing — no ledger entries, no teardown.
- [x] tsc clean; no new tools or nouns (`cli:search` was already exercised), so
      the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `00f0610` (`uat/journeys/explorer.journey.ts`).

- The journey hard-codes no short code. carol's team comes from `whoami`, the
  task under discussion is the first one on her delivery board that has a
  parent initiative, and the search phrase is the longest word in its title —
  so the story reads the same on a deployment that has never seen the seed.
  (Against the demo seed it picks DEMO-T-0004 "Welcome-email trigger".)
- The real assertion is a three-way agreement, not three separate smoke
  checks: the codes the canvas draws (focal graph endpoint), the codes the
  `/search` traversal scope lists (POST /api/search), and the codes
  `kairos search --from … --relationships parent --depth 2` returns. GUI list
  and CLI list must be EQUAL; neither may name anything the canvas did not
  draw. All three agreed on 10 items.
- Selector notes for whoever edits this next:
  - SVG `<text>` has no `innerText` — `allInnerTexts()` returns `undefined`
    per element and blows up on `.trim()`. Use `allTextContents()` for
    `.kairos-graph__code`.
  - The search page's chip rows are ambiguous by label: `task` is both an
    entity type and a task type. Scope to the innermost `.cl-stack` that
    contains the heading text — `has:` matches every ancestor, and the
    innermost match is LAST in document order, so `.last()`.
  - On the graph canvas a node's `<rect class="kairos-graph__box">` REFOCUSES
    (pushes `?trail=`), while its `<text class="kairos-graph__code">` navigates
    to `/items/<code>`. Two different clicks on the same node.
  - The item page's Relationships panel carries an "Open the graph explorer"
    link — the natural bridge from detail to canvas, and better than
    hand-building the `?view=graph` URL.