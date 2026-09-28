// COLLIERY-T-0261 — a board with more than 1000 cards shows each card.
//
// `GET /api/boards/{id}/items` has pages: 200 items by default, and 1000 at
// most. The board view reads each page, up to 2000 cards. Before the ticket
// the route had no limit, and a view that read only the default page would
// show 200 cards of this board.
//
// Runs against a team of its own, so that it does not change the fixtures
// of the other specs.
//
//   1. alice creates a team, and 1005 tasks on its delivery board, through
//      the API (E2E_BOARD_CARDS gives a different number). The board view
//      reads 1000 cards in one request, so this board is 2 requests
//   2. the API gives 200 items by default, and `total` is 1005; the pages
//      have no item in common
//   3. REAL PKCE login (alice), open the board → the board shows 1005
//      cards, and it does not show the note of the cap
//   4. archive the tasks and delete the team (also when a step before this
//      one fails)

import { test, expect } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);
const TEAM = `pages-${RUN}`;
const BOARD = `${TEAM}-delivery`;
const CARDS = Number(process.env.E2E_BOARD_CARDS ?? 1005);
/** The number of requests that run at the same time. */
const BATCH = 8;

test.setTimeout(90_000 + CARDS * 300);

async function call(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(GUI + path, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${await res.text()}`);
  return res.json();
}

const codesOf = (items: any): string[] =>
  (items.columns as any[]).flatMap((group) =>
    ['strategies', 'initiatives', 'tasks', 'adrs'].flatMap((family) =>
      (group[family] as any[]).map((item) => item.short_code as string),
    ),
  );

test('board pages: a board with more than 1000 cards shows each card', async ({ page }) => {
  // The API token FIRST: Dex keeps one refresh token for each user and
  // client (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const created: string[] = [];
  let teamId: string | undefined;

  try {
    let boardId = '';
    await test.step(`create the team and ${CARDS} tasks`, async () => {
      const team = await call(alice, 'POST', '/api/teams', {
        name: `Pages ${RUN}`,
        slug: TEAM,
      });
      teamId = team.id;
      boardId = team.delivery_board_id;
      for (let first = 0; first < CARDS; first += BATCH) {
        const numbers = Array.from(
          { length: Math.min(BATCH, CARDS - first) },
          (_, index) => first + index + 1,
        );
        const tasks = await Promise.all(
          numbers.map((n) =>
            createTask(GUI, alice, {
              title: `page card ${RUN} ${String(n).padStart(4, '0')}`,
              boardId,
            }),
          ),
        );
        created.push(...tasks.map((task) => task.short_code));
      }
      expect(created.length).toBe(CARDS);
    });

    await test.step('the API gives pages, and the pages have no item in common', async () => {
      const first = await call(alice, 'GET', `/api/boards/${boardId}/items`);
      expect(first.total).toBe(CARDS);
      expect(first.limit).toBe(200);
      expect(codesOf(first).length).toBe(Math.min(200, CARDS));
      const read: string[] = [];
      for (let offset = 0; offset < CARDS; offset += 100) {
        const part = await call(
          alice,
          'GET',
          `/api/boards/${boardId}/items?limit=100&offset=${offset}`,
        );
        expect(part.total).toBe(CARDS);
        read.push(...codesOf(part));
      }
      expect(read.length).toBe(CARDS);
      expect(new Set(read).size).toBe(CARDS);
      expect([...read].sort()).toEqual([...created].sort());
    });

    await test.step('login via Dex as alice, open the board: each card is there', async () => {
      await page.goto('/');
      await page.waitForSelector('#login', { timeout: 30_000 });
      await page.fill('#login', 'alice@kairos.test');
      await page.fill('#password', 'alice-password');
      await page.click('#submit-login');
      await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
        timeout: 30_000,
      });
      await page.goto(`/boards/${BOARD}`);
      await expect(
        page.locator('article.kairos-card', { hasText: `page card ${RUN}` }),
      ).toHaveCount(CARDS, { timeout: 30_000 + CARDS * 20 });
      // The first card and the last card of the board.
      for (const n of [1, CARDS]) {
        await expect(
          page.locator('article.kairos-card', {
            hasText: `page card ${RUN} ${String(n).padStart(4, '0')}`,
          }),
        ).toHaveCount(1);
      }
      // The board has each card, so it does not show the note of the cap.
      await expect(page.getByTestId('board-cap')).toHaveCount(0);
    });
  } finally {
    // Clean-up is not a step of the journey: it runs after a failure also.
    for (let first = 0; first < created.length; first += BATCH) {
      const statuses = await Promise.all(
        created.slice(first, first + BATCH).map((code) => tryArchiveTask(GUI, alice, code)),
      );
      for (const status of statuses) expect([200, 204]).toContain(status);
    }
    if (teamId) await call(alice, 'DELETE', `/api/teams/${teamId}`);
  }
});
