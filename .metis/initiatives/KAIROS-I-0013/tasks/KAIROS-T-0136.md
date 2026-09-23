---
id: close-out-mcp-search-in-a-journey
level: task
title: "Close out: MCP search in a journey, ALLOW trimmed to its reasoned residue, README + uat README, both full runs recorded"
short_code: "KAIROS-T-0136"
created_at: 2026-09-23T02:59:24.116176+00:00
updated_at: 2026-09-23T02:59:24.116176+00:00
parent: KAIROS-I-0013
blocked_by: ["KAIROS-T-0131", "KAIROS-T-0132", "KAIROS-T-0133", "KAIROS-T-0134", "KAIROS-T-0135"]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0013
---

# Close out: MCP search in a journey, ALLOW trimmed to its reasoned residue, README + uat README, both full runs recorded

## Parent Initiative

[[KAIROS-I-0013]]

## Objective

I-0013 D6: finish the sweep — the last uncovered surfaces either get a step or an argued reason — and record the runs.

## Implementation Notes

### Technical Approach

- **`mcp:search`**: fold into whichever journey it fits honestly. J5 (audit-trail) is the natural home — alice searches for the item she just repaired — or J2 (planning), which already searches from the CLI. One step, not a new journey.
- **`cli:orgs`**: a one-line read; fold `kairos orgs show` into the smoke journey's CLI step if it costs nothing.
- Remaining entries keep a REASON a reader can disagree with, not "pending": `cli:adrs` (no persona authors an ADR; the e2e lifecycle spec covers the family), `cli:admin` (tenant provisioning needs a deployment-admin token; J1 uses the API path instead), `cli:strategies` / `cli:initiatives` (J2 drives both through the GUI and the relationships API — the CLI nouns would duplicate the same story). If any of those feels wrong while writing it, add the step instead; the reason has to be defensible.
- `uat/README.md`: the Coverage section finalised (what the gate checks, the ALLOW contract, how to add a surface).
- `README.md` "User acceptance runs": mention the coverage gate in one line and list the journeys (now seven) instead of the original four.
- Runs: `angreal test uat` (compose) and `--server` against a kept stack; record run ids and the Surface coverage line in I-0013's progress log. Also re-run `angreal test e2e` once, since journeys share helpers with it.

### Dependencies

T-0131, T-0132, T-0133, T-0134, T-0135.

## Acceptance Criteria

- [ ] Every MCP tool is exercised or carries a defensible reason; the same for CLI nouns.
- [ ] Both full runs green with the Surface coverage section showing the final numbers; run ids in the initiative log.
- [ ] READMEs updated; `angreal test e2e` still 11/11.

## Status Updates

*To be added during implementation*
