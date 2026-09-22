---
id: uat-j3-agent-loop-agent-bootstraps
level: task
title: "UAT J3 agent-loop: agent bootstraps over MCP, picks the repo's ticket, PR opened → merged via webhook, Done; standalone under --server"
short_code: "KAIROS-T-0120"
created_at: 2026-09-22T11:15:25.449796+00:00
updated_at: 2026-09-22T12:01:35.967423+00:00
parent: KAIROS-I-0011
blocked_by: [KAIROS-T-0117, KAIROS-T-0118]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] Green under compose (hand-run) and `angreal test uat --server … --journey agent-loop,onboarding`; report shows the repository line, the queue, "pull_request 7 [open]", state merged, in-flight back to nothing open.
- [x] Standalone in both modes (setup builds its own repo + agent on the existing team).
- [x] `kairos repos list` / `service-accounts list` show no `uat-` rows after the run.

## Status Updates

**2026-09-22** — Completed in `2c2409e`.

- **Design deviation (forced):** a team whose delivery board has ever held an item cannot be deleted — `count_board_items` counts soft-deleted rows on purpose (T-0010: they would orphan on restore). So J3 cannot run on J1's fresh team and stay self-cleaning; it registers its repository and agent on bob's existing team (`UAT_TEAM`, default `platform`) and removes them afterwards. J1 keeps creating a team because it never raises work on it.
- **Finding:** an agent cannot see PR link state over MCP — `get_item` renders no links; the merge is visible only as the PR leaving `get_repository`'s in-flight list. The journey reads the merged state from `GET /api/tasks/{code}/links`. Candidate follow-up: links on `get_item`.
- Finding: the GUI's New task modal has no repository picker; binding happens on the item page (journey does that).
- Left behind by the first failing run on the kept dev stack: team `uat-mucmeudq-mobile` (undeletable per above; the compose lifecycle reseeds anyway).