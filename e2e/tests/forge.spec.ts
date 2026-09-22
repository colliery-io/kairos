// KAIROS-T-0102 — the KAIROS-I-0009 git-forge integration smoke.
//
// The honest test is a REAL signed delivery, not seeded rows: the spec
// registers a repository through the admin API, captures the webhook URL
// and secret it hands back, and POSTs GitHub payloads signed with that
// secret — the same bytes a forge would send.
//
// Flow (login as alice, an org admin, since connections are admin-write):
//   1. seeded links already render on the item and the team page
//   2. register a repo → deliver an "opened" PR → the Development panel
//      shows it WITHOUT a reload (the WS path)
//   3. deliver the merge → the chip flips live
//   4. replay the earlier "opened" → it stays merged (the ordering guard,
//      proven in the browser)
//   5. a wrongly signed delivery changes nothing
//   6. the team page's In flight panel shows open work and hides merged
//
// Conventions match the other specs: visible-text/role selectors plus the
// stable `.kairos-*`/`.cl-*` classes, and in-app navigation only.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import {
  createForgeConnection,
  createRepository,
  deliverGithubWebhook,
  githubPullRequest,
} from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const REPO = 'acme/checkout-api';

const panel = (page: Page, title: string) =>
  page.locator('.cl-panel', {
    has: page.locator('.cl-panel__title', { hasText: title }),
  });

test('forge: seeded links → signed delivery → live merge → replay is ignored → bad signature → team rollup', async ({
  page,
}) => {
  // 1. Login as alice (org admin — connections are admin-write) -------------
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

  // 2. The seeded fixtures render ------------------------------------------
  await test.step('seeded links show on the item detail', async () => {
    await page
      .locator('.cl-appshell__navbar')
      .getByRole('link', { name: 'Boards', exact: true })
      .click();
    await page.waitForURL(/\/boards$/);
    await page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }).click();
    await page.waitForURL(/\/boards\/platform-delivery/);
    await page
      .locator('article.kairos-card', { hasText: 'Password-less email auth' })
      .locator('a.kairos-card__code')
      .click();
    await page.waitForURL(/\/items\/DEMO-T-0002/);

    const dev = panel(page, 'Development');
    await expect(dev).toBeVisible();
    // The seed gives this task an open PR and a branch.
    await expect(dev.getByText('#42', { exact: false })).toBeVisible();
    await expect(dev.locator('.cl-pill', { hasText: 'open' }).first()).toBeVisible();
    await expect(dev.getByText('acme/payments-api', { exact: false }).first()).toBeVisible();
  });

  // 3. A real signed delivery, watched live --------------------------------
  const token = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  // KAIROS-T-0106: the repository is registered first (under platform),
  // then its webhooks are connected by slug.
  const checkout = await createRepository(GUI, token, {
    slug: 'checkout-api',
    repoFullName: REPO,
    team: 'platform',
  });
  const connection = await createForgeConnection(GUI, token, checkout.slug);

  await test.step('a signed delivery appears without a reload', async () => {
    const opened = githubPullRequest({
      number: 501,
      code: 'DEMO-T-0002',
      repoFullName: REPO,
      state: 'open',
      updatedAt: '2026-09-02T09:00:00Z',
      title: 'Checkout wiring for DEMO-T-0002',
    });
    const status = await deliverGithubWebhook(GUI, connection, 'pull_request', opened);
    expect(status).toBe(200);

    // No reload: the page is already open and the WS event drives it.
    const dev = panel(page, 'Development');
    await expect(dev.getByText('#501', { exact: false })).toBeVisible({
      timeout: 20_000,
    });
    await expect(dev.getByText(REPO, { exact: false })).toBeVisible();
  });

  await test.step('merging flips the chip live', async () => {
    const merged = githubPullRequest({
      number: 501,
      code: 'DEMO-T-0002',
      repoFullName: REPO,
      state: 'closed',
      merged: true,
      updatedAt: '2026-09-02T11:00:00Z',
      title: 'Checkout wiring for DEMO-T-0002',
    });
    const status = await deliverGithubWebhook(GUI, connection, 'pull_request', merged);
    expect(status).toBe(200);

    const row = panel(page, 'Development')
      .locator('.cl-group')
      .filter({ hasText: '#501' })
      .first();
    await expect(row.locator('.cl-pill', { hasText: 'merged' })).toBeVisible({
      timeout: 20_000,
    });
  });

  // 4. The ordering guard, proven in the browser ---------------------------
  await test.step('replaying the earlier open does not un-merge it', async () => {
    const stale = githubPullRequest({
      number: 501,
      code: 'DEMO-T-0002',
      repoFullName: REPO,
      state: 'open',
      updatedAt: '2026-09-02T09:00:00Z',
      title: 'Checkout wiring for DEMO-T-0002',
    });
    const status = await deliverGithubWebhook(GUI, connection, 'pull_request', stale);
    expect(status).toBe(200);

    // Give the WS refetch a chance to (incorrectly) apply, then assert the
    // state held.
    await page.waitForTimeout(2_000);
    const row = panel(page, 'Development')
      .locator('.cl-group')
      .filter({ hasText: '#501' })
      .first();
    await expect(row.locator('.cl-pill', { hasText: 'merged' })).toBeVisible();
    await expect(row.locator('.cl-pill', { hasText: 'open' })).toHaveCount(0);
  });

  // 5. A wrongly signed delivery is rejected and changes nothing -----------
  await test.step('a bad signature is rejected', async () => {
    const forged = githubPullRequest({
      number: 502,
      code: 'DEMO-T-0002',
      repoFullName: REPO,
      state: 'open',
      updatedAt: '2026-09-02T12:00:00Z',
      title: 'Should never appear',
    });
    const status = await deliverGithubWebhook(
      GUI,
      connection,
      'pull_request',
      forged,
      'not-the-secret',
    );
    expect(status).toBe(401);
    await page.waitForTimeout(1_000);
    await expect(
      panel(page, 'Development').getByText('#502', { exact: false }),
    ).toHaveCount(0);
  });

  // 6. The team rollup shows in-flight work only ---------------------------
  await test.step('team page lists open work and hides merged', async () => {
    await page
      .locator('.cl-appshell__navbar')
      .getByRole('link', { name: 'Teams', exact: true })
      .click();
    await page.waitForURL(/\/teams$/);
    await page.locator('.kairos-board-tile', { hasText: 'Platform' }).first().click();
    await page.waitForURL(/\/teams\/platform$/);

    const inflight = panel(page, 'In flight');
    await expect(inflight).toBeVisible();
    // The seeded open PR on a platform-attributed repo is in flight…
    await expect(inflight.getByText('#42', { exact: false })).toBeVisible();
    // …and it links back to the Kairos item. Several in-flight links can
    // share one item (the seeded PR and branch, plus the one delivered
    // above), so scope to the first rather than asserting uniqueness.
    await expect(
      inflight.getByRole('link', { name: /DEMO-T-0002/ }).first(),
    ).toBeVisible();
    // The seeded MERGED pull request is not in-flight work.
    await expect(inflight.getByText('#43', { exact: false })).toHaveCount(0);
  });
});
