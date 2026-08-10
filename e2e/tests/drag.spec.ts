// KAIROS-T-0064 — drag-and-drop card transitions.
//
// Runs against the web-delivery board so it never disturbs smoke.spec.ts's
// platform-delivery fixture (suite is serial; specs share one seed).
//
//   1. REAL PKCE login (alice)
//   2. open Web Delivery → "Welcome-email trigger" sits in Backlog,
//      draggable, with the move menu still present as the a11y fallback
//   3. drag it to Todo (legal: Backlog → Todo) → the card lands
//   4. drag it to Backlog (ILLEGAL: no Todo → Backlog transition) → the
//      drop is refused and the card stays in Todo
//
// Playwright's dragTo drives the HTML5 drag events the implementation
// listens for (verified in KAIROS-T-0064's status updates).

import { test, expect, type Locator, type Page } from '@playwright/test';

const CARD = 'Welcome-email trigger';

const column = (page: Page, name: string): Locator =>
  page.locator('section.kairos-board__column', {
    has: page.locator('.kairos-board__column-head', { hasText: name }),
  });

const cardIn = (page: Page, columnName: string): Locator =>
  column(page, columnName).locator('article.kairos-card', { hasText: CARD });

test('drag and drop: legal move lands, illegal move is refused', async ({ page }) => {
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

  // 2. Open the web board; the card is draggable, menu fallback present ------
  await test.step('open web-delivery; card is draggable with menu fallback', async () => {
    await page.locator('.kairos-board-tile', { hasText: 'Web Delivery' }).click();
    await page.waitForURL(/\/boards\/web-delivery/);
    const card = cardIn(page, 'Backlog');
    await expect(card).toBeVisible();
    await expect(card).toHaveAttribute('draggable', 'true');
    await expect(card.getByRole('button', { name: /Move/ })).toBeVisible();
  });

  // 3. Legal drag: Backlog → Todo -------------------------------------------
  await test.step('drag to Todo (legal) — card lands', async () => {
    await cardIn(page, 'Backlog').dragTo(column(page, 'Todo'));
    await expect(cardIn(page, 'Todo')).toBeVisible({ timeout: 15_000 });
    await expect(cardIn(page, 'Backlog')).toHaveCount(0);
  });

  // 4. Illegal drag: Todo → Backlog is not a configured transition ----------
  await test.step('drag back to Backlog (illegal) — drop refused', async () => {
    await cardIn(page, 'Todo').dragTo(column(page, 'Backlog'));
    // Nothing to await server-side (no request should fire): give the UI a
    // beat, then assert the card did not move.
    await page.waitForTimeout(1_000);
    await expect(cardIn(page, 'Todo')).toBeVisible();
    await expect(cardIn(page, 'Backlog')).toHaveCount(0);
  });
});
