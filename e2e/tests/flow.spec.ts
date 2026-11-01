// KAIROS-T-0372 — the flow metrics on the board page:
//
//   1. the button "Flow" opens the panel: the tiles (Done, Cancelled, the
//      cycle time, In progress), the throughput per week, the work in
//      progress, and one row of the cumulative flow for each column;
//   2. a task that the spec moves to Active is in the work in progress;
//   3. the range "Last 30 days" reads the flow again;
//   4. "Hide flow" closes the panel.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, transitionTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), { timeout: 30_000 });
}

test('the board page shows the flow metrics', async ({ page }) => {
  const token = await mintToken();
  const board = await loadBoard(GUI, token, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo')!;
  const active = board.columnId.get('Active')!;
  const task = await createTask(GUI, token, { title: `E2E flow ${Date.now()}`, boardId: board.id });
  if (task.column_id !== todo) await transitionTask(GUI, token, task.short_code, todo);
  await transitionTask(GUI, token, task.short_code, active);

  try {
    await login(page);
    await page.goto('/boards/platform-delivery');
    await expect(page.locator('article.kairos-card').first()).toBeVisible();
    const panel = page.locator('[data-testid="flow-panel"]');

    await test.step('Flow opens the panel', async () => {
      await page.getByRole('button', { name: 'Flow', exact: true }).click();
      await expect(panel).toBeVisible();
      for (const label of ['Done', 'Cancelled', 'Cycle time, median', 'In progress']) {
        await expect(panel.locator('.cl-stat__label', { hasText: label }).first()).toBeVisible();
      }
      await expect(panel.locator('[data-testid="flow-throughput"] tbody tr').first()).toBeVisible();
      // One row for each column; a column with an old name in the history
      // adds a row.
      await expect(panel.locator('[data-testid="flow-cfd"]')).toContainText('Cumulative flow');
      expect(await panel.locator('[data-testid="flow-cfd"] [data-testid="cfd-row"]').count()).toBeGreaterThanOrEqual(
        board.columnId.size,
      );
      await expect(panel.locator('[data-testid="flow-ageing"]')).toContainText(task.short_code);
    });

    await test.step('the range reads the flow again', async () => {
      const weeks = panel.locator('[data-testid="flow-throughput"] tbody tr');
      // 90 days have 13 or 14 weeks; 30 days have 5 or 6.
      expect(await weeks.count()).toBeGreaterThanOrEqual(13);
      await panel.locator('select').selectOption({ label: 'Last 30 days' });
      await expect.poll(() => weeks.count()).toBeLessThanOrEqual(6);
      await expect(panel.locator('[data-testid="flow-ageing"]')).toContainText(task.short_code);
    });

    await test.step('Hide flow closes the panel', async () => {
      await page.getByRole('button', { name: 'Hide flow', exact: true }).click();
      await expect(panel).toHaveCount(0);
    });
  } finally {
    await tryArchiveTask(GUI, token, task.short_code);
  }
});
