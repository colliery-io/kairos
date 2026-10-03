// COLLIERY-T-3100 — a retired short code still finds its item and names
// the current code, on the GUI item page.
//
// The journey: a task gets a new code, and its old code is retired. The
// page `/items/{old code}` goes to the page of the current code (the
// history entry is replaced) and shows a notice that names the two codes.
//
// **The fixture is written to the database.** The move with a rename
// (COLLIERY-T-3101) is the only surface that will retire a code, and it
// does not exist yet. So this spec changes the code and retires the old
// one with psql in the dev container `kairos-dev-postgres` (the container
// of `angreal test e2e`), in the same order as the rename will. When
// COLLIERY-T-3101 lands, this spec must use the rename instead.
//
// **Fixture discipline** (see archived.spec.ts): the card is archived at
// the end, so no board shows it to the specs that count cards.

import { execFileSync } from 'node:child_process';
import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const SERVER = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const BOARD_SLUG = 'platform-delivery';
const SCHEMA = 'org_demo';

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

/** One SQL statement in the dev database; the value of the first column. */
function psql(sql: string): string {
  return execFileSync(
    'docker',
    [
      'exec', '-i', 'kairos-dev-postgres',
      'psql', '-U', 'kairos', '-d', 'kairos', '-v', 'ON_ERROR_STOP=1', '-qtA', '-c', sql,
    ],
    { encoding: 'utf8' },
  )
    .trim()
    .split('\n')[0];
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

test('the item page of a retired code goes to the current code and names the two codes', async ({
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
  const prefix = oldCode.split('-')[0];

  // The code changes to the next code of the sequence, then the old code
  // is retired (the order of the rename, COLLIERY-T-3101).
  const newCode = psql(
    `WITH seq AS (
       UPDATE ${SCHEMA}.short_code_sequences SET last_number = last_number + 1
        WHERE code_prefix = '${prefix}' AND item_type = 'T' RETURNING last_number)
     UPDATE ${SCHEMA}.tasks
        SET short_code = '${prefix}-T-' || lpad((SELECT last_number FROM seq)::text, 4, '0')
      WHERE id = '${task.id}' RETURNING short_code`,
  );
  expect(newCode).not.toBe(oldCode);
  psql(
    `INSERT INTO ${SCHEMA}.retired_codes (code, item_id, reason)
     VALUES ('${oldCode}', '${task.id}', 'e2e: COLLIERY-T-3100') RETURNING code`,
  );

  await test.step('login as alice', () => login(page));

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
    await expect(page.getByRole('textbox', { name: 'Content' })).toHaveValue(
      'The old code still finds me.',
    );
    await expect(page.getByText(newCode, { exact: true }).first()).toBeVisible();
  });

  await test.step('the page of the current code shows no notice', async () => {
    await page.goto(`/items/${newCode}`);
    await expect(page.getByRole('textbox', { name: 'Content' })).toHaveValue(
      'The old code still finds me.',
    );
    await expect(page.locator('[data-testid="retired-code-notice"]')).toHaveCount(0);
  });

  // Leave the seeded board as it was found: the card goes away.
  await api(token, 'DELETE', `/api/tasks/${newCode}`);
});
