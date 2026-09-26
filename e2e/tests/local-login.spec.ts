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
  await expect(page.getByText('incorrect email or password')).toBeVisible();
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

test('local login: a session does not survive a reload', async ({ page }) => {
  // Deliberate, and worth a test so nobody "fixes" it by writing the bearer into
  // storage. Per KAIROS-A-0015 the SPA holds its token in memory, and a session bearer
  // IS the credential — unlike the refresh token KAIROS-T-0071 stashes, which can only
  // be redeemed at the issuer. The cost is this reload; the alternative is a working
  // API credential sitting in browser storage.
  await page.goto('/login');
  await page.getByLabel('Email').fill(EMAIL);
  await page.getByLabel('Password').fill(PASSWORD);
  await page.getByLabel('Password').press('Enter');
  await expect(page.getByRole('link', { name: 'Boards', exact: true })).toBeVisible();

  await page.reload();
  // Back to a sign-in prompt rather than into the app. On this deployment the guard
  // redirects to the issuer, because it HAS one — either way the session is gone.
  await expect(page.getByRole('link', { name: 'Boards', exact: true })).toBeHidden();
});
