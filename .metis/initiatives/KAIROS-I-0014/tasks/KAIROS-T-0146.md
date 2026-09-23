---
id: j12-operations-health-readiness
level: task
title: "J12 operations: health, readiness, metrics, config, and a cascade preview before a big delete"
short_code: "KAIROS-T-0146"
created_at: 2026-09-23T03:46:06.062960+00:00
updated_at: 2026-09-23T10:27:47.528510+00:00
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

- [x] The journey is green in compose mode, and its report reads as the
      story. 8 steps, ~1s: health/readiness with no credentials, the
      metrics scrape with the tenant counter advancing (777 → 778), the
      config bootstrap, the staged workstream, preview (4, where the
      direct children are 1), delete matching it exactly, the children
      404 and recoverable, and the activity record naming alice and the
      whole blast radius.
- [x] Nothing `uat-` is left behind: the strategy, initiative and three
      tasks are ledgered with 404-tolerant deletes, since the story
      deletes them itself. No teardown failures in the report.
- [x] `npx tsc --noEmit` clean in `uat/`; no new tools or nouns
      (`strategies`, `initiatives`, `tasks`, `search` are already
      covered), and `ALLOW` is empty, so the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `9d72e81`.

- **Product finding: `--include-deleted` cannot be combined with
  `--query`.** Full-text matching runs against the `searchable_items`
  view, which is defined `WHERE deleted_at IS NULL` on every branch
  (`migrations/tenant/…/up.sql`), so a text query intersects to nothing
  before `include_deleted` is ever consulted. The flag does work for
  filter-only searches (`--type` / `--board`), which is how the recovery
  step asks the question. Neither the CLI help nor the S-0005 docs say
  so; worth a hygiene ticket (doc the combination, or have the server
  refuse it).
- The cascade story needed a THREE-level tree, not two: `parent` runs
  strategy → initiative → task only (kairos-core rule matrix), so an
  initiative root would have made the preview equal to its direct
  children and the assertion vacuous. Rooting at a strategy is what makes
  "4 items, where a direct-children warning would have said 1" — the gap
  KAIROS-T-0051 exists to close — a real observation.
- There is no CLI noun for relationships, so the tree is wired with
  `POST /api/relationships` (org-admin only). Recorded here because the
  same gap will bite the next journey that needs an edge.
- `/metrics` is asserted by NAME against `crates/kairos-server/src/metrics.rs`
  (`http_request_duration_seconds`, `http_requests_by_tenant_total`,
  `kairos_db_pool_connections`) and by MOVEMENT: read the tenant counter,
  make one authenticated request, require it to be strictly greater. The
  unauthenticated scrape does not resolve a tenant, so the scrape itself
  does not pollute the number.
- `/api/whoami` nests the user: `me.user.id`, not `me.user_id`.