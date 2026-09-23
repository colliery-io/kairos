---
id: uat-j3-back-on-a-fresh-team-j1
level: task
title: "UAT: J3 back on a fresh team, J1 move + refused-until-deleted step; README team deletion; I-0011 finding #7 closed"
short_code: "KAIROS-T-0130"
created_at: 2026-09-23T01:50:50.493748+00:00
updated_at: 2026-09-23T02:28:30.310080+00:00
parent: KAIROS-I-0012
blocked_by: [KAIROS-T-0127, KAIROS-T-0128, KAIROS-T-0129]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0012
---

# UAT: J3 back on a fresh team, J1 move + refused-until-deleted step; README team deletion; I-0011 finding #7 closed

## Parent Initiative

[[KAIROS-I-0012]]

## Objective

I-0012 D4 (UAT) + D5: the journeys prove the new team lifecycle, the docs describe it, and the UAT finding is closed.

## Implementation Notes

### Technical Approach

- `uat/fixtures/team.ts`: restore `setupTeamRepoAgent` (fresh team) and switch `agent-loop.journey.ts` back to it; its ledger deletes the task (archive) before the team, so teardown succeeds. Keep `setupRepoAgentOnTeam` for `--server` targets whose seed lacks a spare team? No — a fresh team works on any target now; remove it unless J1 needs it.
- `onboarding.journey.ts`: after the agent step, alice (CLI) `kairos tasks move` an existing platform task? No — don't touch seed data. Instead: alice creates a task on the new team's board (CLI, `--board`), then the team delete is refused naming that live card (observe `details.items`), alice moves it to `platform-delivery` (`kairos tasks move --to-board`), the team delete is refused for the repo (existing step), and the ledger unwinds repo → team successfully. Assert on the platform board (bob GUI, already open) that the moved card arrived in Backlog live.
- `uat/README.md`: note `UAT_TEAM` is gone (or optional). I-0011 initiative findings list: mark #7 fixed by I-0012. `README.md`: under Teams/admin — deleting a team (must have no live cards and own no repositories; `kairos tasks move`, `kairos repos update --team`), the two-sided move rule.
- Runs: `angreal test uat` compose and `--server` against the kept stack; record run ids in I-0012's log.

### Dependencies

T-0127, T-0128, T-0129.

## Acceptance Criteria

- [x] `agent-loop` uses `setupTeamRepoAgent` again; after full runs in both modes `kairos teams list` shows no `uat-` rows (nor repositories, service accounts or items).
- [x] J1 steps 9–12: refused 409 (owns the repository) → re-home → refused 422 naming the card in `details.items` → `kairos tasks move` → bob sees it arrive live → teardown deletes the team.
- [x] README gained "Teams — and winding one down"; I-0011 finding #7 marked fixed by I-0012; compose `mudhg8lk` 5/5, server `mudhgu2v` 5/5.

## Status Updates

**2026-09-22** — Completed in `3d867a0`.

- The wind-down story had to follow the guards' real order: the repository guard (409) fires BEFORE the board guard (422), so the journey re-homes the repository first and only then sees the card named. Writing it the other way round failed, which is the journey doing its job.
- J1 and J3 both created a `uat-<run>-mobile` team and collided on the slug; J3 now uses the `ios` suffix.
- `setupRepoAgentOnTeam` kept (not deleted) for `--server` targets whose credentials cannot create teams; `UAT_TEAM` re-documented as that fallback.