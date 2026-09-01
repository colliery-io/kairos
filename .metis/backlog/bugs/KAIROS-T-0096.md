---
id: mcp-set-metadata-bypasses-entity
level: task
title: "MCP set_metadata bypasses entity-type scoping (T-0078 enforced on HTTP only)"
short_code: "KAIROS-T-0096"
created_at: 2026-09-01T12:19:14.925414+00:00
updated_at: 2026-09-01T12:19:14.925414+00:00
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

# MCP set_metadata bypasses entity-type scoping (T-0078 enforced on HTTP only)

## Objective

Make MCP `set_metadata` enforce the KAIROS-T-0078 entity-type scoping the HTTP path already enforces. Today an agent can stamp a documents-only definition (e.g. `document_type`) onto a task.

## Backlog Item Details

### Type
Bug

### Priority
P2 — silently violates a shipped T-0078 acceptance criterion; agents are a first-class writer in this product (A-0011), so the MCP path is not a side door.

### Impact Assessment
- **Affected Users**: anyone driving Kairos through Claude/MCP (the primary agent workflow).
- **Reproduction Steps**:
  1. Through MCP, call `set_metadata` on a TASK short code with `{"document_type": "prd"}`.
  2. It succeeds — the row lands in `item_metadata`.
  3. The GUI refuses the same write (422) and the task detail never offers the field.
- **Expected vs Actual**: expected a 422 naming the scope violation (the `api/meta/metadata.rs` behaviour); actual is a silent successful write.

## Implementation Notes

- The HTTP path validates in two steps: `validate_metadata_value` AND `definition_applies_to` (`crates/kairos-server/src/api/meta/metadata.rs`, phase 1). MCP `set_metadata` (`crates/kairos-server/src/mcp/tools.rs`, phase 1 loop) calls only `validate_metadata_value` — the `definition_applies_to` call is simply absent.
- The item's `ItemType` is already resolved in the MCP handler (short-code resolution), so the fix is one guard inside the existing loop, not new plumbing. `kairos_db::items::definition_applies_to` is the shared predicate.
- Prefer collapsing the duplication over patching twice: the two phase-1 loops are near-identical. Extract a shared validated-ops builder both paths call, so the next scoping rule cannot land on one path only.
- Add MCP-tier coverage (`crates/kairos-server/tests/mcp.rs`) asserting the rejection; the HTTP tier already covers it in `meta.rs`.

## Acceptance Criteria

- [ ] MCP `set_metadata` rejects out-of-scope definitions with the same typed 422 as the HTTP path.
- [ ] Clearing a value (null) on an out-of-scope definition stays allowed on both paths (the HTTP path deliberately permits clears).
- [ ] The validation logic is shared between the HTTP and MCP paths, not duplicated.
- [ ] `mcp.rs` covers the rejection; ladder green.

## Status Updates

- 2026-09-01: Found while surveying integration points ahead of [[KAIROS-I-0009]] (git forge integration). Verified by reading the MCP phase-1 loop directly: `validate_metadata_value` is called, `definition_applies_to` is not. Unrelated to that initiative — filed standalone because it breaks a shipped T-0078 guarantee on its own.
