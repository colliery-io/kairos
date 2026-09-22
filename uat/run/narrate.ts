// `journey()` and `step()`: how a journey tells its story. Every acceptance
// step is a `test.step` titled "<persona> <narration>" and records what was
// observed; the record travels to the reporter as a test attachment
// (`uat-journey`), which survives whatever happens to the test itself.
import { test, type Browser, type TestInfo } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { Cast, type Persona } from '../personas';
import type { Human } from '../personas/credentials';
import { runContext, type Mode } from './context';
import { Ledger, type TeardownFailure } from './ledger';

export type Observed = Record<string, string | number | boolean | string[] | undefined>;

export interface StepRecord {
  index: number;
  persona: string;
  narration: string;
  status: 'passed' | 'failed' | 'skipped';
  observed?: Observed;
  reason?: string;
  error?: string;
  screenshots: string[];
  durationMs: number;
}

export interface JourneyRecord {
  id: string;
  title: string;
  mode: Mode;
  steps: StepRecord[];
  teardownFailures: TeardownFailure[];
  traces: string[];
}

export const ATTACHMENT = 'uat-journey';

interface Current {
  record: JourneyRecord;
  cast: Cast;
  testInfo: TestInfo;
}

let current: Current | undefined;

export interface JourneyScope {
  cast: Cast;
  ledger: Ledger;
  mode: Mode;
  run: string;
}

export interface JourneyOptions {
  /** Humans whose tokens are minted before any browser opens. */
  humans?: Human[];
}

/**
 * Declare a journey. `id` becomes the `@id` tag `--journey` filters on;
 * `title` is the user story. The body receives the cast and the ledger;
 * teardown runs whatever happens.
 */
export function journey(
  id: string,
  title: string,
  options: JourneyOptions,
  body: (scope: JourneyScope) => Promise<void>,
): void {
  test(`${title} @${id}`, async ({ browser }, testInfo) => {
    const ctx = runContext();
    fs.mkdirSync(ctx.reportDir, { recursive: true });
    const cast = new Cast(browser as Browser);
    const ledger = new Ledger();
    const record: JourneyRecord = {
      id,
      title,
      mode: ctx.mode,
      steps: [],
      teardownFailures: [],
      traces: [],
    };
    current = { record, cast, testInfo };
    let failed = false;
    try {
      await cast.prepare(options.humans ?? []);
      await body({ cast, ledger, mode: ctx.mode, run: ctx.run });
    } catch (err) {
      failed = true;
      throw err;
    } finally {
      record.teardownFailures = await test.step('teardown', () => ledger.teardown());
      record.traces = await cast.closeAll(failed ? id : undefined);
      await testInfo.attach(ATTACHMENT, {
        body: JSON.stringify(record),
        contentType: 'application/json',
      });
      current = undefined;
    }
  });
}

// Playwright colours its expect messages; the report is plain text.
// eslint-disable-next-line no-control-regex
const ANSI = /\u001b\[[0-9;]*m/g;

function plainError(err: any): string {
  return String(err?.message ?? err)
    .replace(ANSI, '')
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .slice(0, 6)
    .join(' ');
}

function requireCurrent(): Current {
  if (!current) throw new Error('step() called outside a journey()');
  return current;
}

async function screenshotAll(record: JourneyRecord, stepIndex: number): Promise<string[]> {
  const { cast } = requireCurrent();
  const ctx = runContext();
  const files: string[] = [];
  for (const persona of cast.all()) {
    const page = persona.openPage;
    if (!page || page.isClosed()) continue;
    const file = path.join(ctx.reportDir, `${record.id}-step${stepIndex}-${persona.name}.png`);
    try {
      await page.screenshot({ path: file, fullPage: true });
      files.push(file);
    } catch {
      /* a page mid-navigation may refuse; the trace still has it */
    }
  }
  return files;
}

/**
 * One acceptance step. `fn` may return an `observed` record — the values a
 * reader needs to believe the step (short codes, column names, states).
 */
export async function step<T extends Observed | void>(
  persona: Persona,
  narration: string,
  fn: () => Promise<T>,
): Promise<T> {
  const { record } = requireCurrent();
  const index = record.steps.length + 1;
  const entry: StepRecord = {
    index,
    persona: persona.name,
    narration,
    status: 'passed',
    screenshots: [],
    durationMs: 0,
  };
  record.steps.push(entry);
  const started = Date.now();
  return test.step(`${persona.name} ${narration}`, async () => {
    try {
      const observed = await fn();
      if (observed) entry.observed = observed as Observed;
      return observed;
    } catch (err: any) {
      entry.status = 'failed';
      entry.error = plainError(err);
      entry.screenshots = await screenshotAll(record, index);
      throw err;
    } finally {
      entry.durationMs = Date.now() - started;
    }
  });
}

/** A step that only makes sense on the compose stack (fresh seed, deployment admin). */
step.composeOnly = async function composeOnly<T extends Observed | void>(
  persona: Persona,
  narration: string,
  reason: string,
  fn: () => Promise<T>,
): Promise<T | undefined> {
  const { record } = requireCurrent();
  if (runContext().mode === 'compose') return step(persona, narration, fn);
  record.steps.push({
    index: record.steps.length + 1,
    persona: persona.name,
    narration,
    status: 'skipped',
    reason,
    screenshots: [],
    durationMs: 0,
  });
  await test.step(`${persona.name} ${narration} (skipped: ${reason})`, async () => {});
  return undefined;
};
