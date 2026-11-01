// KAIROS-T-0744 — the controls above the columns of a board page:
//
//   1. alice (an organization admin) on platform-delivery: the card
//      filters ("Cards"), the owner lanes ("Lanes"), and the link Members,
//      which opens the members panel of the board settings;
//   2. the New document dialog of the board names the owner board;
//   3. bob (a member, not an admin) gets no Members link; an ADR board has
//      no owner lanes.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { readEachPage } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function login(page: Page, email: string, password: string): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', email);
  await page.fill('#password', password);
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

test('the board header: filters, owner lanes, Members and the New document dialog', async ({ page }) => {
  const alice = await mintToken();
  const boards: any[] = await readEachPage(GUI, alice, '/api/boards');
  const platform = boards.find((b) => b.slug === 'platform-delivery');
  if (!platform) throw new Error('no board platform-delivery');
  const adr = boards.find((b) => b.board_level === 'adr');

  await test.step('alice sees the filters, the lanes and Members', async () => {
    await login(page, 'alice@kairos.test', 'alice-password');
    await page.goto('/boards/platform-delivery');
    await expect(page.locator('article.kairos-card').first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('[data-testid="card-filters"]')).toContainText('Cards');
    await expect(page.locator('[data-testid="card-filters"] [data-testid="filter-mine"]')).toBeVisible();
    await expect(page.locator('[data-testid="owner-lens"]')).toContainText('Lanes');
    await expect(page.locator('[data-testid="owner-lens"] [data-testid="group-by-owner"]')).toBeVisible();
    const members = page.locator('[data-testid="board-members-link"]');
    await expect(members).toHaveAttribute('href', new RegExp(`^/admin/boards/${platform.id}#`));
    await members.click();
    await expect(page).toHaveURL(new RegExp(`/admin/boards/${platform.id}`));
  });

  await test.step('the New document dialog names the owner board', async () => {
    await page.goto('/boards/platform-delivery');
    await page.getByRole('button', { name: 'New document', exact: true }).click();
    await expect(page.locator('[data-testid="create-document-owner"]')).toContainText(
      `The owner board is ${platform.name}.`,
    );
    await page.keyboard.press('Escape');
  });

  if (adr) {
    await test.step('an ADR board has no owner lanes', async () => {
      await page.goto(`/boards/${adr.slug}`);
      await expect(page.locator('[data-testid="card-filters"], .kairos-board').first()).toBeVisible({
        timeout: 30_000,
      });
      await expect(page.locator('[data-testid="owner-lens"]')).toHaveCount(0);
    });
  }
});

test('bob, a member who is not an admin, has no Members link', async ({ page }) => {
  await login(page, 'bob@kairos.test', 'bob-password');
  await page.goto('/boards/platform-delivery');
  await expect(page.locator('article.kairos-card').first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-testid="card-filters"]')).toBeVisible();
  await expect(page.locator('[data-testid="board-members-link"]')).toHaveCount(0);
});
