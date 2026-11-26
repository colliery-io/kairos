// KAIROS-T-0408 — the docs sites in the web app:
//
// - an organization admin turns the site of a repository on in the page
//   Admin, Repositories; after a publish, the team page links to the site,
//   with when and by whom;
// - the nav has "Docs sites", the team row in Admin, Teams links to the
//   team site, and a document has "Open in site", which lands on its page;
// - a member who is not an admin does not get the page Admin, and the API
//   refuses the team site switch to them.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createRepository } from '../helpers/api';
import { targz } from '../helpers/targz';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

/** Sign in through Dex as `email`. */
async function login(page: Page, email: string, password: string): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', email);
  await page.fill('#password', password);
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), { timeout: 30_000 });
}

test('pages in the web app: the switches and the links', async ({ page }) => {
  const token = await mintToken();
  const slug = `e2e-site-${Date.now()}`;
  await createRepository(GUI, token, { slug, repoFullName: `acme/${slug}`, team: 'platform' });
  await login(page, 'alice@kairos.test', 'alice-password');

  await test.step('the admin turns the site on', async () => {
    await page.goto('/admin/repositories');
    const row = page.locator(`[data-repo="${slug}"]`).first();
    await expect(row).toBeVisible({ timeout: 15_000 });
    await row.getByRole('button', { name: 'Edit' }).click();
    await row.getByLabel('Site').selectOption('on');
    await row.getByRole('button', { name: 'Save' }).click();
    await expect
      .poll(async () => {
        const res = await page.request.get(`/api/repositories/${slug}`, {
          headers: { authorization: `Bearer ${token}` },
        });
        return (await res.json()).site?.enabled;
      })
      .toBe(true);
  });

  await test.step('after a publish, the team page links to the site', async () => {
    const put = await page.request.put(`/api/repositories/${slug}/site`, {
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/gzip' },
      data: targz({ 'index.html': '<h1>Published</h1>' }),
    });
    expect(put.status(), await put.text()).toBe(201);
    await page.goto('/teams/platform');
    const line = page.locator(`[data-repo="${slug}"] [data-testid="repo-site"]`);
    await expect(line).toContainText('published', { timeout: 15_000 });
    await expect(line).toContainText('by ');
    const link = line.getByRole('link', { name: 'Open the site' });
    await expect(link).toHaveAttribute('href', `/pages/demo/platform/repos/${slug}/`);
    await link.click();
    await expect(page.getByRole('heading', { name: 'Published' })).toBeVisible();
  });

  await test.step('the nav and the team row link to the docs sites', async () => {
    await page.goto('/boards');
    await expect(page.getByRole('link', { name: 'Docs sites' })).toHaveAttribute(
      'href',
      '/pages/demo/',
    );
    await page.goto('/admin/teams');
    await expect(page.locator('[data-testid="team-site"]').first()).toBeVisible({
      timeout: 15_000,
    });
  });

  await test.step('a document opens in the site', async () => {
    // KAIROS-T-0792: a create takes no content; the text is an update.
    const created = await page.request.post('/api/documents', {
      headers: { authorization: `Bearer ${token}` },
      data: { board: 'platform-delivery', title: `E2E site doc ${Date.now()}` },
    });
    expect(created.ok(), await created.text()).toBeTruthy();
    const made = await created.json();
    const code = made.short_code;
    const filled = await page.request.patch(`/api/documents/${code}`, {
      headers: { authorization: `Bearer ${token}` },
      data: { content: 'Hi.', version: made.version },
    });
    expect(filled.ok(), await filled.text()).toBeTruthy();
    await page.goto(`/items/${code}`);
    await page.locator('[data-testid="open-in-site"]').click();
    await expect(page).toHaveURL(new RegExp(`/pages/demo/platform/docs/${code}/$`));
  });
});

test('pages in the web app: a member gets no admin switch', async ({ page }) => {
  await login(page, 'bob@kairos.test', 'bob-password');
  await page.goto('/admin/repositories');
  await expect(page.getByText('Admin access required')).toBeVisible({ timeout: 15_000 });
  await expect(page.locator('[data-repo]')).toHaveCount(0);

  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const teams = await page.request.get('/api/teams', {
    headers: { authorization: `Bearer ${bob}` },
  });
  const platform = (await teams.json()).items.find((t: { slug: string }) => t.slug === 'platform');
  const refused = await page.request.patch(`/api/teams/${platform.id}`, {
    headers: { authorization: `Bearer ${bob}` },
    data: { site_enabled: false },
  });
  expect(refused.status()).toBe(403);
});
