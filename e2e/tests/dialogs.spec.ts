// COLLIERY-T-1836 — the delete of an item asks in the Aurora ConfirmDialog.
//
// The local dialog of the item page (`.kairos-dialog`) is gone. The
// delete now asks in an Aurora `ConfirmDialog` (role="dialog", a label, a
// focus trap, Escape closes it), and the report of the server shows in an
// Aurora `Modal` after the delete.
//
//   1. alice makes a task through the API and opens its page
//   2. Archive opens the dialog "Archive CODE?" with the cascade preview;
//      Escape closes it and deletes nothing
//   3. Archive again, then "Archive": the report says "Archived",
//      and the server has put the task away
//
// With AURORA_REVIEW_DIR set, the dialog is saved as a screenshot in the
// two themes.

import { test, expect } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, tryArchiveTask } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const REVIEW_DIR = process.env.AURORA_REVIEW_DIR;

test('dialogs: the delete asks in a ConfirmDialog, and the report is a Modal', async ({
  page,
}) => {
  const alice = await mintToken({ server: GUI });
  const boardRes = await fetch(`${GUI}/api/boards/platform-delivery`, {
    headers: { authorization: `Bearer ${alice}` },
  });
  expect(boardRes.ok).toBe(true);
  const board = await boardRes.json();
  const task = await createTask(GUI, alice, {
    title: `T-1836 delete dialog ${Date.now().toString(36)}`,
    boardId: board.id,
  });
  const code: string = task.short_code;

  try {
    await test.step('login via Dex as alice and open the task', async () => {
      await page.goto('/');
      await page.waitForSelector('#login', { timeout: 30_000 });
      await page.fill('#login', 'alice@kairos.test');
      await page.fill('#password', 'alice-password');
      await page.click('#submit-login');
      await page.waitForURL((url) => url.pathname.startsWith('/boards'), { timeout: 30_000 });
      await page.goto(`/items/${code}`);
      await expect(page.locator('.kairos-editor')).toBeVisible({ timeout: 20_000 });
    });

    const dialog = page.getByRole('dialog', { name: `Archive ${code}?` });

    await test.step('Archive opens the confirm dialog; Escape cancels', async () => {
      await page.getByRole('button', { name: 'Archive', exact: true }).click();
      await expect(dialog).toBeVisible();
      await expect(dialog).toHaveAttribute('aria-modal', 'true');
      await expect(dialog.getByText('This cascades')).toBeVisible();
      await expect(
        dialog.getByText('Nothing is below it. Only this item goes to the archive.'),
      ).toBeVisible({ timeout: 15_000 });
      if (REVIEW_DIR) {
        for (const theme of ['light', 'dark']) {
          await page.evaluate((t) => document.documentElement.setAttribute('data-theme', t), theme);
          await page.screenshot({ path: `${REVIEW_DIR}/delete-dialog-${theme}.png` });
        }
        await page.evaluate(() => document.documentElement.removeAttribute('data-theme'));
      }
      await page.keyboard.press('Escape');
      await expect(dialog).toHaveCount(0);
      const still = await fetch(`${GUI}/api/tasks/${code}`, {
        headers: { authorization: `Bearer ${alice}` },
      });
      expect((await still.json()).archived_at ?? null).toBeNull();
    });

    await test.step('confirm archives, and the report shows in a Modal', async () => {
      await page.getByRole('button', { name: 'Archive', exact: true }).click();
      await expect(dialog).toBeVisible();
      await dialog.getByRole('button', { name: 'Archive', exact: true }).click();
      const report = page.getByRole('dialog', { name: 'Archived' });
      await expect(report).toBeVisible({ timeout: 15_000 });
      await expect(report).toContainText(code);
      await expect(report).toContainText('Nothing below it went to the archive.');
      await expect(report).toContainText('You can restore it.');
      await expect(report.getByRole('link', { name: 'Back to boards' })).toBeVisible();
      const gone = await fetch(`${GUI}/api/tasks/${code}`, {
        headers: { authorization: `Bearer ${alice}` },
      });
      expect((await gone.json()).archived_at).toBeTruthy();
    });
  } finally {
    await tryArchiveTask(GUI, alice, code);
  }
});
