---
id: boards-planned-support-swim-lanes
level: task
title: "Boards: Planned/Support swim lanes via work_class field + support ticket type"
short_code: "KAIROS-T-0077"
created_at: 2026-08-16T14:56:15.466841+00:00
updated_at: 2026-08-18T02:58:00.671211+00:00
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

## Acceptance Criteria

- [x] Tenant migration adds `tasks.work_class` TEXT NOT NULL DEFAULT 'planned' CHECK ('planned'|'support'); existing rows backfill via the default; migration is idempotent (fleet-migration guard, verified by tenant_provisioning upgrade-path test).
- [x] `task_type` accepts 'support' end-to-end: DDL, TaskType enum, API parsing, search filter, CLI, MCP (all integration-tested).
- [x] Independence regression-tested at three levels: seeded fixture (DEMO bug in Support lane), API test (bug + work_class=support both preserved), lanes.spec (bug pill visible on a Support-lane card).
- [x] Delivery boards render Support above Planned spanning the same columns (lanes.spec asserts order); lane membership is solely work_class (`card_lane` pure fn); strategy/initiative/ADR boards render the single unlaned row.
- [x] Drop semantics via pure `drop_effect` (host-tested) + lanes.spec: same-column cross-lane = lane write only; cross-column same-lane = plain transition; diagonal = both; the detail page's Lane select + Set lane button is the keyboard path (lanes.spec step 7).
- [x] Rules engine untouched (`can_transition` has no lane input; drop_effect test asserts column legality stays with transitions even on diagonals).
- [x] Create default (support type → Support lane, overridable) server-side — one rule serving web/API/CLI/MCP; API-tested; web modal's "auto" lane omits the field.
- [x] Lane changes: activity_log `work_class:{from}->{to}` + existing item_updated thin event (write_path test: logged once, no-op logs nothing, version/column untouched); boards live-refresh through the existing WS refetch.
- [x] Empty Support lane renders slim (`--empty`, no per-column empty text) but droppable; lane heads show counts; gold expedite accent.
- [x] work_class on board-items task cards; kairos-client Task DTO + set_task_work_class; web mirrors; CLI `boards show` "(support lane)" marker; MCP board view "[support lane]" kind + get_item lane line.
- [x] Search filters by work_class alongside task_type (db test composes both; core validation counts a work_class-only filter as constraining — bug found and fixed by the integration tier, with EmptyWorkClasses validation added).

## Resolved Questions (v1 decisions)

- **Capability**: lane moves require `transition_items` (lane moves are board moves in UX terms; API-tested 403 for a manage_tasks-only user).
- **Naming**: `work_class`, extensible toward expedite/fixed_date.
- **Partitioning**: client-side per renderer for v1; server-side lane grouping deferred.
- **Deferred to future tickets**: CoS-mix roll-up to upper flight levels; per-lane WIP limits; triage inbox as the intake surface; reconciling initiatives' `bucket_type` with the lane axis.

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
- 2026-08-18: Server-side + clients complete (workspace compiles):
  - Migration `2026-08-18-000000_task_work_class` (work_class column + CHECK, task_type CHECK gains 'support'; guarded down); applied to org_demo; schema.rs regenerated (work_class is the LAST column — print-schema ordinal).
  - enums.rs: TaskType::Support, new WorkClass text_enum, ActivityAction::WorkClass (+tests). Models: Task/NewTask/TaskChangeset.
  - kairos-db: create_task threads work_class; NEW `items::set_task_work_class` (transaction: no-op if unchanged, else update + activity `work_class:{from}->{to}` + ItemUpdated thin event). Seed: SeedTask.work_class; 2 new fixture tasks on platform-delivery — "Customer cannot reset password" (support/support, Active) and "Login page 500s on expired trials" (bug/SUPPORT lane, Todo — the no-bug-lane regression example).
  - Search: core SearchWorkClass + filter field; db hydrate_tasks predicate + task-only retain; server parse; client types_search; CLI --work-class; MCP filter.
  - API: create defaults work_class=support iff task_type=support (overridable); NEW POST /api/tasks/{code}/work-class (SetWorkClassRequest, capability transition_items — decision: lane moves are board moves in UX terms); openapi registered; convert.rs Task DTO.
  - Clients: kairos-client Task.work_class + create field + set_task_work_class(); CLI create --work-class + boards show "(support lane)" marker; MCP create/search/get_item/board view expose lanes.
  - DECISIONS on open questions: capability = transition_items; name = work_class; client-side lane partitioning for v1.
  - NEXT: web GUI (lane rows, (column,lane) drops, create-modal lane select, detail lane control), e2e lanes.spec + lane-aware column helpers in existing specs (two column sections per name once lanes land), tests.
- 2026-08-18 (cont.): Web GUI + e2e authored; unit (all crates incl. 2 new pure lane tests), WASM build, lint green:
  - boards.rs: DragData gains source_column + work_class; pure `card_lane` + `drop_effect` helpers (host-tested: same-column cross-lane = lane write only; cross-column same-lane = plain transition; diagonal = both; column legality stays with transitions; unlaned cards can't lane-move); `run_transition` → `run_drop` (sequential transition + lane writes); BoardBody splits delivery boards into LaneSection(Support, gold, top) + LaneSection(Planned); new LaneColumns component (lane-filtered fine-grained column row, empty support columns render slim with no empty-text); ItemCard drags with lane payload and stays draggable in dead-end columns when it can lane-move; create modal gains task_type 'support' + Lane select (auto/planned/support; auto = server default rule).
  - item.rs: MoveControl gains the Lane row (select + Set lane; disabled when unchanged; message-carrying on_moved); TypeFacts shows a gold "support lane" pill; ItemDetail mirror + decode test carry work_class.
  - app.css: lane container/accent (--gold var), --empty slim treatment. data.rs: Task/NewItem/CreateTaskRequest mirrors + set_work_class + test updates.
  - Test-target sweep: work_class threaded through 16 construction sites in db/server test suites + soak + golden-path example.
  - e2e: NEW lanes.spec.ts (lane order, seeded fixtures incl. the no-bug-lane example, create-into-support, cross-lane/same-lane/diagonal drags, detail Lane control); smoke/drag/team-lens column helpers lane-scoped; smoke 7b move-field targeted by label (two selects in the Board panel now).
  - Integration suite running; e2e next.
- 2026-08-18 (final): Full ladder green — unit, integration (30 targets), e2e (API golden path + MCP + 4 Playwright specs incl. the new lanes.spec, no retries).
  - Integration tier caught and fixed 3 real issues: (1) `SearchFilter::is_constraining` didn't count work_class — a work_class-only search 400'd (+ EmptyWorkClasses validation + core unit tests); (2) non-exhaustive validation-error match in the server mapping; (3) tenant_provisioning's upgrade-path simulation was pinned to the previous newest migration — now reverts task_work_class, and the migration itself gained IF-NOT-EXISTS guards (idempotent fleet migrations).
  - New tests: write_path lane arc (activity row, no-op, version/column untouched), search work_class filter + axis composition, entities API lane arc (defaults, independence, transition_items 403, 422 on bad value), seed fixture lane assertions.
  - UAT stack rebooted on the new binary with the reseeded 10-task fixture (support request in Support/Active; unplanned bug in Support/Todo).