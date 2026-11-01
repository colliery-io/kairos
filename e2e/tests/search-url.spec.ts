// KAIROS-T-0583: the search page keeps its state in the URL, offers each
// filter of the API, and the top bar has a search box.
//
//   1. the search box of the top bar opens /search?q= and runs the search;
//      the hit row shows its board and its column;
//   2. the Search button writes the filters, the sort and the page size
//      into the URL;
//   3. a reload gives the same form and the same hits;
//   4. Back gives the search before, with its form;
//   5. a shared link with each filter fills each field of the form;
//   6. a link with an unknown parameter names it.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask } from '../helpers/api';

const SERVER = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const BOARD_SLUG = 'platform-delivery';

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

/** A one-lexeme probe word, fresh per run, so the hit is on page 1. */
function probeWord(): string {
  const letters = 'abcdefghijklmnopqrstuvwxyz';
  const tail = Array.from(
    { length: 8 },
    () => letters[Math.floor(Math.random() * letters.length)],
  ).join('');
  return `urlstate${tail}`;
}

/** The control of a labelled field (`.cl-field` with the label text). */
const field = (page: Page, label: string, control = 'input') =>
  page.locator('.cl-field', { has: page.locator('label', { hasText: label }) }).locator(control).first();

/** The group of the task-type chips, under its caption "Task types". */
const taskTypeChips = (page: Page) =>
  page
    .locator('.cl-stack', { has: page.getByText('Task types', { exact: true }) })
    .last();

test('search keeps its state in the URL, and the top bar opens it', async ({ page }) => {
  const token = await mintToken();
  const probe = probeWord();
  const task = await createTask(SERVER, token, {
    title: `E2E ${probe} search state`,
    boardId: BOARD_SLUG,
  });

  await test.step('login via Dex as alice', () => login(page));

  await test.step('the search box of the top bar opens the search page', async () => {
    const box = page.locator('[data-testid="shell-search"] input');
    await expect(box).toBeVisible();
    await box.fill(probe);
    await box.press('Enter');
    await page.waitForURL(new RegExp(`/search\\?q=${probe}$`));
    await expect(field(page, 'Text query')).toHaveValue(probe);
    const row = page.locator('tr', { hasText: task.short_code });
    await expect(row).toBeVisible({ timeout: 20_000 });
    await expect(row.locator('[data-testid="search-hit-place"]')).toContainText(BOARD_SLUG);
    // No claim and no owner: the person cell is empty.
    await expect(row.locator('[data-testid="search-hit-person"]')).toHaveText('');
  });

  await test.step('Search writes the filters, the sort and the page size', async () => {
    await field(page, 'Board', 'select').selectOption(BOARD_SLUG);
    // The chip "task" of the task types; the entity types have a chip
    // "task" too.
    await taskTypeChips(page).locator('.cl-chip', { hasText: /^task$/ }).click();
    await field(page, 'Sort', 'select').selectOption('title:asc');
    await field(page, 'Page size', 'select').selectOption('5');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await page.waitForURL(/board=platform-delivery/);
    const url = new URL(page.url());
    expect(url.searchParams.get('q')).toBe(probe);
    expect(url.searchParams.get('board')).toBe(BOARD_SLUG);
    expect(url.searchParams.get('task_type')).toBe('task');
    expect(url.searchParams.get('sort')).toBe('title');
    expect(url.searchParams.get('order')).toBe('asc');
    expect(url.searchParams.get('limit')).toBe('5');
    await expect(page.locator('tr', { hasText: task.short_code })).toBeVisible();
  });

  await test.step('a reload gives the same form and the same hits', async () => {
    await page.reload();
    await expect(field(page, 'Text query')).toHaveValue(probe, { timeout: 20_000 });
    await expect(field(page, 'Board', 'select')).toHaveValue(BOARD_SLUG);
    await expect(field(page, 'Sort', 'select')).toHaveValue('title:asc');
    await expect(field(page, 'Page size', 'select')).toHaveValue('5');
    await expect(
      taskTypeChips(page).locator('.cl-chip--active', { hasText: /^task$/ }),
    ).toBeVisible();
    await expect(page.locator('tr', { hasText: task.short_code })).toBeVisible({ timeout: 20_000 });
  });

  await test.step('Back gives the search before', async () => {
    await page.goBack();
    await page.waitForURL(new RegExp(`/search\\?q=${probe}$`));
    await expect(field(page, 'Board', 'select')).toHaveValue('');
    await expect(field(page, 'Sort', 'select')).toHaveValue('');
  });

  await test.step('a shared link fills each field of the form', async () => {
    const link = new URLSearchParams({
      q: probe,
      entity_type: 'task',
      work_class: 'planned',
      board: BOARD_SLUG,
      claimed_by: 'me',
      created_by: 'me',
      is_bucket: 'false',
      'meta.priority': 'high',
      metadata_missing: 'estimate',
      created_after: '2026-01-01',
      updated_before: '2030-12-31',
      include_deleted: 'true',
      sort: 'created_at',
      order: 'desc',
    });
    await page.goto(`/search?${link.toString()}`);
    await expect(field(page, 'Text query')).toHaveValue(probe, { timeout: 20_000 });
    await expect(field(page, 'Claimed by', 'select')).toHaveValue('me');
    await expect(field(page, 'Created by', 'select')).toHaveValue('me');
    await expect(field(page, 'Initiative buckets', 'select')).toHaveValue('false');
    await expect(field(page, 'Created after')).toHaveValue('2026-01-01');
    await expect(field(page, 'Changed before')).toHaveValue('2030-12-31');
    await expect(field(page, 'Metadata with no value')).toHaveValue('estimate');
    await expect(page.locator('input[placeholder="key (definition slug)"]')).toHaveValue('priority');
    await expect(page.locator('.cl-chip--active', { hasText: /^planned$/ })).toBeVisible();
    await expect(page.locator('[data-testid="include-put-away"] .cl-switch--on')).toHaveCount(1);
    // The server answers the search: hits, no hits, or its refusal (an
    // unknown metadata slug). The page must not say that it has nothing.
    await expect(page.getByText('Nothing to search')).toHaveCount(0);
  });

  await test.step('a link with an unknown parameter names it', async () => {
    await page.goto(`/search?q=${probe}&colour=red`);
    await expect(
      page.locator('.cl-alert', { hasText: 'The page cannot use a part of the link' }),
    ).toContainText('"colour"', { timeout: 20_000 });
  });
});
