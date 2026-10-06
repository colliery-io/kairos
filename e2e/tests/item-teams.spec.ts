// KAIROS-T-0322 — the teams of an initiative on the board and on its page:
//
//   1. REAL PKCE login (alice)
//   2. Initiatives board: the seeded "Portal sign-up flow" has tasks on
//      the web and the platform boards, so its card has the two pills
//   3. the team filter: a team chip keeps the cards of that team, and
//      "No team" keeps the cards with no team (in the URL, so a reload
//      keeps it)
//   4. a new initiative has no team; on its page the picker sets "web" by
//      hand, and a second session on the board sees the pill arrive
//      without a reload; Remove clears it again
//
// The new initiative is the only write, and it is the spec's own.

import { test, expect, type Page } from '@playwright/test';

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

async function openInitiatives(page: Page): Promise<void> {
  await page.locator('.kairos-board-tile', { hasText: 'Initiatives' }).click();
  await page.waitForURL(/\/boards\/initiatives/);
}

const card = (page: Page, text: string) =>
  page.locator('article.kairos-card', { hasText: text });

test('item teams: pills, the team filter, and the picker', async ({ page, browser }) => {
  const title = `Teams e2e ${Date.now()}`;

  await test.step('login via Dex as alice', async () => {
    await login(page);
  });

  await test.step('the seeded initiative shows the teams of its tasks', async () => {
    await openInitiatives(page);
    const portal = card(page, 'Portal sign-up flow');
    await expect(portal).toBeVisible();
    await expect(portal.locator('.kairos-card__team[data-team="web"]')).toBeVisible();
    await expect(portal.locator('.kairos-card__team[data-team="platform"]')).toBeVisible();
  });

  await test.step('a new initiative has no team', async () => {
    await page.getByRole('button', { name: 'New initiative', exact: true }).click();
    const modal = page.locator('.cl-modal');
    await modal.locator('input.cl-input').first().fill(title);
    await modal.getByRole('button', { name: 'Create' }).click();
    await expect(card(page, title)).toBeVisible();
    await expect(card(page, title).locator('.kairos-card__team')).toHaveCount(0);
  });

  await test.step('the team filter keeps one team, or the items with no team', async () => {
    const filter = page.getByTestId('team-filter');
    await expect(filter).toBeVisible();
    await filter.locator('[data-team="web"]').click();
    await expect(page).toHaveURL(/team=web/);
    await expect(card(page, 'Portal sign-up flow')).toBeVisible();
    await expect(card(page, title)).toHaveCount(0);

    await filter.getByTestId('no-team').click();
    await expect(page).toHaveURL(/no_team=1/);
    await expect(card(page, title)).toBeVisible();
    await expect(card(page, 'Portal sign-up flow')).toHaveCount(0);

    // The URL is the state: a reload keeps the filter.
    await page.reload();
    await expect(card(page, title)).toBeVisible({ timeout: 30_000 });
    await expect(card(page, 'Portal sign-up flow')).toHaveCount(0);

    await page.getByTestId('clear-team-filter').click();
    await expect(card(page, 'Portal sign-up flow')).toBeVisible();
  });

  // A second session on the board: the pill arrives live.
  const watcher = await browser.newPage();
  await login(watcher);
  await openInitiatives(watcher);
  await expect(card(watcher, title)).toBeVisible();

  await test.step('the picker sets a team by hand, and the card changes live', async () => {
    await card(page, title).locator('a.kairos-card__code').click();
    await page.waitForURL(/\/items\//);
    const panel = page.getByTestId('item-teams');
    await expect(panel).toContainText('No team');
    const add = page.getByTestId('item-teams-add');
    await add.locator('select').selectOption('web');
    await add.getByRole('button', { name: 'Set team' }).click();
    const row = panel.locator('[data-team="web"]');
    await expect(row).toContainText('set by hand');

    await expect(
      card(watcher, title).locator('.kairos-card__team[data-team="web"]'),
    ).toBeVisible({ timeout: 15_000 });
  });

  await test.step('Remove clears the team set by hand', async () => {
    const panel = page.getByTestId('item-teams');
    await panel.locator('[data-team="web"]').getByRole('button', { name: 'Remove' }).click();
    await expect(panel).toContainText('No team');
    await expect(
      card(watcher, title).locator('.kairos-card__team'),
    ).toHaveCount(0, { timeout: 15_000 });
  });

  await watcher.close();
});
