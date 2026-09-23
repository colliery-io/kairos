---
id: j14-growing-team-a-member-joins
level: task
title: "J14 growing-team: a member joins and is granted capabilities; another leaves and their work is reassigned"
short_code: "KAIROS-T-0143"
created_at: 2026-09-23T03:45:57.785370+00:00
updated_at: 2026-09-23T10:32:30.901355+00:00
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

- [x] Green in compose (hand-run): 12 steps, 2.6s. The report reads as a roster
      change — the joining contract, the grant, the offboarding, the inherited
      card, and both LAST_ADMIN refusals.
- [x] Clean: team + two tasks + the board grant, all ledgered; no teardown
      failures (both cards are retired inside the story, so their ledger
      entries tolerate a 404).
- [x] `npx tsc --noEmit` clean; no new tools or nouns, so the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `8c32074` (`uat/journeys/growing-team.journey.ts`).

- **Cast correction:** the seed has three humans and identities are
  JIT-provisioned at first login, so there is no way to mint a fourth person
  for the "joins" beat. carol plays the joiner (she belongs to no team on the
  new board), and the org-JOIN contract is asserted through its refusal
  instead: `kairos members add --email <never-logged-in>` exits 1 with "users
  are provisioned at first login, so ask them to log in once, then add them".
- **The offboarding finding:** tasks have no assignee — a card belongs to a
  board, not a person — so "their in-flight work reassigned" is really "the
  card stays put and someone else must be able to move it". The journey asserts
  both halves: bob cannot transition his own card once his team membership is
  gone, and carol (granted `manage_tasks` + `transition_items` minutes earlier)
  finishes it. Without the grant the board would hold a live card nobody could
  touch, which is the real orphaning risk.
- **Product nit:** the refusal an ex-member gets from `transition_item` is the
  cross-team filer's message — "<code> sits in <team>'s Backlog for their
  triage" — even when the card is in Active, because it is keyed on the caller
  holding the computed `file_backlog`, not on the card's column
  (`mcp/tools.rs:1604`). The assertion is on the capability it names
  (`transition_items`), not the prose.
- **Selector notes:** the admin board page's grant editor (KAIROS-T-0043) is
  aurora `Switch`es — a `span.cl-switch`, not a checkbox — each in a `.cl-group`
  next to a `<code>` printing the raw capability, so the row is
  `.cl-group` filtered by exact text `manage_tasks`. The member picker is a
  `.cl-field` labelled "Organization member", and success is the banner
  "<email> added with: …". `GET /api/boards/{id}/members` returns a bare array
  (no `items` envelope).