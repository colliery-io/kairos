// KAIROS-T-0092 — the KAIROS-I-0008 focal flight-level graph smoke.
//
// One serial flow as bob (platform member, non-admin):
//
//   1. REAL PKCE login via the Dex form
//   2. platform-delivery board: the seeded blocks web renders dependency
//      badges (neutral accents); a badge click lands on the item's graph
//   3. the canvas: three fixed column headers, a focus node, at least one
//      blocks arrow, lanes — and DETERMINISTIC positions across a reload
//   4. +N expands in place (more nodes, no navigation)
//   5. refocus pushes history + extends the trail; browser back returns
//   6. side panel lists the PRD (supports the sign-up initiative)
//   7. WS live: an API writer transitions the focused task; the open
//      canvas updates its status text without user action
//   8. /search: the traverse switch reads as result scoping
//
// Conventions match the other specs: visible-text/role selectors plus the
// stable `.kairos-*` classes; in-app navigation (memory token, A-0015)
// except one deliberate reload (T-0071 silent restore) for the
// determinism assertion; the API writer mints its own PKCE token.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { loadPlatformDelivery, transitionTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:8080';

const canvas = (page: Page) => page.locator('svg .kairos-graph__box');

test('graph: badges → canvas → deterministic reload → expand → refocus/back → side panel → WS live → traverse relabel', async ({
  page,
}) => {
  // 1. Login ---------------------------------------------------------------
  await test.step('login via Dex as bob', async () => {
    await page.goto('/');
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'bob@kairos.test');
    await page.fill('#password', 'bob-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
      timeout: 30_000,
    });
  });

  // 2. Board badges + click-through ----------------------------------------
  await test.step('seeded blocks web renders card badges; badge opens the graph', async () => {
    await page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }).click();
    await page.waitForURL(/\/boards\/platform-delivery/);
    // "Password-less email auth" blocks the tenant-provisioning task.
    const blocker = page.locator('article.kairos-card', {
      hasText: 'Password-less email auth',
    });
    await expect(blocker.locator('.cl-pill', { hasText: 'blocks 1' })).toBeVisible();
    const blocked = page.locator('article.kairos-card', {
      hasText: 'Provision tenant on first login',
    });
    await expect(blocked.locator('.cl-pill', { hasText: 'blocked by 1' })).toBeVisible();

    await blocker.locator('.kairos-card__blocks').first().click();
    await page.waitForURL(/\/items\/DEMO-T-0002\?view=graph/);
  });

  // 3. Canvas shape + deterministic reload ----------------------------------
  let sampled: Array<{ code: string; x: string }> = [];
  await test.step('canvas renders columns, focus, arrows — deterministically', async () => {
    await expect(page.locator('.kairos-graph__header', { hasText: 'Strategy' })).toBeVisible();
    await expect(page.locator('.kairos-graph__header', { hasText: 'Initiative' })).toBeVisible();
    await expect(page.locator('.kairos-graph__header', { hasText: 'Task' })).toBeVisible();
    const focus = page.locator('.kairos-graph__node--focus');
    await expect(focus.locator('.kairos-graph__code')).toHaveText('DEMO-T-0002');
    await expect(page.locator('.kairos-graph__edge').first()).toBeVisible();
    await expect(page.locator('.kairos-graph__lane').first()).toBeVisible();
    expect(await canvas(page).count()).toBeGreaterThanOrEqual(3);

    // Sample two node positions, reload (T-0071 restores the session),
    // and require identical geometry — the no-force-layout proof.
    const nodes = page.locator('.kairos-graph__node');
    sampled = [];
    for (const index of [0, 1]) {
      const node = nodes.nth(index);
      sampled.push({
        code: (await node.locator('.kairos-graph__code').textContent()) ?? '',
        x: (await node.locator('.kairos-graph__box').getAttribute('x')) ?? '',
      });
    }
    await page.reload();
    await expect(page.locator('.kairos-graph__node--focus')).toBeVisible({
      timeout: 20_000,
    });
    for (const index of [0, 1]) {
      const node = page.locator('.kairos-graph__node').nth(index);
      await expect(node.locator('.kairos-graph__code')).toHaveText(sampled[index].code);
      expect(await node.locator('.kairos-graph__box').getAttribute('x')).toBe(
        sampled[index].x,
      );
    }
  });

  // 4. +N expands in place ---------------------------------------------------
  await test.step('+N expands the neighborhood without navigating', async () => {
    const before = await canvas(page).count();
    const more = page.locator('.kairos-graph__more').first();
    await expect(more).toBeVisible();
    await more.click();
    await expect
      .poll(async () => canvas(page).count(), { timeout: 15_000 })
      .toBeGreaterThan(before);
    // Still on the item's graph tab — expansion never navigates.
    expect(new URL(page.url()).pathname).toBe('/items/DEMO-T-0002');
  });

  // 5. Refocus + trail + browser back ---------------------------------------
  await test.step('refocus extends the trail; back returns', async () => {
    // Click a non-focus node body (the blocked task).
    const target = page.locator('.kairos-graph__node', {
      has: page.locator('.kairos-graph__code', { hasText: 'DEMO-T-0003' }),
    });
    await target.locator('.kairos-graph__box').click();
    await page.waitForURL(/\/search\/relationships\/DEMO-T-0003\?trail=DEMO-T-0002/);
    await expect(page.getByText('trail:')).toBeVisible();
    await expect(
      page.locator('.kairos-graph__node--focus .kairos-graph__code'),
    ).toHaveText('DEMO-T-0003');

    await page.goBack();
    await page.waitForURL(/\/items\/DEMO-T-0002\?view=graph/);
    await expect(
      page.locator('.kairos-graph__node--focus .kairos-graph__code'),
    ).toHaveText('DEMO-T-0002', { timeout: 15_000 });
  });

  // 6. Side panel: the PRD under the sign-up initiative ----------------------
  await test.step('supporting material sits in the side panel, not the canvas', async () => {
    const panel = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Supporting material' }),
    });
    await expect(panel.getByText('PRD: Portal sign-up flow')).toBeVisible();
    // Docs never render as canvas nodes.
    await expect(
      page.locator('.kairos-graph__code', { hasText: 'DEMO-D-0001' }),
    ).toHaveCount(0);
  });

  // 7. WS live: an API transition updates the open canvas --------------------
  await test.step('API-side transition refreshes the canvas status', async () => {
    const token = await mintToken({ server: GUI }); // alice
    const snapshot = await loadPlatformDelivery(GUI, token);
    const columnIdOf = (name: string) => {
      for (const [id, columnName] of snapshot.columnName) {
        if (columnName === name) return id;
      }
      throw new Error(`no column named ${name}`);
    };
    // The seeded delivery graph allows Active <-> Blocked, so the move is
    // reversible: the suite's later specs pin DEMO-T-0002 in Active.
    const focus = page.locator('.kairos-graph__node--focus');
    await expect(focus.locator('.kairos-graph__status')).toHaveText('Active');
    await transitionTask(GUI, token, 'DEMO-T-0002', columnIdOf('Blocked'));
    await expect(focus.locator('.kairos-graph__status')).toHaveText('Blocked', {
      timeout: 20_000,
    });
    // Revert — and the canvas follows again (two live updates proven).
    await transitionTask(GUI, token, 'DEMO-T-0002', columnIdOf('Active'));
    await expect(focus.locator('.kairos-graph__status')).toHaveText('Active', {
      timeout: 20_000,
    });
  });

  // 8. Traverse switch reads as scoping --------------------------------------
  await test.step('search traverse switch is labeled as result scoping', async () => {
    await page
      .locator('.cl-appshell__navbar')
      .getByRole('link', { name: 'Search', exact: true })
      .click();
    await page.waitForURL(/\/search$/);
    await expect(
      page.getByText('Limit results to items reachable from…'),
    ).toBeVisible();
  });
});
