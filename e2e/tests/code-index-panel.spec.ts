// KAIROS-T-0333 — the code index panel of a repository, on the admin page
// Repositories and on the team page:
//
//   1. REAL PKCE login (alice, an organization admin); a repository that
//      the spec registers over the API, so its panel starts with no index
//      and no run
//   2. the admin row: "Show code index" opens the panel, which says "No
//      index yet." and that the builder has no run; alice sees the button
//      "Build again"
//   3. a second session of alice has the same panel open on the team page;
//      alice asks for a build in the first session; the run arrives as
//      `running` in BOTH sessions with no reload, and the button goes off
//      while it runs. (The test server has no summarizer, so the builder
//      does not run the request: the row stays `running`.)
//   4. the gate: carol, a member of the web team, sees no button under a
//      platform repository, and sees it under a web repository
//
// The registered repository is the only write of the spec, with a per-run
// suffix; the build request writes one run row of that repository.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createRepository } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);

async function login(page: Page, email: string, password: string): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', email);
  await page.fill('#password', password);
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

const panelOf = (page: Page, slug: string) =>
  page.locator(`[data-testid="code-index-panel"][data-code-index-repo="${slug}"]`).first();

async function openPanel(page: Page, slug: string) {
  const panel = panelOf(page, slug);
  await expect(panel).toBeVisible();
  await panel.getByRole('button', { name: 'Show code index' }).click();
  await expect(panel.locator('[data-testid="code-index-newest"]')).toBeVisible({
    timeout: 10_000,
  });
  return panel;
}

test('code index panel: the read view, a live run, and the button gate', async ({
  page,
  browser,
}) => {
  const slug = `codeindex-${RUN}`;

  await test.step('register a repository of the platform team', async () => {
    const token = await mintToken();
    await createRepository(GUI, token, {
      slug,
      repoFullName: `acme/${slug}`,
      team: 'platform',
    });
  });

  await test.step('login via Dex as alice', async () => {
    await login(page, 'alice@kairos.test', 'alice-password');
  });

  await test.step('the admin row opens the panel: no index, no run, the button', async () => {
    await page.goto('/admin/repositories');
    const row = page.locator(`[data-repo="${slug}"]`).first();
    await expect(row).toBeVisible({ timeout: 15_000 });
    const panel = await openPanel(page, slug);
    await expect(panel.locator('[data-testid="code-index-newest"]')).toHaveText('No index yet.');
    await expect(panel.getByText('No run of the builder yet')).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Build again' })).toBeEnabled();
  });

  const second = await browser.newPage();
  await test.step('a second session has the panel open on the team page', async () => {
    await login(second, 'alice@kairos.test', 'alice-password');
    await second.goto('/teams/platform');
    const panel = await openPanel(second, slug);
    await expect(panel.locator('[data-testid="code-index-runs"]')).toHaveCount(0);
  });

  await test.step('a build request arrives as a running run in both sessions, live', async () => {
    const panel = panelOf(page, slug);
    await panel.getByRole('button', { name: 'Build again' }).click();
    await expect(panel.locator('[data-testid="code-index-notice"]')).toContainText(
      'Kairos builds the index again',
      { timeout: 10_000 },
    );
    await expect(panel.locator('[data-testid="code-index-runs"] [data-outcome="running"]')).toHaveCount(
      1,
      { timeout: 10_000 },
    );
    await expect(panel.getByRole('button', { name: 'Build again' })).toBeDisabled();
    // The second session heard the event: no reload, no click.
    const other = panelOf(second, slug);
    await expect(other.locator('[data-testid="code-index-runs"] [data-outcome="running"]')).toHaveCount(
      1,
      { timeout: 15_000 },
    );
    await expect(other.getByRole('button', { name: 'Build again' })).toBeDisabled();
  });
  await second.close();

  await test.step('carol sees the button only under a repository of her team', async () => {
    const context = await browser.newContext();
    const carol = await context.newPage();
    await login(carol, 'carol@kairos.test', 'carol-password');
    await carol.goto('/teams/platform');
    const platform = await openPanel(carol, slug);
    await expect(platform.getByRole('button', { name: 'Build again' })).toHaveCount(0);
    await carol.goto('/teams/web');
    const web = await openPanel(carol, 'portal-web');
    await expect(web.getByRole('button', { name: 'Build again' })).toBeVisible();
    await context.close();
  });
});
