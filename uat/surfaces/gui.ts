// The GUI surface a human persona holds: the things a person does in the
// Leptos app, named as they would name them. Selectors prefer roles and
// visible text; the stable `.kairos-*` / `.cl-*` classes are used only
// where text alone is ambiguous (a board column, a card).
import { expect, type BrowserContext, type Locator, type Page } from '@playwright/test';
import type { Credentials } from '../personas/credentials';

/** Log in through the real Dex form and land on the boards page. */
export async function login(page: Page, creds: Credentials): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', creds.email);
  await page.fill('#password', creds.password);
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), { timeout: 30_000 });
}

export async function openBoard(page: Page, slug: string): Promise<void> {
  await page.goto(`/boards/${slug}`);
  await expect(page.locator('section.kairos-board__column').first()).toBeVisible();
}

export async function openItem(page: Page, code: string): Promise<void> {
  await page.goto(`/items/${code}`);
  await expect(page.getByText(code).first()).toBeVisible();
}

export async function openTeam(page: Page, slug: string): Promise<void> {
  await page.goto(`/teams/${slug}`);
}

/** A board column by its heading, within the planned lane when the board has lanes. */
export function column(page: Page, name: string): Locator {
  const lane = page.locator('section.kairos-board__lane--planned');
  const scope = lane.or(page.locator('main'));
  return scope.first().locator('section.kairos-board__column', {
    has: page.locator('.kairos-board__column-head', { hasText: name }),
  });
}

/** The card carrying a short code, anywhere on the board. */
export function card(page: Page, code: string): Locator {
  return page.locator('article.kairos-card', { hasText: code });
}

/** The card carrying a short code, in one column. */
export function cardIn(page: Page, columnName: string, code: string): Locator {
  return column(page, columnName).locator('article.kairos-card', { hasText: code });
}

/**
 * Drag a card to a column and wait until it is there. A WS refetch can
 * re-render the board mid-drag and swallow the drop (the e2e drag spec's
 * deflake note), so one retry is allowed before it counts as a failure.
 */
export async function dragCard(page: Page, code: string, toColumn: string): Promise<void> {
  const target = cardIn(page, toColumn, code);
  for (let attempt = 0; attempt < 2; attempt++) {
    await card(page, code).first().dragTo(column(page, toColumn));
    try {
      await expect(target).toBeVisible({ timeout: attempt === 0 ? 5_000 : 15_000 });
      return;
    } catch (err) {
      if (attempt === 1) throw err;
    }
  }
}

/** The column a card currently sits in, read from the DOM. */
export async function columnOf(page: Page, code: string): Promise<string> {
  const col = page.locator('section.kairos-board__column', { has: card(page, code) }).first();
  return (await col.locator('.kairos-board__column-head').first().innerText()).trim().split('\n')[0];
}

/** A named panel on team/item pages (`.cl-panel` titled …). */
export function panel(page: Page, title: string): Locator {
  return page.locator('.cl-panel', { has: page.locator('.cl-panel__title', { hasText: title }) });
}

/** Open a fresh page in a persona's context (the context is already logged in). */
export async function newPage(context: BrowserContext): Promise<Page> {
  return context.newPage();
}
