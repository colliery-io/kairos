// COLLIERY-T-0239, COLLIERY-T-0253 — a drag from the bottom of a long column.
//
// The page scrolls, and a column of 30 cards or more is some screens tall.
// Each column is as tall as the tallest column of its lane
// (COLLIERY-T-0253), so a person drops the card at the bottom of the long
// column on the adjacent column with no scroll. A column of a different
// lane is not in the viewport of that card. The drag helper must move the
// card in that case also (COLLIERY-T-0239): it turns the wheel during the
// drag.
//
// Runs against the web-delivery board, as drag.spec.ts does, so that it
// does not change the platform-delivery fixture of smoke.spec.ts.
//
//   1. alice creates 32 tasks on web-delivery through the API; they start
//      in Backlog, in the planned lane
//   2. REAL PKCE login (alice), open Web Delivery
//   3. each column of the planned lane is as tall as Backlog, and the
//      lanes do not overlap
//   4. drag the card at the bottom of Backlog to Todo at the height of the
//      card, with no scroll → the card lands in Todo of the planned lane
//   5. the column Backlog of the support lane is not in the viewport of
//      the card that is now at the bottom; drag that card to it → the
//      card lands in the support lane
//   6. archive the 32 tasks (also when a step before this one fails), so
//      that the specs that run after this one find the board as it was

import { test, expect, type Locator, type Page } from '@playwright/test';
import { dragAcross, dragTo } from '../helpers/drag';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, readTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);
const BOARD = 'web-delivery';
const CARDS = 32;

type Lane = 'planned' | 'support';

const laneOf = (page: Page, lane: Lane): Locator =>
  page.locator(`section.kairos-board__lane--${lane}`);

const column = (page: Page, name: string, lane: Lane = 'planned'): Locator =>
  laneOf(page, lane).locator('section.kairos-board__column', {
    has: page.locator('.kairos-board__column-head', { hasText: name }),
  });

/** The code of the card at the bottom of Backlog of the planned lane. */
async function bottomOfBacklog(page: Page): Promise<string> {
  const bottom = column(page, 'Backlog').locator('article.kairos-card').last();
  return (await bottom.locator('a.kairos-card__code').innerText()).trim();
}

test('drag and drop: a card at the bottom of a long column moves with no scroll, and to a different lane', async ({
  page,
}) => {
  // The API token FIRST: Dex keeps one refresh token for each user and
  // client, so a mint after the browser login of the same person would
  // make the browser session invalid (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const board = await loadBoard(GUI, alice, (b) => b.slug === BOARD);
  const columns = [...board.columnId.keys()];
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

    await test.step('each column of the planned lane is as tall as Backlog; the lanes do not overlap', async () => {
      const long = await column(page, 'Backlog').boundingBox();
      if (!long) throw new Error('Backlog has no bounding box');
      const viewport = page.viewportSize();
      if (!viewport) throw new Error('the page has no viewport size');
      // The column is some screens tall: the spec is about that board.
      expect(long.height).toBeGreaterThan(viewport.height * 2);
      for (const name of columns) {
        const box = await column(page, name).boundingBox();
        if (!box) throw new Error(`${name} has no bounding box`);
        expect(Math.abs(box.y - long.y), `top of ${name}`).toBeLessThan(1);
        expect(Math.abs(box.height - long.height), `height of ${name}`).toBeLessThan(1);
      }
      const support = await laneOf(page, 'support').boundingBox();
      const planned = await laneOf(page, 'planned').boundingBox();
      if (!support || !planned) throw new Error('a lane has no bounding box');
      expect(support.y + support.height).toBeLessThanOrEqual(planned.y);
      for (const name of columns) {
        const box = await column(page, name, 'support').boundingBox();
        if (!box) throw new Error(`support ${name} has no bounding box`);
        expect(box.y + box.height, `support ${name}`).toBeLessThanOrEqual(planned.y);
      }
    });

    await test.step('drag to Todo at the height of the card, with no scroll — the card lands', async () => {
      const code = await bottomOfBacklog(page);
      expect(created).toContain(code);
      const card = (name: string, lane: Lane = 'planned'): Locator =>
        column(page, name, lane).locator('article.kairos-card', { hasText: code });
      await card('Backlog').scrollIntoViewIfNeeded();
      await expect(card('Backlog')).toBeInViewport();
      // The head of Todo is some screens above. The column is here.
      await expect(column(page, 'Todo').locator('.kairos-board__column-head')).not.toBeInViewport();
      await expect(card('Backlog')).toHaveAttribute('draggable', 'true');
      await dragAcross(page, card('Backlog'), column(page, 'Todo'));
      await expect(card('Todo')).toBeVisible({ timeout: 15_000 });
      await expect(card('Backlog')).toHaveCount(0);
      // The drop is in the lane that the pointer was over.
      await expect(card('Todo', 'support')).toHaveCount(0);
      const task = await readTask(GUI, alice, code);
      expect(task.column_id).toBe(board.columnId.get('Todo'));
      expect(task.work_class).toBe('planned');
    });

    await test.step('drag to Backlog of the support lane, which is not in the viewport — the card lands', async () => {
      const code = await bottomOfBacklog(page);
      expect(created).toContain(code);
      const card = (lane: Lane): Locator =>
        column(page, 'Backlog', lane).locator('article.kairos-card', { hasText: code });
      await card('planned').scrollIntoViewIfNeeded();
      await expect(card('planned')).toBeInViewport();
      await expect(column(page, 'Backlog', 'support')).not.toBeInViewport();
      await dragTo(page, card('planned'), column(page, 'Backlog', 'support'));
      await expect(card('support')).toBeVisible({ timeout: 15_000 });
      await expect(card('planned')).toHaveCount(0);
      const task = await readTask(GUI, alice, code);
      expect(task.column_id).toBe(board.columnId.get('Backlog'));
      expect(task.work_class).toBe('support');
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
