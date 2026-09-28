// Capture the documentation screenshots (KAIROS-T-0185).
//
// Not a test — a capture script that happens to live in the Playwright suite,
// because the suite already knows how to do the hard part: a real in-browser
// PKCE login against Dex. Writing a separate harness would have duplicated it.
//
// It is @docs-tagged, and `angreal test e2e` passes `--grep-invert @docs`, so it
// never runs as part of CI. (That exclusion was missing until KAIROS-T-0194
// found it: this file asserted it was excluded and nothing excluded it, so the
// e2e tier failed on it from the day it landed. The exclusion lives in the
// angreal task rather than the Playwright config, where a config-level
// `grepInvert` would fight the `--grep @docs` below.) Run it deliberately:
//
//   angreal docs images
//
// COLLIERY-T-0252: that task starts the dev stack, makes a new demo seed,
// runs this file, and stops the stack. So each image shows the demo seed and
// not the data of a person. By hand, against a server that runs:
//
//   cd e2e && npx playwright test capture-docs-images --grep @docs
//
// Dylan accepted that these shots go stale (KAIROS-T-0185 D3). This file is
// what makes a recapture mechanical rather than archaeological: it records the
// exact state each shot needs, so the answer to "how was this taken?" is
// "run this".
import { test, expect, type Page } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mintToken } from '../helpers/auth';

// The suite is an ES module, so __dirname is not defined here.
const HERE = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(HERE, '../../docs/src/images');

// A viewport wide enough to show a five-column board without horizontal
// scrolling, and short enough that the shot is readable inline in a book.
const VIEWPORT = { width: 1440, height: 900 };

test.use({ viewport: VIEWPORT });

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

// The task of the demo seed that the last image shows as put away
// (COLLIERY-T-0252). It is a card of the Support lane of Platform Delivery,
// so the capture archives it AFTER the image of that board, and restores it
// at the end.
const PUT_AWAY = 'DEMO-T-0011';

/** One image of the book: the viewport, when the fonts are there. */
async function shot(page: Page, file: string): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({
    path: path.join(OUT, file),
    fullPage: false,
    animations: 'disabled',
    caret: 'hide',
  });
}

test('capture the documentation screenshots @docs', async ({ page }) => {
  test.setTimeout(180_000);

  // The API token FIRST: a mint after the browser login of the same person
  // makes the browser session invalid (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const api = async (method: string, route: string): Promise<Response> =>
    fetch(GUI + route, { method, headers: { authorization: `Bearer ${alice}` } });
  const boards = (await (await api('GET', '/api/boards?limit=100')).json()).items as any[];
  const platform = boards.find((b) => b.slug === 'platform-delivery');
  if (!platform) throw new Error('the demo seed has no board platform-delivery');
  const items = await (await api('GET', `/api/boards/${platform.id}/items`)).json();
  const cards = (items.columns as any[]).reduce((n, group) => n + group.tasks.length, 0);
  let archived = false;

  try {
    await test.step('log in as alice through Dex', async () => {
      await page.goto('/');
      await page.waitForSelector('#login', { timeout: 30_000 });
      await page.fill('#login', 'alice@kairos.test');
      await page.fill('#password', 'alice-password');
      await page.click('button[type="submit"]');
      await page.waitForURL(/localhost:41080\/(?!callback)/, { timeout: 30_000 });
    });

    await test.step('boards overview — the tutorial\'s first browser moment', async () => {
      await page.goto('/boards');
      // Wait for real content rather than a timeout: the tutorial promises the
      // reader sees five boards, so the shot must show them.
      await expect(page.getByText('Platform Delivery').first()).toBeVisible({
        timeout: 30_000,
      });
      await expect(page.locator('.kairos-board-tile')).toHaveCount(boards.length);
      await shot(page, 'boards-overview.png');
    });

    await test.step('a delivery board with cards in columns', async () => {
      await page.goto('/boards/platform-delivery');
      await expect(page.getByText('Backlog').first()).toBeVisible({ timeout: 30_000 });
      // Each card of the board is there: the image must not show a board
      // that is not complete.
      await expect(page.locator('article.kairos-card')).toHaveCount(cards, { timeout: 30_000 });
      await shot(page, 'platform-delivery-board.png');
    });

    await test.step('the search toggle, off by default', async () => {
      await page.goto('/search');
      await expect(page.locator('[data-testid="include-put-away"]')).toBeVisible({
        timeout: 30_000,
      });
      await shot(page, 'search-put-away-toggle.png');
    });

    // The shot that actually demonstrates KAIROS-A-0020: a put-away hit,
    // marked, sitting beside live ones. Needs an archived item that matches the
    // query. The capture archives one here and restores it at the end, so the
    // demo tenant is left as it was found (COLLIERY-T-0252).
    await test.step('put-away work found and marked', async () => {
      const put = await api('DELETE', `/api/tasks/${PUT_AWAY}`);
      expect([200, 204], `archive ${PUT_AWAY}`).toContain(put.status);
      archived = true;

      await page.goto('/search');
      const toggle = page.locator('[data-testid="include-put-away"]');
      await expect(toggle).toBeVisible({ timeout: 30_000 });

      const query = page.locator('.cl-field', { hasText: 'Text query' }).locator('input');
      await query.fill('invoice');

      // Click the switch itself, not the labelled wrapper, and confirm it
      // actually flipped before searching — a silently-off toggle would produce
      // a screenshot of live-only results that looks like the feature working.
      await toggle.locator('.cl-switch').click();
      await expect(toggle.locator('.cl-switch--on')).toBeVisible({ timeout: 5_000 });

      await page.getByRole('button', { name: 'Search', exact: true }).click();

      // Wait for a marked hit rather than a timeout: the shot is worthless
      // without one, so fail loudly instead of capturing an empty result list.
      const badge = page.locator('.kairos-archived-badge').first();
      await expect(badge).toBeVisible({ timeout: 30_000 });

      // The marked row sits below the filter panel, so the default viewport shot
      // captures the toggle and none of the point. Scroll the result into frame.
      await badge.scrollIntoViewIfNeeded();
      await shot(page, 'search-put-away-results.png');
    });
  } finally {
    if (archived) {
      const restored = await api('POST', `/api/tasks/${PUT_AWAY}/restore`);
      expect(restored.status, `restore ${PUT_AWAY}`).toBe(200);
    }
  }
});
