---
id: uat-tier-6-in-a-0012-readme
level: task
title: "UAT tier 6 in A-0012, README section, nightly CI job, full recorded runs (compose and --server)"
short_code: "KAIROS-T-0122"
created_at: 2026-09-22T11:15:31.213057+00:00
updated_at: 2026-09-22T11:15:31.213057+00:00
parent: KAIROS-I-0011
blocked_by: ["KAIROS-T-0117", "KAIROS-T-0118", "KAIROS-T-0119", "KAIROS-T-0120", "KAIROS-T-0121"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# UAT tier 6 in A-0012, README section, nightly CI job, full recorded runs (compose and --server)

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Make the tier official (I-0011 D6): amend KAIROS-A-0012 with tier 6, document the runs in `README.md`, add the nightly GitHub Actions job, and record a full run of all four journeys in both modes in the initiative.

## Implementation Notes

### Technical Approach

- `.metis/adrs/KAIROS-A-0012.md` §Tiers: add **6. UAT (`angreal test uat`, added 2026-09-22 per Dylan, KAIROS-I-0011)** — persona journeys (an agent over MCP/CLI, engineers over the GUI, admins over the CLI), against the compose stack or any deployment by `--server`, self-cleaning, emitting a readable per-run report; a release/milestone gate, never per-task. Extend "Playwright is the only non-Rust test dependency; it's confined to the E2E tier" to "…E2E and UAT tiers".
- `README.md`: under the test/CI material add "User acceptance runs": both invocations, the persona env vars, where the report lands, the `--keep-running` + `--server` pattern for iterating. Keep Diataxis modes separate (how-to vs reference table of env vars).
- `.github/workflows/uat-nightly.yml`: scheduled (cron) + `workflow_dispatch`; checks out, installs Rust/Node/angreal like the existing CI workflow does, runs `angreal test uat`, uploads `uat/reports/**` as an artefact always (`if: always()`). Not on PRs.
- Full runs: `angreal test uat` (compose, all journeys) and, with `--keep-running`, `angreal test uat --server http://localhost:41080`; paste both result lines and the report paths into the initiative's progress log; attach the compose report's per-journey summary rows.

### Dependencies

T-0117 … T-0121.

## Acceptance Criteria

- [ ] A-0012 lists tier 6 with a dated amendment; README section present; nightly workflow file validates (`actionlint` if available, else a careful read against the existing workflow).
- [ ] Both full runs green; result lines and report paths recorded in KAIROS-I-0011's progress log.
- [ ] `angreal test all` unchanged (UAT not in the per-task gate); `angreal tree` shows `test uat`.

## Status Updates

*To be added during implementation*
