// COLLIERY-T-0233 — the graph view shows a blocks arrow to completed work as
// history, and a board card hears a blocker on a different board.
//
// The rule is COLLIERY-T-0214: a `blocks` edge counts only while no end of
// the edge is in a done column. The board counts followed the rule. The
// graph view did not: each arrow had one style. And a board filtered its
// events to its own board, so the count on a card stayed stale when a
// blocker on a DIFFERENT board was completed.
//
// The cast: alice (org admin) writes through the API. bob watches in the
// browser. The spec makes its own cards, with a per-run suffix:
//
//   waiting          on platform-delivery, the board that bob watches
//   blocker, second  on web-delivery, each blocks `waiting`
//
//   a. the card of `waiting` shows "blocked by 2"
//   b. alice completes `blocker` on the other board: the card shows
//      "blocked by 1", with no reload of the page
//   c. the graph of `waiting`: the node of `blocker` says "done", the
//      arrow from it has the resolved style, the arrow from `second` has
//      the open style
//   d. the legend names the two styles
//   e. alice completes `second`: the open canvas changes the second arrow
//      to the resolved style, with no reload
//
// Each transition in the set-up is an API call (the drag helper fails when
// the Backlog column is long). The cards leave Backlog of web-delivery at
// once, and the spec archives its cards at the end, so drag.spec.ts finds
// the board as it was.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, tryArchiveTask, tryCreateRelationship } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);
const WATCHED = 'platform-delivery';
const ELSEWHERE = 'web-delivery';

interface Board {
  id: string;
  columnId: Map<string, string>; // column name -> id
  transitions: { from: string; to: string }[];
}

async function getJson(token: string, path: string): Promise<any> {
  const res = await fetch(GUI + path, { headers: { authorization: `Bearer ${token}` } });
  if (!res.ok) throw new Error(`GET ${path} -> ${res.status}: ${await res.text()}`);
  return res.json();
}

async function loadBoardGraph(token: string, slug: string): Promise<Board> {
  const boards = (await getJson(token, '/api/boards?limit=100')).items as any[];
  const board = boards.find((b) => b.slug === slug);
  if (!board) throw new Error(`no board ${slug}`);
  const detail = await getJson(token, `/api/boards/${board.id}`);
  return {
    id: board.id,
    columnId: new Map(detail.columns.map((c: any) => [c.name, c.id])),
    transitions: detail.transitions.map((t: any) => ({
      from: t.from_column_id,
      to: t.to_column_id,
    })),
  };
}

/**
 * Move a task to the column `name` through the API, along the shortest
 * path that the transition graph of the board permits.
 */
async function moveTo(token: string, board: Board, code: string, name: string) {
  const goal = board.columnId.get(name);
  if (!goal) throw new Error(`no column ${name}`);
  const start = (await getJson(token, `/api/tasks/${code}`)).column_id as string;
  const previous = new Map<string, string>();
  const queue = [start];
  while (queue.length > 0 && !previous.has(goal)) {
    const at = queue.shift()!;
    for (const t of board.transitions.filter((t) => t.from === at)) {
      if (t.to !== start && !previous.has(t.to)) {
        previous.set(t.to, at);
        queue.push(t.to);
      }
    }
  }
  const path: string[] = [];
  for (let at = goal; at !== start; at = previous.get(at)!) {
    if (!previous.has(at)) throw new Error(`no path to ${name} for ${code}`);
    path.unshift(at);
  }
  for (const to of path) {
    const res = await fetch(`${GUI}/api/tasks/${code}/transition`, {
      method: 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ to_column_id: to }),
    });
    if (!res.ok) throw new Error(`transition ${code} -> ${res.status}: ${await res.text()}`);
  }
}

const node = (page: Page, code: string) =>
  page.locator('.kairos-graph__node', {
    has: page.locator('.kairos-graph__code', { hasText: code }),
  });

test('graph-done: a completed blocker on a different board changes the card and the arrow', async ({
  page,
}) => {
  // API token FIRST, and for a different person than the browser (Dex
  // keeps one refresh token for each user and client).
  const alice = await mintToken({ server: GUI });
  const watched = await loadBoardGraph(alice, WATCHED);
  const elsewhere = await loadBoardGraph(alice, ELSEWHERE);

  const made: string[] = [];
  const make = async (title: string, board: Board): Promise<string> => {
    const task = await createTask(GUI, alice, { title: `${title} ${RUN}`, boardId: board.id });
    made.push(task.short_code);
    return task.short_code;
  };

  try {
    const waiting = await make('T-0233 waiting', watched);
    const blocker = await make('T-0233 blocker', elsewhere);
    const second = await make('T-0233 second blocker', elsewhere);
    await moveTo(alice, elsewhere, blocker, 'Active');
    await moveTo(alice, elsewhere, second, 'Active');
    for (const source of [blocker, second]) {
      expect(
        await tryCreateRelationship(GUI, alice, {
          source,
          target: waiting,
          relationship: 'blocks',
        }),
      ).toBe(201);
    }

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

    const card = page.locator('article.kairos-card', { hasText: `T-0233 waiting ${RUN}` });

    await test.step('a. the card shows its two open blockers', async () => {
      await page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }).click();
      await page.waitForURL(/\/boards\/platform-delivery/);
      await expect(card.locator('.cl-pill', { hasText: 'blocked by 2' })).toBeVisible();
    });

    await test.step('b. a blocker on a different board is completed: the count changes, no reload', async () => {
      // A mark on the window object does not survive a reload or a
      // navigation, so it proves that the page stayed.
      await page.evaluate(() => {
        (window as any).__t0233 = 'same page';
      });
      await moveTo(alice, elsewhere, blocker, 'Completed');
      await expect(card.locator('.cl-pill', { hasText: 'blocked by 1' })).toBeVisible({
        timeout: 20_000,
      });
      await expect(card.locator('.cl-pill', { hasText: 'blocked by 2' })).toHaveCount(0);
      expect(await page.evaluate(() => (window as any).__t0233)).toBe('same page');
    });

    const resolved = page.locator('.kairos-graph__edge--resolved');
    const open = page.locator('.kairos-graph__edge:not(.kairos-graph__edge--resolved)');

    await test.step('c. the graph draws the completed blocker as history', async () => {
      await card.locator('.kairos-card__blocks').first().click();
      await page.waitForURL(new RegExp(`/items/${waiting}\\?view=graph`));
      await expect(
        page.locator('.kairos-graph__node--focus .kairos-graph__code'),
      ).toHaveText(waiting);

      // The node says that its item is in a done column.
      await expect(node(page, blocker)).toHaveClass(/kairos-graph__node--done/);
      await expect(node(page, blocker).locator('.kairos-graph__done')).toHaveText('done');
      await expect(node(page, second)).not.toHaveClass(/kairos-graph__node--done/);
      await expect(node(page, second).locator('.kairos-graph__done')).toHaveCount(0);
      await expect(node(page, waiting).locator('.kairos-graph__done')).toHaveCount(0);

      // Two arrows, one of each style.
      await expect(resolved).toHaveCount(1);
      await expect(open).toHaveCount(1);
      await expect(resolved.locator('title')).toHaveText(
        'Resolved: one end is in a done column. This edge does not block.',
      );
      await expect(open.locator('title')).toHaveText('Open blocker');

      // The styles are different for the eye: the resolved arrow has a
      // dash and a different stroke.
      const style = (el: Element) => {
        const computed = getComputedStyle(el);
        return { dash: computed.strokeDasharray, stroke: computed.stroke };
      };
      const resolvedStyle = await resolved.evaluate(style);
      const openStyle = await open.evaluate(style);
      expect(openStyle.dash).toBe('none');
      expect(resolvedStyle.dash).not.toBe('none');
      expect(resolvedStyle.stroke).not.toBe(openStyle.stroke);
      expect(await resolved.getAttribute('marker-end')).toBe(
        'url(#kairos-graph-arrowhead-resolved)',
      );
      expect(await open.getAttribute('marker-end')).toBe('url(#kairos-graph-arrowhead)');
    });

    await test.step('d. the legend names the two styles', async () => {
      const legend = page.locator('.kairos-graph__legend');
      await expect(legend.getByText('Open blocker', { exact: true })).toBeVisible();
      await expect(
        legend.getByText('Resolved: one end is in a done column', { exact: true }),
      ).toBeVisible();
      // Each sample has the style of the arrow that it explains.
      const dash = (el: Element) => getComputedStyle(el).strokeDasharray;
      const samples = legend.locator('.kairos-graph__legend-line');
      await expect(samples).toHaveCount(2);
      expect(await samples.nth(0).evaluate(dash)).toBe(await open.evaluate(dash));
      expect(await samples.nth(1).evaluate(dash)).toBe(await resolved.evaluate(dash));
    });

    await test.step('e. the second blocker is completed: the open canvas follows', async () => {
      await moveTo(alice, elsewhere, second, 'Completed');
      await expect(resolved).toHaveCount(2, { timeout: 20_000 });
      await expect(open).toHaveCount(0);
      await expect(node(page, second).locator('.kairos-graph__done')).toHaveText('done');
    });
  } finally {
    // Put the cards away, so that the boards are as the seed made them.
    for (const code of made) {
      await tryArchiveTask(GUI, alice, code);
    }
  }
});
