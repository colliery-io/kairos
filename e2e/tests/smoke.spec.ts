// KAIROS-T-0045 — the GUI Playwright smoke tier (KAIROS-A-0012 tier 4).
//
// One serial flow against a compose-backed kairos-server on :8080 with
// seed-demo data, exercising the critical GUI paths end to end:
//
//   1. REAL PKCE login through the Dex login form (alice)
//   2. board list renders the seeded boards
//   3. open platform-delivery → seeded items sit in the right columns
//   4. create a task from a column via the UI
//   5. transition it via the click-to-move menu
//   6. LIVE WS: a second writer (API, separate token) transitions another
//      item and the first browser sees the card move WITHOUT a reload
//   7. item detail: edit content + save; then a competing API PATCH forces a
//      409 and we walk one merge path (take theirs)
//   8. logout
//
// Selectors lean on visible text / roles and the crate's stable `.kairos-*`
// and aurora `.cl-*` class names (see docs/gui-conventions.md). The SPA holds
// its token in memory only (A-0015), so navigation between screens is done by
// clicking in-app links — never page.goto, which would drop the session.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import {
  pickMovableTask,
  transitionTask,
  getTask,
  patchTask,
} from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:8080';

// A board column section located by its header name (Backlog/Todo/…).
const column = (page: Page, name: string): Locator =>
  page.locator('section.kairos-board__column', {
    has: page.locator('.kairos-board__column-head', { hasText: name }),
  });

const cardIn = (page: Page, columnName: string, needle: string): Locator =>
  column(page, columnName).locator('article.kairos-card', { hasText: needle });

test('GUI smoke: login → boards → create → move → live WS → edit/409 → logout', async ({
  page,
}) => {
  const createdTitle = `Smoke task ${Date.now()}`;

  // 1. Real in-browser PKCE login via the Dex form -------------------------
  await test.step('login via Dex (real PKCE)', async () => {
    await page.goto('/');
    // The guard kicks off PKCE; the browser lands on the Dex login form.
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'alice@kairos.test');
    await page.fill('#password', 'alice-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
      timeout: 30_000,
    });
    const header = page.locator('.cl-appshell__header');
    await expect(header.getByText('alice', { exact: true })).toBeVisible();
    await expect(header.locator('.cl-pill', { hasText: 'demo' })).toBeVisible();
    await expect(header.locator('.cl-pill', { hasText: 'admin' })).toBeVisible();
  });

  // 2. Board list renders the seeded boards --------------------------------
  await test.step('board list renders seeded boards', async () => {
    const tiles = page.locator('.kairos-board-tile');
    await expect(tiles.filter({ hasText: 'Platform Delivery' })).toBeVisible();
    // seed-demo provisions 5 boards.
    await expect(tiles).toHaveCount(5);
  });

  // 3. Open platform-delivery; seeded items in the right columns -----------
  await test.step('open platform-delivery, seeded items in columns', async () => {
    await page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }).click();
    await page.waitForURL(/\/boards\/platform-delivery/);
    // Seeded placements (crates/kairos-db/src/seed.rs):
    await expect(cardIn(page, 'Active', 'DEMO-T-0002')).toBeVisible();
    await expect(cardIn(page, 'Todo', 'DEMO-T-0003')).toBeVisible();
    await expect(cardIn(page, 'Backlog', 'DEMO-T-0006')).toBeVisible();
  });

  // 4. Create a task from the Todo column via the UI -----------------------
  await test.step('create an item via the UI', async () => {
    await column(page, 'Todo').getByTitle('New item in Todo').click();
    const modal = page.locator('.cl-modal');
    await expect(modal.locator('.cl-modal__title')).toHaveText('New task');
    await modal.locator('input.cl-input').first().fill(createdTitle);
    await modal.getByRole('button', { name: 'Create' }).click();
    await expect(modal).toBeHidden();
    await expect(cardIn(page, 'Todo', createdTitle)).toBeVisible();
  });

  // 5. Transition the new task via the click-to-move menu ------------------
  await test.step('transition via the move menu', async () => {
    const card = cardIn(page, 'Todo', createdTitle);
    await card.getByRole('button', { name: /Move/ }).click();
    await card.locator('.cl-menu__dropdown')
      .getByRole('button', { name: 'Active', exact: true })
      .click();
    await expect(cardIn(page, 'Active', createdTitle)).toBeVisible();
    await expect(cardIn(page, 'Todo', createdTitle)).toHaveCount(0);
  });

  // 6. Live WS: a second (API) writer moves another card; the first browser
  //    reflects it WITHOUT a reload ---------------------------------------
  await test.step('live WS update from a second writer', async () => {
    // A page-scoped marker: a full reload would wipe it, so its survival
    // proves the update arrived over the live socket, not via navigation.
    await page.evaluate(() => ((window as any).__noReload = 'alive'));

    const token = await mintToken({ server: GUI });
    // Move some other seeded task (never DEMO-T-0002 — the edit step below
    // needs it stationary in Active). Dynamic pick keeps this retry-safe.
    const move = await pickMovableTask(GUI, token, ['DEMO-T-0002']);
    await transitionTask(GUI, token, move.code, move.toColumnId);

    // WS-driven: expect-polling, no sleeps.
    await expect(cardIn(page, move.toColumnName, move.code)).toBeVisible({
      timeout: 20_000,
    });
    expect(await page.evaluate(() => (window as any).__noReload)).toBe('alive');
  });

  // 7. Item detail: edit + save, then a competing PATCH forces a 409 -------
  await test.step('item detail edit, save, and 409 merge path', async () => {
    // Navigate in-app (preserve the in-memory session): click the card title.
    await cardIn(page, 'Active', 'DEMO-T-0002')
      .locator('a.kairos-card__title')
      .click();
    await page.waitForURL(/\/items\/DEMO-T-0002/);
    const editor = page.locator('.kairos-editor');
    const contentArea = editor.locator('textarea.kairos-editor__textarea');
    await expect(contentArea).toBeVisible();

    // --- a successful edit + save ---
    await contentArea.fill(`Edited by smoke ${Date.now()}`);
    await editor.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText(/Saved — the item is now at v/)).toBeVisible();

    // The page refetched; wait for the editor to settle on the saved version.
    const token = await mintToken({ server: GUI });
    const saved = await getTask(GUI, token, 'DEMO-T-0002');
    await expect(editor.getByText(`editing v${saved.version}`)).toBeVisible();

    // --- force a 409 via API mid-edit ---
    await contentArea.fill(`My conflicting edit ${Date.now()}`);
    const serverContent = `Server won ${Date.now()}`;
    await patchTask(GUI, token, 'DEMO-T-0002', {
      title: saved.title,
      content: serverContent,
      version: saved.version,
    });

    await editor.getByRole('button', { name: 'Save' }).click();

    // The merge dialog opens on the conflict.
    const dialog = page.locator('[role="dialog"]');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('Edit conflict')).toBeVisible();
    await expect(
      dialog.getByText(`server is at v${saved.version + 1}`),
    ).toBeVisible();

    // Walk one merge path: take theirs → the editor adopts server content.
    await dialog.getByRole('button', { name: 'Take theirs' }).click();
    await expect(dialog).toBeHidden();
    await expect(contentArea).toHaveValue(serverContent);
  });

  // 8. Logout --------------------------------------------------------------
  await test.step('logout', async () => {
    await page.locator('.cl-appshell__header')
      .getByRole('button', { name: 'Log out' })
      .click();
    await page.waitForURL(/\/login/);
    await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
  });
});
