---
id: m4-gui-playwright-smoke-suite-in
level: task
title: "M4: GUI - Playwright smoke suite in angreal e2e"
short_code: "KAIROS-T-0045"
created_at: 2026-07-10T22:02:51.972351+00:00
updated_at: 2026-07-15T23:16:48.746152+00:00
parent: KAIROS-I-0004
blocked_by: [KAIROS-T-0040, KAIROS-T-0041, KAIROS-T-0042, KAIROS-T-0043, KAIROS-T-0044]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0004
---

# M4: GUI - Playwright smoke suite in angreal e2e

## Parent Initiative

[[KAIROS-I-0004]]

## Objective

The GUI's Playwright smoke tier per A-0012 tier 4, replacing the e2e placeholder's GUI leg.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Playwright suite (package.json confined to an e2e dir; node_modules gitignored): login via Dex → board view renders seeded data → create item → transition via UI → live WS update observed (second context mutation reflects) → item detail edit + 409 path → logout
- [x] Wired into `angreal test e2e` (compose up → seed → API golden path → Playwright headless → down), exit codes propagate
- [x] Runtime + flake posture recorded (retries policy); CI wiring documented (may be a separate nightly job if runtime demands — decide and document)

## Implementation Notes

References A-0012 (decided). Depends on seed-demo (KAIROS-T-0035 builds it — coordinate: if not yet landed, this task builds the minimal seed it needs and T-0035 consumes/extends it; document whichever way it falls).

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created at I-0004 decompose (todo).
- 2026-07-15: Active. Read A-0012 tier 4, task_test.py (golden path + "GUI leg SKIPPED" markers), the five GUI status updates (T-0039..T-0044), gui-conventions.md §8, seed.rs, boards.rs/data.rs, item.rs/editor.rs/api.rs, aurora components.rs, server web.rs (relay), task_web.py, dex config. KEY FACTS confirmed empirically against a scratch stack (scratch DB kairos_t0045, GUI server 127.0.0.1:8080, OIDC_AUDIENCE=kairos-web, KAIROS_SINGLE_TENANT=demo, KAIROS_WEB_DIST=dist): Dex registers ONLY http://localhost:8080/callback for kairos-web → REAL in-browser PKCE, no interception; full headless PKCE mint (verifier/S256 → /dex/auth 302 chain → login form #login/#password/#submit-login → 303 /callback?code → POST /api/auth/token relay → access_token → /api/whoami 200 alice/demo/admin) verified; platform-delivery board id + columns [Backlog,Todo,Blocked,Active,Completed] + transitions (Backlog→Todo, Todo→{Active,Blocked}, …) + seeded task placements (DEMO-T-0002 Active, DEMO-T-0003/0005 Todo, DEMO-T-0006 Backlog) confirmed via API. Aurora renders real <button>/<input>/<textarea> with visible text → text+role selectors are stable; SPA holds token in-memory only (A-0015) so the suite navigates via in-app clicks (never page.goto, which would drop the session).
- 2026-07-15: SHIPPED e2e/ (confined dir at repo root, per .gitignore's "JS (Playwright, e2e tier)" note): package.json + @playwright/test ^1.50 (chromium only), package-lock.json committed, e2e/.gitignore (node_modules/test-results/playwright-report/blob-report), playwright.config.ts (chromium project, workers=1, retries=1, expect timeout 15s, no arbitrary sleeps — trace/screenshot/video retain-on-failure), helpers/auth.ts (mintToken: headless real PKCE against Dex + relay exchange, private cookie jar, Node global fetch — independent of the browser under test), helpers/api.ts (loadPlatformDelivery, pickMovableTask [dynamic → retry-safe], transitionTask, getTask, patchTask), tests/smoke.spec.ts (one serial flow: real Dex login on :8080 → 5 seeded board tiles → open platform-delivery, DEMO-T-0002/0003/0006 in correct columns → create task from Todo column via modal → move via the click-to-move Menu Todo→Active → LIVE WS: mintToken + API transition of another item, asserted the card moved WITHOUT reload via a page-scoped window marker + expect-polling → item detail (in-app nav): edit content + Save → success banner; then a competing API PATCH bumps the version, browser Save → 409 "Edit conflict" dialog naming "server is at v{n+1}", walk Take-theirs merge path (editor adopts server content) → Log out → /login "Sign in"), README.md (run/flake/CI/runtime). WIRED into `angreal test e2e` (.angreal/task_test.py): after the API+MCP golden path, new _run_gui_smoke phase — `angreal web build` (in-process import of task_web.build, the pattern `test all` uses), reseed --force, guarded npm install + `npx playwright install chromium` (clean-checkout safe; node/npm presence checked), boot server on :8080 (kairos-web aud, single-tenant demo, WEB_DIST), `npx playwright test`, exit propagates, GUI server stopped in finally. Replaced BOTH "GUI leg: SKIPPED — pending KAIROS-T-0045" markers: task_test.py ToolDescription prose + crates/kairos-server/examples/e2e_golden_path.rs (println + doc comment now say the GUI leg runs as a separate angreal phase). CI: added .github/workflows/e2e.yml (workflow_dispatch + push tags v*) — decision documented (A-0012: e2e gates releases not per-push; NOT added to the per-push ci.yml gate).
- 2026-07-15: VERIFICATION / EVIDENCE. (AC1, Playwright suite) `npx playwright test` against the scratch :8080 GUI server: 1 passed, ~1.0–1.5s; ran 3× consecutively with reseed between → 3/3 green (no flakes). NEGATIVE CHECK (suite has teeth / fails when server absent): killed the server, `npx playwright test --retries=0` → "1 failed", playwright exit code 1 (confirmed directly, not through a pipe). node_modules gitignored (root + e2e/.gitignore); package-lock committed; node --version v22.22.2. (AC2, wired + exit codes) `angreal test e2e` FULL RUN TWICE GREEN: pass 1 exit=0 runtime≈12s, pass 2 exit=0 runtime≈25s (pass 2 brought compose up from a torn-down state; both show "Building the kairos-web bundle… / Running the Playwright GUI smoke suite… / 1 passed / E2E PASSED (API golden path + MCP + GUI smoke)"); the golden-path runner prints "GUI leg: runs next as a separate angreal phase" and hands off. Exit propagation covered by the negative check + the failing-server behavior. (AC3, runtime + flake + CI) suite runtime ~1s recorded (README + here); retries=1, expect-polling only, no sleeps, single worker, dynamic movable-task pick for retry-safety — all documented in playwright.config.ts + e2e/README.md; CI decision documented (release/tag + manual, not per-push) in e2e.yml + README. SELF-CHECK: only Rust surface touched = examples/e2e_golden_path.rs → `cargo fmt --check` clean (workspace-wide), `cargo clippy -p kairos-server --example e2e_golden_path -- -D warnings` clean; `angreal web lint` clean; kairos-web untouched. SEED-DEMO: KAIROS-T-0035 landed; this task consumes it as-is (no seed changes). SERVICES RESTORED: `angreal services up` → kairos-postgres + kairos-dex healthy (e2e tears the stack down -v at the end; re-upped for the concurrent T-0047 agent). DEVIATION: `angreal test unit`/`integration` not re-run in this task (no product code changed — only the e2e example's two marker lines, covered by the example clippy build; the suite additions are JS/TS + angreal wiring); the full e2e (heavier than unit+integration) ran green twice.