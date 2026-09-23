---
id: close-out-mcp-search-in-a-journey
level: task
title: "Close out: MCP search in a journey, ALLOW trimmed to its reasoned residue, README + uat README, both full runs recorded"
short_code: "KAIROS-T-0136"
created_at: 2026-09-23T02:59:24.116176+00:00
updated_at: 2026-09-23T03:24:21.775476+00:00
parent: KAIROS-I-0013
blocked_by: [KAIROS-T-0131, KAIROS-T-0132, KAIROS-T-0133, KAIROS-T-0134, KAIROS-T-0135]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

- [x] Every one has a persona: ALLOW is `{}` — MCP 17/17, CLI 16/16, 0 allow-listed. No reasons were needed in the end.
- [x] compose `mudjfjka` 8/8 (17/17, 16/16); server `mudjeuf9` 8/8 with 2 compose-only steps skipped and the gate reporting "Not measured".
- [x] `README.md` (seven journeys, the gate, the mode rule) and `uat/README.md` updated; `angreal test e2e` 11/11.

## Status Updates

**2026-09-22** — Completed in `ec9174e`.

- Each remaining surface got a step instead of a reason: `admin` (the CLI provisions the tenant — alice IS a deployment admin in compose, and it is what an operator would type), `strategies`/`initiatives`/`adrs` (planning checks its own work from the terminal and records the decision as an ADR — Flight Levels wants the decision next to the initiative), `mcp:search` (audit-trail finds the repaired task by what it says now), `orgs` (smoke).
- **The empty map found a bug in the gate's own rule.** In `--server` mode the tenant step is compose-only and skipped, so `cli:admin` could never be exercised and the gate failed a perfectly good deployment run. Coverage is a property of the SUITE, measured where the suite runs whole — the gate now skips outside compose mode, the same rule it already applied to filtered runs. Worth noting the gate caught this itself, on its first run with nothing allow-listed.