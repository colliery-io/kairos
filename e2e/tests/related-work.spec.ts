// KAIROS-T-0195 — the human side of retrieval, in the browser.
//
// Retrieval shipped over MCP and REST only (KAIROS-I-0017): a person could act on
// proposals an agent made, and had no way to ASK. "Didn't we try this?" is at
// least as much a human question — the one that initiative is named after.
//
//   1. REAL PKCE login (alice)
//   2. an item detail page carries a "Possibly related" panel, and it has NOT
//      searched on load — retrieval costs a vector search per ask, and most
//      visits to an item are not someone wondering what it duplicates
//   3. clicking asks, and the answer arrives framed as proposals
//   4. the framing itself is asserted, not just the presence of results
//
// Read-only: asking changes nothing, so this adds no cleanup and cannot affect
// another spec's counts.

import { test, expect, type Page } from '@playwright/test';

const panel = (page: Page, title: string) =>
  page.locator('.cl-panel', {
    has: page.locator('.cl-panel__title', { hasText: title }),
  });

test('related work: an item page asks what else touches this, and answers as proposals', async ({
  page,
}) => {
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

  await test.step('the panel is present and has not searched yet', async () => {
    await page.goto('/items/DEMO-T-0002');
    await page.waitForURL(/\/items\/DEMO-T-0002$/);

    const related = panel(page, 'Possibly related');
    await expect(related).toBeVisible();

    // The ask is a button, deliberately: it makes this a question the person
    // asked rather than a list the page asserts, and it costs nothing on the
    // visits where nobody wondered.
    const ask = related.getByRole('button', { name: 'Find related work' });
    await expect(ask).toBeVisible();
    // Nothing has been claimed about anything yet.
    await expect(related.getByText('suggestions, not findings')).toHaveCount(0);
  });

  await test.step('asking returns proposals, framed as proposals', async () => {
    const related = panel(page, 'Possibly related');
    await related.getByRole('button', { name: 'Find related work' }).click();

    // Either an answer or an honest empty — both are acceptable outcomes of a
    // search, and asserting "results appeared" would make this test depend on
    // the seed's similarity scores, which are not a contract.
    await expect
      .poll(
        async () => {
          const body = (await related.textContent()) ?? '';
          return /suggestions, not findings|coming up short|not enabled/.test(body);
        },
        { timeout: 30_000 },
      )
      .toBe(true);

    const body = (await related.textContent()) ?? '';

    // Record WHICH outcome this run got, because all three are legitimate and a
    // test that accepts three outcomes can silently stop asserting the
    // interesting one. The annotation shows in the Playwright report, so a human
    // reading CI can see whether the proposal-rendering path was exercised or
    // whether this run only proved the panel asks politely.
    const outcome = /suggestions, not findings/.test(body)
      ? 'proposals rendered'
      : /coming up short/.test(body)
        ? 'empty answer (embeddings may not have caught up with the seed yet)'
        : 'retrieval not enabled on this deployment';
    test.info().annotations.push({ type: 'related-work outcome', description: outcome });
    // Also on stdout, because the `list` reporter does not print annotations and
    // this is the line that says whether the run proved anything interesting.
    console.log(`[related-work] outcome: ${outcome}`);

    // The wording is the contract, not the ranking. KAIROS-A-0021 rule 5 exists
    // because related pairs average 0.80 similarity and unrelated pairs reach
    // 0.82 — about half the strongest matches are wrong, so a confident-looking
    // list would undo in pixels what the API is careful to say in words.
    if (/suggestions, not findings/.test(body)) {
      await expect(related.getByText('suggestions, not findings')).toBeVisible();
      // Each proposal carries a claim type and a reason you can disagree with,
      // and at least one linked short code to go and judge.
      await expect(related.locator('.cl-pill').first()).toBeVisible();
      await expect(related.locator('a[href^="/items/"]').first()).toBeVisible();
      // And NO score: a fused rank is comparable within one response and nowhere
      // else, so a number on screen would read as a confidence whatever it said.
      expect(body).not.toMatch(/\b0\.\d\d\b/);
    } else if (/coming up short/.test(body)) {
      // The empty answer must not claim more than it knows.
      await expect(related.getByText('coming up short')).toBeVisible();
    }
  });
});
