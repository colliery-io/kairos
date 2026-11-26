// KAIROS-T-0580 — the writes of the Relationships panel on the item page,
// and the Parent field of the create dialog.
//
// alice is an org admin, so she may edit each end.
//
//   1. the Parent picker of a task searches, and Set parent sets the
//      first initiative
//   2. Set parent again moves the task to the second initiative in one
//      write: the first initiative is not listed any more
//   3. the add form adds a blocks edge to a second task; Remove takes it
//      away again
//   4. Remove parent leaves the task with no parent
//   5. the create dialog of the board sends the Parent field: the new
//      task has the first initiative as its parent
//
// Each item is new for each run (a timestamped title); the spec archives
// them at the end.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const BOARD = 'platform-delivery';

async function api(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(GUI + path, {
    method,
    headers: {
      authorization: `Bearer ${token}`,
      ...(body ? { 'content-type': 'application/json' } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${text}`);
  return text ? JSON.parse(text) : null;
}

/** The short code of the parent of a task, from the server. */
async function parentOf(token: string, code: string): Promise<string | undefined> {
  const body = await api(token, 'GET', `/api/tasks/${code}/relationships`);
  const group = (body.incoming ?? []).find((g: any) => g.relationship === 'parent');
  return group?.items?.[0]?.short_code;
}

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

const panel = (page: Page) =>
  page.locator('.cl-panel', {
    has: page.locator('.cl-panel__title', { hasText: 'Relationships' }),
  });

test('the Relationships panel adds and removes edges and sets the parent', async ({ page }) => {
  const stamp = Date.now();
  const token = await mintToken();
  const initiatives = await loadBoard(GUI, token, (b) => b.slug === 'initiatives');
  const delivery = await loadBoard(GUI, token, (b) => b.slug === BOARD);
  const newInitiative = (title: string) =>
    api(token, 'POST', '/api/initiatives', {
      board_id: initiatives.id,
      column_id: initiatives.entryColumnId,
      title,
    });
  const first = await newInitiative(`Parent one e2e ${stamp}`);
  const second = await newInitiative(`Parent two e2e ${stamp}`);
  const task = await createTask(GUI, token, { title: `Child e2e ${stamp}`, boardId: delivery.id });
  const other = await createTask(GUI, token, { title: `Blocked e2e ${stamp}`, boardId: delivery.id });
  const made: string[] = [task.short_code, other.short_code];

  await login(page);
  await page.goto(`/items/${task.short_code}`);
  const picker = panel(page).getByTestId('parent-picker');

  await test.step('the Parent picker searches, and Set parent sets it', async () => {
    await picker.locator('input.cl-input').fill(`Parent one e2e ${stamp}`);
    await picker
      .getByTestId('code-picker-matches')
      .getByRole('button', { name: new RegExp(first.short_code) })
      .click();
    await expect(picker.locator('input.cl-input')).toHaveValue(first.short_code);
    await picker.getByRole('button', { name: 'Set parent', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: `The parent is now ${first.short_code}.` }))
      .toBeVisible();
    await expect(panel(page).getByRole('link', { name: new RegExp(first.short_code) })).toBeVisible();
    expect(await parentOf(token, task.short_code)).toBe(first.short_code);
  });

  await test.step('Set parent again moves the task in one write', async () => {
    await picker.locator('input.cl-input').fill(second.short_code);
    await picker.getByRole('button', { name: 'Set parent', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: `The parent is now ${second.short_code}.` }))
      .toBeVisible();
    await expect(panel(page).getByRole('link', { name: new RegExp(second.short_code) })).toBeVisible();
    await expect(panel(page).getByRole('link', { name: new RegExp(first.short_code) })).toHaveCount(0);
    expect(await parentOf(token, task.short_code)).toBe(second.short_code);
  });

  await test.step('the add form adds a blocks edge, and Remove takes it away', async () => {
    const add = panel(page).getByTestId('add-relationship');
    await add.locator('select').selectOption({ label: 'blocks' });
    await add.locator('input.cl-input').fill(other.short_code);
    await add.getByRole('button', { name: 'Add relationship', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: 'Kairos added the edge' })).toBeVisible();
    const row = panel(page).getByRole('link', { name: new RegExp(other.short_code) });
    await expect(row).toBeVisible();
    await panel(page)
      .getByRole('button', { name: `Remove blocks ${other.short_code}`, exact: true })
      .click();
    await expect(page.locator('.cl-toast', { hasText: 'Kairos removed the edge' })).toBeVisible();
    await expect(row).toHaveCount(0);
  });

  await test.step('Remove parent leaves the task with no parent', async () => {
    await picker.getByRole('button', { name: 'Remove parent', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: 'The item has no parent now.' })).toBeVisible();
    expect(await parentOf(token, task.short_code)).toBeUndefined();
  });

  await test.step('the create dialog sends the Parent field', async () => {
    const title = `Created child e2e ${stamp}`;
    await page.goto(`/boards/${BOARD}`);
    await page.getByRole('button', { name: 'New task', exact: true }).click();
    const modal = page.locator('.cl-modal');
    await modal.locator('input.cl-input').first().fill(title);
    await modal.getByTestId('create-parent').locator('input.cl-input').fill(first.short_code);
    await modal.getByRole('button', { name: 'Create', exact: true }).click();
    await expect(modal).toBeHidden();
    const found = await api(token, 'POST', '/api/search', {
      q: title,
      filter: { entity_type: ['task'] },
    });
    const created = found.results.tasks.find((t: any) => t.title === title);
    expect(created).toBeTruthy();
    made.push(created.short_code);
    expect(await parentOf(token, created.short_code)).toBe(first.short_code);
  });

  // Put the items away: the seeded boards stay as they were found.
  for (const code of made) await api(token, 'DELETE', `/api/tasks/${code}`);
  for (const code of [first.short_code, second.short_code]) {
    await api(token, 'DELETE', `/api/initiatives/${code}`);
  }
});
