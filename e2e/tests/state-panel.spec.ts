// KAIROS-T-0586 — the State panel of a task on the item page
// (KAIROS-T-0506). An agent writes the state (`set_state`, `kairos state`);
// the page shows it.
//
//   1. a task with no state shows no State panel;
//   2. a write of each field (the API, as an agent does) shows the banner
//      of the live page; Reload shows the panel: the branch, the commit,
//      Pushed yes, the PR as a link to a new tab, the next step, and who
//      wrote it on which day;
//   3. a write is a patch (KAIROS-T-0533): a field that is not sent stays;
//   4. null clears a field: one row is left, after a reload too.
//
// The task is new for each run; the spec archives it at the end.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);

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

async function setState(token: string, code: string, body: Record<string, unknown>) {
  const res = await fetch(`${GUI}/api/tasks/${code}/state`, {
    method: 'PUT',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  expect(res.ok, `PUT state of ${code} -> ${res.status}`).toBeTruthy();
}

/** The rows of the panel as `label: value` lines. */
async function rows(page: Page): Promise<string[]> {
  const cells = page.locator('[data-testid="state-panel"] .kairos-item__state-row');
  const lines: string[] = [];
  for (let i = 0; i < (await cells.count()); i += 1) {
    const texts = await cells.nth(i).locator('.cl-text').allInnerTexts();
    lines.push(texts.map((t) => t.trim()).join(': '));
  }
  return lines;
}

test('the State panel shows the live state of a task', async ({ page }) => {
  test.setTimeout(180_000);
  const token = await mintToken();
  const task = await createTask(GUI, token, {
    title: `E2E state panel ${RUN}`,
    boardId: 'platform-delivery',
  });
  const code: string = task.short_code;
  const panel = page.locator('[data-testid="state-panel"]');
  const banner = page.getByTestId('item-changed-banner');
  const branch = `kairos-e2e-${RUN}`;
  const pr = `https://example.com/acme/payments-api/pull/${RUN}`;

  try {
    await test.step('login, open the task: no state, no panel', async () => {
      await login(page);
      await page.goto(`/items/${code}`);
      await expect(page.locator('[data-testid="external-refs"]')).toBeVisible({ timeout: 30_000 });
      await expect(panel).toHaveCount(0);
    });

    await test.step('a write of each field: the banner, then the panel', async () => {
      await setState(token, code, {
        branch,
        commit: 'abc1234',
        pushed: true,
        pr_url: pr,
        next_step: 'Wait for the review.',
      });
      await expect(banner).toContainText('the state');
      await banner.getByRole('button', { name: 'Reload' }).click();
      await expect(panel).toBeVisible();
      expect(await rows(page)).toEqual([
        `Branch: ${branch}`,
        'Commit: abc1234',
        'Pushed: yes',
        `PR: ${pr}`,
        'Next step: Wait for the review.',
      ]);
      const link = panel.getByRole('link', { name: pr });
      await expect(link).toHaveAttribute('href', pr);
      await expect(link).toHaveAttribute('target', '_blank');
      // Who wrote it, and the day (UTC).
      await expect(
        page.locator('.cl-panel', { has: panel }).last().locator('.cl-panel__caption'),
      ).toHaveText(/^updated \d{4}-\d{2}-\d{2} by alice$/);
    });

    await test.step('a write is a patch: a field not sent stays', async () => {
      await setState(token, code, { next_step: 'Merge after the queue.' });
      await expect(banner).toContainText('the state');
      await banner.getByRole('button', { name: 'Reload' }).click();
      await expect
        .poll(() => rows(page))
        .toEqual([
          `Branch: ${branch}`,
          'Commit: abc1234',
          'Pushed: yes',
          `PR: ${pr}`,
          'Next step: Merge after the queue.',
        ]);
    });

    await test.step('null clears a field: one row, also after a reload', async () => {
      await setState(token, code, { branch: null, commit: null, pushed: false, pr_url: null });
      await expect(banner).toContainText('the state');
      await banner.getByRole('button', { name: 'Reload' }).click();
      await expect.poll(() => rows(page)).toEqual(['Next step: Merge after the queue.']);
      await page.reload();
      await expect(panel).toBeVisible({ timeout: 30_000 });
      expect(await rows(page)).toEqual(['Next step: Merge after the queue.']);
    });

    await test.step('the card on the board shows the state line', async () => {
      await page.goto('/boards/platform-delivery');
      const line = page
        .locator('.kairos-card', { hasText: code })
        .locator('[data-testid="card-state"]');
      await expect(line).toHaveText('Next: Merge after the queue.', { timeout: 30_000 });
      await expect(line).toHaveAttribute('title', /^State \(not pushed\), updated \d{4}-\d{2}-\d{2} by alice\.$/);
    });
  } finally {
    await tryArchiveTask(GUI, token, code);
  }
});
