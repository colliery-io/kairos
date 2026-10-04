// COLLIERY-T-0269 — a document names its board, and impacts a repository.
//
// The model (the owner decided it on 2026-09-29):
//
//   document -> board                        THE OWNER. The board gives the
//                                            right to edit.
//   document -> repository, by `impacts`     What the document is ABOUT. It
//                                            gives no right.
//
// Three journeys (login as alice, org admin of the demo tenant):
//
//   1. the vision of a repository, from the GUI: on the page of the team
//      that owns `payments-api`, "New document" makes a document from the
//      template "Product Vision". The owner board picker starts with no
//      board; the user selects the delivery board of the team as its
//      owner. The document has an impacts link to the repository. The
//      page of the document shows the owner board and the link, and no
//      column. The team page and the page Admin, Repositories show the
//      document for the repository;
//   2. the impacts links on the page of the document: add a link to a
//      second repository, and remove it;
//   3. each document has an owner board (COLLIERY-T-3109): a create with
//      no board is refused and the refusal names `board`; the "New
//      document" dialog of an item page has an owner board picker that
//      starts with no board; the last `supports` edge can go and the
//      owner board stays; the owner panel changes the board and has no
//      control that removes it.
//
// **Fixture discipline.** The suite is serial over ONE seeded stack, and
// other specs count boards and cards. This spec makes no board and no
// card but one task, and each item that it makes is archived at the end.
// A document is not a card: it is on no board view.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const SERVER = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
// Per-run suffix: a retry makes documents with new titles.
const RUN = Date.now().toString(36);
const OWNER_BOARD = 'platform-delivery';
const REPOSITORY = 'payments-api';
const SECOND_REPOSITORY = 'platform-infra';

const bearer = (token: string) => ({ authorization: `Bearer ${token}` });

async function call(
  token: string,
  method: string,
  path: string,
  body?: unknown,
): Promise<{ status: number; body: any }> {
  const res = await fetch(SERVER + path, {
    method,
    headers: body
      ? { ...bearer(token), 'content-type': 'application/json' }
      : bearer(token),
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  return { status: res.status, body: text ? JSON.parse(text) : null };
}

async function api(
  token: string,
  method: string,
  path: string,
  body?: unknown,
): Promise<any> {
  const res = await call(token, method, path, body);
  if (res.status < 200 || res.status >= 300) {
    throw new Error(`${method} ${path} -> ${res.status}: ${JSON.stringify(res.body)}`);
  }
  return res.body;
}

// The title must match in full: "Board" is not "Owner board".
const panel = (page: Page, title: string) =>
  page.locator('.cl-panel', {
    has: page.locator('.cl-panel__title', {
      hasText: new RegExp(`^${title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`),
    }),
  });

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

/** The short code in the URL of an item page. */
function codeOf(page: Page): string {
  const match = new URL(page.url()).pathname.match(/^\/items\/([A-Z0-9-]+)$/);
  if (!match) throw new Error(`not an item page: ${page.url()}`);
  return match[1];
}

test('the vision of a repository: owner board, impacts link, repository pages', async ({
  page,
}) => {
  // The API token FIRST (see repositories.spec.ts).
  const token = await mintToken();
  const title = `The vision of payments-api ${RUN}`;
  const board = await api(token, 'GET', `/api/boards/${OWNER_BOARD}`);
  let code = '';

  await test.step('login via Dex as alice', () => login(page));

  await test.step('the team page makes a document for the repository', async () => {
    await page.goto('/teams/platform');
    const row = panel(page, 'Repositories').locator(`[data-repo="${REPOSITORY}"]`);
    await expect(row).toBeVisible();
    await row.getByRole('button', { name: 'New document' }).click();

    const form = page.locator('[data-testid="repository-document-form"]');
    await expect(form).toBeVisible();
    // The first value of the template: the template of a product vision.
    const template = form.locator('[data-testid="repository-document-template"]');
    await expect(template.locator('option:checked')).toHaveText('Product Vision');
    // COLLIERY-T-1836: the test id is on the Aurora field; the control is
    // the select in it. COLLIERY-T-3109 (2026-10-04): the owner board
    // picker starts with no board, and the create button is disabled
    // until the user selects a board.
    const owner = form.locator('[data-testid="repository-document-board"] select');
    await expect(owner).toHaveValue('');
    await expect(owner.locator('option:checked')).toHaveText('Select the owner board');
    const create = form.getByRole('button', { name: 'Create document' });
    await expect(create).toBeDisabled();
    await owner.selectOption(OWNER_BOARD);
    await expect(create).toBeEnabled();

    // A title is necessary.
    await create.click();
    await expect(form).toContainText('The title is empty. Write a title.');

    await form
      .locator('.cl-field', { hasText: 'Document title' })
      .locator('input')
      .fill(title);
    await form.getByRole('button', { name: 'Create document' }).click();
    // COLLIERY-T-3099: the document takes the prefix of its owner board.
    await page.waitForURL(/\/items\/PLATFORM-D-\d+$/, { timeout: 30_000 });
    code = codeOf(page);
  });

  await test.step('the page of the document shows the owner and the link', async () => {
    await expect(page.locator('.cl-pageheader, body')).toContainText(title);
    const owner = page.locator('[data-testid="owner-board"]');
    await expect(owner).toContainText(board.name);
    await expect(owner.locator(`a[href="/boards/${OWNER_BOARD}"]`)).toBeVisible();
    await expect(owner).toContainText('The document is not a card on the board.');
    // A document has no column and no transition: the panel of a card is
    // not on the page.
    await expect(panel(page, 'Board')).toHaveCount(0);
    await expect(page.locator('[data-testid="move-board"]')).toHaveCount(0);

    const impacts = page.locator('[data-testid="impacts"]');
    await expect(impacts.locator(`[data-impact="${REPOSITORY}"]`)).toBeVisible();
  });

  await test.step('the server has the same: owner board, impacts, document type', async () => {
    const document = await api(token, 'GET', `/api/documents/${code}`);
    expect(document.board_id).toBe(board.id);
    // The template gave the sections. The page shows the text in its
    // editor, where a text check of the page cannot read it.
    expect(document.content).toContain('## Who It Is For');
    expect(document.impacts.map((impact: any) => impact.repository.slug)).toEqual([
      REPOSITORY,
    ]);
    const relationships = await api(token, 'GET', `/api/documents/${code}/relationships`);
    expect(relationships.incoming).toEqual([]);
    const detail = await api(token, 'GET', `/api/repositories/${REPOSITORY}`);
    expect(detail.impacted_by).toContainEqual({
      short_code: code,
      title,
      entity_type: 'document',
      document_type: 'vision',
      lifecycle: 'draft',
      column: null,
    });
  });

  await test.step('the document is not a card of its owner board', async () => {
    await page.goto(`/boards/${OWNER_BOARD}`);
    await expect(page.locator('article.kairos-card').first()).toBeVisible();
    await expect(page.locator('body')).not.toContainText(title);
  });

  await test.step('the team page shows the document for the repository', async () => {
    await page.goto('/teams/platform');
    const row = panel(page, 'Repositories').locator(`[data-repo="${REPOSITORY}"]`);
    await row.getByRole('button', { name: 'Show documents' }).click();
    const listed = row.locator(`[data-impacting="${code}"]`);
    await expect(listed).toBeVisible();
    await expect(listed).toContainText(title);
    await expect(listed).toContainText('vision');
    await expect(listed).toContainText('draft');
    // The second repository of the team has no document.
    const other = panel(page, 'Repositories').locator(
      `[data-repo="${SECOND_REPOSITORY}"]`,
    );
    await other.getByRole('button', { name: 'Show documents' }).click();
    await expect(other).toContainText(
      'No document and no ADR impacts this repository.',
    );
    // The link of the row opens the document.
    await listed.getByRole('link').click();
    await page.waitForURL(new RegExp(`/items/${code}$`));
  });

  await test.step('the page Admin, Repositories shows the document too', async () => {
    await page.goto('/admin/repositories');
    const row = page.locator(`[data-repo="${REPOSITORY}"]`);
    await expect(row).toBeVisible();
    await row.getByRole('button', { name: 'Show documents' }).click();
    await expect(row.locator(`[data-impacting="${code}"]`)).toContainText(title);
  });

  await test.step('add an impacts link, and remove it', async () => {
    await page.goto(`/items/${code}`);
    const impacts = page.locator('[data-testid="impacts"]');
    const add = impacts.locator('[data-testid="impacts-add"]');
    // The picker offers the repositories that the document does not
    // impact now.
    await expect(
      add.locator(`select option[value="${REPOSITORY}"]`),
    ).toHaveCount(0);
    await add.locator('select').selectOption(SECOND_REPOSITORY);
    await add.getByRole('button', { name: 'Add impacts link' }).click();
    await expect(impacts.locator(`[data-impact="${SECOND_REPOSITORY}"]`)).toBeVisible();
    await expect(impacts.locator(`[data-impact="${REPOSITORY}"]`)).toBeVisible();

    await impacts
      .locator(`[data-impact="${SECOND_REPOSITORY}"]`)
      .getByRole('button', { name: 'Remove' })
      .click();
    await expect(impacts.locator(`[data-impact="${SECOND_REPOSITORY}"]`)).toHaveCount(0);
    await expect(impacts.locator(`[data-impact="${REPOSITORY}"]`)).toBeVisible();

    const document = await api(token, 'GET', `/api/documents/${code}`);
    expect(document.impacts.map((impact: any) => impact.repository.slug)).toEqual([
      REPOSITORY,
    ]);
    // The links are not content: the document has its first version.
    expect(document.version).toBe(1);
  });

  // Nothing visible stays behind. The archive removes no link.
  await api(token, 'DELETE', `/api/documents/${code}`);
  const archived = await api(token, 'GET', `/api/documents/${code}/impacts`);
  expect(archived.impacts.map((impact: any) => impact.repository.slug)).toEqual([
    REPOSITORY,
  ]);
  const detail = await api(token, 'GET', `/api/repositories/${REPOSITORY}`);
  expect(detail.impacted_by.map((item: any) => item.short_code)).not.toContain(code);
});

test('each document has an owner board, and the last supports edge can go', async ({
  page,
}) => {
  const token = await mintToken();
  const board = await api(token, 'GET', `/api/boards/${OWNER_BOARD}`);
  const web = await api(token, 'GET', '/api/boards/web-delivery');
  const task = await api(token, 'POST', '/api/tasks', {
    board_id: board.id,
    title: `E2E: the item that documents support ${RUN}`,
    content: '',
  });
  const made: string[] = [];

  await test.step('a create with no board is refused, and the refusal names board', async () => {
    const refused = await call(token, 'POST', '/api/documents', {
      title: `E2E: a document with no board ${RUN}`,
      parent_short_code: task.short_code,
    });
    expect(refused.status).toBe(422);
    expect(refused.body.error.code).toBe('VALIDATION');
    expect(refused.body.error.details.field).toBe('board');
  });

  const owned = await api(token, 'POST', '/api/documents', {
    title: `E2E: a document that names a board ${RUN}`,
    content: 'The board that it names is its owner.',
    board: OWNER_BOARD,
    parent_short_code: task.short_code,
  });
  made.push(owned.short_code);
  expect(owned.board_id).toBe(board.id);

  await test.step('login via Dex as alice', () => login(page));

  await test.step('the New document dialog of an item names the owner board', async () => {
    await page.goto(`/items/${task.short_code}`);
    await page.getByRole('button', { name: 'New document', exact: true }).click();
    const owner = page.locator('[data-testid="create-document-board"] select');
    // The picker starts with no board (COLLIERY-T-3109, 2026-10-04), and
    // the create button is disabled until the user selects a board.
    await expect(owner).toHaveValue('');
    await expect(owner.locator('option:checked')).toHaveText('Select the owner board');
    const create = page.getByRole('button', { name: 'Create document' });
    await page
      .locator('.cl-field', { hasText: 'Document title' })
      .locator('input')
      .fill(`E2E: a document from the dialog ${RUN}`);
    await expect(create).toBeDisabled();
    await owner.selectOption(OWNER_BOARD);
    await create.click();
    await page.waitForURL(/\/items\/PLATFORM-D-\d+$/, { timeout: 30_000 });
    const code = codeOf(page);
    made.push(code);
    const document = await api(token, 'GET', `/api/documents/${code}`);
    expect(document.board_id).toBe(board.id);
  });

  const manage = () => panel(page, 'Manage links');
  const edge = (code: string) =>
    // The row is the group that has the label AND the button. The label
    // is in an inner group of its own.
    manage()
      .locator('.cl-group', {
        has: page.locator('.cl-pill', { hasText: `supports ← ${code}` }),
      })
      .filter({ has: page.getByRole('button', { name: 'Unlink' }) })
      .last();

  await test.step('the last supports edge can go, and the owner board stays', async () => {
    await page.goto(`/items/${owned.short_code}?view=graph`);
    await expect(manage()).toContainText(`supports ← ${task.short_code}`);
    await edge(task.short_code).getByRole('button', { name: 'Unlink' }).click();
    // The text "Unlinked ..." goes when the list reads the edges again
    // (COLLIERY-T-3106), so the check is on the list and the server.
    await expect(manage()).toContainText('No edges on this item yet.');
    const relationships = await api(
      token,
      'GET',
      `/api/documents/${owned.short_code}/relationships`,
    );
    expect(relationships.incoming).toEqual([]);
    const document = await api(token, 'GET', `/api/documents/${owned.short_code}`);
    expect(document.board_id).toBe(board.id);
  });

  await test.step('the owner panel changes the board, and cannot remove it', async () => {
    await page.goto(`/items/${owned.short_code}`);
    const owner = page.locator('[data-testid="owner-board"]');
    await expect(owner).toContainText(board.name);
    await expect(owner.getByRole('button', { name: 'Remove owner board' })).toHaveCount(0);
    const control = owner.locator('[data-testid="owner-board-control"]');
    await control.locator('select').selectOption('web-delivery');
    await control.getByRole('button', { name: 'Set owner board' }).click();
    await expect(owner.locator('a[href="/boards/web-delivery"]')).toBeVisible();
    const document = await api(token, 'GET', `/api/documents/${owned.short_code}`);
    expect(document.board_id).toBe(web.id);
    const refused = await call(token, 'PATCH', `/api/documents/${owned.short_code}/board`, {
      board: null,
    });
    expect(refused.status).toBe(422);
    expect(refused.body.error.details.field).toBe('board');
  });

  // Nothing visible stays behind.
  for (const code of made) {
    await api(token, 'DELETE', `/api/documents/${code}`);
  }
  await api(token, 'DELETE', `/api/tasks/${task.short_code}`);
});
