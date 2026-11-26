// KAIROS-T-0584 — the pages of four lists, and the pickers of the activity
// page:
//
//   1. the item history page pages its versions: 50 a page, and the pager
//      gives the rest;
//   2. the delivery log of a board has Older and Newer: 50 deliveries a
//      page;
//   3. the Notifications page has Show older: after the 20 newest read
//      notifications, the next page of the read ones;
//   4. the activity page: the item picker finds an item by the words of its
//      title, and a click puts the code in the field; "Since" is a date
//      field.
//
// Each run makes its own items, with a timestamped title, so a retry starts
// clean. The pager of the code index runs has host tests only: an e2e
// stack does not build 6 runs of one repository in the time of a spec.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, patchTask } from '../helpers/api';

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

async function send(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(`${GUI}${path}`, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${await res.text()}`);
  return res.status === 204 ? null : res.json();
}

test('the history page pages its versions', async ({ page }) => {
  const alice = await mintToken();
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const title = `E2E history pages ${Date.now()}`;
  const task = await createTask(GUI, alice, { title, boardId: board.id, content: 'v1' });
  let version: number = task.version;
  // KAIROS-T-0792: version 1 is the template and version 2 is "v1". 54 more
  // versions: 56 in all, so the pager has 2 pages.
  for (let n = 2; n <= 55; n++) {
    version = await patchTask(GUI, alice, task.short_code, {
      title,
      content: `v${n}`,
      version,
    });
  }

  await login(page);
  await page.goto(`/activity/history/${task.short_code}`);
  const pager = page.locator('[data-testid="history-pager"]');
  await expect(pager).toBeVisible({ timeout: 30_000 });
  await expect(pager.getByText('The list shows items 1 to 50 of 56.')).toBeVisible();
  await pager.getByRole('button', { name: 'Next' }).click();
  await expect(pager.getByText('The list shows items 51 to 56 of 56.')).toBeVisible();
  await expect(page.getByText('v1', { exact: true })).toBeVisible();
});

test('the delivery log has Older and Newer', async ({ page }) => {
  const alice = await mintToken();
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const name = `E2E log pages ${Date.now()}`;
  const made = await send(alice, 'POST', `/api/boards/${board.id}/webhook-targets`, {
    name,
    url: 'http://127.0.0.1:9/hook',
  });
  const target = made.target.id as string;
  try {
    // 51 test deliveries: one more than a page.
    for (let n = 0; n < 51; n++) {
      await send(alice, 'POST', `/api/boards/${board.id}/webhook-targets/${target}/test`);
    }
    await login(page);
    await page.goto(`/admin/boards/${board.id}`);
    const pager = page.locator('[data-testid="delivery-log-pager"]');
    await expect(pager).toBeVisible({ timeout: 30_000 });
    await expect(pager.getByText('Page 1')).toBeVisible();
    await expect(pager.getByRole('button', { name: 'Newer' })).toBeDisabled();
    await pager.getByRole('button', { name: 'Older' }).click();
    await expect(pager.getByText('Page 2')).toBeVisible();
    await pager.getByRole('button', { name: 'Newer' }).click();
    await expect(pager.getByText('Page 1')).toBeVisible();
  } finally {
    await send(alice, 'DELETE', `/api/boards/${board.id}/webhook-targets/${target}`);
  }
});

test('the Notifications page shows older read notifications', async ({ page }) => {
  const alice = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const board = await loadBoard(GUI, bob, (b) => b.slug === 'platform-delivery');
  // The demo seed sets the user name of a person to the subject of the
  // login (KAIROS-T-0184), so "@alice" names no one. The token has it.
  const sub = JSON.parse(Buffer.from(alice.split('.')[1], 'base64url').toString()).sub;
  // 22 mentions of alice, then alice reads them: more than 20 read ones.
  for (let n = 0; n < 22; n++) {
    await createTask(GUI, bob, {
      title: `E2E notification pages ${Date.now()} ${n}`,
      boardId: board.id,
      content: `Ask @${sub}.`,
    });
  }
  const list = await send(alice, 'GET', '/api/me/notifications');
  const newest = list.items.find((n: any) => n.read_at === null);
  if (newest) {
    await send(alice, 'POST', '/api/me/notifications/read', { all: true, up_to: newest.id });
  }

  await login(page);
  await page.goto('/notifications');
  const rows = page.locator('[data-testid="notification"]');
  await expect(rows.first()).toBeVisible({ timeout: 30_000 });
  const first = await rows.count();
  const older = page.locator('[data-testid="notifications-older"]');
  await expect(older).toBeVisible();
  await older.click();
  await expect.poll(() => rows.count()).toBeGreaterThan(first);
});

test('the activity page picks an item by its words, and Since is a date', async ({ page }) => {
  const alice = await mintToken();
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const words = `picker${Date.now()}`;
  const task = await createTask(GUI, alice, { title: `E2E ${words}`, boardId: board.id });

  await login(page);
  await page.goto('/activity');
  const item = page.getByLabel('Item (code or words)');
  await item.fill(words);
  const picks = page.locator('[data-testid="activity-item-picks"]');
  await expect(picks).toBeVisible({ timeout: 30_000 });
  await picks.locator(`[data-code="${task.short_code}"]`).click();
  await expect(item).toHaveValue(task.short_code);
  await page.getByLabel('Since (UTC)').fill('2026-01-01');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  await expect(page.getByRole('link', { name: task.short_code }).first()).toBeVisible();
});
