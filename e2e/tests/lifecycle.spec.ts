// KAIROS-T-0078 — document lifecycle + entity-scoped metadata, read-only
// against the seed plus one lifecycle write:
//
//   1. REAL PKCE login (alice)
//   2. reach the seeded PRD (DEMO-D-0001) in-app: initiatives board →
//      Portal sign-up flow → Supporting material link
//   3. the document page renders the LIFECYCLE badge (draft — new docs
//      are born draft; the legacy 'Document status' metadata is gone) and
//      the Lifecycle panel, styled apart from the metadata panel
//   4. set lifecycle to published via the panel → notice + badge update
//   5. the document's add-a-field picker DOES offer Document Type (in
//      scope for documents) — the scoping cuts both ways
//
// Conventions per smoke.spec.ts: in-app navigation only (memory token).

import { test, expect } from '@playwright/test';

test('document lifecycle: badge, free transitions, scoped picker', async ({
  page,
}) => {
  // 1. Login -----------------------------------------------------------------
  await test.step('login via Dex as alice', async () => {
    await page.goto('/');
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'alice@kairos.test');
    await page.fill('#password', 'alice-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
      timeout: 30_000,
    });
  });

  // 2. Navigate to the seeded PRD --------------------------------------------
  await test.step('reach DEMO-D-0001 via the initiative detail', async () => {
    await page.locator('.kairos-board-tile', { hasText: 'Initiatives' }).click();
    await page.waitForURL(/\/boards\/initiatives/);
    await page
      .locator('article.kairos-card', { hasText: 'Portal sign-up flow' })
      .locator('a.kairos-card__code')
      .click();
    await page.waitForURL(/\/items\//);
    const relationships = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Relationships' }),
    });
    await relationships
      .getByRole('link', { name: /DEMO-D-0001/ })
      .click();
    await page.waitForURL(/\/items\/DEMO-D-0001/);
  });

  // 3. Lifecycle badge + panel, distinct from metadata -----------------------
  await test.step('lifecycle renders as its own badge and panel', async () => {
    await expect(page.locator('.kairos-lifecycle-badge')).toContainText(
      'lifecycle: draft',
    );
    const lifecycle = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Lifecycle' }),
    });
    await expect(lifecycle).toBeVisible();
    await expect(lifecycle.getByText('editorial state')).toBeVisible();
    // Never in the shared metadata panel: no lifecycle field there.
    const metadata = page.locator('.kairos-metadata');
    await expect(
      metadata.locator('label', { hasText: 'Document status' }),
    ).toHaveCount(0);
  });

  // 4. Publish via the panel --------------------------------------------------
  await test.step('set lifecycle to published', async () => {
    const lifecycle = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Lifecycle' }),
    });
    await lifecycle.locator('select').selectOption({ label: 'published' });
    await lifecycle.getByRole('button', { name: 'Set', exact: true }).click();
    await expect(page.getByText('Lifecycle set to published.')).toBeVisible();
    await expect(page.locator('.kairos-lifecycle-badge')).toContainText(
      'lifecycle: published',
      { timeout: 15_000 },
    );
  });

  // 5. The scoping cuts both ways: the DOCUMENT carries Document Type ---------
  await test.step('document metadata carries the document-scoped catalog', async () => {
    const metadata = page.locator('.kairos-metadata');
    // The template stamped document_type, so it renders as an editor row
    // (in scope, in use) …
    await expect(
      metadata.locator('label', { hasText: 'Document Type' }),
    ).toHaveCount(1);
    // … and the in-scope, unstamped remainder sits in the picker.
    await expect(
      metadata.locator('option', { hasText: 'Priority' }),
    ).toHaveCount(1);
  });
});
