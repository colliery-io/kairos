// KAIROS-T-0339 and KAIROS-T-0340 — the page Admin, Code index (where the
// summaries and the vectors of the code index of the organization are
// made), and the opt-in of a repository on the page Admin, Repositories.
//
//   1. REAL PKCE login (alice, an organization admin); a repository that
//      the spec registers over the API; the admin home has the card "Code
//      index"
//   2. the page shows the defaults: embedded for both, no secret
//   3. with no hosted provider, the opt-in of the repository is refused on
//      the admin form, and the refusal names the page that sets one
//   4. a save of ollama-cloud with no model is refused, and the notice
//      shows the text of the server
//   5. a save of ollama-cloud with the URL, the model and a secret works;
//      the status says who set the secret; the secret field is empty again
//      and the page never shows the secret
//   6. the repository opts in on the admin form; its status says so
//   7. a save with the secret field empty keeps the secret; the repository
//      goes back to embedded; "Remove the secret" with the provider back on
//      embedded removes it
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
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

const summary = (page: Page) => page.locator('[data-testid="code-index-summary-settings"]');
const field = (page: Page, label: string) =>
  summary(page).locator('.cl-field', { hasText: label }).locator('input, select').first();

/// Click a save of the settings and wait for the answer of the server. A
/// save that works makes the page read the settings again and build the
/// form again, with `updated_at` on its state line: `changes` waits for
/// that line to change, so the next step works on the new form.
async function saveSettings(
  page: Page,
  button = page.locator('[data-testid="code-index-settings-save"]'),
  changes = true,
) {
  const state = page.locator('[data-testid="code-index-settings-state"]');
  const before = await state.textContent();
  const answered = page.waitForResponse(
    (r) => r.url().includes('/api/org/code-index-settings') && r.request().method() === 'PUT',
  );
  await button.click();
  await answered;
  if (changes) {
    await expect(state).not.toHaveText(before ?? '', { timeout: 10_000 });
  }
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

test('code index settings: the admin sets a hosted provider, and a repository opts in', async ({
  page,
}) => {
  const slug = `codeindex-settings-${RUN}`;

  await test.step('register a repository of the platform team', async () => {
    const token = await mintToken();
    await createRepository(GUI, token, {
      slug,
      repoFullName: `acme/${slug}`,
      team: 'platform',
    });
  });

  await test.step('login via Dex as alice, open the page from the admin home', async () => {
    await login(page, 'alice@kairos.test', 'alice-password');
    await page.goto('/admin');
    const card = page.locator('.cl-panel', { hasText: 'Code index' });
    await expect(card).toBeVisible({ timeout: 15_000 });
    await card.getByRole('link', { name: 'Open' }).click();
    await page.waitForURL(/\/admin\/code-index/);
  });

  await test.step('the defaults', async () => {
    await expect(field(page, 'Provider')).toHaveValue('embedded', { timeout: 15_000 });
    await expect(page.locator('[data-testid="summary-secret-status"]')).toHaveText('Secret: none.');
    await expect(page.locator('[data-testid="code-index-settings-state"]')).toContainText(
      'Not set',
    );
  });

  await test.step('with no hosted provider, the opt-in of a repository is refused', async () => {
    const row = await setSummaries(page, slug, 'hosted');
    await expect(page.getByText('no hosted provider of the summaries')).toBeVisible({
      timeout: 10_000,
    });
    // KAIROS-T-0358: a new repository follows the organization, and the
    // organization has the embedded model.
    await expect(row.locator('[data-testid="code-index-summaries-status"]')).toHaveText(
      'Summaries: the default of the organization, now the embedded model.',
    );
    await page.goto('/admin/code-index');
  });

  await test.step('a hosted provider with no model is refused with the text of the server', async () => {
    await expect(field(page, 'Provider')).toHaveValue('embedded', { timeout: 15_000 });
    await field(page, 'Provider').selectOption('ollama-cloud');
    await field(page, 'Base URL').fill('https://ollama.com/v1');
    await field(page, 'Secret').fill(SECRET);
    await saveSettings(page, undefined, false);
    await expect(page.getByText('needs summary.model')).toBeVisible({ timeout: 10_000 });
  });

  await test.step('the save with the model works, and the secret is a status', async () => {
    await field(page, 'Model').fill('gemma4:31b');
    await saveSettings(page);
    await expect(page.getByText('Kairos set the providers of the code index.')).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.locator('[data-testid="summary-secret-status"]')).toContainText(
      'Secret: set by',
      { timeout: 10_000 },
    );
    await expect(field(page, 'Secret')).toHaveValue('');
    const html = await page.content();
    expect(html).not.toContain(SECRET);
  });

  await test.step('the repository opts in on the admin form', async () => {
    const row = await setSummaries(page, slug, 'hosted');
    await expect(row.locator('[data-testid="code-index-summaries-status"]')).toContainText(
      'Summaries: the hosted provider of the organization',
      { timeout: 10_000 },
    );
    await setSummaries(page, slug, 'embedded');
    await page.goto('/admin/code-index');
  });

  await test.step('an empty secret field keeps the secret; the remove button and embedded clear it', async () => {
    await expect(field(page, 'Provider')).toHaveValue('ollama-cloud', { timeout: 15_000 });
    await saveSettings(page);
    await expect(page.locator('[data-testid="summary-secret-status"]')).toContainText(
      'Secret: set by',
    );
    await field(page, 'Provider').selectOption('embedded');
    await saveSettings(page, summary(page).getByRole('button', { name: 'Remove the secret' }));
    await expect(page.locator('[data-testid="summary-secret-status"]')).toHaveText(
      'Secret: none.',
      { timeout: 10_000 },
    );
    await expect(field(page, 'Provider')).toHaveValue('embedded');
  });
});
