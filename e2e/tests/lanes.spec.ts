// KAIROS-T-0077 — Planned/Support swim lanes on delivery boards.
//
// Runs against platform-delivery with a FRESH card per run (unique title,
// retry-safe), plus read-only checks of the seeded support fixtures:
//
//   1. REAL PKCE login (alice)
//   2. open Platform Delivery → Support lane renders ABOVE Planned; the
//      seeded support request sits in Support/Active and the seeded
//      UNPLANNED BUG sits in Support/Todo with its bug pill intact (the
//      recorded no-bug-lane decision: lane = planned-ness, type = kind)
//   3. create a bug with lane=support via the modal → born in
//      Support/Backlog, bug pill intact
//   4. same-column cross-lane drag (Support/Backlog → Planned/Backlog):
//      the lane changes, the column does not — no transition validation
//   5. cross-column same-lane drag (Planned/Backlog → Planned/Todo):
//      a plain transition, exactly as before lanes existed
//   6. diagonal drag (Planned/Todo → Support/Active): transition + lane
//      write in one drop
//   7. the detail page's Lane control (the keyboard path) moves it back
//      to planned
//
// Selector conventions match smoke.spec.ts; columns are lane-scoped
// because a column name matches one section per lane.

import { test, expect, type Locator, type Page } from '@playwright/test';

type Lane = 'planned' | 'support';

const column = (page: Page, name: string, lane: Lane): Locator =>
  page
    .locator(`section.kairos-board__lane--${lane}`)
    .locator('section.kairos-board__column', {
      has: page.locator('.kairos-board__column-head', { hasText: name }),
    });

const cardIn = (
  page: Page,
  columnName: string,
  lane: Lane,
  needle: string,
): Locator =>
  column(page, columnName, lane).locator('article.kairos-card', {
    hasText: needle,
  });

test('lanes: support renders above planned; drags and the lane control move cards across', async ({
  page,
}) => {
  const createdTitle = `Lane bug ${Date.now()}`;

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

  // 2. Lane structure + the seeded support fixtures --------------------------
  await test.step('support lane on top; seeded support cards in place', async () => {
    await page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }).click();
    await page.waitForURL(/\/boards\/platform-delivery/);

    const lanes = page.locator('section.kairos-board__lane');
    await expect(lanes).toHaveCount(2);
    await expect(lanes.first()).toHaveClass(/kairos-board__lane--support/);
    await expect(lanes.last()).toHaveClass(/kairos-board__lane--planned/);

    // The seeded support REQUEST (type support, lane support).
    const request = cardIn(page, 'Active', 'support', 'Customer cannot reset password');
    await expect(request).toBeVisible();
    await expect(request.locator('.cl-pill', { hasText: 'support' })).toBeVisible();

    // The seeded UNPLANNED BUG: Support lane, bug type intact — the
    // no-bug-lane decision in fixture form.
    const bug = cardIn(page, 'Todo', 'support', 'Login page 500s on expired trials');
    await expect(bug).toBeVisible();
    await expect(bug.locator('.cl-pill', { hasText: 'bug' })).toBeVisible();
  });

  // 3. Create a bug directly into the Support lane ---------------------------
  await test.step('create a bug with lane=support', async () => {
    await page.getByRole('button', { name: 'New task', exact: true }).click();
    const modal = page.locator('.cl-modal');
    await modal.locator('input.cl-input').first().fill(createdTitle);
    const field = (label: string) =>
      modal.locator('.cl-field', {
        has: page.locator('.cl-field__label', { hasText: label }),
      });
    await field('Task type').locator('select').selectOption({ label: 'bug' });
    await field('Lane').locator('select').selectOption({ label: 'support' });
    await modal.getByRole('button', { name: 'Create' }).click();
    await expect(modal).toBeHidden();

    const card = cardIn(page, 'Backlog', 'support', createdTitle);
    await expect(card).toBeVisible();
    await expect(card.locator('.cl-pill', { hasText: 'bug' })).toBeVisible();
  });

  // 4. Same-column cross-lane drag: lane write only --------------------------
  await test.step('drag Support/Backlog → Planned/Backlog', async () => {
    await expect(async () => {
      const card = cardIn(page, 'Backlog', 'support', createdTitle);
      if (await card.isVisible()) {
        await card.dragTo(column(page, 'Backlog', 'planned'), { timeout: 2_000 });
      }
      await expect(
        cardIn(page, 'Backlog', 'planned', createdTitle),
      ).toBeVisible({ timeout: 5_000 });
    }).toPass({ timeout: 30_000 });
    await expect(cardIn(page, 'Backlog', 'support', createdTitle)).toHaveCount(0);
  });

  // 5. Cross-column same-lane drag: a plain transition -----------------------
  await test.step('drag Planned/Backlog → Planned/Todo', async () => {
    await expect(async () => {
      const card = cardIn(page, 'Backlog', 'planned', createdTitle);
      if (await card.isVisible()) {
        await card.dragTo(column(page, 'Todo', 'planned'), { timeout: 2_000 });
      }
      await expect(cardIn(page, 'Todo', 'planned', createdTitle)).toBeVisible({
        timeout: 5_000,
      });
    }).toPass({ timeout: 30_000 });
  });

  // 6. Diagonal drag: transition + lane write in one drop --------------------
  await test.step('drag Planned/Todo → Support/Active (diagonal)', async () => {
    await expect(async () => {
      const card = cardIn(page, 'Todo', 'planned', createdTitle);
      if (await card.isVisible()) {
        await card.dragTo(column(page, 'Active', 'support'), { timeout: 2_000 });
      }
      await expect(
        cardIn(page, 'Active', 'support', createdTitle),
      ).toBeVisible({ timeout: 5_000 });
    }).toPass({ timeout: 30_000 });
  });

  // 7. The keyboard path: the detail page's Lane control ---------------------
  await test.step('detail lane control moves it back to planned', async () => {
    await cardIn(page, 'Active', 'support', createdTitle)
      .locator('a.kairos-card__code')
      .click();
    await page.waitForURL(/\/items\//);

    const boardPanel = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Board' }),
    });
    const laneField = boardPanel.locator('.cl-field', {
      has: page.locator('.cl-field__label', { hasText: 'Lane' }),
    });
    await laneField.locator('select').selectOption({ label: 'planned' });
    await boardPanel.getByRole('button', { name: 'Set lane', exact: true }).click();
    await expect(page.getByText('Lane set to planned.')).toBeVisible();
  });
});
