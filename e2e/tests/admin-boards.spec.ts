// COLLIERY-T-0251 — the admin page does not offer a delete that the
// server refuses.
//
// The server refuses the delete of the only delivery board of a team
// (422 `LAST_DELIVERY_BOARD`, COLLIERY-T-0241). The list of boards at
// /admin/boards shows that control disabled and says why.
//
// The spec reads and changes nothing: the only write is a delete that the
// server refuses.
//
//   1. alice (org admin) reads the boards and the teams through the API
//   2. REAL PKCE login (alice), open /admin/boards
//   3. Platform Delivery is the only delivery board of team Platform: the
//      delete is disabled, the row says why and has a link to the teams
//   4. a board of the organization has a delete that is enabled
//   5. the server refuses the delete of Platform Delivery, so the page and
//      the server agree

import { test, expect, type Locator, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

/** The row of a board in the panel "All boards". */
const row = (page: Page, boardName: string): Locator =>
  page
    .locator('.cl-panel', { has: page.locator('.cl-panel__title', { hasText: 'All boards' }) })
    .locator('.cl-group', {
      has: page.getByRole('link', { name: boardName, exact: true }),
    })
    // The row contains the group of the name. The row is the first.
    .first();

test('admin boards: the only delivery board of a team has no delete', async ({ page }) => {
  // The API token FIRST (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const get = async (path: string): Promise<any> => {
    const res = await fetch(GUI + path, { headers: { authorization: `Bearer ${alice}` } });
    expect(res.status, `GET ${path}`).toBe(200);
    return res.json();
  };
  const boards = (await get('/api/boards?limit=100')).items as any[];
  const platform = boards.find((b) => b.slug === 'platform-delivery');
  expect(platform, 'the board platform-delivery').toBeTruthy();
  const team = ((await get('/api/teams?limit=100')).items as any[]).find(
    (t) => t.id === platform.team_id,
  );
  expect(team, 'the team of platform-delivery').toBeTruthy();
  // The fixture: the team has one delivery board.
  expect(
    boards.filter((b) => b.board_level === 'delivery' && b.team_id === team.id),
  ).toHaveLength(1);
  const ofOrganization = boards.find((b) => b.board_level !== 'delivery');
  expect(ofOrganization, 'a board of the organization').toBeTruthy();

  await test.step('login via Dex as alice, open /admin/boards', async () => {
    await page.goto('/');
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'alice@kairos.test');
    await page.fill('#password', 'alice-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
      timeout: 30_000,
    });
    await page.goto('/admin/boards');
    await expect(row(page, platform.name)).toBeVisible({ timeout: 30_000 });
  });

  await test.step('the only delivery board of a team: the delete is disabled and the row says why', async () => {
    const only = row(page, platform.name);
    await expect(only.getByRole('button', { name: 'Delete', exact: true })).toBeDisabled();
    await expect(
      only.getByText(
        `This board is the only delivery board of the team "${team.name}". ` +
          'To remove the board, delete the team.',
      ),
    ).toBeVisible();
    await expect(only.getByRole('link', { name: 'Open the teams' })).toHaveAttribute(
      'href',
      '/admin/teams',
    );
  });

  await test.step('a board of the organization has a delete', async () => {
    const other = row(page, ofOrganization.name);
    await expect(other.getByRole('button', { name: 'Delete', exact: true })).toBeEnabled();
    await expect(other.getByText('only delivery board')).toHaveCount(0);
  });

  await test.step('the server refuses the delete that the page does not offer', async () => {
    const res = await fetch(`${GUI}/api/boards/${platform.id}`, {
      method: 'DELETE',
      headers: { authorization: `Bearer ${alice}` },
    });
    expect(res.status).toBe(422);
    expect((await res.json()).error.code).toBe('LAST_DELIVERY_BOARD');
    await page.reload();
    await expect(row(page, platform.name)).toBeVisible({ timeout: 30_000 });
  });
});
