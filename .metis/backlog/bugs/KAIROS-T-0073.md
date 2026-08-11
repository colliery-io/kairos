---
id: e2e-smoke-move-menu-click-flakes
level: task
title: "E2E: smoke move-menu click flakes when a WS refetch re-renders the board"
short_code: "KAIROS-T-0073"
created_at: 2026-08-11T03:22:18.242669+00:00
updated_at: 2026-08-11T03:23:39.271336+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/active"


exit_criteria_met: false
initiative_id: NULL
---

# E2E: smoke move-menu click flakes when a WS refetch re-renders the board

## Objective **[REQUIRED]**

Deflake the smoke spec's "transition via the move menu" step. It failed on the first attempt in 3 of the last 4 e2e runs (always recovered on the policy retry), consistently with the same signature: the menu-item click times out with "element is not stable / element was detached from the DOM".

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P2 - Medium (nice to have) — the retry policy absorbs it, but a 75% first-attempt failure rate adds ~15s to every e2e run and normalizes red ✘ lines in the output.

### Impact Assessment **[CONDITIONAL: Bug]**
- **Affected Users**: CI/e2e only (plus a real-UX cousin, below)
- **Reproduction Steps**: 
  1. `angreal test e2e` — smoke step 4 creates a task (its `on_changed` refetch AND its `/ws/events` echo each schedule a board refetch)
  2. Step 5 opens the card's Move menu and clicks a target column
  3. If a refetch lands between opening the menu and clicking the item, `BoardBody` rebuilds wholesale — the open menu's DOM is destroyed mid-click
- **Expected vs Actual**: Expected — the two clicks land on one stable menu. Actual — Playwright retries the item click against a detached node forever; the freshly rebuilt card renders with the menu CLOSED, so the click can never succeed within the step.

**Root cause is architectural, not test-side**: the board view rebuilds the entire DOM on every refetch (`LocalResource` → the `Some(Ok(view))` match arm re-runs). Any transient UI state — an open menu — dies with it. The same mechanism means a real user's open Move menu snaps shut whenever any WS event lands on the board. Fine-grained keyed rendering (`<For>` over columns/cards) would fix both, but is a larger change than a deflake warrants.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] The open-menu-then-click sequence in smoke step 5 is atomic-with-retry: a mid-sequence board rebuild causes the WHOLE sequence to retry (expect-polling, per the suite's no-arbitrary-sleeps rule), not a doomed click against a detached node
- [ ] A genuine regression (menu never works) still fails the step — the pattern must converge only when a transition actually succeeds
- [ ] `angreal test e2e` passes with the smoke spec green on the FIRST attempt
- [ ] The real-UX cousin (open menus close on WS refetch; fine-grained board rendering) is noted here as a candidate follow-up, not silently absorbed into this deflake

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
Wrap the menu open + item click in Playwright's `expect(async () => {...}).toPass()` with short per-click timeouts — each retry re-resolves the card fresh, so a rebuild between attempts just means the next attempt starts from a closed menu and opens it again. Success requires the card to actually appear in the target column (already asserted after the clicks).

### Risk Considerations
`toPass` can mask a deterministic first-click failure ONLY if a later identical attempt succeeds — which is the definition of the race being deflaked, not a regression being hidden.

## Status Updates **[REQUIRED]**

- 2026-08-10: Filed from the recurring flake (3 of last 4 runs); fix authorized by Dylan in the same breath.