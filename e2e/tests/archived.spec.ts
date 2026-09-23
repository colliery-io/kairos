// KAIROS-T-0164 / KAIROS-T-0163 — archived work on the GUI: read it, read
// its history, restore it, FIND it, and see it as a neighbour
// (KAIROS-A-0020: archived means hidden by default, not gone).
//
// Four journeys:
//
//   1. an ordinary archived task — the item page renders it with the
//      put-away banner instead of an error, every write affordance is
//      DISABLED (not missing), its history renders with the same banner
//      and rollback off, and Restore puts it back;
//   2. an archived card in a column that has since been REMOVED — the
//      page still names the column it was put away in (KAIROS-T-0161:
//      the fact the column soft delete exists to preserve), and Restore
//      is refused with a message that NAMES what is gone;
//   3. (T-0163) the /search toggle: off by default and the hit stays
//      hidden; on, and the same search finds it MARKED;
//   4. (T-0163, carrying T-0158's GUI scope) an archived neighbour is
//      marked in the Relationships panel and drawn marked on the graph
//      canvas — while the children rollup above it stays live-only,
//      which is correct rather than inconsistent.
//
// **Fixture discipline.** The suite is serial over ONE seeded stack and
// other specs count boards, tiles and cards, so this spec creates no
// board and leaves nothing visible behind: the cards it makes end up
// archived, and the column it makes ends up removed — both invisible by
// construction, which is the state under test.
//
// Deep links are used deliberately: archived work is reached by short
// code (and by search, KAIROS-T-0163), never from a board — that is the
// whole point of the state. The session survives a `goto` via the T-0071
// sessionStorage refresh token, as repositories.spec.ts relies on.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const SERVER = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const BOARD_SLUG = 'platform-delivery';

const bearer = (token: string) => ({ authorization: `Bearer ${token}` });

async function api(
  token: string,
  method: string,
  path: string,
  body?: unknown,
): Promise<any> {
  const res = await fetch(SERVER + path, {
    method,
    headers: body
      ? { ...bearer(token), 'content-type': 'application/json' }
      : bearer(token),
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`${method} ${path} -> ${res.status}: ${text}`);
  }
  return text ? JSON.parse(text) : null;
}

/** The seeded delivery board, with its columns (BoardDetail is flat). */
async function deliveryBoard(token: string): Promise<any> {
  const boards = (await api(token, 'GET', '/api/boards?limit=100')).items as any[];
  const board = boards.find((b) => b.slug === BOARD_SLUG);
  if (!board) throw new Error(`no ${BOARD_SLUG} board in the seed`);
  return api(token, 'GET', `/api/boards/${board.id}`);
}

async function createTask(
  token: string,
  boardId: string,
  columnId: string,
  title: string,
): Promise<any> {
  return api(token, 'POST', '/api/tasks', {
    board_id: boardId,
    column_id: columnId,
    title,
    content: '# What it said\n\nThe record outlives the work.',
  });
}

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

test('an archived item reads, its history reads, and it can be restored', async ({
  page,
}) => {
  const token = await mintToken();
  const board = await deliveryBoard(token);
  const entry = board.columns[0];

  const task = await createTask(
    token,
    board.id,
    entry.id,
    'E2E: finished work, put away',
  );
  await api(token, 'DELETE', `/api/tasks/${task.short_code}`);

  await test.step('login via Dex as alice', () => login(page));

  await test.step('the item page renders it, banner first', async () => {
    await page.goto(`/items/${task.short_code}`);
    const banner = page.locator('[data-testid="archived-banner"]').first();
    await expect(banner).toBeVisible();
    await expect(banner).toContainText('Put away on');
    await expect(banner).toContainText('hidden from boards');
    // The state is badged next to the short code, in the same words —
    // never bare "archived", which is the document lifecycle's word.
    // `.first()` since KAIROS-T-0163: the badge is shared with the
    // Relationships panel, which now marks archived NEIGHBOURS too, so
    // the class is no longer unique on an item page. Document order puts
    // the header badge first.
    await expect(page.locator('.kairos-archived-badge').first()).toContainText(
      'put away',
    );
    // The content is there. That is the audit answer.
    await expect(page.getByText('The record outlives the work.')).toBeVisible();
  });

  await test.step('write affordances are disabled, with the reason', async () => {
    await expect(page.getByText('read-only while put away')).toBeVisible();
    await expect(
      page.getByRole('button', { name: 'Delete', exact: true }),
    ).toBeDisabled();
    await expect(
      page.getByRole('button', { name: 'Save', exact: true }),
    ).toBeDisabled();
    // Nothing to type into: the content renders instead of an editor.
    await expect(page.locator('.kairos-editor__textarea')).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Save metadata' }),
    ).toBeDisabled();
  });

  await test.step('the history page is the audit answer, and says so', async () => {
    await page.goto(`/activity/history/${task.short_code}`);
    const banner = page.locator('[data-testid="archived-banner"]').first();
    await expect(banner).toBeVisible();
    await expect(banner).toContainText('Put away on');
    // The versions are intact; rollback — a write — is off.
    const versions = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Versions' }),
    });
    await expect(versions).toContainText('v1');
    await expect(
      versions.getByRole('button', { name: 'Roll back' }).first(),
    ).toBeDisabled();
  });

  await test.step('restore puts it back', async () => {
    await page.goto(`/items/${task.short_code}`);
    await page.getByRole('button', { name: 'Restore' }).click();
    await expect(
      page.getByText(`${task.short_code} is back on its board.`),
    ).toBeVisible();
    await expect(page.locator('[data-testid="archived-banner"]')).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Delete', exact: true }),
    ).toBeEnabled();
  });

  // Leave the seeded board as it was found: the card goes back away.
  await api(token, 'DELETE', `/api/tasks/${task.short_code}`);
});

test('every family renders archived, and the two "archived"s read apart', async ({
  page,
}) => {
  const token = await mintToken();
  const boards = (await api(token, 'GET', '/api/boards?limit=100')).items as any[];
  const entryOf = async (level: string) => {
    const board = boards.find((b) => b.board_level === level);
    if (!board) throw new Error(`no ${level} board in the seed`);
    const detail = await api(token, 'GET', `/api/boards/${board.id}`);
    return { boardId: board.id, columnId: detail.columns[0].id };
  };

  const strategyAt = await entryOf('strategy');
  const strategy = await api(token, 'POST', '/api/strategies', {
    board_id: strategyAt.boardId,
    column_id: strategyAt.columnId,
    title: 'E2E: archived strategy',
    content: 'A bet that finished.',
  });
  const initiativeAt = await entryOf('initiative');
  const initiative = await api(token, 'POST', '/api/initiatives', {
    board_id: initiativeAt.boardId,
    column_id: initiativeAt.columnId,
    title: 'E2E: archived initiative',
    content: 'Delivered, then put away.',
  });
  const adrAt = await entryOf('adr');
  const adr = await api(token, 'POST', '/api/adrs', {
    board_id: adrAt.boardId,
    column_id: adrAt.columnId,
    title: 'E2E: archived decision',
    content: 'Superseded, but still the record.',
  });
  const deliveryAt = await entryOf('delivery');
  const task = await createTask(
    token,
    deliveryAt.boardId,
    deliveryAt.columnId,
    'E2E: archived task with a document',
  );
  const templates = (await api(token, 'GET', '/api/templates?limit=10'))
    .items as any[];
  const doc = await api(token, 'POST', '/api/documents', {
    title: 'E2E: archived document',
    template_id: templates[0].id,
    parent_short_code: task.short_code,
  });
  // The collision, deliberately set up: this document is EDITORIALLY
  // archived (KAIROS-T-0078) *and* about to be put away (ADR-20). Both
  // words land on one screen; they must not read as one thing.
  await api(token, 'PATCH', `/api/documents/${doc.short_code}/lifecycle`, {
    lifecycle: 'archived',
  });

  for (const [family, code] of [
    ['strategies', strategy.short_code],
    ['initiatives', initiative.short_code],
    ['adrs', adr.short_code],
    // The document goes away on its own: A-0001 cascades along PARENT
    // edges, and a document hangs off its workflow item by `supports`.
    ['documents', doc.short_code],
    ['tasks', task.short_code],
  ] as const) {
    await api(token, 'DELETE', `/api/${family}/${code}`);
  }

  await test.step('login via Dex as alice', () => login(page));

  for (const [label, code] of [
    ['Strategy', strategy.short_code],
    ['Initiative', initiative.short_code],
    ['Task', task.short_code],
    ['ADR', adr.short_code],
    ['Document', doc.short_code],
  ] as const) {
    await test.step(`${label} ${code} renders, archived`, async () => {
      await page.goto(`/items/${code}`);
      const banner = page.locator('[data-testid="archived-banner"]').first();
      await expect(banner).toBeVisible();
      await expect(banner).toContainText('Put away on');
      await expect(page.locator('.cl-page-header__sub')).toHaveText(label);
    });
  }

  await test.step('a document can be archived in BOTH senses at once', async () => {
    await page.goto(`/items/${doc.short_code}`);
    // The editorial badge keeps its prefix; the ADR-20 state says "put
    // away" — and the banner spells the difference out.
    await expect(page.locator('.kairos-lifecycle-badge')).toContainText(
      'lifecycle: archived',
    );
    // The header badge — this document's put-away parent task carries
    // one too, in the Relationships panel (KAIROS-T-0163).
    await expect(page.locator('.kairos-archived-badge').first()).toContainText(
      'put away',
    );
    await expect(
      page.locator('[data-testid="archived-banner"]').first(),
    ).toContainText('Two different things are called');
  });
});

test('an archived card still names the column it was put away in', async ({
  page,
}) => {
  const token = await mintToken();
  const board = await deliveryBoard(token);

  // A column of this spec's own, removed again before any other spec
  // looks at the board. Positions collide loudly (422), so take one well
  // past the seeded set.
  const retired = await api(token, 'POST', `/api/boards/${board.id}/columns`, {
    name: 'E2E Retired',
    position: 90,
  });
  const parked = await createTask(
    token,
    board.id,
    retired.id,
    'E2E: parked in a column that went away',
  );
  await api(token, 'DELETE', `/api/tasks/${parked.short_code}`);
  // Only its archived occupant remains, so the removal goes through
  // (KAIROS-T-0161) — and the column ROW survives, which is the point.
  await api(token, 'DELETE', `/api/boards/${board.id}/columns/${retired.id}`);

  await test.step('login via Dex as alice', () => login(page));

  await test.step('the placement survives the column removal', async () => {
    await page.goto(`/items/${parked.short_code}`);
    const placement = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Board' }),
    });
    await expect(placement).toContainText('E2E Retired');
    await expect(placement).toContainText('column since removed');
    await expect(placement).toContainText('Placement is frozen');
  });

  await test.step('the restore refusal names what is gone', async () => {
    await page.getByRole('button', { name: 'Restore' }).click();
    await expect(page.getByText('It cannot go back yet')).toBeVisible();
    await expect(page.getByText(/its board column/)).toBeVisible();
    // Still archived, still readable — that is what makes it humane.
    await expect(
      page.locator('[data-testid="archived-banner"]').first(),
    ).toBeVisible();
  });
});

/** A one-lexeme, alphabetic-only probe word — Postgres full-text search
 *  stems it to a single token, and a fresh one per run keeps the hit on
 *  page 1 of the results however many times the stack is re-driven. */
function probeWord(): string {
  const letters = 'abcdefghijklmnopqrstuvwxyz';
  const tail = Array.from(
    { length: 8 },
    () => letters[Math.floor(Math.random() * letters.length)],
  ).join('');
  return `putaway${tail}`;
}

test('search can ask for put-away work, and marks what it finds', async ({
  page,
}) => {
  const token = await mintToken();
  const board = await deliveryBoard(token);
  const probe = probeWord();

  const task = await createTask(
    token,
    board.id,
    board.columns[0].id,
    `E2E ${probe} finished and put away`,
  );
  await api(token, 'DELETE', `/api/tasks/${task.short_code}`);

  await test.step('login via Dex as alice', () => login(page));
  await page.goto('/search');

  const toggle = page.locator('[data-testid="include-put-away"]');
  const query = page.locator('.cl-field', { hasText: 'Text query' }).locator('input');
  const search = page.getByRole('button', { name: 'Search', exact: true });

  await test.step('the toggle is visible, labelled, and off', async () => {
    // "Visible for audit" means discoverable without reading the docs:
    // a labelled switch on the page, not a URL-only parameter.
    await expect(toggle).toBeVisible();
    await expect(toggle).toContainText('Include work that has been put away');
    await expect(toggle.locator('.cl-switch--on')).toHaveCount(0);
  });

  await test.step('with the toggle off, put-away work stays hidden', async () => {
    await query.fill(probe);
    await search.click();
    await expect(page.getByText('No matches')).toBeVisible();
  });

  await test.step('asking for it finds it, marked apart from live work', async () => {
    await toggle.locator('.cl-switch').click();
    await expect(toggle.locator('.cl-switch--on')).toHaveCount(1);
    await search.click();

    const row = page.locator('tr.kairos-search__hit--put-away', {
      hasText: task.short_code,
    });
    await expect(row).toBeVisible();
    // The words are the item page's words — never a bare "archived",
    // which is the document lifecycle's (KAIROS-T-0078).
    await expect(row.locator('.kairos-archived-badge')).toContainText('put away');
    await expect(
      page.locator('.cl-panel__caption', { hasText: 'put away' }).first(),
    ).toBeVisible();
  });

  await test.step('and the hit leads to a real page', async () => {
    await page
      .locator('tr.kairos-search__hit--put-away', { hasText: task.short_code })
      .getByRole('link', { name: task.short_code })
      .click();
    await page.waitForURL(new RegExp(`/items/${task.short_code}$`));
    await expect(
      page.locator('[data-testid="archived-banner"]').first(),
    ).toBeVisible();
  });
});

test('an archived neighbour is marked in the panel and on the canvas', async ({
  page,
}) => {
  const token = await mintToken();
  const boards = (await api(token, 'GET', '/api/boards?limit=100')).items as any[];
  const initiativeBoard = boards.find((b) => b.board_level === 'initiative');
  if (!initiativeBoard) throw new Error('no initiative board in the seed');
  const initiativeEntry = (
    await api(token, 'GET', `/api/boards/${initiativeBoard.id}`)
  ).columns[0];
  const delivery = await deliveryBoard(token);

  const parent = await api(token, 'POST', '/api/initiatives', {
    board_id: initiativeBoard.id,
    column_id: initiativeEntry.id,
    title: 'E2E: an initiative with one child that finished',
    content: 'Containment is a fact about the record.',
  });
  const child = await createTask(
    token,
    delivery.id,
    delivery.columns[0].id,
    'E2E: the child that was put away',
  );
  await api(token, 'POST', '/api/relationships', {
    source_short_code: parent.short_code,
    target_short_code: child.short_code,
    relationship: 'parent',
  });
  // Only the CHILD goes away: the parent has to stay live so the panel
  // and the canvas render as they do for ordinary work.
  await api(token, 'DELETE', `/api/tasks/${child.short_code}`);

  await test.step('login via Dex as alice', () => login(page));

  await test.step('the Relationships panel marks it, and says why the counts differ', async () => {
    await page.goto(`/items/${parent.short_code}`);
    const panel = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Relationships' }),
    });
    await expect(panel).toContainText(child.short_code);
    await expect(panel.locator('.kairos-archived-badge')).toContainText(
      'put away',
    );
    // ADR-20 rule 5: the rollup above stays live-only, and the panel
    // makes the difference legible instead of reconciling it away.
    await expect(panel).toContainText('containment is a fact about the record');
  });

  await test.step('the graph draws it, marked', async () => {
    await page.goto(`/search/relationships/${parent.short_code}`);
    await expect(
      page.locator('.kairos-graph__node--focus .kairos-graph__code'),
    ).toHaveText(parent.short_code);
    const putAway = page.locator('.kairos-graph__node--put-away', {
      has: page.locator('.kairos-graph__code', { hasText: child.short_code }),
    });
    await expect(putAway).toHaveCount(1);
    await expect(putAway.locator('.kairos-graph__put-away')).toHaveText(
      'put away',
    );
  });

  // Nothing visible stays behind: the parent goes away too (the child is
  // already archived, so the cascade finds nothing live to take).
  await api(token, 'DELETE', `/api/initiatives/${parent.short_code}`);
});
