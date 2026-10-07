// KAIROS-T-0339 — the page Admin, Code index: where the summaries and the
// vectors of the code index of the organization are made.
//
//   1. REAL PKCE login (alice, an organization admin); the admin home has
//      the card "Code index"
//   2. the page shows the defaults: embedded for both, no secret
//   3. a save of ollama-cloud with no model is refused, and the notice
//      shows the text of the server
//   4. a save of ollama-cloud with the URL, the model and a secret works;
//      the status says who set the secret; the secret field is empty again
//      and the page never shows the secret
//   5. a save with the secret field empty keeps the secret; "Remove the
//      secret" with the provider back on embedded removes it
//
// The spec writes the settings of the demo tenant and puts them back to
// the embedded defaults at its end, so the other specs see the defaults.

import { test, expect, type Page } from '@playwright/test';

const SECRET = `ollama-e2e-${Date.now().toString(36)}`;

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

test('code index settings: the admin sets a hosted provider and its secret', async ({ page }) => {
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

  await test.step('a hosted provider with no model is refused with the text of the server', async () => {
    await field(page, 'Provider').selectOption('ollama-cloud');
    await field(page, 'Base URL').fill('https://ollama.com/v1');
    await field(page, 'Secret').fill(SECRET);
    await page.locator('[data-testid="code-index-settings-save"]').click();
    await expect(page.getByText('needs summary.model')).toBeVisible({ timeout: 10_000 });
  });

  await test.step('the save with the model works, and the secret is a status', async () => {
    await field(page, 'Model').fill('gemma4:31b');
    await page.locator('[data-testid="code-index-settings-save"]').click();
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

  await test.step('an empty secret field keeps the secret; the remove button and embedded clear it', async () => {
    await field(page, 'Model').fill('gemma4:31b');
    await page.locator('[data-testid="code-index-settings-save"]').click();
    await expect(page.getByText('Kairos set the providers of the code index.')).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.locator('[data-testid="summary-secret-status"]')).toContainText(
      'Secret: set by',
    );
    await field(page, 'Provider').selectOption('embedded');
    await summary(page).getByRole('button', { name: 'Remove the secret' }).click();
    await expect(page.locator('[data-testid="summary-secret-status"]')).toHaveText(
      'Secret: none.',
      { timeout: 10_000 },
    );
    await expect(field(page, 'Provider')).toHaveValue('embedded');
  });
});
