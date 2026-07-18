---
id: whoami-expose-board-scoped
level: task
title: "whoami: expose board-scoped capability grants"
short_code: "KAIROS-T-0052"
created_at: 2026-07-15T12:37:15.229350+00:00
updated_at: 2026-07-16T20:49:59.652512+00:00
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

# whoami: expose board-scoped capability grants

## Objective

Extend `/api/whoami` (and the MCP `whoami` tool per S-0006, whose spec already lists "boards where the user holds capabilities (with the capability list)") to include the caller's board-scoped capability grants. Found by T-0043: the GUI gates admin surfaces on org role only, so a non-admin holding `configure_boards`/`manage_members` on specific boards cannot see per-board admin UI.

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

- [x] `/api/whoami` gains `capabilities: [{board_id, board_slug, grants: [..]}]` via one indexed join (board_member_capabilities→boards, filter user_id) in the handler; whoami DTO mirrored in kairos-client `types_org.rs`; utoipa whoami doc-stub prose updated (no schema-body ref, so ApiDoc completeness stays green).
- [x] MCP `whoami` tool — verified ALREADY S-0006-conformant (lists board capabilities grouped by board with the capability list); no change needed. The gap was REST-only.
- [x] GUI admin gating upgraded (`pages/admin/gating.rs` + `admin.rs`/`admin/boards.rs`): per-board config visible to holders of the relevant capability (`role == admin` OR holds a board-config grant); org-admin-only surfaces unchanged. kairos-web glob-match is local (no client/core dep in wasm).
- [x] Integration test (`org_endpoints.rs`): capability-granted non-admin (bob) is admitted to board config; a grant to bob does NOT leak into org-admin alice's explicit capabilities list; plain member gated out. Green in the merged gate.

## Context

S-0006's whoami spec row promises the capability list; the REST whoami (T-0017's probe, extended since) never included it. This closes both surfaces consistently.

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

### 2026-07-15 — active
Investigation complete. Findings:
- REST `/api/whoami` (`crates/kairos-server/src/app.rs`) returns user/org/teams only — no capabilities. This is the gap.
- MCP `whoami` tool (`crates/kairos-server/src/mcp/tools.rs`) ALREADY lists board capabilities grouped by board with the capability list — S-0006 conformant already. Verified, no code change needed.
- `kairos-db/src/abac.rs` has `check_capability` (single indexed EXISTS query) but no "list my grants" query; whoami handler is diesel_async, abac.rs is sync — added an inline indexed load query in the handler (join board_member_capabilities→boards, filter user_id).
- Three parallel whoami shapes kept in sync: server `app.rs` struct (wire), `kairos-client/src/types_org.rs` DTO (mirror), `kairos-web/src/api.rs` Whoami mirror (GUI). kairos-web has NO kairos-core/client dep (wasm), so glob-match helper is local.
- openapi.rs whoami is a prose doc-stub (no `body =` schema ref) — updated prose only, within the distinct `whoami()` anchor (coordinating around T-0051's openapi edits).

Decision (org admins): `capabilities` returns the caller's EXPLICIT `board_member_capabilities` grants only. Org admins hold implicit full access via `organization.role == "admin"` and typically appear with `capabilities: []`; the field's purpose is per-board grants for NON-admins. GUI gate = `role == admin` OR holds a relevant board-config capability.

### 2026-07-16 — Complete (orchestrator finished after a computer reset killed the agent mid-verification)
The agent's code was fully landed; it was killed just before running its integration test. Orchestrator ran the merged gate over the T-0051+T-0052 tree: applied the agent's pending `cargo fmt` (one method-chain in org_endpoints.rs), then `cargo fmt --check` clean · `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal web lint` clean · `cargo test -p kairos-server --test org_endpoints` 1/1 green (the capability-gating test) · openapi completeness 5/5 · full `angreal test integration` green (21 targets). All four ACs met. Committed with T-0051.