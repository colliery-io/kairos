---
id: gui-item-metadata-panel-renders
level: task
title: "GUI: item metadata panel renders inapplicable definitions (document_type on tasks)"
short_code: "KAIROS-T-0065"
created_at: 2026-08-09T17:44:54.980513+00:00
updated_at: 2026-08-10T16:33:06.849316+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# GUI: item metadata panel renders inapplicable definitions (document_type on tasks)

## Objective **[REQUIRED]**

The item-detail metadata panel should only show fields that make sense for the item being viewed. Today it renders an editor for EVERY tenant metadata definition on every item — so a task shows a "Document Type" enum (prd / system_context / architecture / …), which is meaningless for a task.

UAT feedback (Dylan, 2026-08-09).

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (important for user experience)

### Impact Assessment **[CONDITIONAL: Bug]**
- **Affected Users**: Everyone who opens any item detail view
- **Reproduction Steps**: 
  1. Log into the demo tenant, open any task (e.g. DEMO-T-0001)
  2. Look at the metadata panel
- **Expected vs Actual**: Expected — only fields applicable to a task (e.g. Priority, Complexity). Actual — a "Document Type" editor appears because the panel renders every tenant metadata definition (`crates/kairos-web/src/pages/item/metadata.rs`, per KAIROS-T-0041: "every tenant metadata definition renders with an editor").

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] A decision is recorded on the scoping mechanism: per KAIROS-A-0003, templates declare metadata fields — the panel should show stamped/declared fields for the item (plus possibly an explicit "add metadata" affordance), not the full definition catalog by default
- [x] A task's detail view no longer shows "Document Type"
- [x] Document-typed items still show their document_type (stamped from the template)
- [x] Existing stamped values are unaffected — this is display/editing scoping, not data migration
- [x] A-0012 gates green (full chain 2026-08-10; smoke asserts the scoped panel)

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
`item/metadata.rs` fetches definitions + values together and renders all definitions. Likely fix: render editors for values the item actually has (stamped at creation from template defaults) and gate adding new fields behind an explicit picker. May need a small design note on whether definitions get an applicability scope (family/level) server-side — coordinate with KAIROS-T-0066 before choosing.

### Dependencies
Related: KAIROS-T-0066 (the 'Status' default definition — same panel, same root modeling question). Resolve both with one scoping decision.

## Status Updates **[REQUIRED]**

- 2026-08-09: Recorded from UAT feedback session.
- 2026-08-10: Implemented. Scoping decision (per the AC): the panel renders editors ONLY for fields the item actually carries (stamped values — template-declared per A-0003, or explicitly added); the full definition catalog no longer renders wholesale. `item/metadata.rs` MetadataForm partitions definitions into stamped (editor rows, unchanged behavior) vs unstamped (behind an "(add a field…)" Select that promotes the chosen definition to an empty editor row; the value reaches the server on the normal save). No server-side applicability scope needed — template declaration + panel scoping suffice (coordinated with KAIROS-T-0066, which keeps `status` as a document-workflow field). Save semantics, null-clears, and last-write-wins untouched. Smoke spec's item-detail step now asserts a task shows neither "Document Type" nor "Document status" editors and offers the add picker. `cargo check` clean; full gates running.