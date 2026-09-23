---
id: uat-drift-gate-runtime-surface
level: task
title: "UAT drift gate: runtime surface registry, zz-surface-coverage check, ALLOW map, report section"
short_code: "KAIROS-T-0131"
created_at: 2026-09-23T02:59:06.893124+00:00
updated_at: 2026-09-23T03:07:04.157023+00:00
parent: KAIROS-I-0013
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

- [x] Run `mudisuhf`: green, "MCP 9/17 tools, CLI 9/16 nouns, 15 allow-listed" with every entry and its reason listed.
- [x] Removing `mcp:my_boards` failed the run with "these surfaces exist but no journey exercises them: mcp:my_boards"; restored and green again.
- [x] Run `mudiu9e4` (`--journey smoke`): passes, section reads "Not measured: the gate needs every journey, and this run was filtered."
- [x] `npx tsc --noEmit` clean; `uat/README.md` has a "Coverage: the drift gate" section.

## Status Updates

**2026-09-22** — Completed in `aa28130`.

- **Two design corrections found by running it.** (1) The registry had to move from a module-level Set to a per-run JSONL file: Playwright starts a fresh worker for a dependent project AND after any test failure, so in-memory records would vanish and the gate would invent gaps. (2) `zz-` in the filename does not order it last — Playwright sorts by path, and `checks/` sorts before `journeys/`, so the gate ran FIRST and skipped every time. Fixed with two projects and `dependencies: ['journeys']`, which is the only way to express "after everything".
- The gate also fails on STALE allow-list entries (covered now, or gone from the product) — without that the map rots into a list of lies.
- A filtered run excludes the check entirely (angreal maps `--journey` to `--grep`, and the check carries no ``), so the report prints the section unconditionally and says "Not measured" rather than going silent.
- Tooling gotcha repeated from earlier tasks: perl ate `${title}` in `narrate.ts`'s `test()` template, blanking every journey title. Use Edit for TS template strings.