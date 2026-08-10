---
id: my-teams-shell-navigation-from
level: task
title: "My-teams shell navigation from whoami"
short_code: "KAIROS-T-0068"
created_at: 2026-08-09T17:54:42.481563+00:00
updated_at: 2026-08-09T18:15:33.749484+00:00
parent: KAIROS-I-0006
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0006
---

# My-teams shell navigation from whoami

## Parent Initiative **[CONDITIONAL: Assigned Task]**

[[KAIROS-I-0006]]

## Objective **[REQUIRED]**

Surface the caller's own teams in the shell navigation: a "My teams" section driven by `whoami.teams`, linking each team to its `/teams/:slug` page — the one-click orientation path for members.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] The shell nav (the `Shell` component in `app.rs`) shows a "My teams" section listing the authenticated user's teams from the whoami response
- [ ] Each entry navigates to `/teams/:slug`
- [ ] Users with no team memberships see no section (hidden, not an empty-state)
- [ ] The section renders from already-fetched identity state — no extra whoami round-trip per navigation
- [ ] Active-route styling consistent with existing nav entries; Aurora Dark tokens only; `angreal web lint` clean
- [ ] `angreal test unit` green

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
The shell already holds the authenticated identity (`use_auth` / whoami fetched at KAIROS-T-0039's shell). Confirm the whoami response type in the web client includes `teams` (the server returns it per KAIROS-T-0052-era `WhoamiResponse`); if the web-side struct omits the field, extend the deserialization — still client-only work.

### Dependencies
KAIROS-T-0067 (needs `/teams/:slug` to link into).

## Status Updates **[REQUIRED]**

- 2026-08-09: Created from KAIROS-I-0006 decomposition.
- 2026-08-09: Implemented. Shell now creates ONE shared whoami LocalResource (Copy) passed to both WhoamiBadge (refactored to take it as a prop — its private fetch removed) and the new MyTeamsNav component: a "My teams" nav section from `whoami.teams` (slug+name already in the mirror), each entry a NavLink to `/teams/:slug`, hidden entirely when the user has no teams (load/error render nothing — the badge surfaces those). `.kairos-nav__section` style added to app.css (hairline separator, `--edge`/`--space-*` tokens only). Unit tests + web lint green.