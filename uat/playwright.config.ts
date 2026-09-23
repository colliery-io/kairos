// KAIROS-I-0011 — the UAT tier's Playwright configuration.
//
// Journeys are stories told once: serial, one worker, NO retries (a retry
// would narrate a second run into the same report). Every wait is
// expect-polling or an explicit waitFor; there are no sleeps.
import { defineConfig, devices } from '@playwright/test';
import { runContext } from './run/context';

const ctx = runContext();

export default defineConfig({
  // Journeys, then the checks/ gate — its `zz-` name sorts it last, so it
  // sees everything the journeys recorded (KAIROS-I-0013).
  testDir: '.',
  testMatch: [/journeys\/.*\.journey\.ts/, /checks\/.*\.check\.ts/],
  testIgnore: [/node_modules/, /reports/, /test-results/],
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
  // Two projects, not one, purely for ORDER: the coverage gate reads what
  // the journeys recorded, so it must run after all of them. File-name
  // ordering cannot express that (`checks/` sorts before `journeys/`), a
  // project dependency can.
  projects: [
    {
      name: 'journeys',
      testMatch: /journeys\/.*\.journey\.ts/,
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'checks',
      testMatch: /checks\/.*\.check\.ts/,
      dependencies: ['journeys'],
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
