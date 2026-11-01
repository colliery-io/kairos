// KAIROS-T-0744 — the forge lines of a repository on Admin, Repositories,
// against a fake GitHub (e2e/fakes/github.ts; the e2e server has
// KAIROS_GITHUB_API_URL on the port of the fake):
//
//   1. a new GitHub repository: write-back off, the default branch of the
//      forge, the default forge filters, the breaker not tripped;
//   2. Edit: a write token that GitHub refuses; a pull request that names
//      a task makes a write-back, and the line says that it failed;
//   3. a new write token: the write-back runs again, and the fake gets the
//      comment and the status; the line says that it worked;
//   4. the forge filters and the default branch, set in the form;
//   5. a read token, then "Import open issues": the import reads the
//      issues of the fake, and the line says what it imported.
//
// The repository is new for each run. The spec deletes it and archives the
// tasks that it made.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import {
  createForgeConnection,
  createRepository,
  createTask,
  deliverGithubWebhook,
  githubPullRequest,
  readEachPage,
  tryArchiveTask,
} from '../helpers/api';
import { REFUSED_TOKEN, startFakeGithub, type FakeGithub } from '../fakes/github';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/' || url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

/** Reload the page until the line of the row has the text. */
async function waitForLine(page: Page, slug: string, testid: string, text: string | RegExp) {
  await expect(async () => {
    await page.reload();
    await expect(
      page.locator(`[data-repo="${slug}"]`).first().locator(`[data-testid="${testid}"]`),
    ).toContainText(text, { timeout: 3_000 });
  }).toPass({ timeout: 45_000 });
}

let fake: FakeGithub;
test.beforeAll(async () => {
  fake = await startFakeGithub();
});
test.afterAll(async () => {
  await fake?.close();
});

test('the forge lines of a repository: write-back, filters, branch and import', async ({ page }) => {
  test.setTimeout(180_000);
  const alice = await mintToken();
  const run = Date.now().toString(36);
  const fullName = `acme/fake-${run}`;
  const repo = await createRepository(GUI, alice, {
    slug: `fake-${run}`,
    repoFullName: fullName,
    team: 'platform',
  });
  const slug = repo.slug;
  const task = await createTask(GUI, alice, { title: `E2E write-back ${run}`, boardId: 'platform-delivery' });
  const code: string = task.short_code;
  fake.setIssues(fullName, [
    { number: 7, title: `Fake issue seven ${run}` },
    { number: 8, title: `Fake issue eight ${run}` },
  ]);
  const row = page.locator(`[data-repo="${slug}"]`).first();
  const line = (testid: string) => row.locator(`[data-testid="${testid}"]`);

  try {
    await test.step('a new repository: the default lines', async () => {
      await login(page);
      await page.goto('/admin/repositories');
      await expect(row).toBeVisible({ timeout: 30_000 });
      await expect(line('write-back-status')).toHaveText(
        'Write-back: off. The repository has no write token.',
      );
      await expect(line('default-branch-status')).toContainText('Indexed branch: main.');
      await expect(line('forge-filters-status')).toHaveText(
        'Forge filters: the default. Kairos picks up each branch, pull request and issue.',
      );
      await expect(line('code-index-breaker-status')).toHaveText('Code index breaker: not tripped.');
    });

    await test.step('a write token that GitHub refuses: the write-back fails', async () => {
      await row.getByRole('button', { name: 'Edit', exact: true }).click();
      const form = row.locator('[data-testid="write-token-form"]');
      await form.getByLabel('Write token').fill(REFUSED_TOKEN);
      await form.getByRole('button', { name: 'Set write token' }).click();
      await expect(line('write-back-status')).toContainText('Write-back: on.');
      await expect(line('write-back-status')).toContainText('No write yet.');

      const connection = await createForgeConnection(GUI, alice, slug);
      const status = await deliverGithubWebhook(
        GUI,
        connection,
        'pull_request',
        githubPullRequest({
          number: 41,
          code,
          repoFullName: fullName,
          state: 'open',
          updatedAt: new Date().toISOString(),
        }),
      );
      expect(status).toBeLessThan(300);
      await waitForLine(page, slug, 'write-back-status', /The last write at .* failed/);
      expect(
        fake.requests.some((r) => r.token === REFUSED_TOKEN),
        `the requests of the fake: ${JSON.stringify(fake.requests.map((r) => [r.method, r.path, r.token]))}`,
      ).toBe(true);
    });

    await test.step('a new write token: the write-back works', async () => {
      await row.getByRole('button', { name: 'Edit', exact: true }).click();
      const form = row.locator('[data-testid="write-token-form"]');
      await form.getByLabel('Replace the write token').fill(`good-token-${run}`);
      await form.getByRole('button', { name: 'Replace write token' }).click();
      await waitForLine(page, slug, 'write-back-status', /The last write at .* worked\./);
      const good = fake.requests.filter((r) => r.token === `good-token-${run}`);
      expect(good.some((r) => r.method === 'POST' && r.path === `/repos/${fullName}/issues/41/comments`)).toBe(
        true,
      );
      expect(good.some((r) => r.method === 'POST' && r.path.startsWith(`/repos/${fullName}/statuses/`))).toBe(
        true,
      );
    });

    await test.step('the forge filters and the default branch, in the form', async () => {
      await row.getByRole('button', { name: 'Edit', exact: true }).click();
      const filters = row.locator('[data-testid="forge-filters-form"]');
      await filters.getByLabel('pr_author: exclude').fill('^dependabot\\[bot\\]$');
      await filters.getByRole('button', { name: 'Update forge filters' }).click();
      await expect(page.getByText(`Kairos changed the forge filters of "${slug}".`)).toBeVisible();
      await expect(line('forge-filters-status')).toContainText('pr_author exclude ^dependabot\\[bot\\]$');

      if (!(await row.locator('[data-testid="forge-filters-form"]').isVisible())) {
        await row.getByRole('button', { name: 'Edit', exact: true }).click();
      }
      await row.locator('.cl-field', { hasText: 'Default branch from' }).locator('select').selectOption('hand');
      await row.getByLabel('Default branch', { exact: true }).fill('release');
      await row.getByRole('button', { name: 'Save', exact: true }).click();
      await expect(line('default-branch-status')).toContainText('Indexed branch: release, set by hand.');
    });

    await test.step('a read token, then the import of the open issues', async () => {
      if (!(await row.locator('[data-testid="credential-form"]').isVisible())) {
        await row.getByRole('button', { name: 'Edit', exact: true }).click();
      }
      const credential = row.locator('[data-testid="credential-form"]');
      await credential.getByLabel('Read token').fill(`read-token-${run}`);
      await credential.getByRole('button', { name: 'Set token' }).click();
      await expect(page.getByText(`Kairos keeps the read token of "${slug}".`)).toBeVisible();
      await row.getByRole('button', { name: 'Import open issues' }).click();
      await expect(line('issue-import-status')).toContainText('Import of open issues: done.', {
        timeout: 45_000,
      });
      await expect(line('issue-import-status')).toContainText('Kairos imported 2 issue(s)');
      expect(
        fake.requests.some(
          (r) => r.method === 'GET' && r.path === `/repos/${fullName}/issues` && r.token === `read-token-${run}`,
        ),
      ).toBe(true);
    });
  } finally {
    const items: any[] = await readEachPage(GUI, alice, '/api/tasks').catch(
      () => [],
    );
    for (const item of items) {
      if (String(item.title).includes(run)) await tryArchiveTask(GUI, alice, item.short_code);
    }
    await tryArchiveTask(GUI, alice, code);
    await fetch(`${GUI}/api/repositories/${slug}`, {
      method: 'DELETE',
      headers: { authorization: `Bearer ${alice}` },
    });
  }
});
