// KAIROS-T-0744 — the watch panel of the item page (KAIROS-T-0736):
//
//   1. a task that alice does not watch says so, with "Watch" and "Watch
//      with children";
//   2. Watch: the state says that she watches it; bob moves the task, and
//      the bell of alice gets a notification of the kind `watch`, live;
//   3. Watch with children, then Stop watching: the state follows;
//   4. an archived item takes no new watch: alice watches the task, it is
//      archived, and only "Stop watching" shows.
//
// The task is new for each run (a timestamped title), so a retry starts
// clean.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function login(page: Page, path: string): Promise<void> {
  await page.goto(path);
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => !url.pathname.startsWith('/callback') && !url.pathname.includes('dex'), {
    timeout: 30_000,
  });
  await page.goto(path);
}

async function call(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(`${GUI}${path}`, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${await res.text()}`);
  return res.status === 204 ? undefined : res.json();
}

test('the watch panel watches an item and the bell tells of a move', async ({ page }) => {
  const alice = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo');
  if (!todo) throw new Error('platform-delivery has no Todo column');
  const created = await createTask(GUI, alice, { title: `E2E watch ${Date.now()}`, boardId: board.id });
  const code: string = created.short_code;

  const panel = page.locator('[data-testid="watch-panel"]');
  const state = panel.locator('[data-testid="watch-state"]');

  await test.step('a task that alice does not watch', async () => {
    await login(page, `/items/${code}`);
    await expect(state).toHaveText('You do not watch this item.', { timeout: 30_000 });
    await expect(panel.locator('[data-testid="watch-button"]')).toBeVisible();
    await expect(panel.locator('[data-testid="watch-children-button"]')).toBeVisible();
    await expect(panel.locator('[data-testid="unwatch-button"]')).toHaveCount(0);
  });

  await test.step('Watch, and a move of bob shows in the bell', async () => {
    await panel.locator('[data-testid="watch-button"]').getByRole('button').click();
    await expect(state).toContainText('You watch this item.');
    await expect(panel.locator('[data-testid="watch-button"]')).toHaveCount(0);
    expect((await call(alice, 'GET', `/api/items/${code}/watch`)).watching).toBe(true);

    await call(bob, 'POST', `/api/tasks/${code}/transition`, { to_column_id: todo });
    const bell = page.locator('[data-testid="notification-bell"]');
    await expect(bell.locator('[data-testid="notification-count"]')).toBeVisible({ timeout: 15_000 });
    await bell.getByRole('button', { name: 'Notifications' }).click();
    const row = page.locator('[data-testid="notification-drawer"] [data-testid="notification"][data-kind="watch"]', {
      hasText: code,
    });
    await expect(row.first()).toBeVisible({ timeout: 15_000 });
    await expect(row.first()).toContainText(`bob moved ${code}`);
    // Read it, so that the bell of alice is as the next spec expects.
    await row.first().getByRole('button', { name: 'Mark read', exact: true }).click();
    await expect(row.first()).toHaveAttribute('data-unread', 'false');
    await page.keyboard.press('Escape');
  });

  await test.step('Watch with children, then Stop watching', async () => {
    await page.goto(`/items/${code}`);
    await panel.locator('[data-testid="watch-children-button"]').getByRole('button').click();
    await expect(state).toContainText('You watch this item and its children.');
    await expect(panel.locator('[data-testid="watch-children-button"]')).toHaveCount(0);
    await expect(panel.locator('[data-testid="watch-button"]')).toBeVisible();
    await panel.locator('[data-testid="unwatch-button"]').getByRole('button').click();
    await expect(state).toHaveText('You do not watch this item.');
    expect((await call(alice, 'GET', `/api/items/${code}/watch`)).watching).toBe(false);
  });

  await test.step('an archived item takes no new watch', async () => {
    await call(alice, 'PUT', `/api/items/${code}/watch`, { with_children: false });
    expect(await tryArchiveTask(GUI, alice, code)).toBeLessThan(300);
    await page.goto(`/items/${code}`);
    await expect(state).toContainText('You watch this item.', { timeout: 30_000 });
    await expect(panel.locator('[data-testid="unwatch-button"]')).toBeVisible();
    await expect(panel.locator('[data-testid="watch-button"]')).toHaveCount(0);
    await expect(panel.locator('[data-testid="watch-children-button"]')).toHaveCount(0);
  });
});
