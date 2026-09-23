// KAIROS-T-0064 — drag-and-drop card transitions.
//
// Runs against the web-delivery board so it never disturbs smoke.spec.ts's
// platform-delivery fixture (suite is serial; specs share one seed).
//
//   1. REAL PKCE login (alice)
//   2. open Web Delivery → "Welcome-email trigger" sits in Backlog,
//      draggable, with NO per-card move menu (KAIROS-T-0075 — the
//      keyboard path lives on the item detail page)
//   3. drag it to Todo (legal: Backlog → Todo) → the card lands
//   4. drag it to Backlog (ILLEGAL: no Todo → Backlog transition) → the
//      drop is refused and the card stays in Todo
//
// Playwright's dragTo drives the HTML5 drag events the implementation
// listens for (verified in KAIROS-T-0064's status updates).

import { test, expect, type Locator, type Page } from '@playwright/test';
import { dragTo } from '../helpers/drag';

const CARD = 'Welcome-email trigger';

// KAIROS-T-0077: delivery boards render two lanes; scope to the Planned
// lane (this spec's card is planned work) so drag targets are unique.
const column = (page: Page, name: string): Locator =>
  page
    .locator('section.kairos-board__lane--planned')
    .locator('section.kairos-board__column', {
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

  // 2. Open the web board; the card is draggable and menu-free ---------------
  await test.step('open web-delivery; card is draggable, menu-free', async () => {
    await page.locator('.kairos-board-tile', { hasText: 'Web Delivery' }).click();
    await page.waitForURL(/\/boards\/web-delivery/);
    const card = cardIn(page, 'Backlog');
    await expect(card).toBeVisible();
    await expect(card).toHaveAttribute('draggable', 'true');
    // KAIROS-T-0075: no per-card move menu anywhere, for anyone.
    await expect(card.getByRole('button', { name: /Move/ })).toHaveCount(0);
  });

  // 3. Legal drag: Backlog → Todo -------------------------------------------
  await test.step('drag to Todo (legal) — card lands', async () => {
    await dragTo(page, cardIn(page, 'Backlog'), column(page, 'Todo'));
    await expect(cardIn(page, 'Todo')).toBeVisible({ timeout: 15_000 });
    await expect(cardIn(page, 'Backlog')).toHaveCount(0);
  });

  // 4. Illegal drag: Todo → Backlog is not a configured transition ----------
  await test.step('drag back to Backlog (illegal) — drop refused', async () => {
    await dragTo(page, cardIn(page, 'Todo'), column(page, 'Backlog'));
    // Nothing to await server-side (no request should fire): give the UI a
    // beat, then assert the card did not move.
    await page.waitForTimeout(1_000);
    await expect(cardIn(page, 'Todo')).toBeVisible();
    await expect(cardIn(page, 'Backlog')).toHaveCount(0);
  });
});
