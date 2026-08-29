// KAIROS-T-0065/T-0078 — the metadata panel's ADD-A-FIELD flow, end to
// end. Regression spec for two bugs found in T-0078 UAT review: the
// picker's Effect re-entered itself and panicked the WASM runtime, and
// (after that fix) a draft signal created inside the change handler
// produced a dead editor whose writes never enabled Save. The panel now
// pre-creates every row at render time; the picker only reveals them.
//
//   1. REAL PKCE login (alice)
//   2. DEMO-T-0003 (no metadata): pick Complexity in the add-a-field
//      dropdown → the editor row appears, the picker resets to its
//      placeholder
//   3. choose a value → Save enables → save → the refetched panel shows
//      Complexity as a stamped editor with the value
//   4. stamped editing still works: change the now-stamped value and
//      Save re-enables
//
// Mutates DEMO-T-0003's metadata only (fresh per seed; retry-safe since
// re-adding an already-stamped field is impossible — it renders as an
// editor on retry, so the spec tolerates both states via the early
// stamped check).

import { test, expect } from '@playwright/test';

test('metadata: add a field via the picker, save, edit stamped', async ({
  page,
}) => {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });

  await page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }).click();
  await page.waitForURL(/\/boards\/platform-delivery/);
  await page
    .locator('article.kairos-card', { hasText: 'DEMO-T-0003' })
    .locator('a.kairos-card__code')
    .click();
  await page.waitForURL(/\/items\/DEMO-T-0003/);

  const metadata = page.locator('.kairos-metadata');
  await expect(metadata).toBeVisible();
  const complexityField = () =>
    metadata.locator('.kairos-metadata__field', {
      has: page.locator('label', { hasText: 'Complexity' }),
    });
  const save = metadata.getByRole('button', { name: 'Save metadata' });

  // 2. Add via the picker (skip if a retry already stamped it).
  await test.step('picker reveals the editor row', async () => {
    if (await complexityField().isVisible()) {
      return; // retry: already stamped — fall through to stamped editing.
    }
    const picker = metadata.locator('select').last();
    await picker.selectOption({ label: 'Complexity' });
    await expect(complexityField()).toBeVisible();
    // The picker reset to its placeholder, ready for the next add.
    await expect(metadata.locator('select').last()).toHaveValue(
      '(add a field…)',
    );

    // 3. Value → Save → persisted as a stamped editor.
    await complexityField().locator('select').selectOption('m');
    await expect(save).toBeEnabled();
    await save.click();
    await expect(complexityField().locator('select')).toHaveValue('m', {
      timeout: 10_000,
    });
    await expect(save).toBeDisabled(); // clean after refetch
  });

  // 4. Stamped editing still arms Save.
  await test.step('stamped edit re-enables save', async () => {
    await complexityField().locator('select').selectOption('l');
    await expect(save).toBeEnabled();
    await save.click();
    await expect(complexityField().locator('select')).toHaveValue('l', {
      timeout: 10_000,
    });
  });
});
