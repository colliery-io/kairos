// KAIROS-I-0011 — the UAT tier's Playwright configuration.
//
// Journeys are stories told once: serial, one worker, NO retries (a retry
// would narrate a second run into the same report). Every wait is
// expect-polling or an explicit waitFor; there are no sleeps.
import { defineConfig, devices } from '@playwright/test';
import { runContext } from './run/context';

const ctx = runContext();

export default defineConfig({
  testDir: './journeys',
  testMatch: /.*\.journey\.ts/,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  // One journey crosses several surfaces and up to three real Dex logins;
  // 4 minutes is a ceiling, not a target. Assertions poll under 15s.
  timeout: 240_000,
  expect: { timeout: 15_000 },
  reporter: [['list'], ['./run/reporter.ts']],
  outputDir: ctx.reportDir + '/playwright',
  use: {
    baseURL: ctx.server,
    headless: !ctx.headed,
    actionTimeout: 15_000,
    navigationTimeout: 30_000,
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
