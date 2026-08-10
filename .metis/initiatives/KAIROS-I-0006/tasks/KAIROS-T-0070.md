---
id: playwright-team-lens-smoke-coverage
level: task
title: "Playwright team-lens smoke coverage"
short_code: "KAIROS-T-0070"
created_at: 2026-08-09T17:54:44.305880+00:00
updated_at: 2026-08-09T18:15:35.325486+00:00
parent: KAIROS-I-0006
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0006
---

# Playwright team-lens smoke coverage

## Parent Initiative **[CONDITIONAL: Assigned Task]**

[[KAIROS-I-0006]]

## Objective **[REQUIRED]**

Extend the Playwright GUI smoke suite (e2e/, wired by KAIROS-T-0045 into `angreal test e2e`) with a member team-lens leg proving the initiative's exit criteria against the seeded demo fixture.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] New spec: bob (non-admin) logs in via real Dex → "My teams" shows `platform` → team page shows the seeded roster (alice, bob), links to the `platform-delivery` board, and shows the `customer-portal` stream
- [ ] Spec asserts `/boards` shows delivery boards grouped under team headings
- [ ] Spec asserts the `/activity` team filter narrows entries to team members' actions
- [ ] `/teams` directory renders both seeded teams (platform, web)
- [ ] No seed fixture changes (KAIROS-T-0035's fixture already contains users/teams/boards/stream needed) — if a gap emerges, it's recorded here with the fixture change
- [ ] `angreal test e2e` passes end-to-end including the new leg

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
Follow the existing suite's conventions in e2e/ (real Dex PKCE login helper, per KAIROS-T-0045). Selecting bob exercises the non-admin path — the existing suite logs in as alice (org admin), so reuse the login helper with bob's fixture credentials.

### Dependencies
KAIROS-T-0067, KAIROS-T-0068, KAIROS-T-0069 (tests the surfaces they build).

## Status Updates **[REQUIRED]**

- 2026-08-09: Created from KAIROS-I-0006 decomposition.
- 2026-08-09: Spec written: `e2e/tests/team-lens.spec.ts` — bob (non-admin) Dex PKCE login → boards level-bands + team-grouping assertions (4 bands, 5 tiles, Platform/Web heading links) → My-teams nav (Platform present, Web absent for bob) → `/teams/platform` roster/board/stream assertions → delivery-board link-through → `/teams` directory (2 tiles) → activity team-lens (aurora Select is `.cl-field > label + select` siblings — selectOption on the select; asserts the "by members of Platform" lens note). Selector calibration done against the aurora component source (cl-appshell__navbar, cl-panel/__title, cl-field/__label, cl-anchor; getByRole name matching uses exact:true to keep "Platform" from matching "Platform Delivery"). No seed changes needed. `angreal test integration && angreal test e2e` now running as the gate.