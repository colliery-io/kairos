// KAIROS-T-0789 (COLLIERY-I-0683, KAIROS-A-0087): the form of a new item
// opens with the template of its type.
//
//   1. REAL PKCE login (alice), open Platform Delivery, "New task"
//   2. the content is the template of the task type, with its use_when
//   3. the task type bug changes the content to the template of a bug
//   4. the person writes the text and saves: the task has the text of the
//      form at version 2 (version 1 is the template)
//
// The spec deletes its task at the end, so the seeded board stays as it was.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function api(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(GUI + path, {
    method,
    headers: {
      authorization: `Bearer ${token}`,
      ...(body ? { 'content-type': 'application/json' } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${text}`);
  return text ? JSON.parse(text) : null;
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

test('the form of a new task opens with the template of its type', async ({ page }) => {
  const title = `Template e2e ${Date.now()}`;
  const token = await mintToken();
  await login(page);
  await page.goto('/boards/platform-delivery');

  await page.getByRole('button', { name: 'New task', exact: true }).click();
  const modal = page.locator('.cl-modal');
  const content = modal.locator('textarea').first();
  const field = (label: string) =>
    modal.locator('.cl-field', { has: page.locator('.cl-field__label', { hasText: label }) });

  await test.step('the content is the template of a task, with its use_when', async () => {
    await expect(content).toHaveValue(/## Objective/, { timeout: 15_000 });
    await expect(content).toHaveValue(/## Acceptance criteria/);
    await expect(modal.getByTestId('create-use-when')).toContainText('Use when');
  });

  await test.step('the task type bug gives the template of a bug', async () => {
    await field('Task type').locator('select').selectOption({ label: 'bug' });
    await expect(content).toHaveValue(/## Impact/);
    await expect(content).toHaveValue(/Steps to reproduce/);
  });

  await test.step('the save writes the text of the form as version 2', async () => {
    await modal.locator('input.cl-input').first().fill(title);
    await content.fill('## Objective\n\nThe export works again.\n');
    await modal.getByRole('button', { name: 'Create', exact: true }).click();
    await expect(modal).toBeHidden();
    const found = await api(token, 'POST', '/api/search', {
      q: title,
      filter: { entity_type: ['task'] },
    });
    const created = found.results.tasks.find((t: any) => t.title === title);
    expect(created).toBeTruthy();
    const task = await api(token, 'GET', `/api/tasks/${created.short_code}`);
    expect(task.content).toBe('## Objective\n\nThe export works again.\n');
    expect(task.version).toBe(2);
    expect(task.task_type).toBe('bug');
    await api(token, 'DELETE', `/api/tasks/${created.short_code}`);
  });
});
