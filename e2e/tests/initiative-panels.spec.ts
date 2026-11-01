// KAIROS-T-0744 — the panels of an initiative page:
//
//   1. Owner: a new initiative has no owner; Claim makes alice the owner;
//   2. Complexity: a size set on the page is the complexity on the server;
//   3. Metadata: an item type with no definition says so, and a value with
//      no definition in scope is listed read-only. The spec narrows the
//      seeded definition `priority` (all types) so that it leaves out
//      initiatives, and puts the scope back at the end.
//
// The initiative is new for each run; the spec archives it at the end.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { readEachPage } from '../helpers/api';

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

test('an initiative page: owner, complexity and metadata with no definition', async ({ page }) => {
  const alice = await mintToken();
  const boards: any[] = await readEachPage(GUI, alice, '/api/boards');
  const board = boards.find((b) => b.board_level === 'initiative');
  if (!board) throw new Error('no initiative board in the seed');
  const detail = await call(alice, 'GET', `/api/boards/${board.id}`);
  const initiative = await call(alice, 'POST', '/api/initiatives', {
    board_id: board.id,
    column_id: detail.columns[0].id,
    title: `E2E initiative panels ${Date.now()}`,
    content: 'The panels of the page.',
  });
  const code: string = initiative.short_code;
  const definitions = await call(alice, 'GET', '/api/metadata-definitions?limit=200');
  const priority = (definitions.items as any[]).find((d) => d.slug === 'priority');
  if (!priority) throw new Error('no seeded definition priority');
  const scope: string[] = priority.entity_types;

  try {
    const owner = page.locator('[data-testid="owner-panel"]');

    await test.step('Owner: no owner, then Claim', async () => {
      await login(page, `/items/${code}`);
      await expect(owner).toContainText('No owner. A member of the board can claim it.', {
        timeout: 30_000,
      });
      await owner.getByRole('button', { name: 'Claim', exact: true }).click();
      await expect(owner).toContainText(/is the owner since \d{4}-\d{2}-\d{2}\./);
    });

    await test.step('Complexity: set a size', async () => {
      const control = page.locator('[data-testid="complexity-control"]');
      await control.locator('select').selectOption('m');
      await control.getByRole('button', { name: 'Set complexity' }).click();
      await expect(page.getByText('Complexity set to m.').first()).toBeVisible();
      await expect
        .poll(async () => (await call(alice, 'GET', `/api/initiatives/${code}`)).complexity)
        .toBe('m');
    });

    await test.step('Metadata: no definition for the type, and a value with no definition', async () => {
      await call(alice, 'PATCH', `/api/initiatives/${code}/metadata`, { values: { priority: 'high' } });
      await call(alice, 'PATCH', `/api/metadata-definitions/${priority.id}`, {
        entity_types: ['strategy', 'task', 'document', 'adr'],
      });
      await page.reload();
      await expect(page.locator('[data-testid="metadata-no-definitions"]')).toContainText(
        'No metadata definitions for this item type.',
        { timeout: 30_000 },
      );
      const orphans = page.locator('[data-testid="metadata-orphans"]');
      await expect(orphans).toContainText('These values have no definition for this item type.');
      await expect(orphans).toContainText('priority');
      const field = orphans.locator('[data-slug="priority"] input');
      await expect(field).toHaveValue('high');
      await expect(field).toBeDisabled();
    });
  } finally {
    await call(alice, 'PATCH', `/api/metadata-definitions/${priority.id}`, { entity_types: scope }).catch(
      () => undefined,
    );
    await call(alice, 'DELETE', `/api/initiatives/${code}`).catch(() => undefined);
  }
});
