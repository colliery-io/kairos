// KAIROS-T-0744 — a notice from an agent key, and the mute of the key
// (KAIROS-T-0737, KAIROS-A-0078):
//
//   1. bob makes an agent key; the key sends alice a notice that links a
//      task; her bell shows it live, of the kind `notice`, with the pill of
//      the key and "Mute this key";
//   2. Mute this key: the Notifications page lists the key under the muted
//      keys; a second notice of the key makes no notification for alice;
//   3. Unmute: the list of muted keys is empty again;
//   4. Admin, Members lists the key of bob for alice (an organization
//      admin).
//
// The key and the task are new for each run (timestamped names), and the
// spec revokes the key at the end.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

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

async function call(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(`${GUI}${path}`, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${await res.text()}`);
  return res.status === 204 ? undefined : res.json();
}

async function noticesFrom(token: string, keyId: string): Promise<number> {
  const list = await call(token, 'GET', '/api/me/notifications');
  return (list.items as any[]).filter((n) => n.kind === 'notice' && n.agent_key_id === keyId).length;
}

test('a notice of an agent key shows in the bell, and alice mutes the key', async ({ page }) => {
  const alice = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const run = Date.now();
  const board = await loadBoard(GUI, alice, (b) => b.slug === 'platform-delivery');
  const task = await createTask(GUI, alice, { title: `E2E notice ${run}`, boardId: board.id });
  const code: string = task.short_code;
  const keyName = `e2e-notice-${run}`;
  const key = await call(bob, 'POST', '/api/me/agent-keys', { name: keyName });

  try {
    await test.step('the notice shows in the bell, with the key', async () => {
      await login(page);
      await call(key.key, 'POST', '/api/notices', {
        to: 'alice@kairos.test',
        text: `Look at ${code}, run ${run}.`,
        short_code: code,
      });
      const bell = page.locator('[data-testid="notification-bell"]');
      await expect(bell.locator('[data-testid="notification-count"]')).toBeVisible({ timeout: 15_000 });
      await bell.getByRole('button', { name: 'Notifications' }).click();
      const row = page.locator('[data-testid="notification-drawer"] [data-testid="notification"][data-kind="notice"]', {
        hasText: `run ${run}`,
      });
      await expect(row).toHaveCount(1, { timeout: 15_000 });
      await expect(row.locator('[data-testid="notice-agent-key"]')).toContainText(keyName);
      await expect(row.locator(`a[href="/items/${code}"]`)).toBeVisible();

      await row.locator('[data-testid="notice-mute"]').click();
      await expect
        .poll(async () => (await call(alice, 'GET', '/api/me/notification-settings')).muted_keys.length)
        .toBeGreaterThan(0);
      // Read it, so that the bell of alice is as the next spec expects.
      await row.getByRole('button', { name: 'Mark read', exact: true }).click();
      await expect(row).toHaveAttribute('data-unread', 'false');
    });

    await test.step('the muted key is listed, and its notice makes no notification', async () => {
      await page.goto('/notifications');
      const muted = page.locator('[data-testid="muted-key"]', { hasText: keyName });
      await expect(muted).toBeVisible({ timeout: 30_000 });
      const before = await noticesFrom(alice, key.id);
      await call(key.key, 'POST', '/api/notices', {
        to: 'alice@kairos.test',
        text: `A second notice, run ${run}.`,
      });
      expect(await noticesFrom(alice, key.id)).toBe(before);

      await muted.locator('[data-testid="unmute-key"]').click();
      await expect(page.getByText('Kairos unmuted the agent key.')).toBeVisible();
      await expect(page.locator('[data-testid="muted-key"]', { hasText: keyName })).toHaveCount(0);
    });

    await test.step('Admin, Members lists the key of bob', async () => {
      await page.goto('/admin/members');
      const row = page.locator('[data-testid="org-agent-key"]', { hasText: keyName });
      await expect(row).toBeVisible({ timeout: 30_000 });
      await expect(row).toContainText('active');
    });
  } finally {
    await call(bob, 'DELETE', `/api/me/agent-keys/${key.id}`).catch(() => undefined);
  }
});
