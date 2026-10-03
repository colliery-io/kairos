// COLLIERY-T-0265 — the forms that have a slug give the rule of the slug,
// and they show a refusal about the slug below the field.
//
// Before the ticket the forms of a team, a delivery stream and a board gave
// no rule. A refusal of the server was a notice at the top of the page, and
// the editor of a row closed before the answer came. The refusal of a
// repository slug named no field.
//
// The spec writes nothing: the server refuses each request.
//
//   1. REAL PKCE login (alice, org admin)
//   2. /admin/teams: the create form and the editor of a row
//   3. /admin/streams: the create form and the editor of a row
//   4. /admin/boards: the create form
//   5. /admin/repositories: the create form and the editor of a row
//   6. the API: no team, stream, board or repository is different

import { test, expect, type Locator, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

const SLUG_RULE =
  'Slug: 2 to 63 characters. The first character is a lowercase letter. Each other ' +
  'character is a lowercase letter, a digit, - or _. The slug cannot have the form of a UUID.';
const REPOSITORY_SLUG_RULE =
  'Slug: 2 to 63 characters. The first character is a lowercase letter or a digit. Each ' +
  'other character is a lowercase letter, a digit or -. The slug cannot have the form of a UUID.';

// COLLIERY-T-3099: the rule of the short-code prefix of a board.
const PREFIX_RULE =
  'Prefix: 2 to 10 characters. The first character is a capital letter. Each other character ' +
  'is a capital letter or a digit. Each item on the board gets a code with the prefix, for ' +
  'example SKADI-T-0001. The prefix does not change later.';
const prefixRefusal = (prefix: string) =>
  `The prefix "${prefix}" is not correct. A board prefix must match ^[A-Z][A-Z0-9]{1,9}$: a ` +
  'capital letter, then 1 to 9 capital letters or digits. Send a different code_prefix.';

const refusal = (kind: string, slug: string) =>
  `The ${kind} slug "${slug}" is not correct. A ${kind} slug must match ` +
  '^[a-z][a-z0-9_-]{1,62}$, and it cannot have the form of a UUID. Send a different slug.';

const panel = (page: Page, title: string): Locator =>
  page.locator('.cl-panel', {
    has: page.locator('.cl-panel__title', { hasText: title }),
  });

const field = (scope: Locator, label: string): Locator =>
  scope.locator('.cl-field', {
    has: scope.page().locator('.cl-field__label', { hasText: label }),
  });

/** Open a page of the administration with a link: the SPA keeps its session. */
async function openAdmin(page: Page, path: string, title: string) {
  await page.evaluate((to) => {
    window.history.pushState({}, '', to);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }, path);
  await expect(panel(page, title)).toBeVisible({ timeout: 30_000 });
}

/**
 * The editor of the first row of a list: the rule is there, a slug that is
 * not correct shows its refusal below the field, and the editor stays open.
 */
async function refusedInEditor(list: Locator, rule: string, slug: string, message: string) {
  await list.getByRole('button', { name: 'Edit', exact: true }).first().click();
  await expect(list.getByText(rule)).toBeVisible();
  const slugField = field(list, 'Slug');
  await slugField.locator('input').fill(slug);
  await list.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(slugField.locator('.cl-field__error')).toHaveText(message, { timeout: 10_000 });
  await expect(slugField.locator('input')).toHaveValue(slug);
  await expect(list.getByRole('button', { name: 'Save', exact: true })).toBeVisible();
}

test('slug forms: each form gives the rule and shows the refusal below the field', async ({
  page,
}) => {
  // The API token FIRST (see repositories.spec.ts).
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const slugsOf = async (path: string): Promise<string[]> => {
    const res = await fetch(GUI + path, { headers: { authorization: `Bearer ${alice}` } });
    expect(res.status, `GET ${path}`).toBe(200);
    const body = await res.json();
    return ((body.items ?? body) as any[]).map((row) => row.slug as string).sort();
  };
  const lists = [
    '/api/teams?limit=200',
    '/api/delivery-streams?limit=200',
    '/api/boards?limit=200',
    '/api/repositories',
  ];
  const before = await Promise.all(lists.map(slugsOf));

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

  await test.step('teams: the create form and the editor of a row', async () => {
    await openAdmin(page, '/admin/teams', 'Create team');
    const form = panel(page, 'Create team');
    await expect(form.getByTestId('slug-rule')).toHaveText(SLUG_RULE);
    await field(form, 'Name').locator('input').fill('Road map');
    await field(form, 'Slug').locator('input').fill('Road Map');
    await form.getByRole('button', { name: 'Create team' }).click();
    await expect(field(form, 'Slug').locator('.cl-field__error')).toHaveText(
      refusal('team', 'Road Map'),
      { timeout: 10_000 },
    );
    // The form keeps the values, and the other field has no refusal.
    await expect(field(form, 'Name').locator('input')).toHaveValue('Road map');
    await expect(field(form, 'Name').locator('.cl-field__error')).toHaveCount(0);
    // COLLIERY-T-3099: the prefix of the delivery board of the team.
    await expect(form.getByTestId('prefix-rule')).toHaveText(PREFIX_RULE);
    await field(form, 'Slug').locator('input').fill('road-map');
    await field(form, 'Prefix').locator('input').fill('road');
    await form.getByRole('button', { name: 'Create team' }).click();
    await expect(field(form, 'Prefix').locator('.cl-field__error')).toHaveText(
      prefixRefusal('road'),
      { timeout: 10_000 },
    );
    await refusedInEditor(panel(page, 'All teams'), SLUG_RULE, '9lives', refusal('team', '9lives'));
  });

  await test.step('delivery streams: the create form and the editor of a row', async () => {
    await openAdmin(page, '/admin/streams', 'Create delivery stream');
    const form = panel(page, 'Create delivery stream');
    await expect(form.getByTestId('slug-rule')).toHaveText(SLUG_RULE);
    await field(form, 'Name').locator('input').fill('Check out');
    await field(form, 'Slug').locator('input').fill('check.out');
    await form.getByRole('button', { name: 'Create stream' }).click();
    await expect(field(form, 'Slug').locator('.cl-field__error')).toHaveText(
      refusal('delivery stream', 'check.out'),
      { timeout: 10_000 },
    );
    await refusedInEditor(
      panel(page, 'All streams'),
      SLUG_RULE,
      'abcdef12-0000-7000-8000-000000000003',
      refusal('delivery stream', 'abcdef12-0000-7000-8000-000000000003'),
    );
  });

  await test.step('boards: the create form', async () => {
    await openAdmin(page, '/admin/boards', 'Create board');
    const form = panel(page, 'Create board');
    await expect(form.getByTestId('slug-rule')).toHaveText(SLUG_RULE);
    await field(form, 'Name').locator('input').fill('Road map');
    await field(form, 'Slug').locator('input').fill('_roadmap');
    await form.getByRole('button', { name: 'Create board' }).click();
    await expect(field(form, 'Slug').locator('.cl-field__error')).toHaveText(
      refusal('board', '_roadmap'),
      { timeout: 10_000 },
    );
    // COLLIERY-T-3099: the form gives the rule of the prefix, and a
    // refusal about the prefix shows below the field.
    await expect(form.getByTestId('prefix-rule')).toHaveText(PREFIX_RULE);
    await field(form, 'Slug').locator('input').fill('roadmap-prefix');
    await field(form, 'Prefix').locator('input').fill('sk-adi');
    await form.getByRole('button', { name: 'Create board' }).click();
    await expect(field(form, 'Prefix').locator('.cl-field__error')).toHaveText(
      prefixRefusal('sk-adi'),
      { timeout: 10_000 },
    );
    await expect(field(form, 'Slug').locator('.cl-field__error')).toHaveCount(0);
  });

  await test.step('repositories: the create form and the editor of a row', async () => {
    await openAdmin(page, '/admin/repositories', 'Register repository');
    const form = panel(page, 'Register repository');
    await expect(form.locator('[data-testid="repository-rules"]')).toContainText(
      REPOSITORY_SLUG_RULE,
    );
    const message =
      'The repository slug "under_score" is not correct. A repository slug must match ' +
      '^[a-z0-9][a-z0-9-]{1,62}$.';
    await field(form, 'Full name').locator('input').fill('acme/slug-form');
    await field(form, 'URL').locator('input').fill('https://github.com/acme/slug-form');
    await field(form, 'Slug (optional)').locator('input').fill('under_score');
    await form.getByRole('button', { name: 'Register repository' }).click();
    await expect(field(form, 'Slug (optional)').locator('.cl-field__error')).toHaveText(message, {
      timeout: 10_000,
    });
    await expect(field(form, 'URL').locator('.cl-field__error')).toHaveCount(0);
    await refusedInEditor(
      panel(page, 'All repositories'),
      REPOSITORY_SLUG_RULE,
      'under_score',
      message,
    );
  });

  await test.step('the server wrote nothing', async () => {
    expect(await Promise.all(lists.map(slugsOf))).toEqual(before);
  });
});
