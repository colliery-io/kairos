// KAIROS-T-0744 — the "does not pass" mark of a document whose template
// has typed sections (KAIROS-T-0729, KAIROS-T-0731):
//
//   1. alice (an organization admin) makes a template with two typed
//      sections over the API, and a document of the template with one
//      section empty and one missing; the item page shows the badge next to
//      the version and the panel with one line for each problem;
//   2. a change of the sections of the template marks the check stale: the
//      panel says that Kairos checks the document again;
//   3. a save that fills each section clears the mark: no badge, no panel.
//
// The template slug and the document title are new for each run, and the
// spec removes the template at the end.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function login(page: Page, path: string): Promise<void> {
  await page.goto(path);
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => !url.pathname.startsWith('/callback') && !url.pathname.includes('dex'), {
    timeout: 30_000,
  });
  await page.goto(path);
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

test('a document that fails the typed sections of its template has a mark', async ({ page }) => {
  const alice = await mintToken();
  const run = Date.now().toString(36);
  const template = await call(alice, 'POST', '/api/templates', {
    name: `E2E sections ${run}`,
    slug: `e2e-sections-${run}`,
    content: '## Goal\n\n## Scope\n',
    sections: [
      { heading: 'Goal', type: 'free' },
      { heading: 'Scope', type: 'free' },
    ],
  });

  try {
    const doc = await call(alice, 'POST', '/api/documents', {
      title: `E2E sections ${run}`,
      board: 'platform-delivery',
      template_id: template.id,
      content: '## Goal\n\n',
    });
    const code: string = doc.short_code;
    const badge = page.locator('[data-testid="sections-badge"]');
    const panel = page.locator('[data-testid="sections-panel"]');

    await test.step('the badge and the problems show', async () => {
      await login(page, `/items/${code}`);
      await expect(badge).toContainText('does not pass: 2 violations', { timeout: 30_000 });
      const problems = panel.locator('[data-testid="sections-list"] [data-testid="sections-problem"]');
      await expect(problems).toHaveCount(2);
      await expect(problems.nth(0)).toContainText('Goal (free): the section has no text.');
      await expect(problems.nth(1)).toContainText('Scope (free): the section is not in the document.');
      await expect(panel.locator('[data-testid="sections-stale"]')).toHaveCount(0);
    });

    await test.step('a change of the template makes the check stale', async () => {
      await call(alice, 'PATCH', `/api/templates/${template.id}`, {
        sections: [
          { heading: 'Goal', type: 'free' },
          { heading: 'Scope', type: 'free' },
          { heading: 'Risks', type: 'free' },
        ],
      });
      await page.reload();
      await expect(panel.locator('[data-testid="sections-stale"]')).toBeVisible({ timeout: 30_000 });
    });

    await test.step('a save that fills each section clears the mark', async () => {
      const current = await call(alice, 'GET', `/api/documents/${code}`);
      await call(alice, 'PATCH', `/api/documents/${code}`, {
        content: '## Goal\n\nShip the report.\n\n## Scope\n\nThe web page.\n\n## Risks\n\nNone.\n',
        version: current.version,
      });
      await page.reload();
      await expect(page.getByText(`E2E sections ${run}`).first()).toBeVisible({ timeout: 30_000 });
      await expect(badge).toHaveCount(0);
      await expect(panel).toHaveCount(0);
    });
  } finally {
    await call(alice, 'DELETE', `/api/templates/${template.id}`).catch(() => undefined);
  }
});
