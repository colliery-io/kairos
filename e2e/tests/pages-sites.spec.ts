// KAIROS-T-0404 — the rendered docs sites in a browser:
//
// - the CSP holds: a script added to the page inline does not run;
// - a mermaid fence draws as a diagram;
// - dark mode follows prefers-color-scheme.

import { test, expect, type Page } from '@playwright/test';

const EMAIL = process.env.E2E_LOCAL_EMAIL ?? 'alice@kairos.test';
const PASSWORD = process.env.E2E_LOCAL_PASSWORD ?? 'alice-local-password';

/** Log in (the cookie goes to the browser context) and give the bearer. */
async function logIn(page: Page): Promise<string> {
  const response = await page.request.post('/api/login', {
    data: { email: EMAIL, password: PASSWORD },
  });
  expect(response.ok()).toBeTruthy();
  return (await response.json()).token;
}

/** A document of the platform team with a mermaid fence. */
async function mermaidDocument(page: Page, bearer: string): Promise<string> {
  // KAIROS-T-0792: a create takes no content; the text is an update.
  const response = await page.request.post('/api/documents', {
    headers: { authorization: `Bearer ${bearer}` },
    data: { board: 'platform-delivery', title: 'A diagram' },
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  const made = await response.json();
  const filled = await page.request.patch(`/api/documents/${made.short_code}`, {
    headers: { authorization: `Bearer ${bearer}` },
    data: {
      content: '```mermaid\ngraph TD\n  A[Build] --> B[Publish]\n```\n',
      version: made.version,
    },
  });
  expect(filled.ok(), await filled.text()).toBeTruthy();
  return made.short_code;
}

test('pages: a diagram draws, and an inline script does not run', async ({ page }) => {
  const bearer = await logIn(page);
  const code = await mermaidDocument(page, bearer);
  const violations: string[] = [];
  page.on('console', (m) => {
    if (m.text().includes('Content Security Policy')) violations.push(m.text());
  });

  await page.goto(`/pages/demo/platform/docs/${code}/`);
  await expect(page.getByRole('heading', { level: 1, name: 'A diagram' })).toBeVisible();
  await expect(page.locator('.kairos-mermaid-diagram svg')).toBeVisible();
  expect(violations, 'the page itself breaks no rule of its CSP').toEqual([]);

  // A script added inline (as an injection would be) does not run.
  const ran = await page.evaluate(async () => {
    (window as unknown as { injected?: boolean }).injected = false;
    const script = document.createElement('script');
    script.textContent = 'window.injected = true;';
    document.body.appendChild(script);
    await new Promise((resolve) => setTimeout(resolve, 100));
    return (window as unknown as { injected?: boolean }).injected;
  });
  expect(ran).toBe(false);
});

test('pages: dark mode follows the system', async ({ page }) => {
  await logIn(page);
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/pages/demo/platform/');
  const dark = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  await page.emulateMedia({ colorScheme: 'light' });
  const light = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(dark).toBe('rgb(14, 17, 22)');
  expect(light).toBe('rgb(243, 245, 248)');
});
