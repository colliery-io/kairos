---
id: uat-coverage-catches-up-four
level: initiative
title: "UAT Coverage Catches Up - Four Journeys and a Drift Gate"
short_code: "KAIROS-I-0013"
created_at: 2026-09-23T02:57:02.226240+00:00
updated_at: 2026-09-25T00:04:18.531319+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: M
initiative_id: uat-coverage-catches-up-four
---

# UAT Coverage Catches Up - Four Journeys and a Drift Gate Initiative

## Context **[REQUIRED]**

The UAT tier (KAIROS-I-0011) shipped five journeys against the product as it
stood on 2026-09-22. Two initiatives later (I-0012's team lifecycle, and the
I-0011 fix round) the machinery still fits, but coverage has drifted: we
built `move_item` and a GUI board select and neither is walked by a persona,
only by integration and e2e.

Measured on 2026-09-22 (Dylan asked "make sure our uat harness has expanded
to match"):

- **MCP:** 9 of 17 tools untouched by any journey — `move_item`,
  `my_boards`, `search`, `edit_item`, `update_item`, `set_metadata`,
  `get_history`, `delete_item`.
- **GUI:** `/search`, `/search/relationships/:code`, `/teams` (directory),
  `/teams/:slug/pages/*`, all seven `/admin/*` pages, `/activity`,
  `/activity/history/:code` never visited.
- **CLI:** `adrs`, `boards`, `streams`, `orgs`, `strategies`,
  `initiatives`, `admin` never run.
- **Whole product areas with no journey:** team pages + announcements
  (I-0007), item history / rollback / activity feed (A-0004's audit
  promise), metadata definitions and templates (A-0003), board
  configuration (columns, transitions) and delivery streams.

Dylan's decisions (2026-09-22, AskUserQuestion): add all four proposed
journeys, and enforce coverage mechanically — the suite fails when an MCP
tool or CLI noun exists that no journey touches.

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- Every MCP tool and every CLI noun is exercised by at least one journey,
  or is on an allow-list that says why not — enforced by the suite, so the
  next drift fails a run instead of waiting for someone to notice.
- Four new/extended journeys covering the move surfaces, the audit trail,
  team knowledge, and board configuration — each a story a person would
  recognise, not a checklist of calls.
- The report gains a coverage section: what was exercised, what is
  allow-listed, and why.

**Non-Goals:**
- Not chasing 100% of GUI routes or API endpoints. The gate covers the two
  surfaces agents and operators drive (MCP, CLI); GUI coverage stays a
  judgement call per journey.
- No new harness concepts: journeys, personas, ledger and reporter are
  unchanged.
- Not moving coverage that e2e already owns (drag mechanics, lane
  rendering, 409 merge UI) into UAT for its own sake.

## Detailed Design **[REQUIRED]**

### D1. The drift gate measures what RAN, not what the source mentions

Instrument the surfaces rather than grepping journey files (a regex over
sources would pass on a commented-out call and fail on a helper):

- `uat/run/coverage.ts`: a module-level registry (workers = 1, so one
  process). `record(kind, name)` with kinds `mcp` and `cli`.
  `McpSession.call`/`refused` record the tool name; `Cli.run` records
  `args[0]` (the noun). `journey()` records each journey id that started.
- `uat/checks/zz-surface-coverage.check.ts` — named `zz-` so it runs last
  under the serial runner; `playwright.config.ts` `testMatch` widens to
  `[/.*\.journey\.ts/, /.*\.check\.ts/]`. It:
  1. asks the running deployment what exists: an MCP `tools/list` as alice
     (new `McpSession.listTools()`), and `kairos --help` parsed for the
     top-level nouns (new `Cli.nouns()`);
  2. subtracts what the registry recorded;
  3. subtracts `ALLOW` — an explicit map of `surface → reason`;
  4. fails naming anything left, with the journey that ought to cover it.
- **Skipped on a filtered run.** `--journey planning` cannot cover the
  product, so the check skips (with a report line) unless every journey
  file ran. It compares the registry's journey ids against the files in
  `journeys/`.
- `ALLOW` is the shrinking to-do list: seeded with today's untouched
  surfaces, and each task below deletes its entries. What remains at the
  end is deliberate, and each entry carries its reason (e.g. `adrs: "ADRs
  are covered by the e2e lifecycle spec; no persona files one today"`).
- The reporter renders a `## Surface coverage` section: `17/17 MCP tools,
  16/16 CLI nouns, 0 allow-listed` or the shortfall.

### D2. The move surfaces get a persona (extends `cross-team`)

The story already has the right cast. After bob triages the filed card:

- **bob (GUI)** realises the ticket is really the web team's work and moves
  it with the item page's **Board** select → it lands on `web-delivery` in
  Backlog; carol's board shows it arrive live.
- **carol (MCP)** — now it is her team's — takes it: `board_items` on her
  board finds it, and she moves it back with `move_item` when they agree
  it was platform's after all, which also exercises the refusal when she
  lacks `manage_tasks` on the far side.

Covers `move_item` and the GUI board select. Keep it to three steps; the
journey is already the longest.

### D3. J5 `audit-trail` — "An edit goes wrong and the record puts it right"

alice edits a task's content; bob edits the same task from the API between
her load and her save, so her save hits 409 and she merges (the e2e smoke
proves the merge UI; here it is the *story*). Then:

1. **alice (MCP)** `update_item` with a stale version → the tool returns the
   current version and content for reconciliation; she retries and wins.
2. **alice (MCP)** `edit_item` (search/replace) for a small correction.
3. **alice (GUI)** opens the item's **History**: the versions are listed,
   a diff shows what changed and who.
4. **alice (GUI)** rolls back to the pre-mistake version (A-0004
   copy-forward: the rollback writes a NEW version, it does not erase) and
   the item shows the restored content at a higher version.
5. **alice (CLI)** `kairos tasks get` confirms; **alice (MCP)**
   `get_history` shows the whole chain.
6. **bob (GUI)** finds the episode in `/activity`, filtered to the item.

Covers `update_item`, `edit_item`, `get_history`, `/activity`,
`/activity/history/:code`.

### D4. J6 `team-knowledge` — "A team writes down how it works"

1. **bob (GUI)** opens `/teams` (the directory), then his team.
2. **bob (GUI)** creates a page under the docs tree
   (`/teams/platform/pages/...`), writes it, saves.
3. **carol (API, concurrent)** edits the same page → bob's save 409s and he
   merges (team pages carry the same A-0004 contract).
4. **bob (GUI)** posts an announcement; **carol (GUI)** sees it on her own
   team page? No — announcements are per-team, so **alice** sees it on
   platform's page.
5. **carol (GUI)** is refused editing platform's charter (non-member).
6. **bob (MCP)** `my_boards` — the other half of "where do I work" — and
   **alice (CLI)** `kairos teams list`.

Covers `/teams`, `/teams/:slug/pages/*`, announcements, `my_boards`.

### D5. J7 `board-setup` — "An admin shapes a new team's board"

Runs on J1's fresh team (same `teamFixture`), so it cleans up with it.

1. **alice (CLI)** `kairos boards show <board>` — the scaffolded default.
2. **alice (GUI, /admin/boards/:board)** adds a `Review` column between
   Active and Completed, and the transitions Active → Review → Completed.
3. **alice (GUI, /admin/metadata)** defines a metadata field scoped to
   tasks; **bob (MCP)** `set_metadata` stamps it on a card and
   **alice (CLI)** `kairos search --metadata` finds it.
4. **alice (GUI, /admin/streams)** creates a delivery stream and attaches
   the team; the team page shows it.
5. **bob (GUI)** drags his card Active → Review — the transition alice
   just created, proving configuration reaches the board.
6. **alice (MCP)** `delete_item` retires the card; teardown removes the
   rest.

Covers `/admin/boards/:board`, `/admin/metadata`, `/admin/streams`,
`set_metadata`, `delete_item`, `boards`/`streams` CLI nouns.

### D6. Allow-list after all four

Expected residue, each with a reason in `ALLOW`: `adrs` (no persona
authors an ADR today; e2e lifecycle covers the family), `orgs` (a one-line
read the smoke journey could take instead — fold it into smoke if cheap),
`admin` (tenant provisioning runs through the API in J1 because the CLI
needs a deployment-admin token the journeys do not mint), `strategies` /
`initiatives` (J2 drives them through the GUI and the relationships API;
the CLI nouns duplicate that). Whether each stays allow-listed or gets a
step is the implementing task's call — the rule is that every entry
carries a reason a reader can disagree with.

## Alternatives Considered **[REQUIRED]**

- **Grep the journey sources for tool names.** Rejected: passes on
  commented-out code, misses calls made through helpers, and drifts from
  what actually executed.
- **Assert coverage in CI only.** Rejected: the gate belongs where the
  evidence is (the run), and a local run should tell you the same thing CI
  will.
- **One mega-journey per surface family.** Rejected: journeys are stories;
  a "call every MCP tool" test is a checklist and would rot into noise.
- **Cover GUI routes mechanically too.** Rejected for now: routes are not
  a flat vocabulary (params, tabs, modals) and a route list would drive
  padding. Revisit if GUI regressions start slipping through.

## Implementation Plan **[REQUIRED]**

1. **Drift gate** — registry, `McpSession.listTools`, `Cli.nouns`, the
   `zz-surface-coverage` check, reporter section, `ALLOW` seeded with
   today's residue (fails nothing on day one, lists everything).
2. **Move surfaces in `cross-team`** (D2) — removes `move_item` from
   `ALLOW`.
3. **J5 audit-trail** (D3) — removes `update_item`, `edit_item`,
   `get_history`.
4. **J6 team-knowledge** (D4) — removes `my_boards`.
5. **J7 board-setup** (D5) — removes `set_metadata`, `delete_item`,
   `boards`, `streams`.
6. Final: `search` (MCP) picked up by whichever journey fits (J5 or J6);
   `ALLOW` trimmed to its reasoned residue; both full runs recorded.

Gates per task: `npx tsc --noEmit` in `uat/`, the touched journeys green
in compose mode, and the final task runs both modes end to end.

## Progress Log

- 2026-09-22: Dylan: "go" → ready → decompose into T-0131 … T-0136 per the
  Implementation Plan; → active; Ralph loop started. (GUI routes stay out of
  the gate — flagged to Dylan when the design was presented, not objected to.)

- 2026-09-22: Created from Dylan's "make sure our uat harness has expanded
  to match" after a coverage measurement (9/17 MCP tools, 7 CLI nouns and
  four product areas untouched). Scope and the mechanical gate decided via
  AskUserQuestion; design D1–D6 written against the measured gaps.- 2026-09-22: All six tasks completed. `aa28130` the drift gate (T-0131),
  `c184a89` cross-team gains the move surfaces (T-0132), `9e3be95` J5
  audit-trail (T-0133), `71a09dd` J6 team-knowledge (T-0134), `1160a3d`
  J7 board-setup (T-0135), `ec9174e` close-out (T-0136).

  **Where coverage landed:** `ALLOW` is `{}` — **MCP 17/17 tools, CLI
  16/16 nouns, 0 allow-listed**. Every surface got a persona who would
  really use it rather than a paragraph explaining why nobody does; the
  residue the design expected (adrs, admin, strategies, initiatives,
  orgs, search) all turned into honest steps.

  **Verified:** compose `mudjfjka` 8/8 journeys with the gate green;
  server `mudjeuf9` 8/8 with 2 compose-only steps skipped; `angreal test
  e2e` 11/11.

  **What the work found:**
  1. `zz-` in a filename does NOT order a Playwright test last — it sorts
     by path, and `checks/` precedes `journeys/`, so the gate ran first
     and skipped every time. Fixed with a dependent project.
  2. A module-level registry loses records: Playwright restarts the
     worker between projects and after any failure. The ledger is a file.
  3. The gate's own rule was wrong for `--server`: compose-only steps are
     skipped there, so `cli:admin` read as uncovered. Coverage is a
     property of the suite, measured where the suite runs whole — it now
     skips outside compose mode. The gate caught this itself on its first
     run with an empty allow-list.
  4. Product finding (open): `kairos tasks create --board` takes a UUID
     while `tasks move --to-board` and `--repo` accept a slug. Small CLI
     inconsistency, worth a ticket.
  5. The GUI hides the board select when a caller has no eligible target
     (bob, one team) — correct, and now pinned by a step so nobody
     "fixes" it into a dead control.

  Initiative left **active** for Dylan's review.