---
id: j10-decision-record-an-adr-is
level: task
title: "J10 decision-record: an ADR is decided, a later one supersedes it"
short_code: "KAIROS-T-0140"
created_at: 2026-09-23T03:45:47.607868+00:00
updated_at: 2026-09-23T10:32:49.731774+00:00
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

- [x] Green in compose (hand-run): 9 steps, ~1.7s. The report reads as one
      decision's whole life — refused, drafted, argued, decided, implemented,
      replaced, traced from both ends.
- [x] Two ADRs, one document and both relationship edges ledgered; teardown
      clean (no failures reported).
- [x] tsc clean; no new tools or nouns, so the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `754ddeb` (`uat/journeys/decision-record.journey.ts`).

Three product findings, each of which changed the story rather than the
assertion. In every case the product is right:

- **ADRs are org-level.** bob (a platform member, non-admin) cannot create
  one: `FORBIDDEN … requires capability "manage_adrs"` on the ADR board. The
  task doc and the initiative both say "the team records an ADR"; the team
  cannot. His refusal now OPENS the journey — it is a better first beat than
  alice quietly doing everything, and it pins the gate.
- **Draft → Decided is not a legal move.** The seeded ADR board's graph is
  Draft → Discussion → Decided → Superseded, so the hurried jump is refused
  with `invalid transition (422)` and `Allowed target columns:` naming
  Discussion. The journey tries the jump deliberately and then walks it.
- **The editorial lifecycle is a DOCUMENT concept, not an ADR one.**
  `set_document_lifecycle` / `POST /api/documents/{code}/lifecycle` exists
  only for documents (draft|review|published|archived); an ADR's item page
  renders no `Lifecycle` panel and no `.kairos-lifecycle-badge`. So "the old
  one's lifecycle moves" (initiative wording) is really "the old one's board
  COLUMN moves" — the journey asserts both halves of that contrast in one
  step, because the two axes are easy to confuse.

Selector/shape notes for whoever edits this next:

- `gui.ts`'s `column()` is delivery-board-only: it scopes to
  `section.kairos-board__lane--planned` and falls back to `<main>`, and a
  non-delivery board (ADRs, strategies, initiatives) has NEITHER — lanes are
  a projection of `work_class` on delivery boards only, and the app shell has
  no `<main>`. The journey carries a local `adrColumn()` that goes straight
  to `section.kairos-board__column`. `dragCard()` has the same limitation.
- The ADR DTO carries `column_id` (UUID), not a column name — resolve it
  against `/api/boards/{id}` rather than expecting `column.name`.
- ADRs are never canvas nodes in the graph explorer (the canvas is
  strategy | initiative | task). A `supersedes` pair shows up in the
  "Supporting material" panel, as `<new> → supersedes`.