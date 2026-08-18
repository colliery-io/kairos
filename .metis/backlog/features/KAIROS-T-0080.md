---
id: parent-progress-rollup-children-by
level: task
title: "Parent progress rollup: children-by-column summary on board cards and detail header"
short_code: "KAIROS-T-0080"
created_at: 2026-08-16T14:58:38.040094+00:00
updated_at: 2026-08-16T14:58:38.040094+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#feature"


exit_criteria_met: false
initiative_id: NULL
---

# Parent progress rollup: children-by-column summary on board cards and detail header

## Objective

UAT feedback: "when a ticket has children, in general a 'burndown' view should be viewable to see progress based on supporting work." Ship the highest-value increment first: an always-visible **N-of-M rollup** (segmented by column) on the parent's board card and detail header, with the foundations (machine-readable "done", structured transition capture) that a true time-series burnup needs later.

## Backlog Item Details

### Type
Feature

### Priority
P1 — UAT feedback; core flight-levels value (an initiative card should summarize its tasks).

### Business Justification
- **User Value**: See how supporting work is progressing without opening every child — on the board where you already look.
- **Effort Estimate**: M — one migration + flag UI, one read endpoint + a grouped-query extension, two contained GUI renders on existing data-flow patterns.

## Current State

- Hierarchy is exactly Strategy→Initiative→Task via `parent` edges (kairos-core/src/graph.rs:147–150), acyclic-enforced, stored in `item_relationships` (models/graph.rs:22–28).
- A point-in-time snapshot is cheap: `POST /api/search` traverse runs one recursive CTE (kairos-db/src/search.rs:299–348) returning hydrated children with `column_id` — but **nothing computes counts-by-column for a parent today**: the one-hop relationships endpoint hydrates only short_code/type/title (no column, api/meta/relationships.rs:111–154); board-items is board-scoped, not parent-scoped; cascade-preview returns short codes only.
- **No machine-readable "done"**: `board_columns` has only id/board_id/name/position/timestamps (schema.rs:44–51); terminal-ness exists only as the dead-end heuristic that is explicitly ambiguous between "Completed" and misconfiguration (kairos-core/src/board.rs:284–296).
- **Time-series is not practically available**: `item_history` is content-only; transitions land in `activity_log` as free text `column:{from_name}->{to_name}` (kairos-db/src/boards.rs:300–307) — names not ids, renames corrupt replay — and activity queries take one entity_id at a time. WS events are best-effort pg_notify, no replay.
- **GUI shows no progress anywhere**: item detail's RelationshipsPanel is grouped link lists (item.rs:260–301); the graph explorer's responses carry no column data.
- Only `parent`-edge children can contribute progress — supports/informs link to Documents/ADRs which have no board position.

## What Other Tools Do (survey summary)

Every major tool ships the **snapshot rollup first, chart second**. **GitHub** sub-issues: "N of M" pill + micro-bar in lists — count-only. **Jira**: tri-segment epic bar (done/in-progress/to-do) defaulting to issue count; its burndown/burnup live in a buried Reports tab and disagree with each other (cautionary tale). **Linear** (strongest precedent): always-on progress ring on project rows, initiative progress auto-computed from member projects' ratios (two-level rollup), and the Project Graph (scope + completed lines) surfaced in the project overview — all from one data source. **Azure DevOps**: per-column rollup choice (progress %/count/sum-of-field) — cleanest count-vs-points separation, but table-only. Anti-patterns: Monday's weighted status composition (config burden), Asana's dashboard-only burnup (progress nobody sees). Mapping: count-based snapshot on card + detail header, structured so points could swap in later; second increment is a Linear-style **burnup** (scope line suits Kanban's continuous intake), not classic burndown.

## Options Considered

**A. Client-side snapshot (zero server change)** — detail page issues a traverse, groups client-side. *Rejected as the shipped shape*: board-card badges become N+1 (one search per parent card per render); "done" must be inferred from the heuristic core code calls ambiguous; logic stranded in kairos-web (CLI/MCP can't reuse).

**B. Server-side children-progress rollup + explicit done flag (RECOMMENDED)** — see below.

**C. True time-series burnup now** — structured transition capture + aggregation endpoint + charting. *Rejected for v1*: history before capture ships is unreconstructable (free-text column names), so charts start empty; requires a charting subsystem the GUI lacks and time-bucketing the server lacks; still needs everything in B anyway. **V2**, unlocked by B's foundations.

## Recommendation (v1 sketch)

1. **Schema**: migration adds `is_done boolean NOT NULL DEFAULT false` to `board_columns`; seed flags on system_board_defaults' terminal columns; board admin UI exposes a toggle, with the dead-end heuristic (board.rs:284–296) used only to **pre-suggest**, never silently set.
2. **API**: `GET /api/{family}/{code}/children-progress` → `{total, done, by_column: [{column_id, column_name, board_id, is_done, count}]}` over **direct `parent` children** (one grouped join, honoring soft-delete exclusion); extend the board-items response with an optional per-item `children_progress: {done, total}` computed in the same request (single grouped query — no N+1).
3. **GUI**: segmented bar + "N of M done" in the item detail header (above RelationshipsPanel); compact N-of-M micro-badge on parent cards on boards; both refresh via the existing WS-refetch pattern.
4. **Rollup depth**: one level — an initiative summarizes its tasks, a strategy its initiatives (Linear's ratio rollup precedent; hierarchy is only two parent hops deep).
5. **V2 foundation (decide whether to include)**: at transition time, also write structured from/to column ids (new `item_transitions` table or structured activity_log columns — fixes the free-text-names defect at the source). Nothing reads it in v1, but burnup history starts accruing the day it lands.

## Acceptance Criteria

- [ ] Migration adds `is_done` to board_columns; admin UI toggle; heuristic pre-suggests but never auto-sets; system defaults seed terminal columns.
- [ ] `GET /api/{family}/{code}/children-progress` returns {total, done, by_column} over direct parent children, excludes soft-deleted, 404-consistent with existing relationship endpoints.
- [ ] Board-items response embeds `children_progress {done, total}` for items with children, computed without per-item queries (query-count assertion in tests).
- [ ] Detail header renders a segmented bar by column (done segments visually distinct) + "N of M done"; items without children render no progress UI.
- [ ] Parent cards show a compact N-of-M badge with micro-bar only when children exist.
- [ ] When no child column is flagged is_done, the UI shows composition only — never a misleading done percentage.
- [ ] Both surfaces refresh via existing WS-event-triggered refetch when a child transitions/creates/unlinks — no new WS event types.
- [ ] Documents/ADRs (supports/informs) never count toward progress; only parent-edge children do.
- [ ] kairos-client exposes the children-progress types/call for CLI and the skills plugin.
- [ ] Unit tests for grouping/done-count logic; integration tests for multi-board children and the zero-done-columns case.

## Open Questions

- Backfill: auto-apply the heuristic to existing boards with an audit entry, or strictly manual opt-in?
- Strategies: depth-2 rollup (tasks under their initiatives, ratio-of-ratios) or initiative-level composition only for v1?
- Does the graph explorer get the rollup in v1, or detail + boards only?
- Include the structured transition capture (from/to column ids) in v1 so v2's burnup history starts accruing immediately?
- Badge real estate on current card layouts — coordinate with KAIROS-T-0075/T-0076 card rework?

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
