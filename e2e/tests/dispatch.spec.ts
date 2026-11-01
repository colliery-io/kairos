// KAIROS-T-0387 — agent dispatch in the board settings (KAIROS-T-0385):
//
//   1. alice (org admin) makes a webhook target on the board settings page
//      of platform-delivery; the page shows its secret one time;
//   2. she makes a dispatch rule: a task that enters Active goes to the
//      target;
//   3. a task enters Active: a receiver of the spec on 127.0.0.1 gets one
//      signed delivery (the server runs with KAIROS_WEBHOOK_ALLOW_PRIVATE),
//      and the signature checks with the secret;
//   4. the delivery log of the page shows the delivery, delivered.
//
// The target has a name of its own for each run, and the spec removes it
// (with its rule) and archives its task at the end, so a retry starts
// clean and no other spec sends deliveries to it.

import http from 'node:http';
import crypto from 'node:crypto';
import type { AddressInfo } from 'node:net';
import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, transitionTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

interface Received {
  headers: http.IncomingHttpHeaders;
  body: Buffer;
}

/** A receiver on an ephemeral port of 127.0.0.1 that answers 204. */
async function startReceiver(): Promise<{
  url: string;
  received: Received[];
  close: () => Promise<void>;
}> {
  const received: Received[] = [];
  const server = http.createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => {
      received.push({ headers: req.headers, body: Buffer.concat(chunks) });
      res.statusCode = 204;
      res.end();
    });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${port}/hook`,
    received,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  };
}

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), { timeout: 30_000 });
}

test('a dispatch rule of the board settings sends a signed delivery, and the log shows it', async ({
  page,
}) => {
  const token = await mintToken();
  const board = await loadBoard(GUI, token, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo');
  const active = board.columnId.get('Active');
  if (!todo || !active) throw new Error('platform-delivery has no Todo or Active column');
  const receiver = await startReceiver();
  const name = `E2E runner ${Date.now()}`;
  const panel = page.locator('[data-testid="dispatch-panel"]');
  // KAIROS-T-0453: the label EXACTLY. A string hasText is a substring with no
  // case, so 'Column' also found the trigger select ("enters the column") and
  // 'Target' the field "Target name": strict mode refused two matches.
  const field = (label: string) =>
    panel.locator('.cl-field').filter({ has: page.getByText(label, { exact: true }) });
  let code = '';
  let secret = '';

  try {
    await test.step('login, and make a target on the board settings page', async () => {
      await login(page);
      await page.goto(`/admin/boards/${board.id}`);
      await expect(panel).toBeVisible({ timeout: 30_000 });
      await field('Target name').locator('input').fill(name);
      await field('URL').locator('input').fill(receiver.url);
      await panel.getByRole('button', { name: 'Add target', exact: true }).click();
      await expect(panel.getByText(`The secret of ${name}`)).toBeVisible();
      const shown = panel.getByText(/^kairos_whsec_[0-9a-f]{64}$/).first();
      await expect(shown).toBeVisible();
      secret = ((await shown.textContent()) ?? '').trim();
      expect(secret).toMatch(/^kairos_whsec_[0-9a-f]{64}$/);
      await expect(panel.getByText(receiver.url)).toBeVisible();
    });

    await test.step('make a rule: a task that enters Active goes to the target', async () => {
      await field('Column').locator('select').selectOption({ label: 'Active' });
      await field('Target').locator('select').selectOption({ label: name });
      await field('Task type').locator('select').selectOption({ label: 'task' });
      await panel.getByRole('button', { name: 'Add rule', exact: true }).click();
      await expect(
        panel.locator('[data-testid="dispatch-rule"]', {
          hasText: `type task that enters Active goes to ${name}`,
        }),
      ).toBeVisible();
    });

    await test.step('a task enters Active: the receiver gets one signed delivery', async () => {
      const task = await createTask(GUI, token, {
        title: `E2E dispatch ${Date.now()}`,
        boardId: board.id,
      });
      code = task.short_code;
      if (task.column_id !== todo) await transitionTask(GUI, token, code, todo);
      await transitionTask(GUI, token, code, active);
      const mine = () =>
        receiver.received.filter(
          (r) =>
            r.headers['x-kairos-event'] === 'task.entered_column' &&
            JSON.parse(r.body.toString()).data?.code === code,
        );
      await expect.poll(() => mine().length, { timeout: 30_000 }).toBe(1);
      const delivery = mine()[0];
      const expected =
        'sha256=' + crypto.createHmac('sha256', secret).update(delivery.body).digest('hex');
      expect(delivery.headers['x-kairos-signature']).toBe(expected);
      expect(delivery.headers['x-kairos-delivery']).toBeTruthy();
      const body = JSON.parse(delivery.body.toString());
      expect(body.event).toBe('task.entered_column');
      expect(body.data.column.name).toBe('Active');
      expect(body.data.link).toContain(`/items/${code}`);
    });

    await test.step('the delivery log shows it, delivered', async () => {
      const row = panel
        .locator('[data-testid="delivery-log"] tr', { hasText: code })
        .filter({ hasText: 'task.entered_column' });
      await expect(async () => {
        await panel
          .locator('[data-testid="delivery-log"]')
          .getByRole('button', { name: 'Refresh', exact: true })
          .click();
        await expect(row.first()).toContainText('delivered', { timeout: 2_000 });
      }).toPass({ timeout: 30_000 });
    });

    await test.step('the triage setting: an agent decides, then a person again', async () => {
      const triage = page.locator('[data-testid="agent-triage"]');
      const read = async () => {
        const res = await fetch(`${GUI}/api/boards/${board.id}/agent-triage`, {
          headers: { authorization: `Bearer ${token}` },
        });
        return (await res.json()).agents_decide as boolean;
      };
      const before = await read();
      await triage.getByText('An agent decides the triage of intake').click();
      await triage.getByRole('button', { name: 'Save', exact: true }).click();
      await expect(page.getByText('Kairos saved the triage setting.')).toBeVisible();
      await expect.poll(read).toBe(!before);
      await page
        .locator('[data-testid="agent-triage"]')
        .getByText('An agent decides the triage of intake')
        .click();
      await page.locator('[data-testid="agent-triage"]').getByRole('button', { name: 'Save', exact: true }).click();
      await expect.poll(read).toBe(before);
    });
  } finally {
    // Remove the target with its rule, so that no later move sends to it.
    const targets = await fetch(`${GUI}/api/boards/${board.id}/webhook-targets`, {
      headers: { authorization: `Bearer ${token}` },
    });
    if (targets.ok) {
      const list = await targets.json();
      const items: any[] = Array.isArray(list) ? list : (list.items ?? []);
      for (const target of items.filter((t) => t.name === name)) {
        await fetch(`${GUI}/api/boards/${board.id}/webhook-targets/${target.id}`, {
          method: 'DELETE',
          headers: { authorization: `Bearer ${token}` },
        });
      }
    }
    if (code) await tryArchiveTask(GUI, token, code);
    await receiver.close();
  }
});
