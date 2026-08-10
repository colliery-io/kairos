---
id: default-status-metadata-definition
level: task
title: "Default 'Status' metadata definition duplicates board position — remove or rework"
short_code: "KAIROS-T-0066"
created_at: 2026-08-09T17:44:56.038509+00:00
updated_at: 2026-08-09T17:44:56.038509+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---

# Default 'Status' metadata definition duplicates board position — remove or rework

## Objective **[REQUIRED]**

Remove (or fundamentally rework) the default 'Status' metadata definition. An item's status IS its board column — that's the entire kanban model. A parallel free-floating "Status" enum (draft / review / approved) stamped as metadata contradicts the board position and confuses every item detail view.

UAT feedback (Dylan, 2026-08-09): "task status isn't metadata; status is where it is in the kanban flow."

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (important for user experience)

### Impact Assessment **[CONDITIONAL: Bug]**
- **Affected Users**: All tenants — 'Status' is seeded into `public.system_metadata_definitions` by the tenant baseline (`crates/kairos-db/src/tenant.rs`, KAIROS-A-0003 defaults: Priority, Status, Complexity, Document Type; Status enum = draft/review/approved) and copied into every provisioned tenant
- **Reproduction Steps**: 
  1. Open any item in the demo tenant
  2. Metadata panel offers a "Status" enum independent of the item's column
- **Expected vs Actual**: Expected — one source of truth for status: the board column. Actual — two: column position and a disconnected metadata enum that nothing keeps in sync.

## Acceptance Criteria **[REQUIRED]**

- [ ] 'Status' is removed from the system default metadata definitions (new tenants no longer get it), OR a recorded decision narrows it to a surface where it isn't redundant (none is currently known — document types have their own board flow via the ADR/document model)
- [ ] Migration story decided for existing tenants: drop the definition + stamped values, or leave existing tenants untouched (defaults are copy-on-provision) — decision recorded in this ticket
- [ ] Seed-demo fixture and any tests referencing the 'Status' definition updated
- [ ] Item detail views no longer offer a Status metadata editor
- [ ] A-0012 gates green (this touches tenant provisioning defaults — integration tier matters)

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
Unlike the GUI-only tickets, this one reaches the data layer: the definition lives in the tenant-baseline SQL (`system_metadata_definitions` + `system_metadata_enum_options` in tenant.rs) and is copied per-tenant at provision time. Removing it for new tenants is a migration + baseline edit; existing-tenant cleanup is a separate tenant migration if we want it.

### Dependencies
Related: KAIROS-T-0065 (metadata panel scoping — same panel; one design decision should settle both). If T-0065's scoping lands first, Status stops rendering on tasks, but the definition itself remains wrong and should still be removed.

## Status Updates **[REQUIRED]**

- 2026-08-09: Recorded from UAT feedback session.