---
id: boards-planned-support-swim-lanes
level: task
title: "Boards: Planned/Support swim lanes via work_class field + support ticket type"
short_code: "KAIROS-T-0077"
created_at: 2026-08-16T14:56:15.466841+00:00
updated_at: 2026-08-16T14:56:15.466841+00:00
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

# Boards: Planned/Support swim lanes via work_class field + support ticket type

## Objective

Separate support/unplanned work from planned work on delivery boards: a horizontal **Support** lane above **Planned**, driven by a new per-ticket `work_class` field, plus a new `support` task type. Two orthogonal axes: **the lane answers "was this work planned?", the type answers "what kind of work is it."**

## Recorded Product Decision — no bug-remediation lane

There is **NO** dedicated bug-remediation lane. `bug` stays a ticket type; a bug goes in the **Planned** lane if we knew about the work in advance, the **Support** lane if it arrived as unplanned intake. Deriving the lane from the type would collapse the two axes (an unplanned bug couldn't be in the Support lane while staying `task_type=bug`) — this is why the lane needs its own field.

## Backlog Item Details

### Type
Feature

### Priority
P1 — UAT feedback; core team-workflow capability.

### Business Justification
- **User Value**: Teams can see how much in-flight work is interrupt-driven, give it expedite treatment, and protect planned throughput. Support tickets get a real type instead of being misfiled as `task`/`bug`.
- **Effort Estimate**: M — one additive migration + mechanical enum/DTO threading; the real work is the web board's lane rendering and (column, lane) drag targets.

## Current State

- **No lane concept exists** anywhere (grep for lane/swim across db/core/server/web: zero hits). Boards are level-typed containers (`crates/kairos-db/src/models/boards.rs:23–33`) of position-ordered `board_columns` with explicit `board_transitions` edges. A board view is one flat row of columns (`crates/kairos-web/src/pages/boards.rs:719–805`).
- **Rules engine is purely column-scoped**: `can_transition` (crates/kairos-core/src/board.rs:95–115) takes no lane input; item state IS its column (`board_id`/`column_id` NOT NULL on tasks, models/items.rs:152–153).
- **Ticket types are a closed 3-value set**: `CHECK (task_type IN ('task','bug','tech_debt'))` (tenant up.sql:214), enum `TaskType {Task, Bug, TechDebt}` (models/enums.rs:132–139). Only tasks carry a type. `task_type` is already mutable (items.rs:187) and travels as a plain String through client DTOs (kairos-client/src/types.rs:102), search filters (api/search.rs:172–178), and MCP tools (mcp/tools.rs:1791–1804, 1926–1932).
- **Board items query** groups by column only (api/org/boards.rs:451–539). **DnD** drop targets are column ids validated against transition edges (web boards.rs:743–773). **WS**: thin events + wholesale refetch (data.rs:155–169) — lane changes need no WS work since `item_updated` already exists.

## What Other Tools Do (survey summary)

- **Kanban canon (Businessmap/Kanbanize)**: classes of service as horizontal swimlanes — Expedite on top, per-lane policies/WIP. One board, one WIP budget, CoS mix as a capacity signal.
- **Jira**: swimlanes from JQL queries — zero-ceremony but lane membership is invisible on the issue and misconfigured queries silently hide work (documented failure mode).
- **Jira Service Management**: separate request-type/queue intake surface — clean intake, but two systems and support load invisible to the dev board.
- **Linear**: no lanes — a per-team **Triage inbox**; unplanned work never enters WIP without an accept/decline decision. Strong pattern, but doesn't give in-flight planned/support visibility.
- **Azure DevOps / GitHub Projects**: the pattern that fits Kairos best — **the lane is a projection of a card field** (ADO swimlane rules auto-route; GitHub groups by a single-select field), and dragging between lanes writes the field back. Policy lives on the item, auditable, can't silently drop cards.

## Options Considered

**A. Field-derived lanes (RECOMMENDED)** — `work_class TEXT NOT NULL DEFAULT 'planned' CHECK (work_class IN ('planned','support'))` on tasks + `support` added to task_type. Lanes render by partitioning each column's cards on work_class; lane drag writes the field. *Pros*: honors the two-axis model exactly; lane membership lives on the card (auditable, visible everywhere); minimal blast radius (no new tables, no rules-engine or WS changes); extends naturally toward full classes of service (expedite, fixed_date). *Cons*: lane partitioning happens client-side per renderer; the web board DOM restructure is the meatiest change.

**B. First-class `board_lanes` table + lane_id FKs** — full lane CRUD/config like columns. *Rejected for v1*: 5–10x the surface for a binary distinction; splits source of truth (planned-ness becomes board structure, not a ticket fact); per-board lane sets invite semantic drift. It's the migration path if per-lane WIP/config is ever needed.

**C. Type-only (derive lane from task_type=support)** — *Rejected*: directly violates the recorded decision — an unplanned bug can't appear in the Support lane while keeping its type; lane drags would rewrite the type.

**D. Linear-style triage inbox, no lanes** — *Rejected for v1*: an entirely new surface, and once accepted, support work is again indistinguishable in-flight. Natural **v2 intake layer** on top of work_class.

## Recommendation (v1 sketch)

- **Schema**: one tenant migration — add `tasks.work_class` (default 'planned', backfill existing rows); extend the task_type CHECK with 'support'. New `WorkClass` text_enum + `Support` variant in `TaskType` (models/enums.rs).
- **Semantics**: work_class and task_type independently settable. Creating `task_type=support` defaults `work_class=support` (overridable) — support tickets are born in the Support lane, no new intake surface.
- **Rules engine**: untouched — lane never affects transition legality. Lane change = TaskChangeset update, activity-logged, emits existing `item_updated` thin event.
- **API**: work_class in task create/update DTOs, board-items card payload, and a search filter alongside task_type. Board-items response keeps column-only grouping; clients partition.
- **GUI**: delivery boards render **Support above Planned** (expedite convention), lanes spanning the same columns; distinct accent header + per-lane counts; empty Support lane renders slim but droppable. Drag semantics: same-column cross-lane = work_class update only; cross-column same-lane = transition as today; diagonal = both. Strategy/initiative/ADR boards unchanged (tasks are the only typed entity; family per level is fixed 1:1, web data.rs:211–219).
- **CLI/MCP**: `boards show` groups or badges by lane; work_class in create/update/filter tools and MCP board view.

## Acceptance Criteria

- [ ] Tenant migration adds `tasks.work_class` TEXT NOT NULL DEFAULT 'planned' CHECK ('planned'|'support'); existing tasks backfill to 'planned'.
- [ ] `task_type` accepts 'support' end-to-end: DDL, enum, API parsing, search filter, CLI, MCP.
- [ ] work_class and task_type independently settable: a `task_type='bug'` + `work_class='support'` task renders in the Support lane with its bug type intact (regression test for the no-bug-lane decision).
- [ ] Delivery board web view renders Support above Planned spanning the same columns; lane membership determined solely by work_class; other board levels render unchanged.
- [ ] Same-column cross-lane drag updates only work_class (no transition validation); cross-column same-lane drag validates transitions exactly as today; diagonal applies both; the keyboard-accessible transition path offers lane moves too.
- [ ] `can_transition` and the rules engine unchanged — lane never affects transition legality (unit-tested).
- [ ] Creating task_type='support' defaults work_class='support' (overridable) via web, API, CLI, MCP.
- [ ] work_class changes are activity-logged and emit the existing `item_updated` thin event; open boards live-refresh with no WS protocol changes.
- [ ] Empty Support lane renders slim but remains a valid drop target; lane headers show per-lane counts; Support lane has distinct expedite-style accent.
- [ ] `GET /api/boards/{id}/items` includes work_class on task cards; kairos-client, web, CLI `boards show`, and MCP board view expose it.
- [ ] Search API filters tasks by work_class alongside task_type.

## Open Questions

- Capability gating for lane drags: require `transition_items` (UX consistency with column drags) or plain task-update permission?
- Field naming: `work_class` (extensible toward expedite/fixed_date) vs narrower alternatives — recommend work_class, confirm.
- Server-side lane grouping in the board-items API eventually, or per-client partitioning long-term?
- Support-demand roll-up to initiative/strategy levels (Businessmap-style CoS-mix reporting) — v2?
- Per-lane/column WIP limits — separate initiative? (No WIP machinery exists anywhere today.)
- Triage inbox as the eventual intake surface feeding work_class='support' — v2 candidate.
- Reconcile initiatives' existing `bucket_type` ('tech_debt'|'bug'|'ad_hoc', enums.rs:141–159) with the work_class axis, or leave alone?

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
