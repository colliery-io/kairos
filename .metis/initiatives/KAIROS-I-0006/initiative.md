---
id: teams-as-a-first-class-lens-in-the
level: initiative
title: "Teams as a First-Class Lens in the GUI"
short_code: "KAIROS-I-0006"
created_at: 2026-08-09T17:37:05.682088+00:00
updated_at: 2026-08-09T18:16:10.362209+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: M
initiative_id: teams-as-a-first-class-lens-in-the
---

# Teams as a First-Class Lens in the GUI Initiative

## Context **[REQUIRED]**

Teams are fully modeled server-side (KAIROS-T-0019): CRUD, membership (`/api/teams/{id}/members`), stream↔team assignment (`/api/delivery-streams/{id}/teams`), and `whoami` returns the caller's teams (plus board-scoped capability grants per KAIROS-T-0052). The CLI exposes the whole surface.

In the GUI (KAIROS-I-0004), however, teams appear in exactly one place: `/admin/teams` (KAIROS-T-0043) — an **org-admin management panel**. Outside admin, `team_id` is used only invisibly: delivery boards carry it so newly created tasks inherit the owning team. A regular member (bob on `platform`) cannot answer "who is on my team, what is our board, which streams are we in" anywhere in the app; the boards list is a flat, team-blind list.

UAT review of the seeded demo tenant (2026-08-09) surfaced this as the most visible product gap.

**Auth reality check** (verified in `crates/kairos-server/src/api/org/teams.rs`): the MANAGE pseudo-capability gates only the five write handlers (create/update/delete team, add/remove member). List/get teams, list members, and stream-team reads require only authentication. **This initiative is therefore GUI-only — no server endpoint or ABAC changes are expected.**

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- **Team directory & detail pages**: `/teams` (all teams, type, member count) and `/teams/:slug` (roster, link to the team's delivery board, delivery streams the team participates in).
- **"My teams" in the shell**: the nav leads with the caller's own teams (from `whoami`), giving members a one-click path to their team page and delivery board.
- **Team lens on existing views**: the boards list groups delivery boards under their owning team (org-level boards in their own section); the activity view gains a team filter.

**Non-Goals:**
- No new server endpoints, schema, or ABAC changes (reads are already member-accessible).
- Team CRUD and membership management stay in `/admin/teams` — the new pages are read/navigate surfaces, not a second write path.
- No team dashboards/metrics (velocity, throughput, WIP charts) — future initiative if wanted.
- No CLI changes (it already has the full team surface).

## Use Cases **[CONDITIONAL: User-Facing Initiative]**

### Use Case 1: Member orientation
- **Actor**: bob (member of `platform`, not an org admin)
- **Scenario**: Logs in → shell shows "My teams: platform" → opens `/teams/platform` → sees roster (alice, bob), the `platform-delivery` board link, and the `customer-portal` stream
- **Expected Outcome**: Answers "who/what/where is my team" in two clicks without admin access

### Use Case 2: Cross-team discovery
- **Actor**: carol (member of `web`)
- **Scenario**: Wants to know who owns platform work → opens `/teams` directory → opens `platform`
- **Expected Outcome**: Finds the owning team and its members without asking an admin

### Use Case 3: Team-lensed org views
- **Actor**: any member
- **Scenario**: Opens `/boards` and sees delivery boards grouped by owning team; opens `/activity` and filters to one team
- **Expected Outcome**: Existing views become navigable through the team structure

## Detailed Design **[REQUIRED]**

All work lands in `crates/kairos-web` (Leptos CSR, KAIROS-A-0015).

- **API client layer**: team read wrappers (`list_teams`, `get_team`, `list_team_members`, `list_stream_teams`) already exist for the admin pages; hoist/share them so the user-facing pages reuse the same functions rather than duplicating fetch code.
- **Routes** (`app.rs` shell): add `Route /teams → TeamsPage` and `Route /teams/:slug → TeamPage` inside the authenticated `ParentRoute`, mirroring the boards/board pattern.
- **Team detail composition**: `/teams/:slug` composes three existing reads — team + members + streams. Delivery board link resolves via the team's `delivery_board_id`.
- **My teams**: the shell already holds the authenticated identity; `whoami.teams` drives the nav section. Empty for users with no team membership (section hidden, not empty-state noise).
- **Boards grouping**: `/boards` groups delivery boards by `team_id` (team name resolved from the teams list), with strategy/initiative/ADR and other org-level boards in a separate section. Grouping is presentational only.
- **Activity team filter**: **RESOLVED (design decision, 2026-08-09)** — `ActivityQuery` (verified in `crates/kairos-client/src/types_meta.rs` L351-370) supports only `entity_id` / `actor_id` / `action` / `since`; there is no team or board parameter, and server changes are a non-goal. The team filter is therefore an **actor-based client-side lens**: selecting a team resolves its member set (`list_team_members`, member-readable) and filters the fetched activity page to entries whose `actor_id` is in that set. Labeled as filtering "activity by team members" — which is what a team lens on an actor-centric log honestly means. If server-side filtering is wanted later, that's a separate backlog item.
- **Live updates**: same conventions as existing pages (`/ws/events` where applicable); roster/stream membership changes are admin-rare, so plain load-on-navigate is acceptable for team pages.

## UI/UX Design **[CONDITIONAL: Frontend Initiative]**

### User Flows
- Shell nav: `My teams` section above/near `Boards`; each entry → `/teams/:slug`.
- `/teams`: directory table/cards — name, type pill (stream_aligned / platform / enabling / complicated_subsystem), member count → detail.
- `/teams/:slug`: PageHeader (name, type pill), roster panel, delivery board panel (prominent link), streams panel.

### Design System Integration
Aurora Dark components only (Panel, Group, Pill, PageHeader, Empty, Loading, ErrorState — same vocabulary as the admin pages); colors via `token::*` / `var(--…)` only — `angreal web lint` enforces this mechanically per docs/gui-conventions.md.

## Alternatives Considered **[REQUIRED]**

- **Server-side "team home" aggregate endpoint** — rejected: the detail page is three small reads that already exist; an aggregate adds API surface for no measurable gain at this scale (50ms p95 budget is not threatened).
- **Enrich `/admin/teams` instead of new pages** — rejected: admin is capability-gated as a write surface and framed for org admins; members need a read surface without admin framing.
- **Jump straight to team dashboards (metrics/velocity)** — deferred: visibility first; measurement is a separate product decision with its own data questions.
- **Boards-list grouping only (no team pages)** — rejected as insufficient: answers "which board is my team's" but not roster or stream membership.

## Implementation Plan **[REQUIRED]**

Likely decomposition (to be confirmed at decompose phase):

1. **Team directory + detail pages** — shared API wrappers, routes, `/teams`, `/teams/:slug`. The foundation; independently shippable.
2. **My-teams shell integration** — whoami-driven nav section linking into the team pages.
3. **Team lens on boards + activity** — boards grouping by owning team; activity team filter (resolve the server-vs-client filter question first).
4. **E2E coverage** — extend the Playwright smoke (KAIROS-T-0045) with a team-lens leg: bob logs in → my-teams nav → team page shows the seeded roster/board/stream → boards page shows team grouping. Seed fixture (KAIROS-T-0035) already contains everything needed; no fixture changes expected.

Gates per KAIROS-A-0012: unit + integration tiers plus `angreal web lint` per task; the extended GUI smoke at e2e tier.

Exit criteria:
- [x] A non-admin member can reach their team's roster, delivery board, and streams from the shell nav in ≤2 clicks
- [x] `/teams` directory and `/teams/:slug` detail render for all seeded demo teams
- [x] `/boards` groups delivery boards by owning team; org-level boards remain accessible
- [x] `/activity` can be filtered to a single team
- [x] Playwright smoke covers the member team-lens path; all A-0012 per-task gates green

**Completed 2026-08-09.** All four tasks (KAIROS-T-0067..0070) done. Gates: unit tests, `angreal web lint`, `angreal test integration`, and the full `angreal test e2e` (API golden path + MCP + both Playwright specs — the new team-lens spec passed clean; the pre-existing smoke spec flaked once on its move-menu click, the documented WS-hiccup class in code this initiative did not touch, and passed on the policy retry). Every exit criterion is proven by the team-lens spec running as bob, a non-admin.