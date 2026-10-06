// KAIROS-T-0205 — signing in with a Kairos password rather than through an IdP.
//
// The first spec in this suite that never touches Dex. Every other one drives the
// issuer's login form; this one drives Kairos's own, which is the whole point of
// KAIROS-I-0018: a deployment that authenticates people without an identity provider.
//
// The harness runs the GUI server with KAIROS_LOCAL_AUTH=true AND the Dex issuer, so
// the login page is rendering the "both paths at once" state — the one easiest to get
// wrong, and the one every other spec walks straight past on its way to Dex.
//
// Selectors lean on roles and labels, the way a password manager and a screen reader
// see the form, rather than on the class names (see docs/gui-conventions.md).

import { test, expect } from '@playwright/test';

const EMAIL = process.env.E2E_LOCAL_EMAIL ?? 'alice@kairos.test';
const PASSWORD = process.env.E2E_LOCAL_PASSWORD ?? 'alice-local-password';

test('local login: the form signs in with a password and reaches a board', async ({
  page,
}) => {
  // Straight to /login. An unauthenticated visit to `/` still redirects to the issuer
  // on a deployment that HAS one (A-0015), which is the behaviour the other specs rely
  // on and which this must not change.
  await page.goto('/login');

  // --- both paths are offered ---------------------------------------------
  // The provider button first (an organization with an issuer wants its people to use
  // it), then a separator, then the form.
  await expect(
    page.getByText("Sign in with your organization's identity provider."),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Sign in', exact: true })).toBeVisible();
  await expect(page.getByText('or', { exact: true })).toBeVisible();
  // Distinct names, deliberately: two buttons called "Sign in" on one page is
  // ambiguous for a person choosing between them, and for anything addressing the
  // page by role and name.
  await expect(page.getByRole('button', { name: 'Log in' })).toBeVisible();

  const email = page.getByLabel('Email');
  const password = page.getByLabel('Password');
  await expect(email).toBeVisible();
  await expect(password).toBeVisible();

  // --- the attributes a password manager needs ----------------------------
  // Asserted rather than assumed: they are invisible, so nothing else would notice
  // their absence, and without them a password manager neither fills nor offers to
  // save — which is how people end up choosing weak passwords.
  await expect(email).toHaveAttribute('type', 'email');
  await expect(email).toHaveAttribute('autocomplete', 'email');
  await expect(password).toHaveAttribute('type', 'password');
  await expect(password).toHaveAttribute('autocomplete', 'current-password');

  // --- a wrong password is refused, uninformatively -----------------------
  await email.fill(EMAIL);
  await password.fill('definitely-not-the-password');
  await password.press('Enter');

  // The server's own message, rendered verbatim (KAIROS-T-0203): one sentence for a
  // wrong password, an unknown email and an account that has no password. The GUI must
  // not improve on it — "no account with that email" would rebuild the
  // account-enumeration oracle the endpoint was careful to avoid.
  await expect(page.getByText('The email or the password is not correct.')).toBeVisible();
  await expect(page).toHaveURL(/\/login$/);
  // Still on the form, and still able to try again.
  await expect(password).toBeVisible();

  // --- the real thing, submitted with Enter -------------------------------
  // Enter rather than a click, because that is what a real form buys and a `<div>` with
  // a click handler would not.
  await password.fill(PASSWORD);
  await password.press('Enter');

  // Landed inside the app, signed in as the right person.
  await expect(page.getByRole('link', { name: 'Boards', exact: true })).toBeVisible();
  // The header shows the DISPLAY NAME, not the email — the seeded admin is "alice".
  await expect(page.locator('.cl-appshell__header').getByText('alice')).toBeVisible();

  // --- and the session works against the API -----------------------------
  // Reaching a board proves the session bearer is being sent on /api calls, which is
  // the assertion that matters: the token took the same in-memory path an OIDC access
  // token takes.
  await page.getByRole('link', { name: 'Boards', exact: true }).click();
  // `.kairos-board-grid` is per-board, not one grid for the page, so `.first()` —
  // asserting on the bare class matched five elements.
  await expect(page.locator('.kairos-board-grid').first()).toBeVisible();
  await page.getByRole('link', { name: /Platform Delivery/ }).first().click();
  await expect(page.locator('section.kairos-board__column').first()).toBeVisible();

  // --- logout lands back on the form --------------------------------------
  await page.locator('.cl-appshell__header').getByRole('button', { name: 'Log out' }).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByLabel('Email')).toBeVisible();
});

test('local login: the session survives a reload and a new tab, and logout ends it', async ({
  page,
  context,
}) => {
  // KAIROS-T-0327 (the 2026-10-06 amendment of KAIROS-A-0015). A password session
  // is also an HttpOnly cookie, so a reload or a new tab keeps it. No script of the
  // page can read the cookie, and no token is written into browser storage.
  await page.goto('/login');
  await page.getByLabel('Email').fill(EMAIL);
  await page.getByLabel('Password').fill(PASSWORD);
  await page.getByLabel('Password').press('Enter');
  await expect(page.getByRole('link', { name: 'Boards', exact: true })).toBeVisible();

  // --- the cookie: HttpOnly, Strict, and invisible to the page -------------
  const cookie = (await context.cookies()).find((c) => c.name === 'kairos_session');
  expect(cookie, 'the login sets the session cookie').toBeTruthy();
  expect(cookie!.httpOnly).toBe(true);
  expect(cookie!.secure).toBe(true);
  expect(cookie!.sameSite).toBe('Strict');
  expect(cookie!.path).toBe('/');
  expect(await page.evaluate(() => document.cookie)).not.toContain('kairos_session');
  const stored = await page.evaluate(() =>
    JSON.stringify({ ...window.localStorage, ...window.sessionStorage }),
  );
  expect(stored, 'no session token in browser storage').not.toContain('kairos_ss_');

  // --- a reload of a board keeps the session -------------------------------
  await page.getByRole('link', { name: 'Boards', exact: true }).click();
  await page.getByRole('link', { name: /Platform Delivery/ }).first().click();
  await expect(page.locator('section.kairos-board__column').first()).toBeVisible();
  await page.reload();
  await expect(page.locator('section.kairos-board__column').first()).toBeVisible({
    timeout: 15_000,
  });
  await expect(page).toHaveURL(/\/boards\/platform-delivery/);
  await expect(page.locator('.cl-appshell__header').getByText('alice')).toBeVisible();

  // --- a new tab is signed in too ------------------------------------------
  const tab = await context.newPage();
  await tab.goto('/boards');
  await expect(tab.getByRole('link', { name: 'Boards', exact: true })).toBeVisible({
    timeout: 15_000,
  });
  await expect(tab.locator('.kairos-board-grid').first()).toBeVisible();
  await tab.close();

  // --- logout ends the session on the server and clears the cookie ---------
  const loggedOut = page.waitForResponse(
    (r) => r.url().endsWith('/api/logout') && r.request().method() === 'POST',
  );
  await page.locator('.cl-appshell__header').getByRole('button', { name: 'Log out' }).click();
  expect((await loggedOut).status()).toBe(204);
  await expect(page).toHaveURL(/\/login$/);
  expect((await context.cookies()).find((c) => c.name === 'kairos_session')).toBeUndefined();
  await page.reload();
  await expect(page.getByRole('link', { name: 'Boards', exact: true })).toBeHidden();
});
