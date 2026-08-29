---
id: web-team-landing-page-v1-fixed
level: task
title: "Web: team landing page v1 — fixed opinionated layout"
short_code: "KAIROS-T-0085"
created_at: 2026-08-29T02:59:03.767757+00:00
updated_at: 2026-08-29T13:43:39.668969+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

- [x] /teams/:slug renders all v1 panels in the fixed order; charter markdown renders through the safe pipeline.
- [x] Announcements: pinned-first ordering, member/org-admin post box, no comment/reaction surface anywhere.
- [x] Documentation tree shows the seeded scaffold with folder nesting; pages link to the T-0086 route.
- [x] By-slug fetch replaces the client-side scan; unknown slug renders a clean not-found panel.
- [x] Mirrors carry decode tests; unit + lint + build green.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved; wave 2).
- 2026-08-29: COMPLETE. Data layer (pages/teams/api.rs): `team_by_slug` (kills the directory scan), `TeamPageNode` + `Announcement` partial mirrors with decode tests, `team_pages`/`team_announcements`/`post_announcement`/`delete_announcement`. `WhoamiUser` mirror gained `id` (drives the author-delete affordance; boards.rs + gating.rs fixtures updated). teams.rs detail rebuilt in the fixed v1 order: PageHeader → Charter (markdown via item::markdown::to_html — module made pub(crate) — with an "Open / edit" link to the T-0086 route) → Announcements → Members → Delivery board → Streams → Documentation tree → Work documents. AnnouncementsPanel: pinned Pill + date, body through the safe markdown pipeline, post box (plain textarea + Post, gated by whoami team membership/org-admin, server stays authority), delete × for author-or-admin, errors via Alert + item api's `error_text`. DocTree: recursive eager render (views built owned — Leptos 'static lesson), native `<details>` disclosure per folder, pages link to `/teams/:slug/pages/{slug-path}`, charter excluded (own panel), new `.kairos-doctree__*` styles in app.css. Unknown slug: dedicated 404 arm with a clean not-found panel + directory link. T-0086 lands in this same wave, so tree links resolve within the release.
- 2026-08-29: Verified: `angreal test unit` green (61 kairos-web tests incl. 3 new decode tests), `angreal web lint` clean (tokens only), `angreal web build` succeeds. Playwright coverage arrives with T-0087.