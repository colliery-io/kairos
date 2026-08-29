---
id: team-pages-e2e-fixture-wave-seeded
level: task
title: "Team pages e2e + fixture wave: seeded content, teampages.spec, spec fallout"
short_code: "KAIROS-T-0087"
created_at: 2026-08-29T02:59:11.409291+00:00
updated_at: 2026-08-29T02:59:11.409291+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0007
---

# Team pages e2e + fixture wave: seeded content, teampages.spec, spec fallout

## Parent Initiative

[[KAIROS-I-0007]] — Team Landing Pages. Final wave; depends on KAIROS-T-0082..0086.

## Objective

Make the feature demonstrable and regression-proof: seed-demo team content, a teampages e2e spec, and the fallout sweep across existing specs/tests.

## Implementation Notes

- **seed-demo**: charter content for platform/web beyond the template skeleton; one announcement each (one pinned); a how-to page under platform's Documentation; a document attached to a platform TASK so the Work-documents panel has a demo-visible row (the stock PRD supports an org-level initiative and correctly does not appear).
- **teampages.spec.ts**: login → /teams/platform → scaffold visible (charter rendered, tree with the diataxis folders) → announcements pinned-first + post as alice → doc tree navigate to a page → edit/save → conflict path (API writer forces 409, walk one merge) → charter shows no destructive controls → work-documents panel lists the seeded task doc → bob-on-web negative (post box absent for non-member... bob is on platform; use carol or web's page for the negative).
- **Fallout sweep**: team-lens.spec (the /teams/:slug layout changed — roster/board/stream assertions), seed_demo counts (documents/pages/announcements), tenant_provisioning if fixtures moved. Full ladder at the end.

## Acceptance Criteria

- [ ] seed-demo provisions the demo team content above; seed_demo test asserts it.
- [ ] teampages.spec covers scaffold, announcements (one-way, pinned-first, permission), page edit + 409 merge, charter protection, and the work-documents panel — green without retries.
- [ ] Existing specs/tests updated for the new layout; full ladder green (unit, integration, e2e).

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved; wave 2).
