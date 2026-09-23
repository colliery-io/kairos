// The drift gate (KAIROS-I-0013 D1): every MCP tool and every CLI noun
// the deployment offers must have been exercised by some journey, or be
// allow-listed with a reason.
//
// Named `zz-` so the serial runner takes it last, after every journey has
// recorded what it did. It asks the DEPLOYMENT what exists (`tools/list`,
// `kairos --help`) rather than hard-coding a vocabulary, so a surface
// added to the product shows up here the day it ships — which is the
// whole point: the UAT suite fell behind twice before this existed.
import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Cast } from '../personas';
import { runContext } from '../run/context';
import { surfaceUsage } from '../run/coverage';
import { COVERAGE } from '../run/reporter';

/**
 * Surfaces no journey exercises, each with the reason.
 *
 * Empty, and that is the bar: KAIROS-I-0013 gave every MCP tool and every
 * CLI noun a persona who would really use it, rather than a paragraph
 * explaining why nobody does. An entry here is a claim you are willing to
 * defend — "pending <ticket>" while one is open, a real reason otherwise.
 * The gate also fails on STALE entries, so this map cannot rot quietly.
 */
const ALLOW: Record<string, string> = {};

const HERE = path.dirname(fileURLToPath(import.meta.url));

/** The journey ids the suite HAS, from the files on disk. */
function allJourneyIds(): string[] {
  return fs
    .readdirSync(path.join(HERE, '..', 'journeys'))
    .filter((f) => f.endsWith('.journey.ts'))
    .map((f) => f.replace('.journey.ts', ''))
    .sort();
}

test('every MCP tool and CLI noun is exercised by a journey, or allow-listed with a reason', async ({
  browser,
}, testInfo) => {
  const used = surfaceUsage();
  const expected = allJourneyIds();
  const missing = expected.filter((id) => !used.journeys.includes(id));
  // A filtered run (`--journey planning`) cannot speak for the product.
  test.skip(
    missing.length > 0,
    `filtered run: coverage needs every journey, missing [${missing.join(', ')}]`,
  );
  // Neither can a deployment run: `step.composeOnly` steps are skipped
  // there by design (tenant provisioning needs a throwaway tenant and a
  // deployment-admin token), so surfaces only those steps reach — today
  // `kairos admin` — would read as uncovered when they are simply not
  // applicable. Coverage is a property of the SUITE, measured where the
  // suite runs whole.
  test.skip(
    runContext().mode !== 'compose',
    'deployment run: compose-only steps are skipped, so coverage is not measurable here',
  );

  const cast = new Cast(browser);
  const alice = cast.human('alice');
  const [tools, nouns] = await Promise.all([
    (await alice.mcp()).listTools(),
    (await alice.cli()).nouns(),
  ]);

  const uncovered: string[] = [];
  const allowed: string[] = [];
  for (const [kind, offered, exercised] of [
    ['mcp', tools, used.mcp],
    ['cli', nouns, used.cli],
  ] as const) {
    for (const name of offered) {
      if (exercised.includes(name)) continue;
      const key = `${kind}:${name}`;
      if (key in ALLOW) allowed.push(`${key} — ${ALLOW[key]}`);
      else uncovered.push(key);
    }
  }
  // An ALLOW entry for something that IS covered (or no longer exists) is
  // stale bookkeeping; say so rather than let the map rot.
  const stale = Object.keys(ALLOW).filter((key) => {
    const [kind, name] = key.split(':');
    const offered = kind === 'mcp' ? tools : nouns;
    const exercised = kind === 'mcp' ? used.mcp : used.cli;
    return !offered.includes(name) || exercised.includes(name);
  });

  await testInfo.attach(COVERAGE, {
    body: JSON.stringify({
      mcp: { offered: tools.length, exercised: used.mcp.length },
      cli: { offered: nouns.length, exercised: used.cli.length },
      allowed,
      uncovered,
      stale,
    }),
    contentType: 'application/json',
  });

  expect(
    uncovered,
    `these surfaces exist but no journey exercises them — give one a step, or add an ALLOW entry saying why not:\n  ${uncovered.join('\n  ')}`,
  ).toEqual([]);
  expect(
    stale,
    `these ALLOW entries are stale (covered now, or gone from the product) — delete them:\n  ${stale.join('\n  ')}`,
  ).toEqual([]);
});
