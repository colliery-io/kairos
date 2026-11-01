// KAIROS-T-0579 — the Notes panel of the item page: add a note, the anchor
// of a note, and promote a note to an ADR.
//
// The cast: alice (org admin) has the item page open. The token of the spec
// is the other writer (an agent).
//
//   1. alice adds a note: the row n-1 shows, and the content keeps its version
//   2. the other writer adds a note: the row n-2 shows with no banner
//   3. the anchor: `#n-2` highlights its note; a click on n-1 moves the hash
//   4. alice promotes n-1 to an ADR: the row shows the code of the ADR
//   5. the other writer promotes n-2 while the form of alice is open: her
//      promote gets the CONFLICT, and the panel shows the message
//
// The task is new for each run (a timestamped title); the spec archives the
// task and the two ADRs at the end.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, getTask, loadBoard, readEachPage } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const BOARD = 'platform-delivery';

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

async function api(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(GUI + path, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  return { status: res.status, body: text ? JSON.parse(text) : null };
}

const note = (page: Page, id: string) => page.locator(`[data-testid="item-note"][id="${id}"]`);
const banner = (page: Page) => page.getByTestId('item-changed-banner');

test('the Notes panel adds, anchors and promotes a note', async ({ page }) => {
  const token = await mintToken();
  const board = await loadBoard(GUI, token, (b) => b.slug === BOARD);
  const task = await createTask(GUI, token, {
    title: `Notes panel e2e ${Date.now()}`,
    boardId: board.id,
    content: 'Intro.',
  });
  const code: string = task.short_code;
  const boards = await readEachPage(GUI, token, '/api/boards');
  const adrBoard = boards.find((b: any) => b.board_level === 'adr');
  if (!adrBoard) throw new Error('no adr board in the seed');
  const made: string[] = [];

  await login(page);
  await page.goto(`/items/${code}`);
  await expect(page.getByText('This item has no notes.')).toBeVisible();

  await test.step('alice adds a note', async () => {
    await page.locator('#kairos-note-text').fill('First finding.\n\n## Detail\n\nA heading in a note.');
    await page.getByRole('button', { name: 'Add note' }).click();
    await expect(note(page, 'n-1')).toContainText('First finding.');
    await expect(note(page, 'n-1').getByRole('heading', { name: 'Detail' })).toBeVisible();
    await expect(page.locator('#kairos-note-text')).toHaveValue('');
    await expect(page.getByText('This item has no notes.')).toHaveCount(0);
    const after = await getTask(GUI, token, code);
    expect(after.version).toBe(task.version);
    await expect(banner(page)).toHaveCount(0);
  });

  await test.step('a note of the other writer shows with no banner', async () => {
    const res = await api(token, 'POST', `/api/tasks/${code}/notes`, { text: 'Second finding.' });
    expect(res.status).toBe(200);
    expect(res.body.note_id).toBe('n-2');
    await expect(note(page, 'n-2')).toContainText('Second finding.');
    await expect(banner(page)).toHaveCount(0);
  });

  await test.step('the anchor highlights its note', async () => {
    await page.goto(`/items/${code}#n-2`);
    await expect(note(page, 'n-2')).toHaveClass(/kairos-note--target/);
    await expect(note(page, 'n-1')).not.toHaveClass(/kairos-note--target/);
    await expect(note(page, 'n-2')).toBeInViewport();
    await note(page, 'n-1').locator('a.kairos-note-link').click();
    await expect(page).toHaveURL(new RegExp(`/items/${code}#n-1$`));
    await expect(note(page, 'n-1')).toHaveClass(/kairos-note--target/);
    await expect(note(page, 'n-2')).not.toHaveClass(/kairos-note--target/);
  });

  await test.step('alice promotes n-1 to an ADR', async () => {
    await note(page, 'n-1').getByRole('button', { name: 'Promote' }).click();
    const form = note(page, 'n-1').getByTestId('note-promote');
    await form.getByLabel('Kind').selectOption('adr');
    await form.getByLabel('Title').fill('Record the first finding');
    await form.getByLabel('Target board').selectOption(adrBoard.slug);
    await form.getByRole('button', { name: 'Promote' }).click();
    await expect(form).toHaveCount(0);
    await expect(note(page, 'n-1')).toContainText('Promoted to');
    const item = (await api(token, 'GET', `/api/tasks/${code}`)).body;
    const promoted = item.notes.find((n: any) => n.id === 'n-1').promoted_to;
    expect(promoted).toBeTruthy();
    made.push(promoted);
    await expect(note(page, 'n-1').getByRole('link', { name: promoted })).toBeVisible();
    await expect(note(page, 'n-1').getByRole('button', { name: 'Promote' })).toHaveCount(0);
  });

  await test.step('a second promote shows the CONFLICT of the server', async () => {
    await note(page, 'n-2').getByRole('button', { name: 'Promote' }).click();
    const form = note(page, 'n-2').getByTestId('note-promote');
    await form.getByLabel('Target board').selectOption(adrBoard.slug);
    const res = await api(token, 'POST', `/api/tasks/${code}/notes/n-2/promote`, {
      kind: 'adr',
      board: adrBoard.slug,
    });
    expect(res.status).toBe(201);
    made.push(res.body.promoted_to);
    await form.getByRole('button', { name: 'Promote' }).click();
    await expect(form.getByText('Kairos did not promote the note')).toBeVisible();
    await expect(form).toContainText('CONFLICT (409)');
    await expect(form).toContainText('one time only');
    await expect(note(page, 'n-2')).toContainText(`Promoted to ${res.body.promoted_to}`);
    await form.getByRole('button', { name: 'Close' }).click();
    await expect(form).toHaveCount(0);
  });

  // Put the task and the ADRs away: the seeded boards stay as they were found.
  for (const adr of made) {
    await api(token, 'DELETE', `/api/adrs/${adr}`);
  }
  await api(token, 'DELETE', `/api/tasks/${code}`);
});
