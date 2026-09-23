---
id: j18-second-tenant-an-operator
level: task
title: "J18 second-tenant: an operator provisions a second organisation and the seam holds on every surface"
short_code: "KAIROS-T-0147"
created_at: 2026-09-23T03:46:09.329932+00:00
updated_at: 2026-09-23T10:32:00.428245+00:00
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

- [x] Green in compose (hand-run, under the shared-stack lock): 6 steps,
      ~0.4s — provision with bob as first admin, the register with both
      schemas intact, bob handed his own organisation on API/MCP/CLI, bob
      refused `/api/admin/tenants` three ways, the unconfirmed drop
      refused 422, and the retirement leaving demo untouched.
      Also run with `UAT_MODE=server`: all 6 steps skip cleanly with the
      reason printed.
- [x] Nothing `uat-` is left behind — the last step retires the
      organisation and the report ends `organisations_left: 1`; the
      ledger entry is 404-tolerant because the story deletes it.
- [x] `npx tsc --noEmit` clean in `uat/`; `admin` was already the one
      noun reached only by compose-only steps, and `ALLOW` is empty, so
      the gate is unaffected.

## Status Updates

**2026-09-23** — Completed in `177f01d`.

- **The finding that shaped the whole journey: the UAT target runs
  `KAIROS_SINGLE_TENANT=demo`** (`.angreal/task_test.py` `_gui_server_env`,
  the A-0013 evaluation posture), and that is resolution step 1 in
  `middleware/tenant.rs` — it skips request inspection ENTIRELY. So
  `X-Tenant` / `--tenant` are not consulted on this deployment: asking for
  another organisation is not refused, it is ignored, and the caller gets
  the pinned tenant. Verified directly: `X-Tenant: no-such-organisation`
  returns `organization.slug: demo`, 200.
  Consequence: the cross-tenant *data* seam (a member of A reaching B's
  work) **cannot be exercised from the UAT tier as the tier is deployed**,
  in any mode. `kairos-db/tests/isolation.rs` covers schema isolation, and
  a subdomain/header-routed deployment would exercise the
  `MEMBERSHIP_REQUIRED` path. The journey therefore proves the mechanism
  rather than claiming a refusal it never saw — and the control step
  (an organisation that does not exist answering identically) is what
  makes that claim honest instead of a passing test that means nothing.
- The story was re-aimed at what IS live here and is arguably the more
  important half: `/api/admin/tenants` is the ONLY cross-tenant surface in
  the product (registered outside the tenant middleware, gated on
  `KAIROS_DEPLOYMENT_ADMINS`), and bob is refused list, create, and the
  delete of the organisation he is org-admin OF. Org-admin authority stops
  at the deployment boundary; that is worth a test.
- Making **bob** the second organisation's first admin (`--initial-admin`
  takes an OIDC sub, found via `/api/members`) is what gives the journey
  its teeth: he is a member of two organisations and still only ever
  reaches one.
- `boards_created` in `TenantCreatedResponse` is an ARRAY of board slugs
  (`["strategy","initiatives","adrs"]`), not a count — the first draft
  asserted `> 0` and got a type error.
- The unconfirmed delete is a genuinely good guard: 422
  `CONFIRMATION_REQUIRED`, and because the membership purge and the schema
  drop share one transaction, the refusal rolls back with the organisation
  and its memberships intact (asserted).
- **Shared-stack note:** `flock` does not exist on macOS, so the
  serialisation used earlier in this session was silently a no-op. Runs
  here now take `/tmp/kairos-uat.lock` as a DIRECTORY (`until mkdir …;
  do sleep 3; done`, `rmdir` on exit). J16 and J12 were re-run green under
  the real lock after the switch.