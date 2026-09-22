---
id: uat-j1-onboarding-tenant-compose
level: task
title: "UAT J1 onboarding: tenant (compose-only), team + member + repo + service account via CLI, bob and the agent find their team"
short_code: "KAIROS-T-0118"
created_at: 2026-09-22T11:15:19.682088+00:00
updated_at: 2026-09-22T11:15:19.682088+00:00
parent: KAIROS-I-0011
blocked_by: ["KAIROS-T-0117"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# UAT J1 onboarding: tenant (compose-only), team + member + repo + service account via CLI, bob and the agent find their team

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Implement journey J1 (I-0011 D4) — "An organisation is set up and a new engineer finds their team" — as `uat/journeys/onboarding.journey.ts`, plus the reusable `setupTeamRepoAgent(ledger, run)` fixture J3 re-uses under `--server`.

## Implementation Notes

### Technical Approach

Steps, each via `step(persona, …)` with observed values (see D4 J1):
1. `step.composeOnly(alice, "provisions tenant uat-<run>")`: `POST /api/admin/tenants` then `GET` lists it; ledger a tenant delete (`DELETE /api/admin/tenants/{slug}` if it exists — check `api/org/mod.rs`; if there is no delete, record "left for compose teardown" in the report and do not fail).
2. alice (CLI) `kairos teams create` (check the real subcommand/flags in `crates/kairos-cli/src/commands/`) `uat-<run>-mobile` stream-aligned → observe the scaffolded delivery board (`kairos boards get`/API by slug) with default columns + transitions.
3. alice (CLI) adds bob to the team; bob (CLI) `kairos whoami` lists the team and implied board capabilities.
4. alice (CLI) `kairos repos create --name acme/uat-<run>-mobile-app --repo-url … --team uat-<run>-mobile --slug uat-<run>-mobile-app --description "…"` → observe slug/owner.
5. alice (CLI) creates service account `uat-<run>-agent`, grants `manage_tasks` on the team's delivery board, mints a key (captured from stdout once) → `persona.fromApiKey('agent', key)`; export the fixture result `{ teamSlug, boardSlug, repoSlug, agent }`.
6. bob (GUI) logs in: My teams shows the team; `/teams/<slug>` shows the Repositories panel with the repo and description; the board opens and is empty.
7. agent (MCP) `whoami`: team listed, repo under `repositories`, `file_backlog` under `implicit`.
8. Teardown through the ledger: key → service account → repository → team (team delete refuses while it owns a repo; the order is the point — assert the refusal once as an observed value before deleting the repo).

`newhire` persona: under compose bob plays it (step 3/6). Under `--server`, if `UAT_PERSONA_NEWHIRE_EMAIL` is set use that persona for 3/6, else bob (report the substitution).

### Dependencies

T-0117 harness.

### Risk Considerations

- CLI surface for teams/members/service accounts: confirm actual commands; where the CLI lacks one, use the API from alice's `api` surface and say so in the step narration ("(API)").
- Service-account key output format: parse from the CLI's stdout (see `crates/kairos-cli/src/commands/service_accounts.rs`).

## Acceptance Criteria

- [ ] `angreal test uat --journey onboarding` green under compose; report shows all 8 steps with observed board slug/columns, repo slug/owner, agent whoami excerpt.
- [ ] Same journey under `--server` against the kept-running stack: tenant step reported skipped, the rest green, and after the run `kairos repos list` / teams list show no `uat-<run>-` leftovers.
- [ ] `setupTeamRepoAgent` exported and covered by the journey (J3 will import it).

## Status Updates

*To be added during implementation*
