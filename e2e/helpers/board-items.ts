// COLLIERY-T-0265 — each item of a board, page after page.
//
// `GET /api/boards/{id}/items` has pages since COLLIERY-T-0261: 200 items by
// default, and 1000 at most. A helper that reads the route one time gets the
// first 200 items, and a spec that looks for a card after them does not find
// it. Each helper of this package that reads the items of a board uses
// `boardItems`.
//
// `readEachBoardPage` has the rule and no network, so a spec can run it
// against pages that it makes (board-items-helper.spec.ts).

/** The maximum of `limit` of the route. */
export const BOARD_PAGE_LIMIT = 1000;

const FAMILIES = ['strategies', 'initiatives', 'tasks', 'adrs'] as const;

/** One page of the route, for a limit and an offset. */
export type ReadBoardPage = (limit: number, offset: number) => Promise<any>;

/** The number of items of a response of the route. */
export function boardItemCount(items: any): number {
  return ((items.columns ?? []) as any[]).reduce(
    (count, group) =>
      count + FAMILIES.reduce((n, family) => n + (group[family] ?? []).length, 0),
    0,
  );
}

/**
 * Each item of a board as ONE response: the pages of `readPage`, from
 * offset 0, until the response has `total` items. The lists of a column
 * keep the order of the pages. `children_progress` and `blocks_summary`
 * have the entries of each page.
 *
 * A page with no item stops the read: an item that a person archived
 * between two pages must not make a loop with no end.
 */
export async function readEachBoardPage(
  readPage: ReadBoardPage,
  limit: number = BOARD_PAGE_LIMIT,
): Promise<any> {
  const all = await readPage(limit, 0);
  const groupOf = new Map<string, any>(
    ((all.columns ?? []) as any[]).map((group) => [group.column.id, group]),
  );
  let read = boardItemCount(all);
  while (read < (all.total ?? 0)) {
    const page = await readPage(limit, read);
    const added = boardItemCount(page);
    if (added === 0) {
      all.total = page.total;
      break;
    }
    for (const group of (page.columns ?? []) as any[]) {
      const known = groupOf.get(group.column.id);
      if (!known) {
        groupOf.set(group.column.id, group);
        all.columns.push(group);
        continue;
      }
      for (const family of FAMILIES) {
        known[family] = [...(known[family] ?? []), ...(group[family] ?? [])];
      }
    }
    all.children_progress = { ...(all.children_progress ?? {}), ...(page.children_progress ?? {}) };
    all.blocks_summary = { ...(all.blocks_summary ?? {}), ...(page.blocks_summary ?? {}) };
    read += added;
  }
  all.limit = read;
  all.offset = 0;
  return all;
}

/**
 * Each item of a board. `board` is the slug or the id of the board
 * (COLLIERY-T-0265). `query` has the filters of the route, without
 * `limit` and `offset`: `include_deleted=true`, `repository=<slug>`.
 */
export async function boardItems(
  server: string,
  token: string,
  board: string,
  query: Record<string, string> = {},
): Promise<any> {
  return readEachBoardPage(async (limit, offset) => {
    const params = new URLSearchParams({ ...query, limit: String(limit), offset: String(offset) });
    const path = `/api/boards/${encodeURIComponent(board)}/items?${params}`;
    const res = await fetch(server + path, { headers: { authorization: `Bearer ${token}` } });
    if (!res.ok) throw new Error(`GET ${path} -> ${res.status}: ${await res.text()}`);
    return res.json();
  });
}
