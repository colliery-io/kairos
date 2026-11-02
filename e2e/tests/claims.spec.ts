// KAIROS-T-0369, KAIROS-T-0535 — the claim of a task on the item page,
// and the ends of a task, in the GUI:
//
//   1. a move to Active on the item page gives alice the claim;
//   2. Hand off gives the claim to bob: the picker starts with no person,
//      and a dialog asks before the hand-off;
//   3. the board grouped by owner (KAIROS-T-0416) puts the task in the
//      lane of the person with the claim, and follows a hand-off back to
//      alice live;
//   4. Release ends the claim; the panel then shows Claim and Hand off;
//   5. Claim gives alice the free task again;
//   6. Cancel with a reason puts the cancel mark on the task;
//   7. Delete for good (a board manager) removes the task, once it is
//      archived (KAIROS-T-0570);
//   8. the Agent keys page makes a key, rotates it and revokes it.
//
// The task is new for each run (a timestamped title), so a retry starts
// clean.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard, readTask } from '../helpers/api';

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

test('a task is claimed, handed off, released, cancelled and deleted', async ({
  page,
}) => {
  const token = await mintToken();
  const board = await loadBoard(GUI, token, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo');
  if (!todo) throw new Error('platform-delivery has no Todo column');
  const created = await createTask(GUI, token, {
    title: `E2E claim ${Date.now()}`,
    boardId: board.id,
  });
  const code: string = created.short_code;
  // The task starts in the entry column; put it in Todo, where a move to
  // Active is allowed.
  if (created.column_id !== todo) {
    const res = await fetch(`${GUI}/api/tasks/${code}/transition`, {
      method: 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ to_column_id: todo }),
    });
    expect(res.ok).toBeTruthy();
  }

  await test.step('login via Dex as alice', () => login(page));
  await page.goto(`/items/${code}`);
  const claim = page.locator('[data-testid="claim-panel"]');

  await test.step('a move to Active gives alice the claim', async () => {
    await expect(claim).toHaveCount(0);
    const move = page.locator('.kairos-item__move');
    await move.locator('select').first().selectOption({ label: 'Active' });
    await move.getByRole('button', { name: 'Move', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: 'Moved to Active.' })).toBeVisible();
    await expect(claim).toContainText(/alice @\S+ has the claim since/);
    expect((await readTask(GUI, token, code)).claim?.display_name).toBe('alice');
  });

  await test.step('Hand off gives the claim to bob', async () => {
    const picker = claim.locator('select');
    const handOff = claim.getByRole('button', { name: 'Hand off', exact: true });
    // The picker starts with no person, so Hand off waits for a choice.
    await expect(picker).toHaveValue('');
    await expect(handOff).toBeDisabled();
    await picker.selectOption({ label: 'bob (bob@kairos.test)' });
    await handOff.click();
    const dialog = page.getByRole('dialog', { name: `Hand off ${code}?` });
    await expect(dialog).toContainText('Kairos gives the claim to bob (bob@kairos.test).');
    await dialog.getByRole('button', { name: 'Hand off', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: 'Kairos gave the claim to bob.' })).toBeVisible();
    await expect(claim).toContainText(/bob @\S+ has the claim since/);
    expect((await readTask(GUI, token, code)).claim?.display_name).toBe('bob');
  });

  await test.step('the board grouped by owner puts the task in the lane of its claim', async () => {
    // KAIROS-T-0416: one grouping at a time — the owner toggle turns the
    // repository grouping off.
    await page.goto('/boards/platform-delivery?by_repo=1');
    await page.locator('[data-testid="group-by-owner"]').click();
    await expect(page).toHaveURL(/by_owner=1/);
    await expect(page).not.toHaveURL(/by_repo/);
    const lanes = page.locator('.kairos-board__lane--owner');
    const card = (name: string) =>
      page.locator(`.kairos-board__lane--owner[data-owner-name="${name}"] .kairos-card`, {
        hasText: code,
      });
    await expect(card('bob')).toHaveCount(1);
    await expect(lanes.last()).toHaveAttribute('data-owner-name', 'No owner');
    // A hand-off back to alice: the live update moves the card, and the
    // lane of the reader (alice) is first.
    const res = await fetch(`${GUI}/api/tasks/${code}/hand-off`, {
      method: 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ to: 'alice@kairos.test' }),
    });
    expect(res.ok).toBeTruthy();
    await expect(card('alice')).toHaveCount(1);
    await expect(card('bob')).toHaveCount(0);
    await expect(lanes.first()).toHaveAttribute('data-owner-mine', 'true');
    await expect(lanes.first()).toHaveAttribute('data-owner-name', 'alice');
    // The repository toggle turns the owner grouping off.
    const byRepo = page.locator('[data-testid="group-by-repo"]');
    if (await byRepo.count()) {
      await byRepo.click();
      await expect(page).toHaveURL(/by_repo=1/);
      await expect(page).not.toHaveURL(/by_owner/);
    }
    await page.goto(`/items/${code}`);
  });

  await test.step('Release ends the claim', async () => {
    await expect(claim).toContainText(/alice @\S+ has the claim since/);
    await claim.getByRole('button', { name: 'Release', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: 'Kairos released the claim.' })).toBeVisible();
    await expect(claim).toContainText('No claim.');
    expect((await readTask(GUI, token, code)).claim ?? null).toBeNull();
    // A task with no claim offers Claim and Hand off, and no Release.
    await expect(claim.getByRole('button', { name: 'Claim', exact: true })).toBeVisible();
    await expect(claim.getByRole('button', { name: 'Hand off', exact: true })).toBeVisible();
    await expect(claim.getByRole('button', { name: 'Release', exact: true })).toHaveCount(0);
  });

  await test.step('Claim gives alice the free task again', async () => {
    await claim.getByRole('button', { name: 'Claim', exact: true }).click();
    await expect(page.locator('.cl-toast', { hasText: 'You have the claim.' })).toBeVisible();
    await expect(claim).toContainText(/alice @\S+ has the claim since/);
    expect((await readTask(GUI, token, code)).claim?.display_name).toBe('alice');
  });

  await test.step('Cancel with a reason puts the mark on the task', async () => {
    await page.getByRole('button', { name: 'Cancel task', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: `Cancel ${code}?` });
    await expect(dialog).toBeVisible();
    await dialog.locator('textarea').fill('The team does not need it.');
    await dialog.getByRole('button', { name: 'Cancel task', exact: true }).click();
    const banner = page.locator('[data-testid="cancelled-banner"]');
    await expect(banner).toBeVisible();
    await expect(banner).toContainText('Reason: The team does not need it.');
    expect((await readTask(GUI, token, code)).cancellation?.reason).toBe(
      'The team does not need it.',
    );
  });

  await test.step('a board manager deletes the task for good', async () => {
    // KAIROS-T-0570: Delete for good is for an archived task. Archive it
    // first, then open the page again.
    const archived = await fetch(`${GUI}/api/tasks/${code}`, {
      method: 'DELETE',
      headers: { authorization: `Bearer ${token}` },
    });
    expect(archived.ok, `archive ${code} -> ${archived.status}`).toBeTruthy();
    await page.goto(`/items/${code}`);
    await expect(page.locator('[data-testid="archived-banner"]').first()).toBeVisible({
      timeout: 20_000,
    });
    await page.getByRole('button', { name: 'Delete', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: `Delete ${code} for good?` });
    await expect(dialog).toBeVisible();
    await dialog.locator('input').fill(code);
    await dialog.getByRole('button', { name: 'Delete for good', exact: true }).click();
    await page.waitForURL((url) => url.pathname === '/boards', { timeout: 15_000 });
    const gone = await fetch(`${GUI}/api/tasks/${code}`, {
      headers: { authorization: `Bearer ${token}` },
    });
    expect(gone.status).toBe(404);
  });
});

test('the Agent keys page makes a key, rotates it and revokes it', async ({ page }) => {
  const name = `e2e-key-${Date.now()}`;
  await test.step('login via Dex as alice', () => login(page));
  await page.locator('a[href="/agent-keys"]').first().click();
  await page.waitForURL(/\/agent-keys/);

  await test.step('make a key: the secret shows once', async () => {
    await page.getByLabel('Name', { exact: true }).fill(name);
    await page.getByRole('button', { name: 'Make key', exact: true }).click();
    await expect(page.getByText(`The key ${name}`)).toBeVisible();
    await expect(page.getByText('Copy the key now.')).toBeVisible();
  });

  const rows = page.locator('[data-testid="agent-key"]', { hasText: name });

  await test.step('rotate the key: a new secret, the old one ends later', async () => {
    await expect(rows).toHaveCount(1);
    await expect(rows.first()).toContainText('active');
    await rows.first().getByRole('button', { name: 'Rotate', exact: true }).click();
    await expect(page.getByText('The old secret works until')).toBeVisible();
    await expect(rows).toHaveCount(2);
    await expect(rows.filter({ hasText: 'rotated' })).toHaveCount(1);
  });

  const row = rows.filter({ hasText: 'active' });

  await test.step('revoke the new key: it stays in the list, marked', async () => {
    await expect(row).toHaveCount(1);
    await row.getByRole('button', { name: 'Revoke', exact: true }).click();
    await expect(page.getByText(`Kairos revoked the key ${name}.`)).toBeVisible();
    const token = await mintToken();
    const res = await fetch(`${GUI}/api/me/agent-keys`, {
      headers: { authorization: `Bearer ${token}` },
    });
    const keys = (await res.json()).items as any[];
    const mine = keys.filter((k) => k.name === name);
    expect(mine).toHaveLength(2);
    const fresh = mine.find((k) => !k.replaced_by);
    const old = mine.find((k) => k.replaced_by);
    expect(fresh?.revoked_at).toBeTruthy();
    expect(old?.replaced_by).toBe(fresh?.id);
  });
});
