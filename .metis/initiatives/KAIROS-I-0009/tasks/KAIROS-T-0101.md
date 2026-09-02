---
id: team-landing-page-in-flight-rollup
level: task
title: "Team landing-page in-flight rollup: derived team links query and panel"
short_code: "KAIROS-T-0101"
created_at: 2026-09-01T23:12:38.101728+00:00
updated_at: 2026-09-02T10:31:15.763890+00:00
parent: KAIROS-I-0009
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0009
---

# Team landing-page in-flight rollup: derived team links query and panel

## Parent Initiative

[[KAIROS-I-0009]] — Git Forge Integration. The PO's second requirement ("that should probably work also for team landing pages"). Depends on KAIROS-T-0097 and KAIROS-T-0100 (shares its DTOs).

## Objective

`GET /api/teams/{id}/links` plus an "In flight" panel on `/teams/:slug`: what is open across this team's work, spanning all its repos.

## Implementation Notes

- **Reuse the KAIROS-T-0084 derivation shape exactly.** `team_work_documents` already answers "things attached to this team's work" with one grouped query: items are the team's when they are a task with `team_id = {team}` OR any live item on the team's delivery board. Same predicate here, joined to `item_links`; put it beside that function in `crates/kairos-db/src/graph.rs` (or a sibling module) so the two stay visibly parallel — if the team-work definition ever changes, both must change together, and adjacency is what makes that obvious.
- Also include links whose `forge_connections.team_id` matches directly (KAIROS-T-0097's optional column), so a repo explicitly attributed to a team contributes even when a specific work item's team is unset. `DISTINCT` on the link; one row per link regardless of how it qualified.
- **Default to open work**: the panel's question is "what is in flight," so the endpoint takes `?state=` (default `open,draft`) rather than dumping merged history. Merged PRs belong to the item's own panel, not the team rollup.
- Ordering: newest `forge_updated_at` first. Cap the result (the T-0084 precedent takes everything, but a busy team could have hundreds of PRs — pick a sane limit and record it; a `limit` query param with a documented default is fine).
- **Web** (`crates/kairos-web/src/pages/teams.rs`): an "In flight" panel in the fixed v1 layout — place it after Work documents so the landing page reads charter → announcements → people/board/streams → docs → work documents → in flight. Rows: state chip, PR title linking to the forge, repo name (mono, dimmed), and the Kairos item short code linking to `/items/{code}` so the two worlds connect in both directions.
- Empty state is honest and scope-naming, in the T-0084 style: "Nothing open across this team's repos" — and say what counts (this team's work items and repos attributed to the team), so an empty panel does not read as broken.
- Mirror + decode test in `pages/teams/api.rs`, per convention.

## Acceptance Criteria

## Acceptance Criteria

- [x] `GET /api/teams/{id}/links` returns open links for the team by all three qualifying paths (task `team_id`, delivery-board item, repo attributed to the team), DISTINCT, newest first, 404 on unknown team.
- [x] Default state filter is open+draft; merged/closed reachable via the query param; result cap documented.
- [x] The team-work predicate is implemented adjacent to `team_work_documents` and the parallel is noted in both, so the definitions cannot drift silently.
- [x] `/teams/:slug` renders the "In flight" panel with forge links and item short-code links; the empty state names what counts.
- [x] kairos-client DTO + method; web mirror decode test; unit + lint + build green; server integration test covers all three qualifying paths plus the DISTINCT case.

## Status Updates

- 2026-09-01: Created from the KAIROS-I-0009 decomposition.
- 2026-09-02: COMPLETE. `graph::team_link_rollup` sits directly beside `team_work_documents` with a doc comment in each naming the other, since paths 1 and 2 are that function's exact predicate — the drift risk the ticket called out. `DISTINCT ON (l.id)` so a link qualifying several ways appears once; `state = ANY($3)` with the default `open,draft` applied at the handler; `limit` clamped to 1..=500, default 100 (recorded — the T-0084 precedent takes everything, which a busy team would regret).
- 2026-09-02: `GET /api/teams/{id}/links` in org/teams.rs beside `work-documents`, open tenant-wide, explicit `TeamLinksQuery` struct (serde_urlencoded cannot flatten), openapi registered. `types_forge::TeamLink` carries the work item's short code and title alongside the link, so the panel connects both directions; `client.team_links()`; web mirror + decode test in pages/teams/api.rs.
- 2026-09-02: Web: "In flight" panel added last in the fixed v1 layout (charter → announcements → members → board → streams → documentation → work documents → in flight). Rows: state Pill using the same `link_state_color` mapping as the item Development panel (no red), PR title as a `target="_blank" rel="noopener noreferrer"` anchor to the forge, `forge · owner/repo` mono-dimmed, and a dimmed "on {CODE} — {title}" line linking back to `/items/{code}`. Empty state names all three qualifying paths so a blank panel does not read as broken.
- 2026-09-02: Verified: `forge_webhook` extended to prove each qualifying path in isolation — unattributed task+repo yields nothing; attributing the REPO qualifies the link; clearing that and setting the TASK's team qualifies it again; the default filter drops the merged PR (the "in flight, not history" contract); unknown team 404s. `angreal test unit` clean, `angreal web lint` clean, `angreal web build` succeeds, `angreal test integration` 34/34 targets green.