// Capture the documentation screenshots (KAIROS-T-0185).
//
// Not a test — a capture script that happens to live in the Playwright suite,
// because the suite already knows how to do the hard part: a real in-browser
// PKCE login against Dex. Writing a separate harness would have duplicated it.
//
// It is @docs-tagged and excluded from `angreal test e2e`, so it never runs as
// part of CI. Run it deliberately:
//
//   cd e2e && npx playwright test capture-docs-images --grep @docs
//
// Dylan accepted that these shots go stale (KAIROS-T-0185 D3). This file is
// what makes a recapture mechanical rather than archaeological: it records the
// exact state each shot needs, so the answer to "how was this taken?" is
// "run this".
import { test, expect } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// The suite is an ES module, so __dirname is not defined here.
const HERE = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(HERE, '../../docs/src/images');

// A viewport wide enough to show a five-column board without horizontal
// scrolling, and short enough that the shot is readable inline in a book.
const VIEWPORT = { width: 1440, height: 900 };

test.use({ viewport: VIEWPORT });

test('capture the documentation screenshots @docs', async ({ page }) => {
  test.setTimeout(180_000);

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
    await page.screenshot({
      path: path.join(OUT, 'boards-overview.png'),
      fullPage: false,
    });
  });

  await test.step('a delivery board with cards in columns', async () => {
    await page.goto('/boards/platform-delivery');
    await expect(page.getByText('Backlog').first()).toBeVisible({ timeout: 30_000 });
    await page.screenshot({
      path: path.join(OUT, 'platform-delivery-board.png'),
      fullPage: false,
    });
  });

  await test.step('the search toggle, off by default', async () => {
    await page.goto('/search');
    await expect(page.locator('[data-testid="include-put-away"]')).toBeVisible({
      timeout: 30_000,
    });
    await page.screenshot({
      path: path.join(OUT, 'search-put-away-toggle.png'),
      fullPage: false,
    });
  });

  // The shot that actually demonstrates KAIROS-A-0020: a put-away hit,
  // marked, sitting beside live ones. Needs an archived item that matches the
  // query — the runner archives one before this spec and restores it after,
  // so the demo tenant is left as it was found.
  await test.step('put-away work found and marked', async () => {
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
    await page.screenshot({
      path: path.join(OUT, 'search-put-away-results.png'),
      fullPage: false,
    });
  });
});
