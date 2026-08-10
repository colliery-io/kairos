---
id: team-directory-and-detail-pages
level: task
title: "Team directory and detail pages (/teams, /teams/:slug)"
short_code: "KAIROS-T-0067"
created_at: 2026-08-09T17:54:41.126974+00:00
updated_at: 2026-08-09T18:15:27.580792+00:00
parent: KAIROS-I-0006
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0006
---

# Team directory and detail pages (/teams, /teams/:slug)

## Parent Initiative **[CONDITIONAL: Assigned Task]**

[[KAIROS-I-0006]]

## Objective **[REQUIRED]**

Add user-facing team pages to kairos-web: a `/teams` directory and a `/teams/:slug` detail page (roster, delivery board link, delivery streams). Foundation task of KAIROS-I-0006 — independently shippable.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] Team read wrappers (`list_teams`, `get_team`, `list_team_members`, `list_stream_teams`) are shared between admin and user-facing pages (hoisted from `pages/admin/api.rs` or equivalent) — no duplicated fetch code
- [ ] `/teams` renders all teams: name, type pill (stream_aligned / platform / enabling / complicated_subsystem), member count; each links to detail
- [ ] `/teams/:slug` renders: PageHeader with name + type pill; roster panel (member display names); delivery board panel linking to `/boards/{delivery_board_slug}`; streams panel listing delivery streams the team participates in
- [ ] Both routes live inside the authenticated `ParentRoute` shell in `app.rs`, mirroring the boards/board pattern; unknown slug → the standard not-found/error treatment
- [ ] Works for a non-admin user (bob) — no admin capability required
- [ ] Aurora Dark components and `token::*` colors only; `angreal web lint` clean
- [ ] `angreal test unit` green; `cargo build -p kairos-web` (trunk build) succeeds

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
Per the initiative design: compose three existing member-readable reads. Follow the structure of `pages/boards.rs` + `pages/boards/` for page module layout, and `pages/admin/teams.rs` for the team API types already modeled client-side. Stream membership comes from listing delivery streams and their teams (or `list_stream_teams` per stream) — pick whichever the client API surface makes cheap; teams are small.

### Dependencies
None — this is the foundation; KAIROS-T-0068/0069/0070 build on it.

## Status Updates **[REQUIRED]**

- 2026-08-09: Created from KAIROS-I-0006 decomposition.
- 2026-08-09: Implemented. New `pages/teams.rs` (TeamsPage directory + TeamPage detail) with `pages/teams/api.rs` as the shared team data layer (Team/TeamMember/DeliveryStream/BoardRef mirrors + list_teams/team_members/list_streams/stream_teams/list_board_refs + team_type_color); `pages/admin/api.rs` now re-exports the team/stream reads instead of defining its own (writes stay admin-only). Routes `/teams`, `/teams/:slug` registered in app.rs (+ route-map doc); "Teams" NavLink added. Detail resolves slug via list_teams, delivery board via list_board_refs, streams via per-stream stream_teams (no reverse endpoint; documented). Mirror field-name-lock tests moved/added. `cargo check` clean, unit tests green (incl. 3 teams::api tests), `angreal web lint` clean.