---
id: close-out-both-full-runs-recorded
level: task
title: "Close out: both full runs recorded, READMEs list the arc, the drift gate still green"
short_code: "KAIROS-T-0149"
created_at: 2026-09-23T03:46:15.200199+00:00
updated_at: 2026-09-23T03:46:15.200199+00:00
parent: KAIROS-I-0014
blocked_by: ["KAIROS-T-0137", "KAIROS-T-0138", "KAIROS-T-0139", "KAIROS-T-0140", "KAIROS-T-0141", "KAIROS-T-0142", "KAIROS-T-0143", "KAIROS-T-0144", "KAIROS-T-0145", "KAIROS-T-0146", "KAIROS-T-0147", "KAIROS-T-0148"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0014
---



## Parent Initiative

[[KAIROS-I-0014]]

## Implementation Notes

- `uat/README.md`: list the journeys as the ARC (new user → mature org) rather than an unordered set, so a reader can see what stretch of an organisation's life each one covers.
- `README.md` "User acceptance runs": update the journey count and the `--journey` id list.
- Runs: `angreal test uat` (compose, all journeys + the gate) and `--server` against a kept stack; record run ids and the Surface coverage line in the initiative's progress log. Re-run `angreal test e2e` once, since journeys share helpers with it.
- If the suite has grown slow enough to be annoying, say so in the status update with the number — do not silently split it.

## Acceptance Criteria

- [ ] Both full runs green; run ids and the coverage line in the initiative log.
- [ ] READMEs describe the arc; `angreal test e2e` 11/11.

## Status Updates

*To be added during implementation*
