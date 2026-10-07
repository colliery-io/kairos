// COLLIERY-T-0239, COLLIERY-T-0253, KAIROS-T-0328 — a long column.
//
// A column has a floor of 280px. The board scrolls sideways as one, so the
// columns of the two lanes stay in line, and the page never scrolls
// sideways. Up and down, each column scrolls on its own: a column of 30
// cards or more is no taller than the window, and its cards scroll inside
// it. Each column of a lane is as tall as the others (COLLIERY-T-0253), so
// a card drops at its own height in each column.
//
// Runs against the web-delivery board, as drag.spec.ts does, so that it
// does not change the platform-delivery fixture of smoke.spec.ts.
//
//   1. alice creates 32 tasks on web-delivery through the API; they start
//      in Backlog, in the planned lane
//   2. REAL PKCE login (alice), open Web Delivery
//   3. the columns are at least 280px wide and a card fills its column;
//      the columns of the two lanes are in line; the page does not scroll
//      sideways; Backlog scrolls inside itself, and its scroll moves
//      neither Todo nor the page; the columns of a lane are equally tall
//   4. drag the card at the bottom of Backlog to Todo → it lands in Todo of
//      the planned lane
//   5. drag the next bottom card to Backlog of the support lane → it lands
//      in the support lane
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

    await test.step('the columns are wide, the board scrolls sideways as one, each column scrolls on its own', async () => {
      // KAIROS-T-0328: a column has a floor of 280px, and a card is as wide
      // as its column.
      for (const name of columns) {
        const box = await column(page, name).boundingBox();
        if (!box) throw new Error(`${name} has no bounding box`);
        expect(box.width, `width of ${name}`).toBeGreaterThanOrEqual(279);
      }
      const backlog = column(page, 'Backlog');
      const backlogBox = await backlog.boundingBox();
      const cardBox = await backlog.locator('article.kairos-card').first().boundingBox();
      if (!backlogBox || !cardBox) throw new Error('no bounding box');
      expect(cardBox.width).toBeGreaterThan(backlogBox.width - 40);

      // The columns of the two lanes stay in line: the board scrolls
      // sideways as one container, and the page does not.
      for (const name of columns) {
        const planned = await column(page, name).boundingBox();
        const support = await column(page, name, 'support').boundingBox();
        if (!planned || !support) throw new Error(`${name} has no bounding box`);
        expect(Math.abs(planned.x - support.x), `x of ${name}`).toBeLessThan(1);
      }
      const lanes = page.locator('.kairos-board__lanes');
      await expect(lanes).toHaveCSS('overflow-x', 'auto');
      const pageScrolls = await page.evaluate(
        () => document.documentElement.scrollWidth > window.innerWidth,
      );
      expect(pageScrolls, 'the page does not scroll sideways').toBe(false);

      // Up and down, each column scrolls on its own: the long Backlog is
      // no taller than the window, its cards scroll inside it, and a
      // scroll of Backlog moves neither Todo nor the page.
      const viewport = page.viewportSize();
      if (!viewport) throw new Error('the page has no viewport size');
      expect(backlogBox.height).toBeLessThan(viewport.height);
      const cards = backlog.locator('.kairos-board__cards');
      const overflows = await cards.evaluate((el) => el.scrollHeight > el.clientHeight);
      expect(overflows, 'the cards of Backlog scroll inside the column').toBe(true);
      const todoCards = column(page, 'Todo').locator('.kairos-board__cards');
      const pageY = await page.evaluate(() => window.scrollY);
      await cards.evaluate((el) => el.scrollTo(0, el.scrollHeight));
      expect(await cards.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
      expect(await todoCards.evaluate((el) => el.scrollTop)).toBe(0);
      expect(await page.evaluate(() => window.scrollY)).toBe(pageY);
      // Each column of a lane is as tall as the others (COLLIERY-T-0253):
      // a card drops at its own height in each column.
      for (const name of columns) {
        const box = await column(page, name).boundingBox();
        if (!box) throw new Error(`${name} has no bounding box`);
        expect(Math.abs(box.y - backlogBox.y), `top of ${name}`).toBeLessThan(1);
        expect(Math.abs(box.height - backlogBox.height), `height of ${name}`).toBeLessThan(1);
      }
    });

    await test.step('drag the card at the bottom of Backlog to Todo — the card lands', async () => {
      const code = await bottomOfBacklog(page);
      expect(created).toContain(code);
      const card = (name: string, lane: Lane = 'planned'): Locator =>
        column(page, name, lane).locator('article.kairos-card', { hasText: code });
      await card('Backlog').scrollIntoViewIfNeeded();
      await expect(card('Backlog')).toBeInViewport();
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

    await test.step('drag a card to Backlog of the support lane — the card lands', async () => {
      const code = await bottomOfBacklog(page);
      expect(created).toContain(code);
      const card = (lane: Lane): Locator =>
        column(page, 'Backlog', lane).locator('article.kairos-card', { hasText: code });
      await card('planned').scrollIntoViewIfNeeded();
      await expect(card('planned')).toBeInViewport();
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
