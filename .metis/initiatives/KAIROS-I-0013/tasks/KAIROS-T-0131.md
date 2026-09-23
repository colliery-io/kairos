---
id: uat-drift-gate-runtime-surface
level: task
title: "UAT drift gate: runtime surface registry, zz-surface-coverage check, ALLOW map, report section"
short_code: "KAIROS-T-0131"
created_at: 2026-09-23T02:59:06.893124+00:00
updated_at: 2026-09-23T02:59:06.893124+00:00
parent: KAIROS-I-0013
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0013
---

# UAT drift gate: runtime surface registry, zz-surface-coverage check, ALLOW map, report section

## Parent Initiative

[[KAIROS-I-0013]]

## Objective

I-0013 D1: the suite fails when an MCP tool or CLI noun exists that no journey exercised, measured from what actually RAN. Seeded so that day one fails nothing but lists everything.

## Implementation Notes

### Technical Approach

- `uat/run/coverage.ts`: module-level registry (workers = 1, one process). `recordSurface(kind: 'mcp' | 'cli', name: string)`, `recordJourney(id: string)`, `surfaceReport()` returning `{ mcp: Set, cli: Set, journeys: Set }`. No I/O.
- Instrument the surfaces, not the journeys: `surfaces/mcp.ts` `McpSession.call`/`refused` record the tool name; `surfaces/cli.ts` `Cli.run` records `args[0]`; `run/narrate.ts` `journey()` records its id. That way a call made through a helper or a fixture still counts.
- New reads so the check asks the DEPLOYMENT what exists rather than hard-coding: `McpSession.listTools()` (a `tools/list` JSON-RPC call, same post helper) and `Cli.nouns()` (`kairos --help`, parse the `Commands:` block's first column; strip `help`).
- `uat/checks/zz-surface-coverage.check.ts` — `zz-` so the serial runner takes it last; widen `playwright.config.ts` `testMatch` to `[/.*\.journey\.ts/, /.*\.check\.ts/]`. Body: list the journey files in `journeys/` (fs.readdirSync); if the registry's journey ids do not cover all of them the run was filtered → `test.skip()` with an annotation the reporter prints ("skipped: filtered run, coverage needs every journey"). Otherwise fetch the two vocabularies, subtract the registry, subtract `ALLOW`, and fail listing what is left with the suggested owner journey.
- `ALLOW: Record<string, string>` in the check file, keyed `mcp:<tool>` / `cli:<noun>`, value = the reason. Seed it with today's untouched set so this task is green on its own: `mcp:move_item`, `mcp:my_boards`, `mcp:search`, `mcp:edit_item`, `mcp:update_item`, `mcp:set_metadata`, `mcp:get_history`, `mcp:delete_item`, `cli:adrs`, `cli:boards`, `cli:streams`, `cli:orgs`, `cli:strategies`, `cli:initiatives`, `cli:admin` — each with "pending KAIROS-T-01xx" as the reason so the map reads as the to-do list it is. Later tasks delete their entries.
- Reporter (`run/reporter.ts`): a `## Surface coverage` section — `MCP 17/17, CLI 16/16, 0 allow-listed`, or the shortfall as a list with reasons. Read the registry directly (same process); when the check skipped, print that instead.
- `uat/README.md`: a short "Coverage" heading — what the gate checks, how to allow-list with a reason, why it measures runtime.

### Dependencies

None.

## Acceptance Criteria

- [ ] `angreal test uat` is green with the seeded ALLOW, and its report's Surface coverage section lists every allow-listed surface with a reason.
- [ ] Deleting one ALLOW entry (e.g. `mcp:my_boards`) makes the run FAIL naming that surface; restoring it passes. (Demonstrate in the status update; leave the entry in.)
- [ ] `angreal test uat --journey smoke` skips the check with the filtered-run reason rather than failing.
- [ ] `npx tsc --noEmit` clean in `uat/`.

## Status Updates

*To be added during implementation*
