// COLLIERY-T-0265 — the helper that reads the items of a board reads each
// page.
//
// `GET /api/boards/{id}/items` gives 200 items by default. The helpers of
// this package read the route one time, so they had the first 200 items of
// a board only. This spec has no browser and no server: it gives pages to
// `readEachBoardPage`, as the route gives them.
//
//   1. a board of 450 items in 3 columns: the helper has each item, in the
//      order of the board, and one read of the route has 200
//   2. the maps `children_progress` and `blocks_summary` have the entries
//      of each page
//   3. a page with no item stops the read

import { test, expect } from '@playwright/test';
import { boardItemCount, readEachBoardPage } from '../helpers/board-items';

const COLUMNS = ['Backlog', 'Active', 'Done'];

/** The items of the board, in the order of the route. */
function board(total: number): { column: string; family: string; code: string }[] {
  return Array.from({ length: total }, (_, index) => ({
    column: COLUMNS[Math.floor((index * COLUMNS.length) / total)],
    // An initiative now and then: the helper joins the four lists.
    family: index % 50 === 0 ? 'initiatives' : 'tasks',
    code: `DEMO-X-${String(index + 1).padStart(4, '0')}`,
  }));
}

/** One page, as the route gives it: each column is in each page. */
function pageOf(items: ReturnType<typeof board>, limit: number, offset: number, total = items.length) {
  const part = items.slice(offset, offset + limit);
  return {
    board: { id: 'b-1', slug: 'demo-delivery' },
    columns: COLUMNS.map((name) => ({
      column: { id: `c-${name}`, name },
      strategies: [],
      initiatives: part
        .filter((item) => item.column === name && item.family === 'initiatives')
        .map((item) => ({ short_code: item.code })),
      tasks: part
        .filter((item) => item.column === name && item.family === 'tasks')
        .map((item) => ({ short_code: item.code })),
      adrs: [],
    })),
    total,
    limit,
    offset,
    children_progress: Object.fromEntries(
      part.filter((item) => item.family === 'initiatives').map((item) => [item.code, { total: 1 }]),
    ),
    blocks_summary: part.length > 0 ? { [part[0].code]: { blocked_by: 1, blocks: 0 } } : {},
  };
}

const codesOf = (items: any, family: string): string[] =>
  (items.columns as any[]).flatMap((group) => group[family].map((item: any) => item.short_code));

test('board items helper: the helper reads each page of a board', async () => {
  const items = board(450);
  const reads: [number, number][] = [];
  const all = await readEachBoardPage(async (limit, offset) => {
    reads.push([limit, offset]);
    return pageOf(items, limit, offset);
  }, 200);

  // One read of the route has the first 200 items only.
  expect(boardItemCount(pageOf(items, 200, 0))).toBe(200);
  // The helper has each item.
  expect(reads).toEqual([
    [200, 0],
    [200, 200],
    [200, 400],
  ]);
  expect(boardItemCount(all)).toBe(450);
  expect(all.total).toBe(450);
  expect(all.columns.map((group: any) => group.column.name)).toEqual(COLUMNS);
  expect(codesOf(all, 'tasks')).toEqual(
    items.filter((item) => item.family === 'tasks').map((item) => item.code),
  );
  expect(codesOf(all, 'initiatives')).toEqual(
    items.filter((item) => item.family === 'initiatives').map((item) => item.code),
  );
  // The last item of the board is in the last column.
  expect(all.columns[2].tasks.at(-1).short_code).toBe('DEMO-X-0450');
  // The maps have the entries of each page.
  expect(Object.keys(all.children_progress).length).toBe(9);
  expect(Object.keys(all.blocks_summary).sort()).toEqual([
    'DEMO-X-0001',
    'DEMO-X-0201',
    'DEMO-X-0401',
  ]);
});

test('board items helper: a small board is one read, and an empty page stops the read', async () => {
  const small = board(12);
  let reads = 0;
  const all = await readEachBoardPage(async (limit, offset) => {
    reads += 1;
    return pageOf(small, limit, offset);
  });
  expect(reads).toBe(1);
  expect(boardItemCount(all)).toBe(12);

  // A person archived 100 items after the first page: `total` of the first
  // page is 300, and the second page has no item.
  const shrunk = board(200);
  reads = 0;
  const partial = await readEachBoardPage(async (limit, offset) => {
    reads += 1;
    return pageOf(shrunk, limit, offset, offset === 0 ? 300 : 200);
  }, 200);
  expect(reads).toBe(2);
  expect(boardItemCount(partial)).toBe(200);
  expect(partial.total).toBe(200);
});
