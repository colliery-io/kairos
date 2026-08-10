---
id: gui-task-creation-should-be-global
level: task
title: "GUI: task creation should be global and always land in Backlog, not per-column"
short_code: "KAIROS-T-0062"
created_at: 2026-08-09T17:44:51.452543+00:00
updated_at: 2026-08-09T17:44:51.452543+00:00
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

# GUI: task creation should be global and always land in Backlog, not per-column

## Objective **[REQUIRED]**

Move item creation off the kanban columns: creating a task should be a global action (board-level, not column-level), and a newly created task should ALWAYS land in the Backlog column of its delivery board — never directly in an arbitrary column.

UAT feedback (Dylan, 2026-08-09).

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [ ] P1 - High (important for user experience)

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: Per-column composers invite creating work directly into mid-flow columns, bypassing intake. Backlog-first creation matches the Flight Levels intake model the product itself preaches (delivery default columns: Backlog → Todo → Blocked/Active → Completed, seeded in `crates/kairos-db/src/tenant.rs` system_board_defaults).
- **Effort Estimate**: S/M

## Acceptance Criteria **[REQUIRED]**

- [ ] The per-column composer (`ComposerCard` in `crates/kairos-web/src/pages/boards.rs`) is removed from column rendering
- [ ] A single, always-visible "new item" action exists on the board view (and/or the shell)
- [ ] A newly created task lands in the board's Backlog (entry) column regardless of where creation was initiated
- [ ] Team inheritance is preserved: tasks created on a delivery board still inherit the board's `team_id`
- [ ] Non-delivery boards (strategy/initiative/ADR) get the same treatment: creation lands in the configured entry column (Draft/Discovery)
- [ ] Playwright smoke's create-item step updated; `angreal web lint` + A-0012 gates green

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
The composer currently renders per column and carries `team_id` (boards.rs ~L395-545). Whether the server permits creating directly into a non-entry column is irrelevant once the GUI always targets the entry column; check whether the create API takes a column at all — if it does, the GUI should simply stop passing one / pass the entry column.

### Dependencies
Interacts with KAIROS-T-0063 (boards-view level layout) — coordinate if both land together.

## Status Updates **[REQUIRED]**

- 2026-08-09: Recorded from UAT feedback session.