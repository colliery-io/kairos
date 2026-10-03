// COLLIERY-T-3104 — an admin creates a board with a short-code prefix in
// the admin GUI, and a bad prefix is refused with its field named.
//
//   1. REAL PKCE login (alice, org admin), open /admin/boards
//   2. a prefix that does not match the rule: the refusal shows below the
//      field Prefix, and the server writes nothing
//   3. a correct prefix: the board is made, and the list of boards shows
//      its prefix
//   4. an initiative on the new board gets a code with the prefix
//
// **Fixture discipline:** the spec archives the initiative and deletes the
// board at the end, so the specs that count boards find what they expect.
// A per-run suffix gives a retry a new slug and a new prefix.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = (Date.now() % 1_000_000).toString().padStart(6, '0');
const PREFIX = `PX${RUN}`;
const SLUG = `prefix-e2e-${RUN}`;
const NAME = `Prefix E2E ${RUN}`;

const panel = (page: Page, title: string): Locator =>
  page.locator('.cl-panel', {
    has: page.locator('.cl-panel__title', { hasText: title }),
  });

const field = (scope: Locator, label: string): Locator =>
  scope.locator('.cl-field', {
    has: scope.page().locator('.cl-field__label', { hasText: label }),
  });

test('board prefix: the admin GUI creates a board with a prefix and refuses a bad prefix', async ({
  page,
}) => {
  // The API token FIRST (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const api = async (method: string, path: string, body?: unknown): Promise<any> => {
    const res = await fetch(GUI + path, {
      method,
      headers: body
        ? { authorization: `Bearer ${alice}`, 'content-type': 'application/json' }
        : { authorization: `Bearer ${alice}` },
      body: body ? JSON.stringify(body) : undefined,
    });
    const text = await res.text();
    if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${text}`);
    return text ? JSON.parse(text) : null;
  };
  const boardSlugs = async (): Promise<string[]> =>
    ((await api('GET', '/api/boards?limit=200')).items as any[]).map((b) => b.slug).sort();
  const before = await boardSlugs();

  await test.step('login via Dex as alice, open /admin/boards', async () => {
    await page.goto('/');
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'alice@kairos.test');
    await page.fill('#password', 'alice-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), { timeout: 30_000 });
    await page.evaluate(() => {
      window.history.pushState({}, '', '/admin/boards');
      window.dispatchEvent(new PopStateEvent('popstate'));
    });
    await expect(panel(page, 'Create board')).toBeVisible({ timeout: 30_000 });
  });

  const form = panel(page, 'Create board');
  await test.step('a bad prefix is refused below the field Prefix', async () => {
    await field(form, 'Name').locator('input').fill(NAME);
    await field(form, 'Slug').locator('input').fill(SLUG);
    await field(form, 'Prefix').locator('input').fill('px-1');
    await form.getByRole('button', { name: 'Create board' }).click();
    await expect(field(form, 'Prefix').locator('.cl-field__error')).toHaveText(
      'The prefix "px-1" is not correct. A board prefix must match ^[A-Z][A-Z0-9]{1,9}$: a ' +
        'capital letter, then 1 to 9 capital letters or digits. Send a different code_prefix.',
      { timeout: 10_000 },
    );
    // The other fields have no refusal, and the form keeps the values.
    await expect(field(form, 'Slug').locator('.cl-field__error')).toHaveCount(0);
    await expect(field(form, 'Name').locator('input')).toHaveValue(NAME);
    expect(await boardSlugs()).toEqual(before);
  });

  await test.step('a correct prefix makes the board, and the list shows the prefix', async () => {
    await field(form, 'Prefix').locator('input').fill(PREFIX);
    await form.getByRole('button', { name: 'Create board' }).click();
    await expect(
      page.getByText(`Kairos made the board "${NAME}" with the default columns of the level initiative.`),
    ).toBeVisible({ timeout: 10_000 });
    const row = panel(page, 'All boards')
      .locator('.cl-group', { has: page.getByRole('link', { name: NAME, exact: true }) })
      .first();
    await expect(row.getByTestId('board-prefix')).toHaveText(PREFIX);
  });

  const board = ((await api('GET', '/api/boards?limit=200')).items as any[]).find(
    (b) => b.slug === SLUG,
  );
  expect(board, `the board ${SLUG}`).toBeTruthy();
  expect(board.code_prefix).toBe(PREFIX);
  expect(board.board_level).toBe('initiative');

  await test.step('an initiative on the board gets a code with the prefix', async () => {
    const initiative = await api('POST', '/api/initiatives', {
      board_id: board.id,
      title: 'E2E: the first initiative of the board',
    });
    expect(initiative.short_code).toBe(`${PREFIX}-I-0001`);
    await api('DELETE', `/api/initiatives/${initiative.short_code}`);
  });

  await api('DELETE', `/api/boards/${board.id}`);
  expect(await boardSlugs()).toEqual(before);
});
