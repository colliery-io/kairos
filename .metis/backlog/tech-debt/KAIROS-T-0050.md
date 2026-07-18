---
id: dev-dex-config-register-additional
level: task
title: "Dev Dex config: register additional localhost callback ports"
short_code: "KAIROS-T-0050"
created_at: 2026-07-15T12:23:23.470779+00:00
updated_at: 2026-07-16T03:03:40.439747+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#tech-debt"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Dev Dex config: register additional localhost callback ports

## Objective

Register additional localhost callback ports (or drop redirectURIs entirely) for the `kairos-web` public client in `.angreal/dex/config.yaml`, so multiple locally-run servers can complete PKCE without colliding over the single registered `http://localhost:8080/callback`.

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

- [x] `.angreal/dex/config.yaml` kairos-web client registers the explicit range `http://localhost:{8080..8099}/callback`. Chose the RANGE, NOT drop-redirectURIs: verified in isolated Dex v2.43.1 that a public client with no redirectURIs accepts even a non-loopback host at authorize (open-redirect footgun), so dropping is unsafe even in dev.
- [x] PKCE login verified live from two different ports without CDP interception — full authorize→login→callback→token curl flow issued access tokens from :8083 AND :8093 (in-range); out-of-range :9999 was REJECTED (no code, bounced to Dex root at the approval step), confirming Dex v2.43.1 exact-matches and the range is functional/necessary (reconciles T-0041/T-0044's rejections). Shared :5558 dex restarted (`angreal services reset`) and re-verified: :8095 accepted (authorize 302).
- [x] docs/gui-conventions.md updated: documents the 8080–8099 multi-port PKCE capability, notes the exact-match behavior + range boundary, and retires the CDP Fetch-interception workaround.

## Context

Found during the T-0040..T-0044 concurrent GUI wave (2026-07-15): four agents needed browser-verified PKCE simultaneously; Dex's exact-match on the single registered URI forced per-agent workarounds (CDP Fetch rewrites, second server instances, port queuing). All four recorded the same follow-up. Dev config only — production IdPs are the customer's (A-0016).

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

- 2026-07-15: Active. Read config.yaml (kairos-web public client registers exactly http://localhost:8080/callback) + T-0041/T-0044 status updates. CONFIRMED FINDINGS: both tasks empirically hit Dex exact-match on the single registered redirect URI (:8082 and :8084 → 400 "Unregistered redirect_uri"); both assert (untested) that a public client with NO redirectURIs allows any loopback port. Shared stack dex image = dexidp/dex:v2.43.1. Plan: stand up an ISOLATED throwaway dexidp/dex:v2.43.1 on :5559 with an edited copy of this config, empirically compare (a) no-redirectURIs any-port vs (b) explicit port range, then run a full PKCE authorize→login→callback→token flow via curl from TWO different app ports against :5559 (no CDP). Shared :5558 dex/postgres left untouched (T-0048/T-0049 depend on it).
- 2026-07-16: Completed (orchestrator finished after the investigating agent was killed mid-trace). Config: enumerated `http://localhost:8080/callback`..`8099/callback` for the `kairos-web` client. EMPIRICAL RESULT (isolated dexidp/dex:v2.43.1 on :5559, full curl PKCE, no CDP): in-range :8083 and :8093 → access_token issued; out-of-range :9999 → rejected at the approval step (no code). This overturns the mid-trace worry that "no-redirectURIs = any localhost port" would be the fix — that path is BOTH unsafe (accepts non-loopback hosts) AND unnecessary; the explicit range is the correct, safe, functional fix, and it confirms Dex genuinely exact-matches (matching T-0041/T-0044). Shared dev Dex restarted via `angreal services reset` to load the new config; re-verified :8095 accepted. docs/gui-conventions.md updated. YAML validity confirmed by Dex successfully parsing+serving the config.