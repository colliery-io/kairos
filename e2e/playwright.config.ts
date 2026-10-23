import { defineConfig, devices } from '@playwright/test';

// The GUI is served by a compose-backed kairos-server on :8080 (the ONLY
// redirect_uri Dex registers for the `kairos-web` public client, so real
// in-browser PKCE works with no interception). `angreal test e2e` boots it;
// override for a hand-run dev server via E2E_GUI_BASE_URL.
const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

// KAIROS-T-0409: the sandbox of the uploaded sites is proved in each engine,
// so site-sandbox.spec.ts also runs in Firefox and WebKit. Those two browsers
// and their system libraries are a large install (#276: a slow apt mirror
// spent the whole 30 minutes of a queue shard on them), so they run only
// with KAIROS_E2E_ENGINES=all: the release-tag run of e2e.yml, or by hand.
// The merge queue runs chromium only. `angreal test e2e` installs the same
// set of browsers.
const ALL_ENGINES = process.env.KAIROS_E2E_ENGINES === 'all';
const sandboxOnly = /site-sandbox\.spec\.ts/;

export default defineConfig({
  testDir: './tests',
  // The smoke flow is inherently serial (login → mutate → observe a live WS
  // update → logout), so a single worker, no cross-test parallelism.
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  // Flake posture (KAIROS-T-0045): retries = 1. The suite has NO arbitrary
  // sleeps — every wait is expect-polling (auto-retrying assertions) or an
  // explicit waitForURL/waitForSelector, so a retry only ever absorbs a rare
  // scheduling/WS-delivery hiccup, never masks a deterministic regression.
  retries: 1,
  reporter: [['list']],
  // A whole-run ceiling generous enough for two real Dex logins + a WS
  // round-trip on a cold machine; individual assertions poll under 15s.
  timeout: 90_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: GUI,
    headless: true,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    actionTimeout: 15_000,
    navigationTimeout: 30_000,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    ...(ALL_ENGINES
      ? [
          { name: 'firefox', use: { ...devices['Desktop Firefox'] }, testMatch: sandboxOnly },
          { name: 'webkit', use: { ...devices['Desktop Safari'] }, testMatch: sandboxOnly },
        ]
      : []),
  ],
});
