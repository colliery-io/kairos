---
id: mcp-set-metadata-bypasses-entity
level: task
title: "MCP set_metadata bypasses entity-type scoping (T-0078 enforced on HTTP only)"
short_code: "KAIROS-T-0096"
created_at: 2026-09-01T12:19:14.925414+00:00
updated_at: 2026-09-25T01:48:59.185222+00:00
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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] MCP `set_metadata` rejects out-of-scope definitions with the same typed 422 as the HTTP path.
- [x] Clearing a value (null) on an out-of-scope definition stays allowed on both paths (the HTTP path deliberately permits clears).
- [x] The validation logic is shared between the HTTP and MCP paths, not duplicated.
- [x] `mcp.rs` covers the rejection; ladder green.

## Status Updates

- 2026-09-01: Found while surveying integration points ahead of [[KAIROS-I-0009]] (git forge integration). Verified by reading the MCP phase-1 loop directly: `validate_metadata_value` is called, `definition_applies_to` is not. Unrelated to that initiative — filed standalone because it breaks a shipped T-0078 guarantee on its own.
### 2026-09-25 — one function now, rather than two copies and a guard on one

Fixed by deleting the duplication rather than by adding the missing guard to the
second copy, which is what the ticket asked for and is the only version of this
fix that holds.

`validated_metadata_ops` in `api/meta/mod.rs` is phase 1 for both writers: it
resolves each definition, applies the [[KAIROS-T-0078]] entity-type scoping, runs
`validate_metadata_value`, and returns the ops list. The REST handler and the MCP
tool each call it and keep their own phase 2. Both call sites lost about thirty
lines.

The two phase-1 loops were near-identical copies, and the T-0078 guard had landed
on the REST one only. Patching MCP to match would have left the next rule free to
do the same thing again — and there is no reason to think T-0078 was special. The
duplication was the defect; the missing guard was a symptom of it.

Worth being precise about why this mattered rather than treating it as tidiness:
agents are a first-class writer in this product ([[KAIROS-A-0011]]), so MCP is not
a side door where rules can be looser. The observable result was that the GUI
refused `document_type` on a task with a 422 and never offered the field, while an
agent could write exactly that row and nothing complained.

### Clears stay allowed, on both paths

The guard is `value.is_some() && !applies_to(...)`, so setting is refused and
**clearing is not** — the REST path always behaved that way and now MCP does too.
That asymmetry is deliberate and now has the reason written next to it: a clear is
how an item sheds a value some earlier version let it acquire, and refusing the
cleanup would strand exactly the rows the guard exists to prevent.

### Coverage

`mcp.rs` asserts the rejection names both the definition and the entity type,
that an in-scope definition on the same item still works (so the guard is scoping
rather than refusing metadata over MCP wholesale), and that a clear of the
out-of-scope definition is accepted.

The scope fixtures come from the system defaults, checked rather than assumed:
`document_type` is scoped to `document` alone, `priority` is unscoped, and
`complexity` covers adr/document/strategy/task.

**Verified by disabling the guard**, which fails the MCP test. A scoping test that
has never been seen to fail is worth very little — and on this ticket it would
have been especially easy to write one that passed for the wrong reason, since
before the fix the write *succeeded* rather than erroring.

### Gates

lint clean, **397 unit tests**, integration **47/47 exit 0** (the REST `meta` tier
and the MCP tier both exercise the shared function).