---
id: user-acceptance-journeys-persona
level: initiative
title: "User Acceptance Journeys - Persona-Driven UAT Tier with Readable Reports"
short_code: "KAIROS-I-0011"
created_at: 2026-09-22T11:10:59.088845+00:00
updated_at: 2026-09-22T11:17:32.135748+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/active"


exit_criteria_met: false
estimated_complexity: M
initiative_id: user-acceptance-journeys-persona
---

# User Acceptance Journeys - Persona-Driven UAT Tier with Readable Reports Initiative

## Context **[REQUIRED]**

KAIROS-A-0012 gives the project five test tiers: unit, DB integration, API
integration (38 compose-backed targets through `kairos-client`), an e2e smoke
(the `e2e_golden_path` API+MCP runner plus eleven Playwright specs), and the
soak harness. Every one of them is **feature-shaped**: one spec per initiative,
asserting implementation detail (`data-testid`s, DTO fields, activity-row
counts) as alice/bob/carol on the demo seed against the local compose stack.

Nothing walks the application the way a *user* does — a persona with a goal,
crossing the surfaces that persona would actually use (an agent through MCP
and the CLI, an engineer through the GUI, an admin through the CLI/API),
against either the compose stack or a deployed instance — and nothing produces
an artefact a person who did not write the test can read to decide "yes, it
does the thing". That is user acceptance testing, and it is the gap this
initiative fills: a sixth tier, `angreal test uat`.

Dylan's decisions (2026-09-22, via AskUserQuestion):

- **Form:** automated persona journeys that emit a readable per-run report.
- **Target:** compose stack by default, any deployment by `--server <url>`;
  journeys create namespaced data and clean up after themselves.
- **Surface:** whichever the persona would really use; one journey may cross
  surfaces.
- **First-pass scope:** all four proposed journeys — the core agent loop, org
  onboarding, cross-team collaboration, strategy → delivery planning.

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- `angreal test uat` runs four persona journeys against a live Kairos and
  exits non-zero on the first failed acceptance step, naming journey, persona
  and step.
- Every run writes `uat/reports/<run-id>/report.md` (+ `report.json`): per
  journey, the narrated steps, what was observed (short codes, column names,
  link states), pass/fail, and on failure the screenshot/trace paths. A
  product owner can read it without the source.
- The same suite runs unchanged against the compose stack (fresh seed) and a
  deployed instance (`--server`), differing only in credentials and in the
  steps that are marked *compose-only*.
- Journeys are written from the persona's vantage point: they use the CLI
  binary, the MCP endpoint and the browser; they never reach into the
  database or call internal helpers.
- A-0012 amended with tier 6; the plugin's `code-review`/`implement` gate
  text unchanged (UAT is a release gate, not a per-task gate).

**Non-Goals:**
- Not a replacement for the e2e smoke or the Playwright feature specs; those
  keep asserting implementation detail per initiative.
- Not driving a real LLM agent. "The agent" is a service account whose
  actions are scripted through the same MCP/CLI calls the plugin skills make.
- Not a load or performance tier (soak owns that).
- Not a Gherkin/Cucumber DSL. Journeys are plain Playwright tests with
  narrated steps; the report is rendered from the step tree.
- Not human-run scripts. If a manual sign-off checklist is wanted later it
  renders from the same step narration (listed as follow-up).

## Detailed Design **[REQUIRED]**

### D0. Runner: Playwright's test runner is the journey engine

A new top-level `uat/` package (its own `package.json`, `playwright.config.ts`,
`tsconfig.json`), sibling to `e2e/`, not inside it: different lifecycle
(target-parameterised, self-cleaning), different posture (journeys, not
feature specs), different reporter. It shares nothing with `e2e/` except
copied-then-diverged helpers; a shared `test-support/` package is explicitly
deferred until a third consumer exists.

Why Playwright and not a Rust runner (the stack preference): the browser
persona needs Playwright regardless; a Rust journey engine would have to drive
Playwright out of process for every GUI step and lose the step tree that the
report is rendered from. Playwright already gives serial journeys,
`test.step` nesting, per-persona browser contexts, retries, traces and a
reporter API. A-0012 §"Playwright is the only non-Rust test dependency"
extends from "confined to the e2e tier" to "confined to the e2e and UAT
tiers". API/MCP/CLI steps are plain TypeScript over `fetch` and
`child_process`; the CLI steps execute the **real `kairos` binary** built by
the harness, so the CLI is under acceptance, not mocked.

### D1. `angreal test uat`

```
angreal test uat [--server URL] [--journey NAME]... [--keep-running]
                 [--headed] [--report-dir PATH]
```

- **Default (compose):** `compose up --wait` → `cargo build` (kairos-server,
  kairos-cli) → `angreal web build` → `seed-demo --force` → boot one
  kairos-server on `:41080` exactly as the e2e GUI leg does
  (`OIDC_AUDIENCE=kairos-web`, `KAIROS_SINGLE_TENANT=demo`,
  `KAIROS_WEB_DIST=dist`, plus `KAIROS_DEPLOYMENT_ADMINS=<alice's sub>` so
  the onboarding journey can exercise tenant provisioning) → run the suite →
  stop server, `compose down -v`. Destructive to the dev `demo` tenant, same
  as e2e.
- **`--server URL`:** no compose, no seed, no server boot. The suite runs
  against the URL with credentials from the environment (D3). Compose-only
  steps are skipped and the report says so.
- `--journey` filters by journey id (`onboarding`, `planning`,
  `agent-loop`, `cross-team`); repeatable. `--keep-running` leaves the stack
  up for a hand-run (`npx playwright test --ui` from `uat/`). `--headed`
  passes through. The task's `ToolDescription` documents all of this and is
  `risk_level="destructive"` (compose) — the description states that
  `--server` mode is non-destructive to pre-existing data.
- `angreal test all` does **not** include it. It joins `angreal test e2e` as
  the release/milestone acceptance pair.

### D2. Journey anatomy

```
uat/
  playwright.config.ts        serial, workers=1, retries=0 (a journey is not
                              retry-safe by construction: it narrates one run)
  journeys/
    onboarding.journey.ts
    planning.journey.ts
    agent-loop.journey.ts
    cross-team.journey.ts
  personas/
    index.ts                  persona(name) → { name, role, gui, api, cli, mcp }
    credentials.ts            env → per-persona credentials (D3)
  surfaces/
    gui.ts                    login(page), boards, item page, team page — the
                              user-visible actions (by role/text, not testid)
    api.ts                    typed fetch wrappers over the public REST API
    mcp.ts                    streamable-HTTP MCP session: initialize +
                              call_tool, parsing the tool text the way the
                              plugin skills read it
    cli.ts                    execFile the built `kairos` binary with a
                              per-persona config dir (`kairos auth` state)
    forge.ts                  signed GitHub webhook deliveries (from e2e)
  run/
    ledger.ts                 records everything a journey created; teardown
                              deletes in reverse dependency order
    narrate.ts                `step(persona, "files a task against …", fn)`:
                              wraps test.step with persona + observed values
    reporter.ts               Playwright reporter → report.md + report.json
  reports/                    gitignored
```

- A **journey** is one `test()` whose title is the user story ("An
  organisation is set up and a new engineer finds their team"). Inside, every
  acceptance step goes through `step(persona, narration, fn)`, which nests a
  `test.step` titled `"<persona> <narration>"` and lets `fn` return an
  `observed` record (short codes, column names, link states) that the
  reporter prints under the step. A journey may cross surfaces freely: `bob`
  drags a card in the GUI while `agent` reads the board over MCP.
- **Personas** (D3) are created per journey from `persona('carol')`. `gui` is
  a lazily-opened browser context logged in as that persona (real Dex PKCE
  in-browser, as the e2e specs do); `api` is a bearer client; `cli` runs the
  binary with `KAIROS_CONFIG_DIR=<tmp>/<persona>` after a scripted
  `kairos auth` (token injected, not interactive); `mcp` is a session
  initialised with the persona's token and the tenant header.
- **Namespacing:** every created thing carries `uat-<run>-` in its slug or
  title (`run` = base36 timestamp, also the report directory name). The
  `ledger` records `{ kind, id, deleteVia }` for every create and the journey's
  `afterAll` deletes in reverse order (service-account keys → service
  accounts → forge connections → tasks → initiatives/strategies →
  repositories → teams → tenant). Deletion failures are reported, not fatal:
  the run already passed or failed on its acceptance steps. On compose the
  stack is torn down anyway; on `--server` this is what "self-cleaning"
  means.
- **Compose-only steps** are declared `step.composeOnly(...)`; under
  `--server` they are skipped with a report line ("skipped: needs a
  deployment-admin token / fresh tenant").
- **No sleeps.** Same posture as e2e: expect-polling and explicit waits only.

### D3. Personas and credentials

| Persona | Role | Surfaces | Compose default |
|---|---|---|---|
| `alice` | org admin, and deployment admin for tenant steps | CLI, API, GUI | seed user, `KAIROS_DEPLOYMENT_ADMINS` set by the task |
| `bob` | platform engineer (team member, non-admin) | GUI, CLI | seed user |
| `carol` | web-team member — the cross-team filer | GUI, MCP (as her agent) | seed user |
| `agent` | service account on the platform team, created by the onboarding journey | MCP, CLI | created by `alice` in-run; API key captured once |
| `newhire` | a member added mid-run to prove onboarding | GUI | `bob` re-used under compose (added to the new team); a real second human under `--server` if provided, else the step degrades to `bob` |

Credentials come from `UAT_PERSONA_<NAME>_EMAIL` / `_PASSWORD` (Dex/Keycloak
password login through the same headless PKCE walk `e2e/helpers/auth.ts`
does), with the seed users as defaults. `UAT_TENANT` names the tenant
(default `demo`); `UAT_ISSUER` the OIDC issuer. Deployments whose IdP has no
password-capable login (Google Workspace) are out of scope for this pass —
listed as follow-up: a `--capture-login <persona>` headed flow that stores
Playwright storage state per persona.

### D4. The four journeys

Each journey is a story; the bullets are its acceptance steps (persona in
bold). Observed values named in *italics* are what the report prints.

**J1 `onboarding` — "An organisation is set up and a new engineer finds their team"**
1. *(compose-only)* **alice** provisions a tenant `uat-<run>` through
   `POST /api/admin/tenants` and sees it in `GET /api/admin/tenants`; the
   rest of the journey runs in the demo tenant (single-tenant server), so
   this step proves provisioning, not the new tenant's contents.
2. **alice** (CLI) creates team `uat-<run>-mobile` (stream-aligned) and
   observes that a delivery board with the default columns and transitions
   was scaffolded for it → *board slug, column names*.
3. **alice** (CLI) adds **bob** to the team; `kairos whoami` as bob lists the
   team and the implied board capabilities.
4. **alice** (CLI) registers repository `uat-<run>-mobile-app` owned by the
   team with a "how to work here" description → *slug, owner*.
5. **alice** (CLI) creates service account `uat-<run>-agent`, grants it the
   team board's `manage_tasks`, mints a key (shown once) → the key becomes
   **agent**'s credential for J3.
6. **bob** (GUI) logs in: *My teams* shows the new team; the team page shows
   the Repositories panel with the repo and its description; the board is
   empty but navigable.
7. **agent** (MCP) `whoami` lists the team, the repo under `repositories`,
   and `file_backlog` under `implicit`.
8. Teardown: key, service account, repository, team (order matters: team
   delete refuses while it owns a repo — the ledger encodes this).

**J2 `planning` — "A strategy is broken down until it is work on a board"**
1. **alice** (GUI) creates a strategy document and an initiative under it;
   the initiatives board shows the initiative card.
2. **alice** (CLI) decomposes: two tasks under the initiative on the
   platform delivery board, one bound to `payments-api`, the second
   `blocks` the first → *short codes*.
3. **alice** (GUI) opens the initiative: the children-progress bar reads
   0/2; the Graph tab shows the initiative, both tasks and the blocks arrow.
4. **bob** (GUI, second browser) has the platform board open; **alice**
   (CLI) transitions the unblocked task to Active; **bob**'s board reflects
   the move without a reload; the blocked task's card carries the
   blocked-by badge.
5. **alice** (CLI) `kairos search --traverse` from the initiative finds both
   tasks; `--repo payments-api` finds exactly the bound one.
6. **alice** (GUI) moves the strategy document's lifecycle to *review* and
   sees the badge change.
7. Teardown: tasks, initiative, strategy.

**J3 `agent-loop` — "An agent picks up a ticket in its repository and lands a PR"**
(depends on J1's team/repo/agent under compose; under `--server` the journey
creates its own repo + service account with alice first — same code path,
different ledger)
1. **agent** (MCP) bootstraps: `whoami` → `list_repositories` →
   `get_repository <slug>` reads the description and sees no in-flight PRs.
2. **bob** (GUI) creates a task on the board bound to the repo and drags it
   to Todo → *short code*.
3. **agent** (MCP) `board_items` narrowed to the repo finds exactly that
   task; `get_item` shows `repository: <slug> (owner: …)`; `transition`
   moves it to Active.
4. **agent** (CLI) `kairos tasks get <code>` agrees; `kairos repos get
   <slug>` shows the task as open work.
5. A pull request naming the short code is opened in the repo (signed
   webhook through the connection **alice** creates in-journey) → **bob**
   (GUI) sees it in the item's Development panel; **agent** (MCP)
   `get_repository` lists it in flight.
6. The PR is merged (second webhook) → the link reads *merged* for both
   personas; **agent** transitions the task to Done; **bob**'s board shows
   it in Done.
7. Teardown: forge connection, task (+ J1's objects when run standalone).

**J4 `cross-team` — "A web engineer needs something from platform and gets it"**
1. **carol** (MCP, as her own agent session) `list_repositories` → reads
   `payments-api`'s description → `create_item` with `repository:
   payments-api` and no board → the item lands in platform's **Backlog**
   → *short code, column*.
2. **carol** (MCP) creates her own task on `portal-web` and `link_items`
   `blocks` from the platform task to it (allowed: she authored the source);
   an attempt to `transition` the platform task is refused with the
   Backlog-only explanation.
3. **bob** (GUI) sees the filed card in Backlog with the repo chip and drags
   it to Todo — the owning team's triage.
4. **carol** (GUI) opens her task: the blocked-by badge names the platform
   task; the web board card shows it too.
5. **bob** (GUI) moves the platform task to Done → **carol**'s badge clears
   (WS, no reload).
6. **alice** (CLI) `kairos search --repo payments-api` lists the filed task
   with carol as creator; the platform team page's in-flight rollup is
   unchanged (no PR in this story).
7. Teardown: both tasks, the edge.

### D5. Report

`run/reporter.ts` implements Playwright's `Reporter`. It walks each
journey's step tree (persona, narration, `observed`, duration, status) and
writes:

```
# Kairos UAT — run 2026-09-22T11:42Z (k3f9x2)  target: http://localhost:41080 (compose)
Result: 4 journeys, 4 passed, 0 failed, 1 step skipped

## J1 — An organisation is set up and a new engineer finds their team   ✅ 41.2s
| # | Persona | Step | Observed | Status |
| 1 | alice | provisions tenant uat-k3f9x2 | tenant listed | ✅ |
| 2 | alice | creates team uat-k3f9x2-mobile | board uat-k3f9x2-mobile-delivery; columns Backlog, Todo, Active, Review, Done | ✅ |
…
| 6 | bob | sees the team in My teams | — | ❌ expected "uat-k3f9x2-mobile" in nav; screenshot: ./j1-step6.png, trace: ./j1.zip |
```

plus `report.json` (the same tree, machine-readable) and, on failure,
Playwright's screenshot/trace copied into the run directory. Console output
is the standard `list` reporter plus the one-line result and the report
path. `uat/reports/` is gitignored; `--report-dir` overrides.

### D6. A-0012 amendment and docs

- A-0012: add **tier 6 — UAT (`angreal test uat`)** to §Tiers with the
  one-paragraph definition (persona journeys, compose or `--server`,
  readable report, release gate not per-task gate) and extend the Playwright
  confinement sentence. Dated amendment line, as the soak tier was added.
- `README.md`: a "User acceptance runs" subsection under the test section:
  how to run against compose and against a deployment, where the report
  lands, the persona env vars.
- `uat/README.md`: journey-writing conventions (narrate every acceptance
  step, no testids, ledger every create, compose-only marking).
- `.github/workflows`: a nightly job running `angreal test uat` (compose)
  and uploading `uat/reports/**` as an artefact; not on PRs.

### D7. Open points for Dylan

1. **Runner language (D0).** Playwright/TypeScript as recommended, or a Rust
   `kairos-uat` crate driving the browser out of process (more code, weaker
   step tree, but all-Rust)?
2. **Tenant provisioning in J1.** Keep as a compose-only proof step, or drop
   it entirely and treat tenants as an operator concern outside UAT?
3. **Nightly CI.** Wire the nightly job in this initiative or leave CI
   wiring for a later ops pass?

## Alternatives Considered **[REQUIRED]**

- **Extend `e2e/` with more specs.** Rejected: e2e is feature-shaped and
  asserts implementation detail; mixing in journeys blurs both, and the
  compose-only lifecycle cannot target a deployment.
- **Gherkin/Cucumber (`@cucumber/cucumber` or `playwright-bdd`).** Rejected
  for now: a second DSL and glue layer to maintain for a benefit — readable
  scenarios — that the narrated step tree and rendered report already give.
  Revisit if non-engineers start authoring journeys.
- **Rust journey engine (`kairos-soak`-style) with Playwright as a
  subprocess for GUI steps.** Rejected as the default (D0) — kept as open
  point 1.
- **Human-run checklists only.** Rejected by Dylan's form decision; a
  rendered checklist from the same narration is a cheap follow-up.
- **Driving a real Claude session for the agent persona.** Rejected: slow,
  non-deterministic, and the plugin skills are already verified as scenario
  runs (A-0012 §Skills verification). The agent persona scripts the same
  MCP/CLI calls the skills document.

## Implementation Plan **[REQUIRED]**

Decomposition after design approval (one task each, in dependency order):

1. **UAT harness** — `uat/` package, config, personas + credentials,
   surfaces (gui/api/mcp/cli/forge), ledger, `step()` narration, reporter,
   `angreal test uat` with compose and `--server` modes, `.gitignore`,
   `uat/README.md`; a one-step smoke journey proving login on every surface
   and a rendered report.
2. **J1 onboarding** (also exercises `KAIROS_DEPLOYMENT_ADMINS` wiring in
   the task).
3. **J2 planning.**
4. **J3 agent-loop** (depends on J1's objects; standalone path under
   `--server`).
5. **J4 cross-team.**
6. **A-0012 amendment, README section, nightly CI job**, and a full
   `angreal test uat` run recorded in the initiative (both modes: compose,
   and `--server` against the compose-booted server left up with
   `--keep-running`).

Gates per task: fmt/clippy unaffected (no Rust) except the angreal task;
`npx tsc --noEmit` in `uat/`; `angreal test uat --journey <id>` green;
the report for the journey attached to the task's status update.

## Progress Log

- 2026-09-22: Dylan: "go" on the defaults for the three open points
  (Playwright/TypeScript runner; tenant provisioning kept as a compose-only
  J1 step; nightly CI job in scope). → ready → decompose into T-0117 … T-0122
  per the Implementation Plan; → active; Ralph loop started.

- 2026-09-22: Created in discovery from Dylan's "let's get UAT tests set up";
  four scope/form/target/surface decisions taken via AskUserQuestion; design
  D0–D7 written against the current code (A-0012 tiers, e2e harness,
  `KAIROS_DEPLOYMENT_ADMINS`, public API surface). Three open points for
  Dylan before → ready.- 2026-09-22: All six tasks completed. `6d8b3a3` harness (T-0117),
  `81a818d` J1 onboarding (T-0118), `4fdb6a1` J2 planning + a CLI fix
  (T-0119), `2c2409e` J3 agent-loop (T-0120), `d46da4d` J4 cross-team
  (T-0121), `ec4e5ff` A-0012 tier 6 + README + nightly workflow (T-0122).
  **Recorded runs:** compose `angreal test uat` → run `mucmoyog`,
  "5 journeys, 5 passed, 0 failed, 0 steps skipped" (42 ✅ steps); server
  `angreal test uat --server http://localhost:41080` against the kept
  stack → run `mucmps8b`, "5 journeys, 5 passed, 0 failed, 2 steps
  skipped" (tenant provisioning, the smoke admin probe). e2e still 11/11
  on the shared boot helpers. Initiative left **active** for Dylan's review.

  **What the journeys found** (candidate follow-ups, none blocking):
  1. *Fixed in T-0119:* `kairos search --repo <slug>` alone was refused as
     "nothing to search for" — README documents that invocation.
  2. `/items/:code` builds its Details/Graph tab anchors from the route
     param, which is empty on the first render (`/items/?view=graph` → the
     router's "Nothing here"). Only a first-frame programmatic click hits it.
  3. An agent cannot see PR link state over MCP: `get_item` renders no
     links; a merge is visible only as the PR leaving `get_repository`'s
     in-flight list. Links on `get_item` would close the loop.
  4. `get_repository` prints `delivery board: <UUID>` while every other tool
     prints slugs.
  5. The refusal a cross-team filer gets on `transition_item` is the generic
     `FORBIDDEN … requires capability "transition_items"`; it never mentions
     the Backlog-only rule the recipe teaches.
  6. The team page's Repositories panel does not show the description
     agents read; the New task modal has no repository picker (binding
     happens on the item page).
  7. *Fixed by KAIROS-I-0012:* a team whose delivery board ever held an
     item could never be deleted (soft-deleted rows pinned the board,
     T-0010). The guard now counts LIVE cards, and a task can be moved to
     another delivery board, so a team winds down by moving or archiving
     its cards. J3 is back on a fresh team; J1 walks the wind-down.
  8. The first HTML5 drop after a board navigation is swallowed reliably
     enough that `dragCard` retries once (Todo→Active in J4 every run).

  **Design deviations, as built:** `--journey` is comma-separated (angreal
  arguments are not repeatable); the journey record reaches the reporter as
  a test attachment, not annotations; the agent persona joins its team as a
  member instead of holding a board grant; J2's lifecycle step acts on a
  design note (documents carry the editorial lifecycle, A-0018) and links
  the initiative to the strategy in the GUI's Manage links panel; J4's
  badge clears when carol removes the satisfied edge, which is what the
  product actually promises. Follow-ups deferred as listed in the design:
  storage-state login capture for IdPs without a password form, a rendered
  manual checklist from the same narration.
- 2026-09-22 (fix round): Dylan: "make fixes, I need to think through #7".
  T-0123 `3cba74b` (MCP: `## Development` on get_item, delivery board as
  slug, Backlog-only refusal text), T-0124 `7a7ca93` (web: tab hrefs never
  from an empty code, repo description on the team page, Repository select
  in New task; #8 root-caused to Playwright's `dragTo` hovering — and
  scrolling — the target between mousedown and mousemove on an overflowing
  board, so Chromium never starts the drag: fixed in the UAT driver, not
  the GUI), T-0125 `f625fa2` (journeys assert the real behaviour, no
  workarounds). Findings status: #1–#6, #8 **fixed** here; #7 **fixed by KAIROS-I-0012**
  (Dylan, later the same day: "all cards must be archived or moved to
  delete a team"). Recorded runs: compose `muco84aq` 5/5 (0
  skipped), server `muco8qgq` 5/5 (2 skipped); e2e 11/11; integration
  38/38. Initiative left **active** for review.
- 2026-09-22: Dylan: "fluid column across sounds great" → T-0126 `f051dbf`:
  board columns share the lane width (`flex: 1 1 0`, 180px floor,
  border-box), titles wrap, pills wrap under the code; e2e asserts no
  horizontal scroll at 1280px and every e2e drag now uses the scroll-safe
  driver (`e2e/helpers/drag.ts`). Gates: lint, web 72/72, e2e 11/11, UAT
  compose `mudf590b` 5/5. (#7 closed afterwards by KAIROS-I-0012.)
