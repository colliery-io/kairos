// KAIROS-T-0490 — the "for me" page at `/` (COLLIERY-I-0624):
//
//   1. after login, `/` is the For me page, and it has the work of alice:
//      her claimed task in the Tasks lane;
//   2. a task of alice with an open blocker is in the Blocked column, and
//      the card shows the blocker;
//   3. a task that alice claims over the API shows with no reload (live
//      refresh on item events);
//   4. a card opens its item; the "For me" link of the top bar comes back.
//
// The tasks are new for each run (timestamped titles), so a retry starts
// clean.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function post(token: string, path: string, body: unknown): Promise<any> {
  const res = await fetch(`${GUI}${path}`, {
    method: 'POST',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`POST ${path} -> ${res.status}: ${await res.text()}`);
  return res.json();
}

/** A new task on the board, moved through `path` (column ids). */
async function task(token: string, boardId: string, title: string, path: string[]): Promise<string> {
  const created = await createTask(GUI, token, { title, boardId });
  const code: string = created.short_code;
  for (const column of path) {
    await post(token, `/api/tasks/${code}/transition`, { to_column_id: column });
  }
  return code;
}

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/', { timeout: 30_000 });
}

test('the For me page shows the work of alice, live', async ({ page }) => {
  const alice = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo');
  const active = board.columnId.get('Active');
  if (!todo || !active) throw new Error('platform-delivery has no Todo or Active column');
  const run = Date.now();

  // alice claims `mine`; `waiting` is hers too, and bob's `blocker` blocks it.
  const mine = await task(alice, board.id, `E2E for me ${run}`, [todo, active]);
  const waiting = await task(alice, board.id, `E2E for me waiting ${run}`, [todo, active]);
  const blocker = await task(bob, board.id, `E2E for me blocker ${run}`, [todo]);
  await post(alice, '/api/relationships', {
    source_short_code: blocker,
    target_short_code: waiting,
    relationship: 'blocks',
  });

  const card = (code: string) => page.locator(`[data-testid="work-card"][data-code="${code}"]`);
  const column = (state: string) =>
    page.locator(`[data-testid="for-me-tasks"] [data-column="${state}"]`);

  await test.step('after login, / is the For me page with the work of alice', async () => {
    await login(page);
    await expect(page.locator('[data-testid="for-me"]')).toBeVisible();
    await expect(column('active').locator(`[data-code="${mine}"]`)).toBeVisible({
      timeout: 30_000,
    });
    await expect(card(mine).locator('[data-testid="work-holder"]')).toContainText('alice');
    await expect(card(mine).locator('[data-testid="work-board"]')).toHaveAttribute(
      'data-board',
      'platform-delivery',
    );
  });

  await test.step('a blocked task is in Blocked, and its card shows the blocker', async () => {
    await expect(column('blocked').locator(`[data-code="${waiting}"]`)).toBeVisible();
    const pill = card(waiting).locator('[data-testid="work-blocker"]');
    await expect(pill).toContainText(`blocked by ${blocker}`);
    await expect(pill).toHaveAttribute('href', `/items/${blocker}`);
  });

  await test.step('a new claim shows with no reload', async () => {
    const later = await task(alice, board.id, `E2E for me later ${run}`, [todo, active]);
    await expect(column('active').locator(`[data-code="${later}"]`)).toBeVisible({
      timeout: 15_000,
    });
  });

  await test.step('a card opens its item, and the top bar link comes back', async () => {
    await card(mine).locator('a.kairos-card__code').click();
    await page.waitForURL(new RegExp(`/items/${mine}$`));
    await page.locator('[data-testid="for-me-link"] a, [data-testid="for-me-link"] button').first().click();
    await page.waitForURL((url) => url.pathname === '/');
    await expect(card(mine)).toBeVisible();
  });
});

// KAIROS-T-0744: a person with no work gets the empty page, which says
// what to do next. carol has no claim and owns no initiative in the seed;
// the spec releases a claim of hers that an earlier spec left.
test('a person with no work gets the empty For me page', async ({ page }) => {
  const carol = await mintToken({ email: 'carol@kairos.test', password: 'carol-password' });
  const work = async () => {
    const res = await fetch(`${GUI}/api/me/work`, { headers: { authorization: `Bearer ${carol}` } });
    if (!res.ok) throw new Error(`GET /api/me/work -> ${res.status}`);
    return res.json();
  };
  for (const task of (await work()).tasks as any[]) {
    await fetch(`${GUI}/api/tasks/${task.short_code}/release`, {
      method: 'POST',
      headers: { authorization: `Bearer ${carol}` },
    });
  }
  const left = await work();
  expect(left.initiatives.length + left.tasks.length, JSON.stringify(left)).toBe(0);

  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'carol@kairos.test');
  await page.fill('#password', 'carol-password');
  await page.click('#submit-login');
  await expect(page.locator('[data-testid="for-me-empty"]')).toContainText(
    'You have no work in Kairos now.',
    { timeout: 30_000 },
  );
});
