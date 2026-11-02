// KAIROS-T-0665 — the `@` picker of the note box, and the user names in
// the lists of people:
//
//   1. the team page shows the user name of each member (`bob @<user name>`);
//   2. alice writes `@b` in the note box: the picker lists bob with his user
//      name; Escape closes it, a letter opens it again;
//   3. Enter puts `@<user name of bob> ` in the note; the note shows the
//      user name of alice next to her name;
//   4. bob gets a mention notification for the task.
//
// The demo seed sets each user name to the login subject (KAIROS-T-0184),
// so the spec reads the user names from the server. The task is new for
// each run (a timestamped title), so a retry starts clean.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, readEachPage } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function call(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(`${GUI}${path}`, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${await res.text()}`);
  return res.json();
}

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

test('alice mentions bob through the picker, and bob is told', async ({ page }) => {
  const alice = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const teams: any[] = await readEachPage(GUI, alice, '/api/teams');
  const team = teams.find((t) => t.delivery_board_id === board.id);
  if (!team) throw new Error('no team has the board platform-delivery');
  const members: any[] = await call(alice, 'GET', `/api/teams/${team.id}/members`);
  const bobMember = members.find((m) => m.email === 'bob@kairos.test');
  const aliceMember = members.find((m) => m.email === 'alice@kairos.test');
  if (!bobMember || !aliceMember) throw new Error(`alice and bob are not members of ${team.slug}`);
  // The REST lists give the user name (the seed: the login subject).
  expect(bobMember.user_name).toBeTruthy();
  expect(aliceMember.user_name).toBeTruthy();
  const bobName: string = bobMember.user_name;
  const aliceName: string = aliceMember.user_name;

  const created = await createTask(GUI, alice, {
    title: `E2E mention ${Date.now()}`,
    boardId: board.id,
  });
  const code: string = created.short_code;
  const mentionsOf = async (): Promise<any[]> => {
    const list = await call(bob, 'GET', '/api/me/notifications');
    return (list.items as any[]).filter((n) => n.kind === 'mention' && n.short_code === code);
  };
  expect(await mentionsOf()).toHaveLength(0);

  await test.step('login via Dex as alice', () => login(page));

  await test.step('the team page shows the user names', async () => {
    await page.goto(`/teams/${team.slug}`);
    const roster = page.locator('.cl-panel', { hasText: 'the roster' });
    await expect(roster).toContainText(`bob @${bobName}`, { timeout: 30_000 });
    await expect(roster).toContainText(`alice @${aliceName}`);
  });

  const box = page.locator('#kairos-note-text');
  const picker = page.getByTestId('mention-picker');

  await test.step('@ and a letter open the picker; Escape closes it', async () => {
    await page.goto(`/items/${code}`);
    await expect(page.getByText('This item has no notes.')).toBeVisible({ timeout: 30_000 });
    await box.click();
    await box.pressSequentially('Ask @b');
    const first = picker.getByRole('option').first();
    await expect(first).toContainText('bob', { timeout: 15_000 });
    await expect(first).toContainText(`@${bobName}`);
    await expect(first).toHaveAttribute('aria-selected', 'true');
    await page.screenshot({ path: 'test-results/mention-picker.png', fullPage: true });
    await box.press('Escape');
    await expect(picker.getByRole('option')).toHaveCount(0);
    await expect(box).toHaveValue('Ask @b');
    await box.pressSequentially('o');
    await expect(picker.getByRole('option').first()).toContainText(`@${bobName}`);
  });

  await test.step('Enter puts the user name of bob in the note', async () => {
    await box.press('Enter');
    await expect(picker.getByRole('option')).toHaveCount(0);
    await expect(box).toHaveValue(`Ask @${bobName} `);
    await box.pressSequentially('to check the logs.');
    await page.getByRole('button', { name: 'Add note' }).click();
    const note = page.locator('[data-testid="item-note"][id="n-1"]');
    await expect(note).toContainText(`Ask @${bobName} to check the logs.`, { timeout: 15_000 });
    // The byline gives the user name of the author.
    await expect(note).toContainText(`alice @${aliceName}`);
  });

  await test.step('bob gets a mention notification', async () => {
    await expect.poll(async () => (await mentionsOf()).length, { timeout: 15_000 }).toBe(1);
    const [mention] = await mentionsOf();
    expect(mention.text).toContain(`wrote your name in ${code}`);
  });
});
