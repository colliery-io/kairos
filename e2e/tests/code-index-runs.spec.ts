// KAIROS-T-0744 — the runs and the indexed commits of the code index panel
// (crates/kairos-web/src/pages/repositories/code_index.rs):
//
//   1. six uploads of an index (an empty index of the schema of
//      kairos-index, e2e/helpers/index-db.ts): the panel lists the indexed
//      commits, and the runs of the builder have pages;
//   2. "Build again" asks for a build; the e2e server runs no builder, so
//      the run stays `running`. The repository then turns its builder off:
//      Kairos stops the run, and the run shows its error;
//   3. a server with no builder (KAIROS_ROLE=api) says so in the panel. The
//      e2e server has the role `all`, so the spec replaces the answer of the
//      runs (page.route), as code-index-warnings.spec.ts does.
//
// The repository is new for each run; the spec deletes it at the end.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createRepository } from '../helpers/api';
import { commitId, emptyIndex } from '../helpers/index-db';

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

async function call(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(`${GUI}${path}`, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${await res.text()}`);
  return res.status === 204 ? undefined : res.json();
}

const panelOf = (page: Page, slug: string) =>
  page.locator(`[data-testid="code-index-panel"][data-code-index-repo="${slug}"]`).first();

async function openPanel(page: Page, slug: string) {
  await page.goto('/admin/repositories');
  const panel = panelOf(page, slug);
  await expect(panel).toBeVisible({ timeout: 30_000 });
  await panel.getByRole('button', { name: 'Show code index' }).click();
  await expect(panel.locator('[data-testid="code-index-newest"]')).toBeVisible({ timeout: 15_000 });
  return panel;
}

test('code index runs: uploads, pages of runs, a stopped run, no builder', async ({ page }) => {
  const alice = await mintToken();
  const slug = `coderuns-${RUN}`;
  await createRepository(GUI, alice, { slug, repoFullName: `acme/${slug}`, team: 'platform' });
  const base = Date.now();
  const commits = Array.from({ length: 6 }, (_, i) => commitId(base + i));

  try {
    await test.step('six uploads: the commits and the pages of runs', async () => {
      const index = emptyIndex();
      for (const commit of commits) {
        const res = await fetch(`${GUI}/api/repositories/${slug}/code-indexes/${commit}?ref=main`, {
          method: 'PUT',
          headers: { authorization: `Bearer ${alice}`, 'content-type': 'application/vnd.sqlite3' },
          body: index,
        });
        expect(res.status, await res.text()).toBe(201);
      }
      await login(page);
      const panel = await openPanel(page, slug);
      await expect(panel.locator('[data-testid="code-index-newest"]')).toContainText('Newest index:');
      const list = panel.locator('[data-testid="code-index-commits"]');
      await expect(list.locator('[data-commit]')).toHaveCount(6);
      await expect(list.locator(`[data-commit="${commits[5]}"]`)).toContainText(
        `main · ${commits[5].slice(0, 12)}`,
      );
      await expect(panel.locator('[data-testid="code-index-runs"] [data-outcome="ok"]')).toHaveCount(5);
      const pager = panel.locator('[data-testid="code-index-runs-pager"]');
      await expect(pager).toBeVisible();
      await pager.getByRole('button', { name: /next/i }).click();
      await expect(panel.locator('[data-testid="code-index-runs"] [data-run]')).toHaveCount(1);
    });

    await test.step('Build again, then the builder off: the run stops with an error', async () => {
      const panel = await openPanel(page, slug);
      await panel.locator('[data-testid="code-index-rebuild"]').click();
      await expect(panel.locator('[data-testid="code-index-notice"]')).toBeVisible();
      await expect(panel.locator('[data-testid="code-index-runs"] [data-outcome="running"]')).toHaveCount(1);
      await expect(panel.locator('[data-testid="code-index-rebuild"]')).toBeDisabled();

      await call(alice, 'PATCH', `/api/repositories/${slug}`, { code_index_build: 'off' });
      const failed = panel.locator('[data-testid="code-index-runs"] [data-outcome="failed"]');
      await expect(failed).toHaveCount(1, { timeout: 15_000 });
      await expect(failed.locator('[data-testid="code-index-run-error"]')).toHaveText(
        'cancelled: code_index_build was set to off',
      );
      await call(alice, 'PATCH', `/api/repositories/${slug}`, { code_index_build: 'on' });
    });

    await test.step('a server with no builder says so', async () => {
      const note = 'No code index builder runs. This server has the role api (KAIROS_ROLE=api).';
      await page.route(`**/api/repositories/${slug}/code-indexes/builds*`, async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        await route.fulfill({ response, json: { ...body, builder: note } });
      });
      const panel = await openPanel(page, slug);
      await expect(panel.locator('[data-testid="code-index-no-builder"]')).toHaveText(note);
      await page.unroute(`**/api/repositories/${slug}/code-indexes/builds*`);
    });
  } finally {
    await fetch(`${GUI}/api/repositories/${slug}`, {
      method: 'DELETE',
      headers: { authorization: `Bearer ${alice}` },
    });
  }
});

// A breaker trips when a build runs out of time (KAIROS-T-0399). The e2e
// server runs no build, and no API trips a breaker, so the second half
// replaces the answer of the repository list (page.route) and checks what
// the pages show.
test('code index breakers: none tripped, then one tripped', async ({ page }) => {
  await login(page);

  await test.step('no breaker is tripped', async () => {
    await page.goto('/admin/code-index');
    await expect(page.locator('[data-testid="code-index-breakers"]')).toContainText(
      'No breaker is tripped.',
      { timeout: 30_000 },
    );
  });

  await test.step('a tripped breaker shows on Code index and on Repositories', async () => {
    let slug = '';
    await page.route('**/api/repositories?*', async (route) => {
      const response = await route.fetch();
      const body = await response.json();
      const items = (body.items as any[]).map((repo, index) => {
        if (index !== 0) return repo;
        slug = repo.slug;
        return {
          ...repo,
          code_index_breaker: {
            tripped: true,
            tripped_at: '2026-10-30T09:00:00Z',
            reason: 'The lease of the build ran out',
          },
        };
      });
      await route.fulfill({ response, json: { ...body, items } });
    });
    await page.goto('/admin/code-index');
    const breakers = page.locator('[data-testid="code-index-breakers"]');
    await expect(breakers).toContainText('Code index breaker: tripped at 2026-10-30T09:00:00Z.', {
      timeout: 30_000,
    });
    await expect(breakers).toContainText('The lease of the build ran out.');
    await expect(breakers.getByRole('link', { name: slug })).toHaveAttribute('href', '/admin/repositories');

    await page.goto('/admin/repositories');
    await expect(
      page.locator(`[data-repo="${slug}"]`).first().locator('[data-testid="code-index-breaker-status"]'),
    ).toContainText('Kairos starts no build of this repository until a person pushes "Build again"', {
      timeout: 30_000,
    });
    await page.unroute('**/api/repositories?*');
  });
});
