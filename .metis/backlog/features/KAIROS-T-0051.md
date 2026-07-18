---
id: pre-delete-cascade-preview-endpoint
level: task
title: "Pre-delete cascade preview endpoint"
short_code: "KAIROS-T-0051"
created_at: 2026-07-15T12:23:30.311293+00:00
updated_at: 2026-07-16T03:25:33.417551+00:00
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

# Pre-delete cascade preview endpoint

## Objective

Add a pre-delete cascade preview so clients can warn users with the AUTHORITATIVE descendant set before a soft-delete, instead of the current post-delete report. Found by T-0041: the GUI's delete confirm can only show direct relationship children pre-delete; the full transitive cascade is known only after the fact (DeleteResponse).

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

{Delete this section when task is assigned to an initiative}

### Type
- [ ] Bug - Production issue that needs fixing
- [ ] Feature - New functionality or enhancement  
- [ ] Tech Debt - Code improvement or refactoring
- [ ] Chore - Maintenance or setup work

### Priority
- [ ] P0 - Critical (blocks users/revenue)
- [ ] P1 - High (important for user experience)
- [ ] P2 - Medium (nice to have)
- [ ] P3 - Low (when time permits)

### Impact Assessment **[CONDITIONAL: Bug]**
- **Affected Users**: {Number/percentage of users affected}
- **Reproduction Steps**: 
  1. {Step 1}
  2. {Step 2}
  3. {Step 3}
- **Expected vs Actual**: {What should happen vs what happens}

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: {Why users need this}
- **Business Value**: {Impact on metrics/revenue}
- **Effort Estimate**: {Rough size - S/M/L/XL}

### Technical Debt Impact **[CONDITIONAL: Tech Debt]**
- **Current Problems**: {What's difficult/slow/buggy now}
- **Benefits of Fixing**: {What improves after refactoring}
- **Risk Assessment**: {Risks of not addressing this}

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Endpoint `GET /api/{entity_type}/{short_code}/cascade-preview` returns the transitive descendant set a soft-delete WOULD remove, without deleting (side-effect-free GET chosen over `DELETE ?dry_run=true`; documented in the S-0005 addendum)
- [x] Reuses `kairos_core::items::cascade_descendants` — the SAME BFS `soft_delete_item` uses (new `kairos_db::items::preview_cascade`; no second traversal)
- [x] utoipa-annotated (`api/cascade.rs`) + registered in ApiDoc paths (route-vs-spec completeness test passes); `KairosClient::cascade_preview` typed method + `CascadePreviewResponse` DTO; S-0005 addendum recorded
- [x] GUI delete confirm (`pages/item/delete.rs`) switched to the authoritative preview; MCP delete_item unchanged (additive preview does not alter delete semantics; MCP is T-0052's lane + currently quarantined) — documented
- [x] Integration test `tests/cascade_preview.rs` proves preview == actual DELETE cascade on a 3-level strategy→initiative→task tree

## Test Cases **[CONDITIONAL: Testing Task]**

{Delete unless this is a testing task}

### Test Case 1: {Test Case Name}
- **Test ID**: TC-001
- **Preconditions**: {What must be true before testing}
- **Steps**: 
  1. {Step 1}
  2. {Step 2}
  3. {Step 3}
- **Expected Results**: {What should happen}
- **Actual Results**: {To be filled during execution}
- **Status**: {Pass/Fail/Blocked}

### Test Case 2: {Test Case Name}
- **Test ID**: TC-002
- **Preconditions**: {What must be true before testing}
- **Steps**: 
  1. {Step 1}
  2. {Step 2}
- **Expected Results**: {What should happen}
- **Actual Results**: {To be filled during execution}
- **Status**: {Pass/Fail/Blocked}

## Documentation Sections **[CONDITIONAL: Documentation Task]**

{Delete unless this is a documentation task}

### User Guide Content
- **Feature Description**: {What this feature does and why it's useful}
- **Prerequisites**: {What users need before using this feature}
- **Step-by-Step Instructions**:
  1. {Step 1 with screenshots/examples}
  2. {Step 2 with screenshots/examples}
  3. {Step 3 with screenshots/examples}

### Troubleshooting Guide
- **Common Issue 1**: {Problem description and solution}
- **Common Issue 2**: {Problem description and solution}
- **Error Messages**: {List of error messages and what they mean}

### API Documentation **[CONDITIONAL: API Documentation]**
- **Endpoint**: {API endpoint description}
- **Parameters**: {Required and optional parameters}
- **Example Request**: {Code example}
- **Example Response**: {Expected response format}

## Implementation Notes **[CONDITIONAL: Technical Task]**

{Keep for technical tasks, delete for non-technical. Technical details, approach, or important considerations}

### Technical Approach
{How this will be implemented}

### Dependencies
{Other tasks or systems this depends on}

### Risk Considerations
{Technical risks and mitigation strategies}

## Status Updates **[REQUIRED]**

### 2026-07-15 — Design decided, implementation starting

**Endpoint shape**: `GET /api/{entity_type}/{short_code}/cascade-preview` — a
single generic handler mirroring the existing `{entity_type}` read endpoints
(`relationships`, `metadata`, `history`). Chosen over `DELETE ?dry_run=true`
because a preview is a side-effect-free READ: GET on a subresource is the
RESTful, idempotent shape and matches every existing generic per-item read.
`dry_run` on a mutating verb muddies the DELETE contract.

**Reuse (single source of truth)**: new `kairos_db::items::preview_cascade`
loads the tenant `parent` edges and calls the SAME
`kairos_core::items::cascade_descendants` BFS that `soft_delete_item` uses —
no second traversal. It reads the LIVE short codes of exactly the descendant
ids (read-only mirror of the `$delete_fn` filter), so
`preview.cascaded_short_codes == soft_delete_item(...).cascaded_short_codes`
by construction.

**Surfaces**:
- kairos-db: `preview_cascade` + `CascadePreview` (items.rs); new `$preview_fn`
  in the `content_table_ops!` macro (live-short-code read mirror of delete).
- kairos-client: `CascadePreviewResponse` DTO in types.rs (sibling of
  `DeleteResponse`); `KairosClient::cascade_preview(kind, short_code)` method.
- kairos-server: new `api/cascade.rs` handler, merged in `api::router()`
  (mod.rs), registered in `openapi.rs` ApiDoc paths(). No app.rs edit (route
  rides the existing entity-families stack) — avoids T-0052 contention.
- kairos-web: `fetch_cascade_preview` + `CascadePreview` mirror (item/api.rs);
  delete.rs confirm switched from direct-children to the authoritative preview.

**MCP delete_item**: owned by T-0052; the preview is additive and does NOT
change delete semantics, so no confirm-story change is required (documented).

**Tests**: integration test proving preview == actual cascade on a 3-level
strategy→initiative→task tree.

### 2026-07-15 — Complete, all ACs met

Implemented and verified. Endpoint shape: `GET /api/{entity_type}/{short_code}/cascade-preview`
(side-effect-free read).

**Verification (all green)**:
- `cargo fmt --check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo test -p kairos-server` — ALL binaries pass, incl. the openapi
  route-vs-spec completeness test (new route registered), entities, meta, and
  the new `cascade_preview` test.
- `tests/cascade_preview.rs::cascade_preview_matches_actual_cascade` passes:
  preview at each level (leaf=0, mid=[T], root=[I,T]) is correct; the tree is
  still LIVE after previewing (read-only proven); and `delete_strategy`'s
  `DeleteResponse.cascaded_short_codes` == the immediately-prior preview's —
  the two agree by construction and in fact. Previewing a deleted item 404s.
- `cargo build -p kairos-client` + `-p kairos-web` build; `angreal web lint` clean.

**GUI**: `pages/item/delete.rs` now warns pre-delete with the authoritative
transitive descendant set (via `fetch_cascade_preview`) instead of only direct
children; the post-delete report still confirms and matches. (No CDP run — the
change is a data-source swap on the confirm dialog, covered by the server test
proving the payload equals the eventual cascade; the component renders the
`cascaded_short_codes` list the same way it already rendered the post-delete
codes.)

**Note on openapi.rs**: the concurrent T-0052 agent also edited the file's
whoami doc-stub description; my cascade path registration is independent and
both coexist cleanly.

Scratch DB dropped by the test; shared services left UP.