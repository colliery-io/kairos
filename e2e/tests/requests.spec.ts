// COLLIERY-T-0232 — a member sends a request to a different team from the GUI.
//
// Teams request work of each other, and no team pushes work to a different
// team (COLLIERY-T-0218, COLLIERY-A-0023). A member of the tenant who does
// not manage a delivery board sends a REQUEST to the team of that board: a
// task in the entry column, in the Support lane. The server had the rule;
// the GUI had no button for it.
//
// The cast: carol is a member of team web, with nothing on the board of
// team platform. bob is a member of team platform and not an admin, so
// what he can do comes from the team and not from the admin bypass.
//
//   a. carol sees "New request" on the platform board, and no "New task"
//   b. bob sees "New task" on the same board, and no "New request"
//   c. the request dialog: a title that names the team, a caption that
//      names the entry column and the Support lane, no Lane control
//   d. a request with no repository is in the entry column, in the Support
//      lane — on the board, and in the task that the API returns
//   e. a request with a repository of team platform shows the repository
//   f. carol opens her request: she edits the title, and sees no control
//      that moves it
//   g. the initiative board shows neither button to carol
//   h. bob puts the request in the planned lane; carol still cannot move it
//
// Conventions match the other specs: visible text and roles, the stable
// `.kairos-*` / `.cl-*` classes, data-testid hooks. Each title carries a
// per-run suffix, so a retry finds its own cards.

import { test, expect, type Browser, type Locator, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import {
  loadBoard,
  readTask,
  tryArchiveTask,
  trySetWorkClass,
  tryTransitionTask,
} from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);
const BOARD = 'platform-delivery';

type Lane = 'planned' | 'support';

const column = (page: Page, name: string, lane: Lane): Locator =>
  page
    .locator(`section.kairos-board__lane--${lane}`)
    .locator('section.kairos-board__column', {
      has: page.locator('.kairos-board__column-head', { hasText: name }),
    });

const cardIn = (page: Page, columnName: string, lane: Lane, needle: string): Locator =>
  column(page, columnName, lane).locator('article.kairos-card', { hasText: needle });

const field = (scope: Locator, label: string): Locator =>
  scope.locator('.cl-field', {
    has: scope.page().locator('.cl-field__label', { hasText: label }),
  });

const newRequest = (page: Page) => page.getByRole('button', { name: 'New request', exact: true });
const newTask = (page: Page) => page.getByRole('button', { name: 'New task', exact: true });

/** A browser of its own for one person, signed in through Dex. */
async function signIn(browser: Browser, name: string): Promise<Page> {
  const context = await browser.newContext({ baseURL: GUI });
  const page = await context.newPage();
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', `${name}@kairos.test`);
  await page.fill('#password', `${name}-password`);
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), { timeout: 30_000 });
  // The header shows the name when `whoami` resolved. Each "is not there"
  // assertion below is made after this, so that a button that is absent
  // is absent by the rule and not because the page does not know the
  // person yet.
  await expect(
    page.locator('.cl-appshell__header').getByText(name, { exact: true }),
  ).toBeVisible();
  return page;
}

async function openBoard(page: Page, slug: string, name: string) {
  await page.goto(`/boards/${slug}`);
  await expect(page.locator('section.kairos-board__column').first()).toBeVisible();
  await expect(
    page.locator('.cl-appshell__header').getByText(name, { exact: true }),
  ).toBeVisible();
}

/** Open the request dialog, fill it, send it; return the short code. */
async function sendRequest(
  page: Page,
  opts: { title: string; repository?: string },
): Promise<string> {
  await newRequest(page).click();
  const modal = page.locator('.cl-modal');
  await field(modal, 'Title').locator('input').fill(opts.title);
  if (opts.repository) {
    await modal
      .locator('[data-testid="create-repository"] select')
      .selectOption(opts.repository);
  }
  await modal.getByRole('button', { name: 'Send request', exact: true }).click();
  await expect(modal).toBeHidden();
  const notice = page.locator('[data-testid="request-sent"]');
  await expect(notice).toContainText('Request sent to Platform: ');
  return (await notice.locator('a').innerText()).trim();
}

test('requests: a member who does not manage a delivery board sends a request to its team', async ({
  browser,
}) => {
  // API tokens FIRST: Dex keeps one refresh token for each user and client,
  // so a mint after the browser login of the same person would make the
  // browser session invalid (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const carolToken = await mintToken({
    server: GUI,
    email: 'carol@kairos.test',
    password: 'carol-password',
  });
  const platform = await loadBoard(GUI, alice, (b) => b.slug === BOARD);
  const initiatives = await loadBoard(GUI, alice, (b) => b.board_level === 'initiative');
  expect(platform.level).toBe('delivery');

  const carol = await signIn(browser, 'carol');
  const bob = await signIn(browser, 'bob');

  await test.step('a. carol sees New request on the platform board, and no New task', async () => {
    await openBoard(carol, BOARD, 'carol');
    await expect(newRequest(carol)).toBeVisible();
    await expect(newTask(carol)).toHaveCount(0);
  });

  await test.step('b. bob, a member of platform, sees New task and no New request', async () => {
    await openBoard(bob, BOARD, 'bob');
    await expect(newTask(bob)).toBeVisible();
    await expect(newRequest(bob)).toHaveCount(0);
  });

  await test.step('c. the request dialog names the team, the column and the lane, and has no Lane control', async () => {
    await newRequest(carol).click();
    const modal = carol.locator('.cl-modal');
    await expect(modal.locator('.cl-modal__title')).toHaveText('Request to Platform');
    const caption = modal.locator('[data-testid="create-caption"]');
    await expect(caption).toContainText(`${platform.entryColumnName}, the entry column`);
    await expect(caption).toContainText('in the Support lane');
    await expect(caption).toContainText('Platform moves it');
    // The controls that stay...
    await expect(field(modal, 'Title').locator('input')).toBeVisible();
    await expect(field(modal, 'Content').locator('textarea')).toBeVisible();
    await expect(field(modal, 'Task type').locator('option')).toHaveText([
      'task',
      'bug',
      'tech_debt',
      'support',
    ]);
    const picker = modal.locator('[data-testid="create-repository"] select');
    await expect(picker).toBeVisible();
    // ...and the ones that a request does not have: no lane, no column.
    await expect(field(modal, 'Lane')).toHaveCount(0);
    await expect(field(modal, 'Column')).toHaveCount(0);
    await expect(modal.locator('option', { hasText: 'planned' })).toHaveCount(0);
    // The repositories of the team that gets the request come first
    // (COLLIERY-T-0221): for carol they belong to a different team, and
    // they still lead, because the order follows the board.
    const labels = await picker.locator('option').allTextContents();
    expect(labels[0]).toBe('(none)');
    expect(labels[1]).not.toContain(' · owner: ');
    expect(labels).toContain('payments-api · acme/payments-api');
    expect(labels).toContain('portal-web · acme/portal-web · owner: Web');
    // The dialog is reachable by keyboard, as the dialog of a manager is:
    // the title takes the text that the keyboard types.
    await field(modal, 'Title').locator('input').focus();
    await carol.keyboard.type('typed');
    await expect(field(modal, 'Title').locator('input')).toHaveValue('typed');
    await modal.getByRole('button', { name: 'Cancel' }).click();
    await expect(modal).toBeHidden();
  });

  let plain = '';
  const plainTitle = `Request with no repository ${RUN}`;
  await test.step('d. a request with no repository is in the entry column, in the Support lane', async () => {
    plain = await sendRequest(carol, { title: plainTitle });
    expect(plain).toMatch(/^[A-Z]+-T-\d+$/);
    // Through the GUI: the Support lane of the entry column, and not the
    // Planned lane.
    const card = cardIn(carol, platform.entryColumnName, 'support', plainTitle);
    await expect(card).toBeVisible();
    await expect(card.locator('a.kairos-card__code')).toHaveText(plain);
    await expect(cardIn(carol, platform.entryColumnName, 'planned', plainTitle)).toHaveCount(0);
    const notice = carol.locator('[data-testid="request-sent"]');
    await expect(notice).toContainText(
      `Request sent to Platform: ${plain}. It is in ${platform.entryColumnName}, in the Support lane.`,
    );
    await expect(notice.locator('a')).toHaveAttribute('href', `/items/${plain}`);
    // Through the API: what the server stored.
    const task = await readTask(GUI, alice, plain);
    expect(task.column_id).toBe(platform.entryColumnId);
    expect(task.work_class).toBe('support');
    expect(task.board_id).toBe(platform.id);
    expect(task.task_type).toBe('task');
    expect(task.repository ?? null).toBeNull();
  });

  let linked = '';
  await test.step('e. a request with a repository of team platform shows the repository', async () => {
    const title = `Request with a repository ${RUN}`;
    const code = await sendRequest(carol, { title, repository: 'payments-api' });
    linked = code;
    const card = cardIn(carol, platform.entryColumnName, 'support', title);
    await expect(card).toBeVisible();
    await expect(card.locator('.kairos-card__repo[data-repo="payments-api"]')).toBeVisible();
    const task = await readTask(GUI, alice, code);
    expect(task.repository.slug).toBe('payments-api');
    expect(task.column_id).toBe(platform.entryColumnId);
    expect(task.work_class).toBe('support');
  });

  const expectNoMoveControls = async (page: Page) => {
    // The repository control needs `whoami` and the board, so when it is
    // there the move controls are absent by the rule.
    await expect(page.locator('[data-testid="repository-control"]')).toBeVisible();
    await expect(page.locator('.kairos-item__move'), 'no column move, no lane').toHaveCount(0);
    await expect(page.locator('[data-testid="move-board"]'), 'no board move').toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Set lane', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Move', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Move board', exact: true })).toHaveCount(0);
  };

  const editedTitle = `${plainTitle} (corrected)`;
  await test.step('f. carol opens her request: she edits the title, and has no control that moves it', async () => {
    // By the card: the notice names the request that was sent last.
    await cardIn(carol, platform.entryColumnName, 'support', plainTitle)
      .locator('a.kairos-card__code')
      .click();
    await carol.waitForURL(new RegExp(`/items/${plain}`));
    const editor = carol.locator('.kairos-editor');
    // KAIROS-T-0323: the page opens in Preview; Edit gives the form.
    await editor.getByRole('button', { name: 'Edit', exact: true }).click();
    await expect(editor.locator('textarea.kairos-editor__textarea')).toBeVisible();
    await field(editor, 'Title').locator('input').fill(editedTitle);
    await editor.getByRole('button', { name: 'Save' }).click();
    await expect(carol.getByText(/Saved — the item is now at v/)).toBeVisible();
    expect((await readTask(GUI, alice, plain)).title).toBe(editedTitle);
    // She created it, so she can also archive it.
    await expect(carol.getByRole('button', { name: 'Delete' })).toBeEnabled();
    await expectNoMoveControls(carol);
  });

  await test.step('g. the initiative board shows neither button to carol', async () => {
    await openBoard(carol, initiatives.slug, 'carol');
    await expect(carol.locator('.cl-page-header__sub')).toHaveText(
      `initiative board · ${initiatives.slug}`,
    );
    await expect(newRequest(carol)).toHaveCount(0);
    await expect(newTask(carol)).toHaveCount(0);
    await expect(carol.getByRole('button', { name: 'New initiative', exact: true })).toHaveCount(0);
  });

  await test.step('h. bob puts the request in the planned lane; carol still cannot move it', async () => {
    await bob.goto(`/items/${plain}`);
    const boardPanel = bob.locator('.cl-panel', {
      has: bob.locator('.cl-panel__title', { hasText: 'Board' }),
    });
    await field(boardPanel, 'Lane').locator('select').selectOption({ label: 'planned' });
    await boardPanel.getByRole('button', { name: 'Set lane', exact: true }).click();
    await expect(bob.getByText('Lane set to planned.')).toBeVisible();
    const task = await readTask(GUI, alice, plain);
    expect(task.work_class).toBe('planned');
    expect(task.column_id).toBe(platform.entryColumnId);

    // carol: the card is in the Planned lane now, and it is still not hers
    // to move — in the GUI, and at the server.
    await openBoard(carol, BOARD, 'carol');
    const card = cardIn(carol, platform.entryColumnName, 'planned', editedTitle);
    await expect(card).toBeVisible();
    await expect(card).toHaveAttribute('draggable', 'false');
    await card.locator('a.kairos-card__code').click();
    await carol.waitForURL(new RegExp(`/items/${plain}`));
    await expectNoMoveControls(carol);
    const todo = platform.columnId.get('Todo')!;
    expect(await tryTransitionTask(GUI, carolToken, plain, todo)).toBe(403);
    expect(await trySetWorkClass(GUI, carolToken, plain, 'support')).toBe(403);
    const after = await readTask(GUI, alice, plain);
    expect(after.work_class).toBe('planned');
    expect(after.column_id).toBe(platform.entryColumnId);
  });

  // carol archives her two requests: she created them (COLLIERY-T-0228).
  // This step is an assertion of that rule. It is not there for the drag
  // helper: since COLLIERY-T-0239 the helper moves a card when the column
  // is long (drag-long-column.spec.ts).
  await test.step('carol archives her requests', async () => {
    for (const code of [plain, linked]) {
      const status = await tryArchiveTask(GUI, carolToken, code);
      expect([200, 204], `archive ${code} -> ${status}`).toContain(status);
    }
    await openBoard(carol, BOARD, 'carol');
    await expect(carol.locator('article.kairos-card', { hasText: plain })).toHaveCount(0);
    await expect(carol.locator('article.kairos-card', { hasText: linked })).toHaveCount(0);
  });

  await carol.context().close();
  await bob.context().close();
});
