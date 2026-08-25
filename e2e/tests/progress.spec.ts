// KAIROS-T-0080 — children-progress rollups, read-only against the seed:
//
//   1. REAL PKCE login (alice)
//   2. Initiatives board: the "Portal sign-up flow" card carries the
//      N-of-M micro-badge (its seeded children: 1 in web Completed, the
//      rest open — 1/5 done)
//   3. its detail page renders the segmented bar + "1 of 5 done"
//   4. a childless item (DEMO-T-0002 detail) renders NO progress UI
//
// Runs before smoke alphabetically; neither adds children to the seeded
// initiative, so the counts are stable within a run.

import { test, expect } from '@playwright/test';

test('children progress: card badge and detail bar for a seeded parent', async ({
  page,
}) => {
  // 1. Login -----------------------------------------------------------------
  await test.step('login via Dex as alice', async () => {
    await page.goto('/');
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'alice@kairos.test');
    await page.fill('#password', 'alice-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
      timeout: 30_000,
    });
  });

  // 2. The initiative card carries the micro-badge ---------------------------
  await test.step('initiative card shows the N-of-M badge', async () => {
    await page.locator('.kairos-board-tile', { hasText: 'Initiatives' }).click();
    await page.waitForURL(/\/boards\/initiatives/);
    const card = page.locator('article.kairos-card', {
      hasText: 'Portal sign-up flow',
    });
    await expect(card).toBeVisible();
    const badge = card.locator('.kairos-card__progress');
    await expect(badge).toContainText('1/5 done');
    await expect(badge.locator('.kairos-progress__fill')).toBeVisible();
  });

  // 3. The detail page renders the segmented bar -----------------------------
  await test.step('initiative detail renders the segmented bar', async () => {
    await page
      .locator('article.kairos-card', { hasText: 'Portal sign-up flow' })
      .locator('a.kairos-card__code')
      .click();
    await page.waitForURL(/\/items\//);
    const bar = page.locator('.kairos-progress');
    await expect(bar).toBeVisible();
    await expect(bar).toContainText('1 of 5 done');
    // Segments: one per involved column, done segments distinct.
    await expect(
      bar.locator('.kairos-progress__segment--done'),
    ).toHaveCount(1);
  });

  // 4. A childless item renders no progress UI -------------------------------
  await test.step('childless task detail has no progress bar', async () => {
    // In-app navigation (memory-only token): boards → platform-delivery.
    await page.locator('.cl-appshell__navbar')
      .getByRole('link', { name: 'Boards', exact: true })
      .click();
    await page.waitForURL(/\/boards$/);
    await page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }).click();
    await page.waitForURL(/\/boards\/platform-delivery/);
    await page
      .locator('article.kairos-card', { hasText: 'DEMO-T-0002' })
      .locator('a.kairos-card__code')
      .click();
    await page.waitForURL(/\/items\/DEMO-T-0002/);
    // The editor renders; the progress bar does not.
    await expect(page.locator('.kairos-editor')).toBeVisible();
    await expect(page.locator('.kairos-progress')).toHaveCount(0);
  });
});
