// KAIROS-T-0364 — a reload of the page keeps the person signed in after an OIDC
// login, also where the issuer gives no refresh token.
//
// Google answers a request for `offline_access` with `invalid_scope`, so the GUI
// does not ask for it there (/api/config names the scope), and Google then gives no
// refresh token. Before KAIROS-T-0364 the GUI kept its OIDC session in memory only,
// so a reload had nothing to restore it from and sent the person to sign in again.
// The GUI now opens a Kairos session from the OIDC login (POST /api/session): an
// HttpOnly cookie, as after a password login (KAIROS-T-0327, local-login.spec.ts).
//
// The dev Dex does give refresh tokens. The first test gives the page the scope that
// /api/config names for Google (no `offline_access`), so Dex gives none, as Google.

import { test, expect, type Page } from '@playwright/test';

async function signInWithDex(page: Page) {
  await page.goto('/boards');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await expect(page.getByRole('link', { name: 'Boards', exact: true })).toBeVisible({
    timeout: 30_000,
  });
}

async function reloadOnABoardAndFindAlice(page: Page) {
  await page.getByRole('link', { name: 'Boards', exact: true }).click();
  await page.getByRole('link', { name: /Platform Delivery/ }).first().click();
  await expect(page.locator('section.kairos-board__column').first()).toBeVisible();
  await page.reload();
  await expect(page.locator('section.kairos-board__column').first()).toBeVisible({
    timeout: 15_000,
  });
  await expect(page).toHaveURL(/\/boards\/platform-delivery/);
  await expect(page.locator('.cl-appshell__header').getByText('alice')).toBeVisible();
}

test('OIDC login with no refresh token: a reload keeps the person signed in', async ({
  page,
  context,
}) => {
  // The scope /api/config names for Google: no offline_access.
  await page.route('**/api/config', async (route) => {
    const response = await route.fetch();
    const config = await response.json();
    expect(config.scope, 'the dev Dex offers offline_access').toContain('offline_access');
    config.scope = 'openid profile email';
    await route.fulfill({ response, json: config });
  });

  const opened = page.waitForResponse(
    (r) => r.url().endsWith('/api/session') && r.request().method() === 'POST',
  );
  await signInWithDex(page);
  expect((await opened).status(), 'the OIDC login opens a Kairos session').toBe(200);

  // The session is the HttpOnly cookie; nothing in browser storage.
  const cookie = (await context.cookies()).find((c) => c.name === 'kairos_session');
  expect(cookie, 'the OIDC login sets the session cookie').toBeTruthy();
  expect(cookie!.httpOnly).toBe(true);
  const stored = await page.evaluate(() =>
    JSON.stringify({ ...window.localStorage, ...window.sessionStorage }),
  );
  expect(stored).not.toContain('kairos_refresh_token');
  expect(stored).not.toContain('kairos_ss_');

  await test.step('a reload keeps the person signed in', async () => {
    await reloadOnABoardAndFindAlice(page);
    // Not through the issuer: the page never left Kairos.
    await expect(page.locator('#login')).toHaveCount(0);
  });

  await test.step('logout ends the session', async () => {
    const loggedOut = page.waitForResponse(
      (r) => r.url().endsWith('/api/logout') && r.request().method() === 'POST',
    );
    await page.locator('.cl-appshell__header').getByRole('button', { name: 'Log out' }).click();
    expect((await loggedOut).status()).toBe(204);
    await expect(page).toHaveURL(/\/login$/);
    expect((await context.cookies()).find((c) => c.name === 'kairos_session')).toBeUndefined();
  });
});

test('OIDC login with a refresh token: a reload keeps the person signed in', async ({
  page,
}) => {
  await signInWithDex(page);
  await test.step('a reload keeps the person signed in', async () => {
    await reloadOnABoardAndFindAlice(page);
  });
});
