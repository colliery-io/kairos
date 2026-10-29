// KAIROS-T-0723 — the stand-up page at `/teams/:slug/standup`
// (COLLIERY-I-0663):
//
//   1. `?person=<user name>` opens the stop of alice: her claimed task is in
//      the Active column of the Tasks lane;
//   2. the round has a stop for each person of the team and ends with
//      Unclaimed; `?stop=unclaimed` shows a Todo task that no one claims,
//      and Next is off at the last stop;
//   3. Previous goes back to a person; Reshuffle goes to the first person.
//
// The tasks are new for each run (timestamped titles), so a retry starts
// clean.

import { test, expect } from '@playwright/test';
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

async function task(token: string, boardId: string, title: string, path: string[]): Promise<string> {
  const created = await createTask(GUI, token, { title, boardId });
  const code: string = created.short_code;
  for (const column of path) {
    await call(token, 'POST', `/api/tasks/${code}/transition`, { to_column_id: column });
  }
  return code;
}

test('the stand-up goes around the team and ends with the unclaimed tasks', async ({ page }) => {
  const alice = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo');
  const active = board.columnId.get('Active');
  if (!todo || !active) throw new Error('platform-delivery has no Todo or Active column');
  const teams: any[] = await readEachPage(GUI, alice, '/api/teams');
  const team = teams.find((t) => t.delivery_board_id === board.id);
  if (!team) throw new Error('no team has the board platform-delivery');
  const run = Date.now();

  const mine = await task(alice, board.id, `E2E stand-up ${run}`, [todo, active]);
  const open = await task(bob, board.id, `E2E stand-up open ${run}`, [todo]);

  const standup = await call(alice, 'GET', `/api/teams/${team.slug}/standup`);
  const me = standup.people.find((p: any) => p.display_name.toLowerCase().includes('alice'));
  if (!me) throw new Error(`alice is not a person of ${team.slug}`);

  await test.step('the stop of alice shows her claim', async () => {
    await page.goto(`/teams/${team.slug}/standup?person=${encodeURIComponent(me.user_name)}`);
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'alice@kairos.test');
    await page.fill('#password', 'alice-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.endsWith('/standup'), { timeout: 30_000 });
    // The login can land without the query: open the stop again.
    await page.goto(`/teams/${team.slug}/standup?person=${encodeURIComponent(me.user_name)}`);
    await expect(page.locator('[data-testid="standup-person"]')).toHaveAttribute(
      'data-person',
      me.user_name,
      { timeout: 30_000 },
    );
    await expect(
      page.locator(`[data-testid="standup-tasks"] [data-column="active"] [data-code="${mine}"]`),
    ).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('[data-testid="standup-current"]')).toContainText(me.display_name);
    await page.screenshot({ path: 'test-results/standup-person.png', fullPage: true });
  });

  await test.step('the round ends with Unclaimed', async () => {
    const stops = page.locator('[data-testid="standup-stop"]');
    await expect(stops).toHaveCount(standup.people.length + 1);
    await expect(stops.last()).toContainText('Unclaimed');
    await stops.last().click();
    await expect(page).toHaveURL(/stop=unclaimed/);
    await expect(
      page.locator(`[data-testid="standup-unclaimed-tasks"] [data-column="todo"] [data-code="${open}"]`),
    ).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(`[data-testid="standup-unclaimed"] [data-code="${mine}"]`)).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Next' })).toBeDisabled();
    await page.screenshot({ path: 'test-results/standup-unclaimed.png', fullPage: true });
  });

  await test.step('Previous and Reshuffle go to a person', async () => {
    await page.getByRole('link', { name: 'Previous' }).click();
    await expect(page).toHaveURL(/person=/);
    await expect(page.locator('[data-testid="standup-person"]')).toBeVisible({ timeout: 30_000 });
    await page.goto(`/teams/${team.slug}/standup?stop=unclaimed`);
    await expect(page.locator('[data-testid="standup-unclaimed"]')).toBeVisible({ timeout: 30_000 });
    await page.getByRole('button', { name: 'Reshuffle' }).click();
    await expect(page).toHaveURL(/person=/);
    await expect(page.locator('[data-testid="standup-current"]')).not.toContainText('Unclaimed');
  });
});
