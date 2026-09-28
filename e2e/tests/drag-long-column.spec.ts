// COLLIERY-T-0239 — a drag from the bottom of a long column.
//
// The columns of a board are as tall as their cards, and the page scrolls.
// When a column has 30 cards or more, the card at the bottom and the head
// of the adjacent column are not in one viewport. The drag helper must
// move the card in that case also.
//
// Runs against the web-delivery board, as drag.spec.ts does, so that it
// does not change the platform-delivery fixture of smoke.spec.ts.
//
//   1. alice creates 32 tasks on web-delivery through the API; they start
//      in Backlog, in the planned lane
//   2. REAL PKCE login (alice), open Web Delivery
//   3. the card at the bottom of Backlog and the column Todo are not in
//      one viewport
//   4. drag that card to Todo → the card lands
//   5. archive the 32 tasks (also when a step before this one fails), so
//      that the specs that run after this one find the board as it was

import { test, expect, type Locator, type Page } from '@playwright/test';
import { dragTo } from '../helpers/drag';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, readTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);
const BOARD = 'web-delivery';
const CARDS = 32;

const column = (page: Page, name: string): Locator =>
  page
    .locator('section.kairos-board__lane--planned')
    .locator('section.kairos-board__column', {
      has: page.locator('.kairos-board__column-head', { hasText: name }),
    });

test('drag and drop: a card at the bottom of a long column moves to the adjacent column', async ({
  page,
}) => {
  // The API token FIRST: Dex keeps one refresh token for each user and
  // client, so a mint after the browser login of the same person would
  // make the browser session invalid (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const board = await loadBoard(GUI, alice, (b) => b.slug === BOARD);
  const created: string[] = [];

  try {
    await test.step(`create ${CARDS} tasks in Backlog`, async () => {
      for (let n = 1; n <= CARDS; n += 1) {
        const task = await createTask(GUI, alice, {
          title: `long column ${RUN} ${String(n).padStart(2, '0')}`,
          boardId: board.id,
        });
        expect(task.column_id).toBe(board.entryColumnId);
        created.push(task.short_code);
      }
    });

    await test.step('login via Dex as alice, open web-delivery', async () => {
      await page.goto('/');
      await page.waitForSelector('#login', { timeout: 30_000 });
      await page.fill('#login', 'alice@kairos.test');
      await page.fill('#password', 'alice-password');
      await page.click('#submit-login');
      await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
        timeout: 30_000,
      });
      await page.locator('.kairos-board-tile', { hasText: 'Web Delivery' }).click();
      await page.waitForURL(/\/boards\/web-delivery/);
      await expect(
        column(page, 'Backlog').locator('article.kairos-card', { hasText: `long column ${RUN}` }),
      ).toHaveCount(CARDS);
    });

    // The card at the bottom of the column, whichever of the 32 it is.
    const bottom = column(page, 'Backlog').locator('article.kairos-card').last();
    const code = (await bottom.locator('a.kairos-card__code').innerText()).trim();
    expect(created).toContain(code);
    const card = (columnName: string): Locator =>
      column(page, columnName).locator('article.kairos-card', { hasText: code });

    await test.step('the two ends of the drag are not in one viewport', async () => {
      await card('Backlog').scrollIntoViewIfNeeded();
      await expect(card('Backlog')).toBeInViewport();
      await expect(column(page, 'Todo')).not.toBeInViewport();
    });

    await test.step('drag to Todo — the card lands', async () => {
      await expect(card('Backlog')).toHaveAttribute('draggable', 'true');
      await dragTo(page, card('Backlog'), column(page, 'Todo'));
      await expect(card('Todo')).toBeVisible({ timeout: 15_000 });
      await expect(card('Backlog')).toHaveCount(0);
      const task = await readTask(GUI, alice, code);
      expect(task.column_id).toBe(board.columnId.get('Todo'));
    });
  } finally {
    // Clean-up is not a step of the journey: it runs after a failure also.
    for (const code of created) {
      const status = await tryArchiveTask(GUI, alice, code);
      expect([200, 204], `archive ${code} -> ${status}`).toContain(status);
    }
  }

  await test.step('the board is as it was', async () => {
    await page.reload();
    await expect(page.locator('section.kairos-board__column').first()).toBeVisible();
    await expect(
      page.locator('article.kairos-card', { hasText: `long column ${RUN}` }),
    ).toHaveCount(0);
  });
});
