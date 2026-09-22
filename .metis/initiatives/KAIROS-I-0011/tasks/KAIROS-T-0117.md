---
id: uat-harness-uat-package-personas
level: task
title: "UAT harness: uat/ package, personas + surfaces, ledger, narrated steps, report reporter, angreal test uat (compose + --server)"
short_code: "KAIROS-T-0117"
created_at: 2026-09-22T11:15:16.902924+00:00
updated_at: 2026-09-22T11:43:54.874951+00:00
parent: KAIROS-I-0011
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# UAT harness: uat/ package, personas + surfaces, ledger, narrated steps, report reporter, angreal test uat (compose + --server)

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Stand up the sixth test tier's machinery (I-0011 D0–D3, D5, part of D1): a `uat/` Playwright package with persona-scoped surfaces (GUI, API, MCP, CLI, forge), a create-ledger with reverse-order teardown, the `step(persona, narration, fn)` narrator, a reporter that renders `uat/reports/<run>/report.md` + `report.json`, and `angreal test uat` in both compose and `--server` modes — proven by a one-step smoke journey that logs in on every surface and renders a report.

## Implementation Notes

### Technical Approach

- `uat/`: `package.json` (`@playwright/test` pinned to the same version as `e2e/`, `typescript`), `tsconfig.json` (strict), `playwright.config.ts` (testDir `journeys`, testMatch `*.journey.ts`, serial, `workers: 1`, **`retries: 0`**, timeout 180s/expect 15s, trace/screenshot/video retain-on-failure, `reporter: [['list'], ['./run/reporter.ts']]`, `baseURL` from `UAT_SERVER`), `.gitignore` (`node_modules`, `reports`, `test-results`, `.auth`).
- `personas/credentials.ts`: `UAT_SERVER` (default `http://localhost:41080`), `UAT_ISSUER` (default the compose Dex), `UAT_TENANT` (default `demo`), `UAT_MODE` (`compose`|`server`, set by the angreal task), `UAT_PERSONA_<NAME>_EMAIL/_PASSWORD` with the seed defaults for alice/bob/carol. `personas/index.ts`: `persona(name)` → `{ name, role, token(), gui(), api, mcp(), cli }`; tokens minted once per persona per run via the headless PKCE walk (port `e2e/helpers/auth.ts`); **mint every human token before any browser login** (Dex keeps one refresh token per user+client — the e2e lesson). `agent` persona is constructed from an API key (`persona.fromApiKey('agent', key)`).
- `surfaces/gui.ts`: `login(page, persona)`, `openBoard(page, slug)`, `openItem(page, code)`, `openTeam(page, slug)`, `dragCard(page, code, toColumn)` (reuse drag.spec's approach), `expectCardInColumn`, `expectBadge…` — role/text selectors first, the stable `.kairos-*`/`.cl-*` classes only where text is ambiguous. `surfaces/api.ts`: typed wrappers over the public REST paths the journeys need (teams, members, repositories, tasks, transitions, relationships, service accounts + keys, forge connections, admin tenants, search, whoami). `surfaces/mcp.ts`: streamable-HTTP session (`initialize` → `mcp-session-id`, `X-Tenant`, JSON or SSE `data:` framing as in `examples/e2e_golden_path.rs`), `callTool(name, args)` returning the text, plus small parsers for the lines the plugin skills read (`repository: <slug> (owner: <team>)`, `[repo:<slug>]`, short codes). `surfaces/cli.ts`: `execFile` of `target/debug/kairos` (path from `UAT_KAIROS_BIN`) with `KAIROS_CONFIG_DIR=<tmp>/<persona>`; a `login(token)` that writes the config the way `kairos auth` does (read `crates/kairos-cli/src/config.rs` for the file shape — do not shell out to an interactive login); `run(args)` → `{ stdout, stderr, code }` and a `json(args)` variant when the CLI supports `--json` (check; else parse text). `surfaces/forge.ts`: port `createForgeConnection`, `deliverGithubWebhook`, `githubPullRequest` from `e2e/helpers/api.ts`.
- `run/ledger.ts`: `ledger.add({ kind, label, delete: () => Promise<void> })`; `ledger.teardown()` runs deletes in reverse insertion order, collects failures, and hands them to the reporter (non-fatal). Kinds and order documented in the file header (keys → service accounts → forge connections → tasks → initiatives → strategies → repositories → teams → tenants).
- `run/narrate.ts`: `step(persona, narration, fn)` → `test.step(`${persona.name} ${narration}`, …)`; `fn` may return an `observed` record; the narrator stashes `{ persona, narration, observed }` on a per-test annotation the reporter reads (Playwright `test.info().annotations` with a JSON payload — verify this survives to `onStepEnd`/`onTestEnd`; if not, keep a module-level registry keyed by test id). `step.composeOnly(...)` skips with an annotation under `UAT_MODE=server`. `journey(id, title, fn)` wraps `test()` with the id as a tag (`@onboarding`) so `--journey` maps to `--grep`.
- `run/reporter.ts`: implements `Reporter`; `onEnd` writes `report.md` per I-0011 D5 (header: run id, target, mode, result line; per journey a table `# | Persona | Step | Observed | Status`, skipped rows say why, failed rows link copied screenshots/traces) and `report.json`; prints the result line + path. Report dir from `UAT_REPORT_DIR` (default `uat/reports/<run>`).
- `journeys/smoke.journey.ts` (`@smoke`): alice logs into the GUI and sees the boards list; alice's CLI `kairos whoami` names her; alice's MCP `whoami` returns; forge helper is not exercised. It exists to prove the harness and is kept (cheap).
- `.angreal/task_test.py`: `uat` command per I-0011 D1 — `--server`, `--journey` (repeatable → `--grep @id|@id`), `--keep-running`, `--headed`, `--report-dir`. Compose mode: reuse the e2e GUI-leg helpers (`docker_up`, build server **and `kairos` CLI** and `angreal web build`, `seed-demo --force`, boot on `:41080` with `KAIROS_SINGLE_TENANT=demo`, `OIDC_AUDIENCE=kairos-web`, `KAIROS_WEB_DIST`, and `KAIROS_DEPLOYMENT_ADMINS=<alice's Dex sub>` — find alice's `sub` in the Dex static config or seed and pass it), `_ensure_playwright`-style guard for `uat/node_modules` + chromium, run `npx playwright test`, teardown unless `--keep-running`. Server mode: skip lifecycle, require `--server`, set `UAT_MODE=server`. `ToolDescription` per D1 (`risk_level="destructive"`, states `--server` is non-destructive to pre-existing data). Factor the shared boot code out of `_run_gui_smoke` rather than copy it.
- `uat/README.md`: how to run both modes, the env vars, journey-writing conventions (narrate every acceptance step, ledger every create, no testids, no sleeps, compose-only marking).

### Dependencies

None (first task). Reads `e2e/helpers/*.ts`, `.angreal/task_test.py`, `crates/kairos-cli/src/config.rs`, `crates/kairos-server/examples/e2e_golden_path.rs` (MCP framing).

### Risk Considerations

- Annotation plumbing to the reporter: verify early with the smoke journey; fall back to a module registry.
- `kairos` CLI config shape: read the source, write the file; never prompt.
- Port `41080` is the only Dex redirect_uri for `kairos-web`; `--server` deployments must register their own (documented).

## Acceptance Criteria

## Acceptance Criteria

- [x] `angreal test uat --journey smoke` boots compose, runs the smoke journey green, writes `uat/reports/<run>/report.md` + `.json` (four rows: GUI / CLI / MCP / compose-only admin), tears down — run `muclufwa`.
- [x] `--keep-running` then `--server http://localhost:41080 --journey smoke` passed in server mode with the compose-only step reported "skipped: needs a deployment-admin token" — run `muclm8d2`.
- [x] A deliberately failing journey (temporary file) exited 1 ("UAT FAILED at phase: UAT: playwright test"), rendered a ❌ row with the plain-text expect message, `failing-step2-alice.png` and `failing-alice.trace.zip` in the run dir — run `muclmg0d`.
- [x] `npx tsc --noEmit` clean; `uat/README.md` written; `angreal tree` lists `test uat`; `angreal test e2e` still green (11/11 + golden path) on the shared boot helpers.

## Status Updates

**2026-09-22** — Completed in `6d8b3a3`.

- Built as designed (I-0011 D0–D3, D5, D1). Deviations: `--journey` is comma-separated (angreal arguments are not repeatable) and maps to Playwright `--grep "@a$|@b$"`; the journey record reaches the reporter as a test attachment (`uat-journey`) rather than annotations — attachments are guaranteed on `TestResult`; per-persona traces are started on each context and kept only when the journey fails; a kept server (`--keep-running`) logs to `target/uat-server.log` because an inherited stdout pipe kept the calling shell open.
- The CLI has no API-key login mode; the agent persona's CLI gets a credential store whose `access_token` IS the key (the server accepts `kairos_sk_` keys as bearer), `expires_at` 6h out, no refresh token.
- Cosmetic: Playwright's list reporter attributes every journey to `run/narrate.ts:69` (the `test()` call site); the UAT report is the readable artefact, so left as is.
- Gates: tsc clean; `angreal test uat --journey smoke` compose + server modes; failure path; `angreal test e2e` 11/11.