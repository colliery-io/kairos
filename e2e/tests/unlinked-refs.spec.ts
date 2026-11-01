// KAIROS-T-0744 — the open pull requests with no item (KAIROS-T-0480):
//
//   1. a new repository: "Show not linked" on Admin, Repositories says that
//      each open branch and pull request is linked;
//   2. a signed webhook of a pull request that names no short code: the
//      row lists it, with a link to the forge, and the team page lists it
//      under "Not linked to an item".
//
// The repository is new for each run; the spec deletes it at the end.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createForgeConnection, createRepository, deliverGithubWebhook } from '../helpers/api';

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

test('a pull request with no item shows on the repository and on the team page', async ({ page }) => {
  const alice = await mintToken();
  const run = Date.now().toString(36);
  const slug = `refs-${run}`;
  const fullName = `acme/${slug}`;
  await createRepository(GUI, alice, { slug, repoFullName: fullName, team: 'platform' });
  const title = `Bump the lint tool ${run}`;
  const row = page.locator(`[data-repo="${slug}"]`).first();
  const refs = row.locator('[data-testid="repository-unlinked-refs"]');

  try {
    await test.step('a new repository has no pull request with no item', async () => {
      await login(page);
      await page.goto('/admin/repositories');
      await expect(row).toBeVisible({ timeout: 30_000 });
      await refs.getByRole('button', { name: 'Show not linked' }).click();
      await expect(refs).toContainText(
        'Each open branch and pull request of this repository is linked to an item.',
      );
    });

    await test.step('a pull request that names no item is listed', async () => {
      const connection = await createForgeConnection(GUI, alice, slug);
      const status = await deliverGithubWebhook(GUI, connection, 'pull_request', {
        action: 'opened',
        repository: { full_name: fullName, html_url: `https://github.com/${fullName}` },
        sender: { login: 'robin' },
        pull_request: {
          number: 12,
          state: 'open',
          merged: false,
          draft: false,
          title,
          body: '',
          html_url: `https://github.com/${fullName}/pull/12`,
          updated_at: new Date().toISOString(),
          user: { login: 'robin' },
          head: { ref: `robin/bump-lint-${run}` },
        },
      });
      expect(status).toBeLessThan(300);
      await page.reload();
      await refs.getByRole('button', { name: 'Show not linked' }).click();
      await expect(refs).toContainText(title, { timeout: 15_000 });
      await expect(refs).toContainText('by robin');
      await expect(refs.getByRole('link', { name: new RegExp(title) })).toHaveAttribute(
        'href',
        `https://github.com/${fullName}/pull/12`,
      );

      await page.goto('/teams/platform');
      const team = page.locator('[data-testid="team-unlinked-refs"]');
      await expect(team).toContainText('Not linked to an item', { timeout: 30_000 });
      await expect(team).toContainText(title);
    });
  } finally {
    await fetch(`${GUI}/api/repositories/${slug}`, {
      method: 'DELETE',
      headers: { authorization: `Bearer ${alice}` },
    });
  }
});
