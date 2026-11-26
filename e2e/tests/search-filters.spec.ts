// KAIROS-T-0586 — each filter of the search form narrows the hits
// (KAIROS-T-0583). search-url.spec.ts proves that the filters go into the
// URL and back; archived.spec.ts covers "Include work that has been put
// away". This spec proves that the server gets each filter from the form:
// with one probe word in four new tasks, each filter leaves the tasks that
// match it, and no other.
//
//   A  alice · platform-delivery · Active (claimed by alice) · bug ·
//      payments-api · priority=high
//   B  alice · web-delivery (team web)
//   C  bob · platform-delivery · platform-infra
//   D  alice · platform-delivery · support (the Support lane)
//
// Filters: Board, Column, Team, Repository, Claimed by, Created by, Task
// types, Lanes, a metadata value, Metadata with no value, Created after,
// Changed after; and Initiative buckets with the initiative type and no
// query.
//
// The tasks are new for each run; the spec archives them at the end.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { loadBoard, transitionTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

async function call(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(GUI + path, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${text}`);
  return text ? JSON.parse(text) : null;
}

/** A one-lexeme probe word, fresh per run. */
function probeWord(): string {
  const letters = 'abcdefghijklmnopqrstuvwxyz';
  return `filters${Array.from({ length: 8 }, () => letters[Math.floor(Math.random() * 26)]).join('')}`;
}

const day = (offset: number) => new Date(Date.now() + offset * 86_400_000).toISOString().slice(0, 10);

/** The control of a labelled field. */
const field = (page: Page, label: string, control = 'input') =>
  page
    .locator('.cl-field', { has: page.locator('label', { hasText: label }) })
    .locator(control)
    .first();

/** A chip of one chip row ("Task types", "Lanes"). */
const chip = (page: Page, row: string, label: string) =>
  page
    .locator('.cl-stack', { has: page.getByText(row, { exact: true }) })
    .last()
    .locator('.cl-chip', { hasText: new RegExp(`^${label}$`) });

/** The short codes of the hits on the page, sorted. */
async function hits(page: Page): Promise<string[]> {
  if ((await page.getByText('No matches. Remove a filter or change the query.').count()) > 0) {
    return [];
  }
  const codes = await page.locator('tbody tr td:first-child .cl-mono').allInnerTexts();
  return codes.map((c) => c.trim()).sort();
}

test('search: each filter of the form narrows the hits', async ({ page }) => {
  // One page load; each filter is a search on the same page.
  test.setTimeout(240_000);
  const alice = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const probe = probeWord();
  const platform = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const make = async (token: string, board: string, name: string, extra = {}) =>
    (
      await call(token, 'POST', '/api/tasks', {
        board_id: board,
        title: `E2E ${probe} ${name}`,
          ...extra,
      })
    ).short_code as string;
  const codes: string[] = [];

  try {
    const a = await make(alice, 'platform-delivery', 'A', {
      task_type: 'bug',
      repository: 'payments-api',
    });
    const b = await make(alice, 'web-delivery', 'B');
    const c = await make(bob, 'platform-delivery', 'C', { repository: 'platform-infra' });
    const d = await make(alice, 'platform-delivery', 'D', { task_type: 'support' });
    codes.push(a, b, c, d);
    for (const column of ['Todo', 'Active']) {
      await transitionTask(GUI, alice, a, platform.columnId.get(column)!);
    }
    await call(alice, 'PATCH', `/api/tasks/${a}/metadata`, { values: { priority: 'high' } });
    const all = [a, b, c, d].sort();

    /** Search with the form as it is, and wait for the hits. */
    const search = async (expected: string[], what: string) => {
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      await expect.poll(() => hits(page), { message: what, timeout: 20_000 }).toEqual(
        [...expected].sort(),
      );
    };
    const select = async (label: string, value: string, expected: string[]) => {
      await field(page, label, 'select').selectOption(value);
      await search(expected, `${label} = ${value}`);
      await field(page, label, 'select').selectOption('');
    };
    const chipOnly = async (row: string, label: string, expected: string[]) => {
      await chip(page, row, label).click();
      await search(expected, `${row}: ${label}`);
      await chip(page, row, label).click();
    };
    const date = async (label: string, value: string, expected: string[]) => {
      await field(page, label).fill(value);
      await search(expected, `${label} ${value}`);
      await field(page, label).fill('');
    };

    await test.step('login; the probe word alone finds the four tasks', async () => {
      await login(page);
      await page.goto(`/search?q=${probe}`);
      await expect(field(page, 'Text query')).toHaveValue(probe, { timeout: 30_000 });
      await expect.poll(() => hits(page), { timeout: 20_000 }).toEqual(all);
    });

    await test.step('Board, then Board and Column', async () => {
      await select('Board', 'web-delivery', [b]);
      await field(page, 'Board', 'select').selectOption('platform-delivery');
      await search([a, c, d], 'Board = platform-delivery');
      await field(page, 'Column', 'select').selectOption('Active');
      await search([a], 'Column = Active');
      await field(page, 'Board', 'select').selectOption('');
      await expect(field(page, 'Column', 'select')).toHaveValue('');
      await search(all, 'no board');
    });

    await test.step('Team and Repository', async () => {
      await select('Team', 'web', [b]);
      await select('Repository', 'payments-api', [a]);
      await select('Repository', 'platform-infra', [c]);
    });

    await test.step('Claimed by and Created by', async () => {
      await select('Claimed by', 'me', [a]);
      await select('Created by', 'bob@kairos.test', [c]);
      await select('Created by', 'me', [a, b, d]);
    });

    await test.step('Task types and Lanes', async () => {
      await chipOnly('Task types', 'bug', [a]);
      await chipOnly('Lanes', 'support', [d]);
      await chipOnly('Lanes', 'planned', [a, b, c]);
    });

    await test.step('a metadata value, and Metadata with no value', async () => {
      await page.getByRole('button', { name: '+ add', exact: true }).click();
      await page.locator('input[placeholder="key (definition slug)"]').fill('priority');
      await page.locator('input[placeholder="value (trailing * glob)"]').fill('high');
      await search([a], 'priority = high');
      await page.getByTitle('Remove this metadata filter').click();
      await field(page, 'Metadata with no value').fill('priority');
      await search([b, c, d], 'no priority');
      await field(page, 'Metadata with no value').fill('');
    });

    await test.step('the dates', async () => {
      await date('Created after', day(2), []);
      await date('Created after', day(-2), all);
      await date('Changed before', day(-2), []);
      await date('Changed after', day(-2), all);
      await search(all, 'no filter');
    });

    await test.step('Initiative buckets, with no query', async () => {
      await field(page, 'Text query').fill('');
      await page
        .locator('.cl-chip', { hasText: /^initiative$/ })
        .first()
        .click();
      await field(page, 'Initiative buckets', 'select').selectOption('true');
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      await expect(page).toHaveURL(/is_bucket=true/);
      /** The hits of the page by group: initiatives, buckets among them, the rest. */
      const groups = () =>
        page.locator('tbody tr').evaluateAll((rows) => {
          const out = { initiatives: 0, buckets: 0, other: 0 };
          for (const row of rows) {
            if (!row.querySelector('.cl-mono')) continue;
            const panel = row.closest('.cl-panel');
            const title = panel?.querySelector('.cl-panel__title')?.textContent?.trim();
            if (title !== 'Initiatives') {
              out.other += 1;
              continue;
            }
            out.initiatives += 1;
            const pills = [...row.querySelectorAll('.cl-pill')].map((p) => p.textContent?.trim());
            if (pills.includes('bucket')) out.buckets += 1;
          }
          return out;
        });
      // Each hit is a bucket initiative: the seed has "bugs" and "tech-debt".
      await expect
        .poll(async () => {
          const g = await groups();
          return g.other === 0 && g.initiatives >= 2 && g.buckets === g.initiatives;
        })
        .toBe(true);
      await field(page, 'Initiative buckets', 'select').selectOption('false');
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      await expect(page).toHaveURL(/is_bucket=false/);
      await expect
        .poll(async () => {
          const g = await groups();
          return g.other === 0 && g.initiatives > 0 && g.buckets === 0;
        })
        .toBe(true);
    });
  } finally {
    for (const code of codes) await tryArchiveTask(GUI, alice, code);
  }
});
