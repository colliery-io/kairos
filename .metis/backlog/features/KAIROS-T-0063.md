---
id: gui-boards-view-should-show-flight
level: task
title: "GUI: boards view should show flight levels — strategy above initiatives above delivery"
short_code: "KAIROS-T-0063"
created_at: 2026-08-09T17:44:52.855443+00:00
updated_at: 2026-08-10T16:21:53.145474+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# GUI: boards view should show flight levels — strategy above initiatives above delivery

## Objective **[REQUIRED]**

The `/boards` view should express the Flight Levels hierarchy visually: strategy board(s) on top, initiative board(s) below them, and the per-team delivery boards at the bottom — instead of today's flat, order-free board list.

UAT feedback (Dylan, 2026-08-09).

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P1 - High (important for user experience)

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: The whole product is built on the levels model (Strategy → Initiative → Delivery); a flat board list hides the product's central idea. The `Board` list response already carries `board_level` (strategy | initiative | delivery | adr), so the data for a tiered layout is already on the client.
- **Effort Estimate**: S/M

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] `/boards` renders boards in level bands: strategy on top, then initiative, then delivery (ADR board placement decided during implementation — alongside strategy or its own band)
- [x] The delivery band groups boards by owning team (coordinate with KAIROS-I-0006's team-lens grouping — whichever lands second reuses the first, no duplicate implementation)
- [x] The layout communicates flow direction (strategy feeds initiatives feed delivery)
- [x] Opening a board behaves exactly as before
- [x] `angreal web lint` + A-0012 gates green; Playwright board-list assertions updated

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
`crates/kairos-web/src/pages/boards.rs` list rendering: bucket by `board_level`, order bands strategy → initiative → delivery, group the delivery band by `team_id`. Presentational only — no API change.

### Dependencies
Overlaps KAIROS-I-0006 (team grouping of delivery boards); interacts with KAIROS-T-0062 (the global create action lives on this view).

## Status Updates **[REQUIRED]**

- 2026-08-09: Recorded from UAT feedback session.
- 2026-08-10: Completed — delivered by KAIROS-T-0069 (KAIROS-I-0006), which implemented the shared banding (`band_models` in boards.rs, host-tested): bands ordered strategy → initiative → delivery → "Decisions" (ADR as its own band, last), delivery band grouped per owning team with `/teams/:slug` heading links, PageHeader subtitle names the flow direction, board tiles/click-through unchanged. Gates ran with that work (unit, web lint, integration, e2e) and the team-lens spec asserts 4 bands + team headings + all 5 tiles. No further work; closed against the shipped implementation (commit 8a06a30).