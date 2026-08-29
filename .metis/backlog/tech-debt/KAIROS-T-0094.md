---
id: team-pages-offer-root-level-page
level: task
title: "Team pages: offer root-level page/folder creation (deferred from v1)"
short_code: "KAIROS-T-0094"
created_at: 2026-08-29T15:51:45.318008+00:00
updated_at: 2026-08-29T15:51:45.318008+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#tech-debt"


exit_criteria_met: false
initiative_id: NULL
---

# Team pages: offer root-level page/folder creation (deferred from v1)

## Objective

Give team members a way to create ROOT-level pages and folders on a team's documentation tree — the one create surface KAIROS-T-0086 deliberately left out of v1 (creation exists only inside folder indexes).

## Backlog Item Details

### Type
Tech Debt

### Priority
P3 — the scaffold's root sections (Charter, Support Processes, Documentation) cover the common shapes; this only bites when a team wants a NEW top-level section.

### Technical Debt Impact
- **Current Problems**: The API supports root creation (`POST /api/teams/{id}/pages` with no `parent_id` — integration-tested in kairos-server/tests/team_pages.rs), but no UI offers it: folder indexes carry the only create form, so users can't add a top-level sibling of Documentation without an API call. Recorded as a v1 decision on KAIROS-T-0086.
- **Benefits of Fixing**: Teams can shape their own tree top level; removes the "the API can, the UI can't" asymmetry.
- **Risk Assessment**: None operationally; mild UX confusion when someone looks for the affordance.

## Implementation Notes

- Smallest honest fix: reuse `CreateForm` (pages/teams/doc.rs) with `parent_id: None` — mount it under the Documentation panel on `/teams/:slug` (member/org-admin gated via the same whoami check) or on a small root index view. The form already handles kind/slug/title and surfaces the typed 422s (SLUG_CONFLICT etc.).
- Remember NULL-parent sibling-slug uniqueness is enforced by the COALESCE index (T-0082) — a root "charter" duplicate 422s correctly; nothing new server-side.
- e2e: one step in teampages.spec (create a root folder as bob, see it in the tree).

## Acceptance Criteria

- [ ] A member/org-admin can create a root-level page or folder from the team UI; non-members see no affordance.
- [ ] Root slug conflicts surface the server's 422 in the standard Alert pattern.
- [ ] teampages.spec covers the root-create path; ladder green for touched tiers.

## Status Updates

- 2026-08-29: Ticketed — recorded v1 deferral from KAIROS-T-0086 ("folder indexes are the create surface; creating new ROOT nodes is deliberately not offered in v1").