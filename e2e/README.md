# Kairos GUI e2e (Playwright smoke tier)

The KAIROS-A-0012 **tier 4** GUI smoke suite (KAIROS-T-0045). One serial flow
against a compose-backed `kairos-server` serving the Leptos SPA with seed-demo
data, exercising the critical GUI paths end to end:

1. **Real PKCE login** through the Dex login form (alice).
2. **Board list** renders the seeded boards.
3. **platform-delivery** board — seeded items sit in the right columns.
4. **Create** a task from a column via the UI.
5. **Transition** it via the click-to-move menu.
6. **Live WS** — a second writer (API, a separately minted token) transitions
   another item and the first browser sees the card move **without a reload**.
7. **Item detail** — edit content + save, then a competing API `PATCH` forces a
   **409** and we walk one merge path (take theirs).
8. **Logout**.

This directory is the only non-Rust test dependency in the repo, confined to
the e2e tier (KAIROS-A-0012). `node_modules/`, `test-results/`, and
`playwright-report/` are gitignored; `package-lock.json` is committed.

## Running it

Almost always via angreal — it owns the whole stack lifecycle:

```sh
angreal test e2e     # compose up → seed → API+MCP golden path → GUI smoke → down
```

The GUI leg builds the SPA, reseeds, boots a server on **:41080**, and runs this
suite. `:41080` is not arbitrary — it is the **only** `redirect_uri` Dex
registers for the `kairos-web` public client (`.angreal/dex/config.yaml`), so
the browser login is genuine Authorization-Code + PKCE with no interception.

### Hand-running against your own dev server

Point the suite at an already-running GUI server (see
`docs/gui-conventions.md` for the dev-loop env — it must run on :41080 with
`OIDC_AUDIENCE=kairos-web`, `KAIROS_SINGLE_TENANT=demo`, seeded demo data):

```sh
npm install                         # first time only
npx playwright install chromium     # first time only (idempotent)
E2E_GUI_BASE_URL=http://localhost:41080 npx playwright test
```

Env knobs: `E2E_GUI_BASE_URL` (default `http://localhost:41080`), `E2E_ISSUER`
(default `http://localhost:41558/dex`).

## Flake posture

- **`retries: 1`** (`playwright.config.ts`). The flow is inherently serial, so
  a single retry absorbs a rare WS-delivery / scheduling hiccup without masking
  a deterministic regression.
- **No arbitrary sleeps.** Every wait is auto-retrying `expect(...)` polling or
  an explicit `waitForURL` / `waitForSelector`. The WS assertion in particular
  polls the DOM for the moved card and asserts a page-scoped marker survived
  (proving the update arrived over the socket, not via a reload).
- **Retry-safe within a run.** The competing-writer step picks a movable task
  dynamically from the live board state, so a Playwright retry (which re-runs
  against a partially mutated board) still finds a valid move; created items use
  timestamped titles.
- **Single worker** (`workers: 1`) — the suite mutates shared demo-tenant state.

## Runtime

The suite itself runs in **~1 second** (3.8 MB wasm bundle is loaded once; all
services are local). Measured across repeated reseed+run cycles it is stable at
~1.0–1.2 s. The dominant cost of `angreal test e2e` as a whole is the Rust
build + compose lifecycle + wasm build, not this suite.

## CI decision

Per KAIROS-A-0012 the e2e tier **gates releases, not every task**, so it is
**not** in the per-push `ci` gate (`.github/workflows/ci.yml`). It runs:

- locally / at agent completion, on demand (`angreal test e2e`);
- in CI via `.github/workflows/e2e.yml` — **manually** (`workflow_dispatch`)
  and on **release tags** (`push: tags: v*`), so no release is cut without the
  GUI critical paths having been exercised.
