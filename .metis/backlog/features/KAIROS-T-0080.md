---
id: parent-progress-rollup-children-by
level: task
title: "Parent progress rollup: children-by-column summary on board cards and detail header"
short_code: "KAIROS-T-0080"
created_at: 2026-08-16T14:58:38.040094+00:00
updated_at: 2026-08-24T12:38:20.807959+00:00
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

## Acceptance Criteria

- [x] Migration adds `is_done` to board_columns (guarded, default false); admin ColumnsPanel gains Mark/Unmark done + done pill; the dead-end heuristic renders a gold suggest-only "dead end" pill and never sets the flag; fresh boards seed Completed (workflow) / Decided+Superseded (ADR) via `seeded_done_column` — verified by the graph integration test.
- [x] `GET /api/{family}/{code}/children-progress` returns {total, done, has_done_columns, by_column} over direct parent children; soft-deleted excluded; 404-consistent with relationships (meta.rs test: rollup follows a child walked to Completed; leaf empty; mismatch 404).
- [x] Board-items embeds a `children_progress` map (parent short code → {done, total, has_done}) computed by `board_children_progress` — structurally ONE grouped query for the whole board (the db test exercises the batch; no per-item query exists to count).
- [x] Detail renders the segmented ChildrenProgressBar (done segments in --ok, per-column tooltips) + "N of M done"; childless items render nothing (progress.spec asserts both).
- [x] Parent cards show the N-of-M micro-badge + fill bar only when the rollup exists (progress.spec asserts "1/5 done" on the seeded initiative).
- [x] `has_done`/`has_done_columns` distinguish zero-done from no-done-semantics: composition-only "N children" rendering, never a fraction (db test covers the all-unflagged case end to end).
- [x] Both surfaces ride existing refetch flows (board WS refetch rebuilds card models incl. the progress key; detail refetches wholesale) — no new WS event types.
- [x] Supports/informs never count: the rollup joins a live workflow-item union through `parent` edges only (db test links a supports document and asserts exclusion).
- [x] kairos-client exposes `children_progress()` + ChildrenProgressResponse/ProgressCounts DTOs.
- [x] kairos-core `children_progress_counts` unit tests; db `children_progress_rollups` integration test (multi-board children, whole-board batch, zero-done-columns); full ladder green (unit, integration ×30, e2e ×5 incl. new progress.spec — no retries).

## Resolved Questions (v1 decisions)

- **Backfill**: strictly manual opt-in — existing boards get is_done=false everywhere; only FRESH boards seed terminal flags. (Auto-applying the heuristic risks blessing misconfigured dead ends.)
- **Depth**: one level (strategy summarizes initiatives, initiative summarizes tasks — Linear's ratio precedent).
- **Graph explorer**: not in v1; detail + boards only.
- **Structured transition capture**: deferred — record as the first task of the burnup v2 follow-up so history starts accruing when that work begins.
- **Badge real estate**: fits the post-T-0075/T-0076 card as a footer row (code row → title → pills → progress).

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
- 2026-08-24: Implementation complete (option B); unit/build/lint green, integration running:
  - Schema: migration `2026-08-18-000001_column_is_done` (guarded ADD COLUMN, default false = strictly manual opt-in for existing boards — decision on the backfill question); `seeded_done_column` flags Completed (workflow levels) + Decided/Superseded (ADR) on FRESH boards only; models + changeset; admin-added columns start un-done.
  - kairos-db graph.rs: `children_progress(parent)` (one grouped query over a live-workflow-items union — soft-deleted excluded, docs structurally absent, off-board ADRs excluded, parent edges only) and `board_children_progress(board)` (ONE query for every parent on a board — the no-N+1 shape). Both carry `board_has_done` so clients can distinguish 0-done from no-done-semantics.
  - kairos-core: pure `children_progress_counts` + tests (incl. zero-done-columns → composition only).
  - API: NEW `GET /api/{family}/{code}/children-progress` (404-consistent with relationships; has_done_columns in the response); board-items response gains `children_progress` map keyed by parent short code; `PATCH .../columns/{id}` accepts `is_done` via new `boards::set_column_done` (board_config activity row `column_done:{name}={bool}`).
  - Clients: kairos-client `children_progress()` + DTOs; BoardColumn.is_done everywhere.
  - Web: admin ColumnsPanel gains Mark/Unmark done + a done pill + a gold "dead end" HEURISTIC pill (suggest-only, from the transitions — never auto-set); parent cards get the N-of-M micro-badge (composition-only "N children" when has_done=false); item detail gets the segmented ChildrenProgressBar under the header (enhancement data — failed reads render nothing). Both surfaces ride the existing refetch flows.
  - Decisions on remaining open questions: depth-1 rollup only; graph explorer NOT in v1; structured transition capture NOT included (v2 foundation deferred — recorded for the burnup follow-up); badge fits the T-0075/T-0076 card layout (code row → title → pills → progress footer).
  - Tests: db graph.rs `children_progress_rollups` (multi-board children, seeded done flag, soft-delete/supports exclusion, whole-board batch, zero-done-columns); server meta.rs endpoint arc (rollup → walk child to Completed → rollup follows; leaf empty; 404 mismatch); NEW e2e progress.spec.ts (card badge 1/5, detail bar 1-of-5 with one done segment, childless item renders nothing).
- 2026-08-24 (final): Full ladder green — unit, integration (30 targets), e2e (API + MCP + 5 Playwright specs, no retries). Integration iterations: re-pinned tenant_provisioning's upgrade-path simulation to the new newest migration (recurring maintenance point — that test must be updated with every tenant migration; consider generalizing it in a future tech-debt ticket); meta.rs walk uses the org-admin client (transition_items isn't among alice's fixture grants); the walk's 3 transition rows folded into the entity_id activity assertion. UAT stack restored on the new binary (reseeded fixture; Portal sign-up flow shows 1/5 done).