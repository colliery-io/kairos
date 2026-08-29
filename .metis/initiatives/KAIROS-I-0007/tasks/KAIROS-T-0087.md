---
id: team-pages-e2e-fixture-wave-seeded
level: task
title: "Team pages e2e + fixture wave: seeded content, teampages.spec, spec fallout"
short_code: "KAIROS-T-0087"
created_at: 2026-08-29T02:59:11.409291+00:00
updated_at: 2026-08-29T14:08:50.289210+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

- [x] seed-demo provisions the demo team content above; seed_demo test asserts it.
- [x] teampages.spec covers scaffold, announcements (one-way, pinned-first, permission), page edit + 409 merge, charter protection, and the work-documents panel — green without retries.
- [x] Existing specs/tests updated for the new layout; full ladder green (unit, integration, e2e).

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved; wave 2).
- 2026-08-29: Seed additions (seed.rs, one transaction): real charter content for platform+web via `update_page_content` (v2 + history rows — the chain looks lived-in), platform announcement pinned (alice) + web announcement (carol), how-to page `documentation/how-to-guides/deploy-kairos` (bob), and "Runbook: password-less auth rollout" document supporting the platform "Password-less email auth" task (the work-documents demo row; PRD stays org-level and correctly absent). `SeedError::TeamPage` variant added. seed_demo test: documents 1→2, supports edges 1→2, + new assertions (21 live team pages, 4 charter history rows, 2 announcements exactly 1 pinned).
- 2026-08-29: e2e: `helpers/api.ts` gained `getTeamPage` (slug-path resolution) + `patchTeamPage` (the competing writer); new `teampages.spec.ts` runs as bob — landing layout (charter markdown, pinned-first announcements, diataxis tree, runbook in Work documents + PRD absent), member announcement post, tree-navigate → toolbar bold-insert → edit/save → v2, API-forced 409 → take-theirs walk, charter shows Edit but no Manage/rename/delete, and the /teams/web negative (no post box, no Edit, reads open). Fallout checked: team-lens.spec's Members/board/stream assertions still hold on the rebuilt layout; smoke.spec's conflict step only pins "Edit conflict" (unchanged).
- 2026-08-29: Deflake + review fixes before green: (1) the announcement post refetches the landing page and Leptos may REUSE the details DOM (open state preserved) — blind summary clicks toggled the tree closed; the spec now forces `details.open = true` via evaluate instead of clicking. (2) doc.rs computed `manage` once at body render — if whoami landed after the pages fetch the Edit/Manage affordances never appeared; the identity is now read inside the reactive closure so the body re-renders when whoami lands.
- 2026-08-29: COMPLETE. Full ladder green: `angreal test unit` clean, `angreal test integration` 32/32 targets (incl. new seed_demo counts + the T-0086 cycle-prevention db test), `angreal test e2e` 8/8 specs passed in 9.6s with zero retries (API golden path + MCP + GUI).