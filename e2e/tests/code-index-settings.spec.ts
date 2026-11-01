// KAIROS-T-0339, KAIROS-T-0340 and KAIROS-T-0415 — the page Admin, Code
// index (where the summaries and the vectors of the code index of the
// organization are made), and the opt-in of a repository on the page
// Admin, Repositories.
//
//   1. REAL PKCE login (alice, an organization admin); a repository that
//      the spec registers over the API; the admin home has the card "Code
//      index"
//   2. the read view shows the defaults: embedded for both, no secret
//   3. with no hosted provider, the opt-in of the repository is refused on
//      the admin form, and the refusal names the page that sets one
//   4. an edit of the summaries with no model: the confirmation lists the
//      change; the save is refused with the text of the server, and the
//      form stays open
//   5. the save with the model works; the read view shows it; the secret is
//      a status with a name; the page never shows the secret
//   6. the repository opts in; the page lists it under hosted summaries
//   7. a save of the builder only leaves the summaries as they are; a
//      concurrency that is not a number is refused next to its field
//   8. a stale save (the settings changed over the API after the page read
//      them) is refused; the page says so and shows the current values
//   9. back to embedded: the confirmation says the stored secret of
//      ollama-cloud goes, and it does
//
// The spec owns the state of the demo tenant for its run: the other specs
// run in parallel on the same database, so every assertion that depends on
// the provider of the tenant is here, and the spec puts the defaults back.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createRepository } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
const RUN = Date.now().toString(36);
const SECRET = `ollama-e2e-${RUN}`;

async function login(page: Page, email: string, password: string): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', email);
  await page.fill('#password', password);
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

const summary = (page: Page) => page.locator('[data-testid="code-index-summary-settings"]');
const summaryView = (page: Page) => page.locator('[data-testid="code-index-summary-view"]');
const builderView = (page: Page) => page.locator('[data-testid="code-index-builder-view"]');
const field = (scope: ReturnType<Page['locator']>, label: string) =>
  scope.locator('.cl-field', { hasText: label }).locator('input, select').first();
const dialog = (page: Page) => page.getByRole('dialog');

/// Confirm the open dialog and wait for the answer of the server.
async function confirmSave(page: Page, label = 'Save') {
  const answered = page.waitForResponse(
    (r) => r.url().includes('/api/org/code-index-settings') && r.request().method() === 'PUT',
  );
  await dialog(page).getByRole('button', { name: label }).click();
  return answered;
}

/// On the admin page Repositories, set the summaries of the repository.
async function setSummaries(page: Page, slug: string, value: 'embedded' | 'hosted' | 'organization') {
  await page.goto('/admin/repositories');
  const row = page.locator(`[data-repo="${slug}"]`).first();
  await expect(row).toBeVisible({ timeout: 15_000 });
  await row.getByRole('button', { name: 'Edit' }).click();
  await row
    .locator('.cl-field', { hasText: 'Code index summaries' })
    .locator('select')
    .selectOption(value);
  const answered = page.waitForResponse(
    (r) => r.url().includes(`/api/repositories/${slug}`) && r.request().method() === 'PATCH',
  );
  await row.getByRole('button', { name: 'Save' }).click();
  await answered;
  return row;
}

test('code index settings: read, edit one section, and no stale overwrite', async ({ page }) => {
  const slug = `codeindex-settings-${RUN}`;
  const token = await mintToken();

  await test.step('register a repository of the platform team', async () => {
    await createRepository(GUI, token, { slug, repoFullName: `acme/${slug}`, team: 'platform' });
  });

  await test.step('login via Dex as alice, open the page from the admin home', async () => {
    await login(page, 'alice@kairos.test', 'alice-password');
    await page.goto('/admin');
    const card = page.locator('.cl-panel', { hasText: 'Code index' });
    await expect(card).toBeVisible({ timeout: 15_000 });
    await card.getByRole('link', { name: 'Open' }).click();
    await page.waitForURL(/\/admin\/code-index/);
  });

  await test.step('the read view shows the defaults', async () => {
    await expect(summaryView(page)).toContainText('Provider:embedded', { timeout: 15_000 });
    await expect(page.locator('[data-testid="summary-secret-status"]')).toHaveText('Secret: none.');
    await expect(page.locator('[data-testid="code-index-settings-state"]')).toContainText('Not set');
  });

  await test.step('with no hosted provider, the opt-in of a repository is refused', async () => {
    const row = await setSummaries(page, slug, 'hosted');
    await expect(page.getByText('no hosted provider of the summaries')).toBeVisible({ timeout: 10_000 });
    await expect(row.locator('[data-testid="code-index-summaries-status"]')).toHaveText(
      'Summaries: the default of the organization, now the embedded model.',
    );
    await page.goto('/admin/code-index');
  });

  await test.step('an edit with no model: the confirmation lists it, and the server refuses it', async () => {
    await page.locator('[data-testid="code-index-summary-edit"]').click();
    await field(summary(page), 'Provider').selectOption('ollama-cloud');
    await field(summary(page), 'Base URL').fill('https://ollama.com/v1');
    await field(summary(page), 'Secret').fill(SECRET);
    await page.locator('[data-testid="code-index-summary-save"]').click();
    await expect(dialog(page)).toContainText('Provider: embedded → ollama-cloud');
    await expect(dialog(page)).toContainText('Secret: the new value replaces the stored one.');
    await confirmSave(page);
    await expect(page.getByText('needs summary.model')).toBeVisible({ timeout: 10_000 });
    // The form stays open, with what was typed.
    await expect(field(summary(page), 'Base URL')).toHaveValue('https://ollama.com/v1');
  });

  await test.step('the save with the model works; the read view and the secret status', async () => {
    await field(summary(page), 'Model').fill('gemma4:31b');
    await page.locator('[data-testid="code-index-summary-save"]').click();
    await confirmSave(page);
    await expect(page.getByText('Kairos changed the summaries.')).toBeVisible({ timeout: 10_000 });
    await expect(summaryView(page)).toContainText('Model:gemma4:31b');
    await expect(page.locator('[data-testid="summary-secret-status"]')).toContainText('Secret: set by');
    expect(await page.content()).not.toContain(SECRET);
  });

  await test.step('the repository opts in, and the page lists it', async () => {
    await setSummaries(page, slug, 'hosted');
    await page.goto('/admin/code-index');
    await expect(page.locator('[data-testid="code-index-hosted-repositories"]')).toContainText(slug, {
      timeout: 15_000,
    });
    await setSummaries(page, slug, 'embedded');
    await page.goto('/admin/code-index');
  });

  await test.step('a save of the builder leaves the summaries; a bad number is refused', async () => {
    await page.locator('[data-testid="code-index-builder-edit"]').click();
    const requests = page.locator('.cl-field', { hasText: 'Requests at a time' });
    await requests.locator('input').fill('eight');
    await expect(requests).toContainText('"eight" is not a number from 1 to 32.');
    await expect(page.locator('[data-testid="code-index-settings-save"]')).toBeDisabled();
    await requests.locator('input').fill('6');
    await page.locator('[data-testid="code-index-settings-save"]').click();
    await expect(dialog(page)).toContainText('Requests at a time:');
    await confirmSave(page);
    await expect(builderView(page)).toContainText('Requests at a time:6', { timeout: 10_000 });
    await expect(summaryView(page)).toContainText('Model:gemma4:31b');
    await expect(page.locator('[data-testid="summary-secret-status"]')).toContainText('Secret: set by');
  });

  await test.step('a stale save is refused, and the page shows the current values', async () => {
    await page.locator('[data-testid="code-index-builder-edit"]').click();
    await page.locator('.cl-field', { hasText: 'Requests at a time' }).locator('input').fill('9');
    // Another change over the API, after the page read the settings.
    const read = await (
      await page.request.get('/api/org/code-index-settings', {
        headers: { authorization: `Bearer ${token}` },
      })
    ).json();
    const put = await page.request.put('/api/org/code-index-settings', {
      headers: { authorization: `Bearer ${token}` },
      data: { concurrency: 3, version: read.version },
    });
    expect(put.ok(), await put.text()).toBeTruthy();
    await page.locator('[data-testid="code-index-settings-save"]').click();
    const answer = await confirmSave(page);
    expect(answer.status()).toBe(409);
    await expect(page.getByText('changed after you read them')).toBeVisible({ timeout: 10_000 });
    await expect(builderView(page)).toContainText('Requests at a time:3', { timeout: 10_000 });
  });

  await test.step('the default summarizer: hosted, then embedded again', async () => {
    const pick = page
      .locator('select[data-testid="code-index-default-summaries"], [data-testid="code-index-default-summaries"] select')
      .first();
    await page.locator('[data-testid="code-index-builder-edit"]').click();
    await pick.selectOption('hosted');
    await page.locator('[data-testid="code-index-settings-save"]').click();
    await confirmSave(page);
    await expect(builderView(page)).toContainText('Default summarizer of the repositories:hosted', {
      timeout: 10_000,
    });
    await page.locator('[data-testid="code-index-builder-edit"]').click();
    await pick.selectOption('embedded');
    await page.locator('[data-testid="code-index-settings-save"]').click();
    await confirmSave(page);
    await expect(builderView(page)).toContainText('Default summarizer of the repositories:embedded', {
      timeout: 10_000,
    });
  });

  await test.step('the vectors: a remote provider with a secret, then embedded again', async () => {
    // The page only stores the provider. The builder calls it, and the e2e
    // server runs no builder, so the URL names no server.
    const vectors = page.locator('[data-testid="code-index-vector-settings"]');
    const view = page.locator('[data-testid="code-index-vector-view"]');
    const secret = page.locator('[data-testid="vector-secret-status"]');
    await expect(view).toContainText('Provider:embedded');
    await expect(secret).toHaveText('Secret: none.');
    await page.locator('[data-testid="code-index-vector-edit"]').click();
    await field(vectors, 'Provider').selectOption('remote');
    await field(vectors, 'Base URL').fill('https://vectors.example.com/v1');
    await field(vectors, 'Model').fill('nomic-embed-text');
    await field(vectors, 'Secret').fill(`vector-e2e-${RUN}`);
    await page.locator('[data-testid="code-index-vector-save"]').click();
    await expect(dialog(page)).toContainText('Provider: embedded → remote');
    await confirmSave(page);
    await expect(view).toContainText('Provider:remote', { timeout: 10_000 });
    await expect(view).toContainText('Model:nomic-embed-text');
    await expect(secret).toContainText('Secret: set by');
    expect(await page.content()).not.toContain(`vector-e2e-${RUN}`);

    await view.getByRole('button', { name: 'Remove the secret' }).click();
    await confirmSave(page, 'Remove');
    await expect(secret).toHaveText('Secret: none.', { timeout: 10_000 });
    await page.locator('[data-testid="code-index-vector-edit"]').click();
    await field(vectors, 'Provider').selectOption('embedded');
    await page.locator('[data-testid="code-index-vector-save"]').click();
    await confirmSave(page);
    await expect(view).toContainText('Provider:embedded', { timeout: 10_000 });
  });

  await test.step('back to embedded: the stored secret of the old provider goes', async () => {
    // A hosted provider keeps its secret: no remove button while it is set.
    await expect(summaryView(page).getByRole('button', { name: 'Remove the secret' })).toHaveCount(0);
    await page.locator('[data-testid="code-index-summary-edit"]').click();
    await field(summary(page), 'Provider').selectOption('embedded');
    await page.locator('[data-testid="code-index-summary-save"]').click();
    await expect(dialog(page)).toContainText(
      'Secret: Kairos removes the stored secret: it is the secret of ollama-cloud.',
    );
    await confirmSave(page);
    await expect(summaryView(page)).toContainText('Provider:embedded', { timeout: 10_000 });
    await expect(page.locator('[data-testid="summary-secret-status"]')).toHaveText('Secret: none.');
  });
});
