// KAIROS-T-0324 — gherkin and mermaid fences in the rendered markdown:
//
//   1. a gherkin fence shows highlighted keywords, steps and tags, and its
//      text stays escaped
//   2. a mermaid fence becomes an SVG diagram (the vendored Mermaid loads
//      only for such a page)
//   3. a mermaid fence that does not parse shows the error, and keeps its
//      source; the rest of the page still renders
//
// The task is the spec's own, and the spec deletes it.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const SERVER = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const BOARD_SLUG = 'platform-delivery';

const bearer = (token: string) => ({ authorization: `Bearer ${token}` });

async function api(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(SERVER + path, {
    method,
    headers: body ? { ...bearer(token), 'content-type': 'application/json' } : bearer(token),
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`${method} ${path} -> ${res.status}: ${text}`);
  }
  return text ? JSON.parse(text) : null;
}

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), { timeout: 30_000 });
}

const CONTENT = [
  '## Acceptance',
  '',
  '```gherkin',
  '@teams',
  'Feature: Team pills',
  '  Scenario: An initiative with tasks',
  '    Given an initiative with a task on "<b>web</b>"',
  '    Then the card shows the pill web',
  '```',
  '',
  '```mermaid',
  'graph TD',
  '  Strategy --> Initiative',
  '  Initiative --> Task',
  '```',
  '',
  '```mermaid',
  'this is not a diagram ->->',
  '```',
  '',
  'The end of the page.',
].join('\n');

test('markdown: gherkin is highlighted and mermaid is drawn', async ({ page }) => {
  const token = await mintToken();
  const boards = (await api(token, 'GET', '/api/boards?limit=100')).items as any[];
  const board = boards.find((b) => b.slug === BOARD_SLUG);
  if (!board) throw new Error(`no ${BOARD_SLUG} board in the seed`);
  const detail = await api(token, 'GET', `/api/boards/${board.id}`);
  const task = await api(token, 'POST', '/api/tasks', {
    board_id: board.id,
    column_id: detail.columns[0].id,
    title: `E2E: fences ${Date.now().toString(36)}`,
    content: CONTENT,
  });
  const code: string = task.short_code;

  // Count the requests for the vendored Mermaid.
  const mermaidLoads: string[] = [];
  page.on('request', (request) => {
    if (/\/mermaid-[\d.]+\.min\.js$/.test(request.url())) mermaidLoads.push(request.url());
  });

  try {
    await test.step('login as alice; the boards page loads no Mermaid', async () => {
      await login(page);
      expect(mermaidLoads).toHaveLength(0);
    });

    await page.goto(`/items/${code}`);
    const preview = page.locator('.kairos-editor .kairos-markdown');
    await expect(preview).toContainText('The end of the page.', { timeout: 15_000 });

    await test.step('the gherkin fence is highlighted and escaped', async () => {
      const gherkin = preview.locator('pre.kairos-gherkin');
      await expect(gherkin.locator('.kairos-gherkin__keyword', { hasText: 'Feature:' })).toBeVisible();
      await expect(gherkin.locator('.kairos-gherkin__keyword', { hasText: 'Scenario:' })).toBeVisible();
      await expect(gherkin.locator('.kairos-gherkin__step', { hasText: 'Given' })).toBeVisible();
      await expect(gherkin.locator('.kairos-gherkin__tag')).toHaveText('@teams');
      await expect(gherkin.locator('.kairos-gherkin__string')).toHaveText('"<b>web</b>"');
      await expect(gherkin.locator('b')).toHaveCount(0);
    });

    await test.step('the mermaid fence is an SVG diagram', async () => {
      const diagram = preview.locator('.kairos-mermaid-diagram svg');
      await expect(diagram).toHaveCount(1, { timeout: 15_000 });
      await expect(diagram).toContainText('Initiative');
      expect(mermaidLoads).toHaveLength(1);
    });

    await test.step('a fence that does not parse shows the error and its source', async () => {
      const broken = preview.locator('pre.kairos-mermaid.kairos-mermaid--error');
      await expect(broken).toContainText('this is not a diagram');
      await expect(preview.locator('.kairos-mermaid__error')).toContainText('The diagram has an error');
    });
  } finally {
    await api(token, 'DELETE', `/api/tasks/${code}`);
  }
});
