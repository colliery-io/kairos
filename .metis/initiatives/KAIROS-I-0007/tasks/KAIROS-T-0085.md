---
id: web-team-landing-page-v1-fixed
level: task
title: "Web: team landing page v1 — fixed opinionated layout"
short_code: "KAIROS-T-0085"
created_at: 2026-08-29T02:59:03.767757+00:00
updated_at: 2026-08-29T02:59:03.767757+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0007
---

# Web: team landing page v1 — fixed opinionated layout

## Parent Initiative

[[KAIROS-I-0007]] — Team Landing Pages. Depends on KAIROS-T-0083 (pages/announcements API) and composes KAIROS-T-0084's panel.

## Objective

Rebuild `/teams/:slug` as the fixed v1 landing layout: header, rendered Charter, Announcements, Members, Delivery board, Streams, Documentation tree, Work documents.

## Implementation Notes

- Switch slug resolution to `GET /api/teams/by-slug/{slug}` (kill the fetch-all scan at teams.rs:125–133).
- Layout (fixed, not configurable — v1 decision): PageHeader; Charter panel rendering the charter page's markdown via the existing XSS-safe pipeline; Announcements panel (pinned first, newest first, post box for team members/org admins — plain textarea + Post; delete affordance per permission); the existing Members/Board/Streams panels; Documentation tree navigator (folders expandable, pages linking to `/teams/:slug/pages/{path}` — route lands in T-0086, so link targets may 404 until then: land T-0086 in the same release or stub the route); Work documents panel (T-0084).
- Data layer: team pages tree + announcements mirrors in pages/teams/api.rs (partial mirrors + decode tests per gui-conventions §4).
- Post/delete announcement calls with error surfacing via the standard Alert pattern; permission-gate the post box client-side from whoami (team membership/org admin) — server remains authority.

## Acceptance Criteria

- [ ] /teams/:slug renders all v1 panels in the fixed order; charter markdown renders through the safe pipeline.
- [ ] Announcements: pinned-first ordering, member/org-admin post box, no comment/reaction surface anywhere.
- [ ] Documentation tree shows the seeded scaffold with folder nesting; pages link to the T-0086 route.
- [ ] By-slug fetch replaces the client-side scan; unknown slug renders a clean not-found panel.
- [ ] Mirrors carry decode tests; unit + lint + build green.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved; wave 2).
