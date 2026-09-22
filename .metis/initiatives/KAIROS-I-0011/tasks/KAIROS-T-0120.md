---
id: uat-j3-agent-loop-agent-bootstraps
level: task
title: "UAT J3 agent-loop: agent bootstraps over MCP, picks the repo's ticket, PR opened → merged via webhook, Done; standalone under --server"
short_code: "KAIROS-T-0120"
created_at: 2026-09-22T11:15:25.449796+00:00
updated_at: 2026-09-22T11:15:25.449796+00:00
parent: KAIROS-I-0011
blocked_by: ["KAIROS-T-0117", "KAIROS-T-0118"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# UAT J3 agent-loop: agent bootstraps over MCP, picks the repo's ticket, PR opened → merged via webhook, Done; standalone under --server

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Implement journey J3 (I-0011 D4) — "An agent picks up a ticket in its repository and lands a PR" — as `uat/journeys/agent-loop.journey.ts`, driving the agent persona through exactly the MCP/CLI calls the plugin's `implement` skill documents.

## Implementation Notes

### Technical Approach

- Setup: call J1's `setupTeamRepoAgent(ledger, run)` (T-0118) in `beforeAll` — same code path in both modes, so J3 never depends on J1 having run. alice creates the forge connection for the repo (`surfaces/forge.ts`) and ledgers it.
1. agent (MCP) `whoami` → `list_repositories` → `get_repository <slug>`: description present, no in-flight PRs.
2. bob (GUI) creates a task on the team board bound to the repo (the create form's repository picker) and drags it Backlog → Todo → observe the short code.
3. agent (MCP) `board_items` with `repository=<slug>` finds exactly that task; `get_item` shows `repository: <slug> (owner: uat-<run>-mobile)`; `transition` to Active.
4. agent (CLI, logged in with the API key) `kairos tasks get <code>` shows Active; `kairos repos get <slug>` lists it as open work.
5. Signed `pull_request` opened webhook naming the code (`githubPullRequest`) → bob (GUI) sees `#<n>` in the item's Development panel; agent (MCP) `get_repository` lists it in flight.
6. Merged webhook → link state *merged* on both surfaces; agent (MCP) transitions to Done; bob's board shows the card in Done.
7. Teardown: forge connection, task, then J1's fixture objects.

Read `plugin/skills/workflow/implement/SKILL.md` first and mirror its call sequence — the journey is the acceptance test of that documented workflow.

### Dependencies

T-0117, T-0118.

### Risk Considerations

- The agent's board transition needs the `manage_tasks` grant J1 gives; assert `whoami` shows it before step 3 so a grant bug reads as a clear failure.
- Webhook delivery URL names the public URL; deliver to the same path on `UAT_SERVER` (e2e pattern).

## Acceptance Criteria

- [ ] `angreal test uat --journey agent-loop` green under compose and `--server`; report shows the MCP excerpts (repository line, in-flight PR line) and the link state transitions.
- [ ] Journey runs standalone (no `--journey onboarding` in the same invocation) in both modes.
- [ ] Nothing `uat-<run>-` remains after teardown.

## Status Updates

*To be added during implementation*
