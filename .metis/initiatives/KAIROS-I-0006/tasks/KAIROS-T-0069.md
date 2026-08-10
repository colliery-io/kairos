---
id: team-lens-on-boards-and-activity
level: task
title: "Team lens on boards and activity views"
short_code: "KAIROS-T-0069"
created_at: 2026-08-09T17:54:43.221038+00:00
updated_at: 2026-08-09T18:15:35.291947+00:00
parent: KAIROS-I-0006
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0006
---

# Team lens on boards and activity views

## Parent Initiative **[CONDITIONAL: Assigned Task]**

[[KAIROS-I-0006]]

## Objective **[REQUIRED]**

Apply the team lens to existing views: `/boards` groups delivery boards under their owning team, and `/activity` gains a team filter (actor-based client-side lens per the initiative's recorded design decision).

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] `/boards` groups delivery boards by owning team (team names resolved from the shared teams read); boards without a team (strategy/initiative/ADR/org-level) render in a distinct section — nothing becomes unreachable
- [ ] Each team group's heading links to that team's `/teams/:slug` page
- [ ] `/activity` offers a team selector; selecting a team filters displayed entries to those whose `actor_id` is in the team's member set (via `list_team_members`)
- [ ] The activity filter is labeled as filtering by team members and clearing it restores the unfiltered view; filter composes with the view's existing controls
- [ ] Layout satisfies KAIROS-T-0063's banding intent where cheap (delivery grouping is this task; full level-band layout may land there — coordinate, don't duplicate)
- [ ] Aurora Dark tokens only; `angreal web lint` clean; `angreal test unit` green

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
Boards: bucket the board list by `team_id` in `pages/boards.rs` list rendering — presentational only. Activity: `pages/activity.rs` already fetches pages via `ActivityQuery`; add a team RwSignal, fetch members on selection, filter the rendered entries client-side. Per the initiative: `ActivityQuery` has no team/board param and server changes are out of scope.

### Dependencies
KAIROS-T-0067 (shared team API wrappers + `/teams/:slug` to link group headings to).

## Status Updates **[REQUIRED]**

- 2026-08-09: Created from KAIROS-I-0006 decomposition.
- 2026-08-09: Implemented. Boards: `band_models()` (pure, host-tested ×2) buckets boards into flight-level bands (strategy → initiative → delivery → adr, unknown levels last — also satisfies the KAIROS-T-0063 banding intent), delivery band grouped per team with headings linking to `/teams/:slug`; teamless/unknown-team boards render under "No team" (never unreachable); failed teams read degrades to that group. `.kairos-board-band` CSS added. Activity: "Team (by members)" Select added to the filter bar; Apply resolves team name→id, a `team_lens` LocalResource fetches the roster and the FETCHED PAGE filters client-side by actor_id (per the initiative design decision — ActivityQuery has no team param), with an explicit "N of M entries on this page are by members of X" note, a lens-specific empty state, an error Banner if the roster read fails (unfiltered rather than silently wrong), and the pager kept on the server page. Unit tests + web lint green.