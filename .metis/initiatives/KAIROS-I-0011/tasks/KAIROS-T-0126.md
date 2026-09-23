---
id: gui-fluid-board-columns-columns
level: task
title: "GUI: fluid board columns — columns share the lane width instead of fixed 260px, so a five-column board fits a laptop viewport"
short_code: "KAIROS-T-0126"
created_at: 2026-09-23T01:14:11.106744+00:00
updated_at: 2026-09-23T01:14:11.106744+00:00
parent: KAIROS-I-0011
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/active"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# GUI: fluid board columns — columns share the lane width instead of fixed 260px, so a five-column board fits a laptop viewport

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Dylan's call on the T-0124 #8 design note ("fluid column across sounds great"): the default five-column delivery board (Backlog, Todo, Blocked, Active, Completed) overflows a 1280px viewport at a fixed 260px per column (1478px in a ~996px lane), forcing horizontal scroll inside each lane. Make columns fluid so they share the lane width, keeping a readable floor and the existing horizontal scroll only as the fallback below it.

## Implementation Notes

### Technical Approach

- `crates/kairos-web/app.css` `.kairos-board__column`: `flex: 1 1 0; min-width: 180px;` (five columns + four gaps ≈ 940px fits the 1280px journey viewport; below that the lane's existing `overflow-x: auto` takes over). Cards: `.kairos-card { min-width: 0; }` and `overflow-wrap: anywhere` on the title/code so long titles wrap rather than widen the column.
- Repo-grouped lanes (`.kairos-board__lane--repo`) and the Support lane share the same column class, so they follow.
- No Rust change expected; update the "horizontally scrolling row of equal-width columns" comment.
- Gates: `angreal web lint`, `angreal web build`, `angreal test e2e` (drag.spec, lanes.spec, smoke drag paths), `angreal test uat --journey cross-team,agent-loop,planning` (the drag journeys, now without the scroll-mid-press hazard on a fitting board).

## Acceptance Criteria

- [x] lanes.spec asserts `scrollWidth - clientWidth === 0` on every `.kairos-board` row at 1280×720 (needed `box-sizing: border-box` — the 180px floor was content-box and overflowed by 82px).
- [x] Titles wrap (`overflow-wrap: anywhere` on `.kairos-card__title` only); the short code stays one token and the head Group wraps the pills underneath.
- [x] e2e 11/11 (lanes.spec 1.4min → 1.0s once its drags were scroll-safe); UAT compose run `mudf590b` 5/5.

## Status Updates

**2026-09-22** — Completed in `f051dbf`.

- Side effect worth knowing: wrapped card heads make the Support lane taller, which exposed the T-0124 #8 `dragTo` hazard in the e2e tier (drops landed on whatever sat under the stale pointer — seeded cards ended up in Support/Active). Fixed by porting the UAT scroll-safe driver to `e2e/helpers/drag.ts` and using it in drag/lanes/smoke/team-lens; the `toPass` wrappers stay but no longer retry.
