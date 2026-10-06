// COLLIERY-T-3100 and COLLIERY-T-3101 — a move with a rename retires the
// old code, and a retired short code still finds its item and names the
// current code, on the GUI item page.
//
// The journey: alice moves a task from platform-delivery to web-delivery
// with the switch "Give it a code of the new board" (COLLIERY-T-3101). The
// page goes to the page of the new code. Then the page `/items/{old code}`
// goes to the page of the current code (the history entry is replaced) and
// shows a notice that names the two codes.
//
// **Fixture discipline** (see archived.spec.ts): the card is archived at
// the end, so no board shows it to the specs that count cards.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const SERVER = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const BOARD_SLUG = 'platform-delivery';
const TARGET_SLUG = 'web-delivery';

const bearer = (token: string) => ({ authorization: `Bearer ${token}` });

async function api(
  token: string,
  method: string,
  path: string,
  body?: unknown,
): Promise<any> {
  const res = await fetch(SERVER + path, {
    method,
    headers: body
      ? { ...bearer(token), 'content-type': 'application/json' }
      : bearer(token),
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`${method} ${path} -> ${res.status}: ${text}`);
  }
  return text ? JSON.parse(text) : null;
}

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

test('a move with rename retires the old code, and its item page goes to the current code', async ({
  page,
}) => {
  const token = await mintToken();
  const boards = (await api(token, 'GET', '/api/boards?limit=100')).items as any[];
  const board = boards.find((b) => b.slug === BOARD_SLUG);
  if (!board) throw new Error(`no ${BOARD_SLUG} board in the seed`);
  const detail = await api(token, 'GET', `/api/boards/${board.id}`);
  const task = await api(token, 'POST', '/api/tasks', {
    board_id: board.id,
    column_id: detail.columns[0].id,
    title: 'E2E: a task with a new code',
    content: 'The old code still finds me.',
  });
  const oldCode: string = task.short_code;
  expect(oldCode).toMatch(/^[A-Z][A-Z0-9]*-T-\d{4,}$/);
  let newCode = '';

  await test.step('login as alice', () => login(page));

  await test.step('a move with rename gives the task a code of the new board', async () => {
    await page.goto(`/items/${oldCode}`);
    const mover = page.locator('[data-testid="move-board"]');
    await expect(mover).toBeVisible({ timeout: 15_000 });
    await mover.locator('select').selectOption(TARGET_SLUG);
    const rename = mover.getByRole('switch', { name: 'Give it a code of the new board' });
    await expect(rename).toHaveAttribute('aria-checked', 'false');
    await rename.click();
    await expect(rename).toHaveAttribute('aria-checked', 'true');
    await mover.getByRole('button', { name: 'Move board' }).click();
    // The page goes to the page of the new code, with no notice.
    await expect(page).toHaveURL(
      (url) => /^\/items\/WEB-T-\d{4,}$/.test(url.pathname) && !url.searchParams.has('retired'),
      { timeout: 15_000 },
    );
    newCode = new URL(page.url()).pathname.split('/').pop() ?? '';
    expect(newCode).not.toBe(oldCode);
    await expect(page.locator('[data-testid="retired-code-notice"]')).toHaveCount(0);
    const moved = await api(token, 'GET', `/api/tasks/${newCode}`);
    expect(moved.short_code).toBe(newCode);
  });

  await test.step('the old code goes to the page of the current code', async () => {
    await page.goto(`/items/${oldCode}`);
    // A client-side navigation: it fires no "load" event, so wait on the URL.
    await expect(page).toHaveURL(
      (url) => url.pathname === `/items/${newCode}` && url.searchParams.get('retired') === oldCode,
      { timeout: 15_000 },
    );
    const notice = page.locator('[data-testid="retired-code-notice"]');
    await expect(notice).toBeVisible();
    await expect(notice).toContainText(
      `The code ${oldCode} is retired. The current code of this item is ${newCode}.`,
    );
    // KAIROS-T-0323: the page opens in Preview: the content is rendered.
    await expect(page.locator('.kairos-editor .kairos-markdown')).toContainText(
      'The old code still finds me.',
    );
    await expect(page.getByText(newCode, { exact: true }).first()).toBeVisible();
  });

  await test.step('the page of the current code shows no notice', async () => {
    await page.goto(`/items/${newCode}`);
    // KAIROS-T-0323: the page opens in Preview: the content is rendered.
    await expect(page.locator('.kairos-editor .kairos-markdown')).toContainText(
      'The old code still finds me.',
    );
    await expect(page.locator('[data-testid="retired-code-notice"]')).toHaveCount(0);
  });

  // Leave the seeded board as it was found: the card goes away.
  await api(token, 'DELETE', `/api/tasks/${newCode}`);
});

// COLLIERY-T-3104: the owner control of a document has the same switch. A
// document that platform-delivery owns moves to web-delivery with a rename:
// it gets a WEB-D code, and the old code goes to the page of the new code.
test('a document that moves to a new owner board with rename gets a code of that board', async ({
  page,
}) => {
  const token = await mintToken();
  const document = await api(token, 'POST', '/api/documents', {
    title: `E2E: a document with a new code ${Date.now().toString(36)}`,
    content: 'The owner board changes, and the code too.',
    board: BOARD_SLUG,
  });
  const oldCode: string = document.short_code;
  expect(oldCode).toMatch(/^PLATFORM-D-\d{4,}$/);

  await test.step('login as alice', () => login(page));

  let newCode = '';
  await test.step('the owner control moves the document with rename', async () => {
    await page.goto(`/items/${oldCode}`);
    const control = page.locator('[data-testid="owner-board-control"]');
    await expect(control).toBeVisible({ timeout: 15_000 });
    await control.locator('select').selectOption(TARGET_SLUG);
    const rename = control.getByRole('switch', { name: 'Give it a code of the new board' });
    await expect(rename).toHaveAttribute('aria-checked', 'false');
    await rename.click();
    await expect(rename).toHaveAttribute('aria-checked', 'true');
    await control.getByRole('button', { name: 'Set owner board' }).click();
    await expect(page).toHaveURL(
      (url) => /^\/items\/WEB-D-\d{4,}$/.test(url.pathname) && !url.searchParams.has('retired'),
      { timeout: 15_000 },
    );
    newCode = new URL(page.url()).pathname.split('/').pop() ?? '';
    const moved = await api(token, 'GET', `/api/documents/${newCode}`);
    expect(moved.short_code).toBe(newCode);
    expect(moved.content).toBe('The owner board changes, and the code too.');
  });

  await test.step('the old code goes to the page of the current code', async () => {
    await page.goto(`/items/${oldCode}`);
    await expect(page).toHaveURL(
      (url) => url.pathname === `/items/${newCode}` && url.searchParams.get('retired') === oldCode,
      { timeout: 15_000 },
    );
    await expect(page.locator('[data-testid="retired-code-notice"]')).toContainText(
      `The code ${oldCode} is retired. The current code of this item is ${newCode}.`,
    );
  });

  await api(token, 'DELETE', `/api/documents/${newCode}`);
});
