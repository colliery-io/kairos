// COLLIERY-T-0265 — the board view reads the one board that it shows.
//
// The URL of the board view has the slug of the board. Before the ticket
// the view read the full list of the boards at each read, and at each live
// re-fetch, to get the id of the board. The read routes of a board now take
// the slug, so the view reads `/api/boards/<slug>` and
// `/api/boards/<slug>/items`.
//
//   1. alice makes a task of her own on platform-delivery, through the API
//   2. REAL PKCE login (alice), open the board from the overview → the
//      view reads the board by its slug, and it does not read the list
//   3. a second writer moves the task → the board shows the move (the live
//      re-fetch), and the re-fetch does not read the list
//   4. a slug that no board has → the page shows the 404 of the server
//   5. archive the task (also when a step before this one fails)

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadPlatformDelivery, transitionTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);
const BOARD = 'platform-delivery';

/** The paths of the requests to the board routes, from `start()`. */
function watchBoardRequests(page: Page) {
  let paths: string[] = [];
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (request.method() === 'GET' && url.pathname.startsWith('/api/boards')) {
      paths.push(url.pathname);
    }
  });
  return {
    start: () => {
      paths = [];
    },
    paths: () => [...paths],
  };
}

test('board view: the view reads the one board that it shows', async ({ page }) => {
  // The API token FIRST (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const title = `board view read ${RUN}`;
  const task = await createTask(GUI, alice, { title, boardId: BOARD });
  const code = task.short_code as string;

  try {
    const board = await loadPlatformDelivery(GUI, alice);
    const move = board.transitions.find((t) => t.from === task.column_id);
    expect(move, 'a transition from the column of the new task').toBeTruthy();
    const target = board.columnName.get(move!.to)!;
    const requests = watchBoardRequests(page);

    await test.step('login via Dex as alice, open the board from the overview', async () => {
      await page.goto('/');
      await page.waitForSelector('#login', { timeout: 30_000 });
      await page.fill('#login', 'alice@kairos.test');
      await page.fill('#password', 'alice-password');
      await page.click('#submit-login');
      await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
        timeout: 30_000,
      });
      const tile = page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' });
      await expect(tile).toBeVisible();
      // The overview reads the list of the boards. The board view starts
      // at the click.
      requests.start();
      await tile.click();
      await page.waitForURL(new RegExp(`/boards/${BOARD}$`));
      await expect(page.locator('article.kairos-card', { hasText: title })).toBeVisible({
        timeout: 30_000,
      });
    });

    await test.step('the view reads the board by its slug, and not the list', async () => {
      const paths = requests.paths();
      expect(paths).toContain(`/api/boards/${BOARD}`);
      expect(paths).toContain(`/api/boards/${BOARD}/items`);
      expect(paths, 'no read of the list of the boards').not.toContain('/api/boards');
    });

    await test.step('a live re-fetch reads the board, and not the list', async () => {
      requests.start();
      await transitionTask(GUI, alice, code, move!.to);
      const column = page.locator('section.kairos-board__column', {
        has: page.locator('.kairos-board__column-head', { hasText: target }),
      });
      await expect(column.locator('article.kairos-card', { hasText: title })).toBeVisible({
        timeout: 20_000,
      });
      const paths = requests.paths();
      expect(paths).toContain(`/api/boards/${BOARD}/items`);
      expect(paths, 'no read of the list of the boards').not.toContain('/api/boards');
    });

    await test.step('a slug that no board has: the page shows the refusal', async () => {
      // An in-app navigation: the SPA keeps its session.
      await page.evaluate(() => {
        window.history.pushState({}, '', '/boards/no-such-board');
        window.dispatchEvent(new PopStateEvent('popstate'));
      });
      await expect(
        page.getByText('No live board has the slug or the id "no-such-board".'),
      ).toBeVisible({ timeout: 20_000 });
    });
  } finally {
    expect([200, 204]).toContain(await tryArchiveTask(GUI, alice, code));
  }
});
