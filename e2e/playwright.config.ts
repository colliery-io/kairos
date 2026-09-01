import { defineConfig, devices } from '@playwright/test';

// The GUI is served by a compose-backed kairos-server on :8080 (the ONLY
// redirect_uri Dex registers for the `kairos-web` public client, so real
// in-browser PKCE works with no interception). `angreal test e2e` boots it;
// override for a hand-run dev server via E2E_GUI_BASE_URL.
const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

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
  ],
});
