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
 * Drag a card to a column and wait until it is there — once, no retry.
 *
 * KAIROS-T-0124 #8: `locator.dragTo` hovers the target AFTER the mouse is
 * down, and a hover scrolls the target into view. When the board overflows
 * the viewport (five 260px columns, two lanes) that scroll lands between
 * `mousedown` and the first `mousemove`; Chromium then hit-tests the drag
 * origin at the stale viewport point, finds no draggable element there and
 * never starts the drag (no `dragstart`, no `drop` — nothing reaches the
 * page). The second attempt only worked because the page was already
 * scrolled. So: bring both ends into view first, press, and move to a point
 * of the target that is on screen without scrolling again.
 */
export async function dragCard(page: Page, code: string, toColumn: string): Promise<void> {
  const source = card(page, code).first();
  const target = column(page, toColumn);
  await target.scrollIntoViewIfNeeded();
  await source.scrollIntoViewIfNeeded();
  const from = await source.boundingBox();
  if (!from) throw new Error(`card ${code} has no bounding box`);
  const at = await visiblePoint(page, target, `column ${toColumn}`);
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(at.x, at.y, { steps: 2 });
  await page.mouse.up();
  await expect(cardIn(page, toColumn, code)).toBeVisible({ timeout: 15_000 });
}

/** The centre of the part of `target` that is inside the viewport. */
async function visiblePoint(page: Page, target: Locator, what: string): Promise<{ x: number; y: number }> {
  const box = await target.boundingBox();
  const viewport = page.viewportSize();
  if (!box) throw new Error(`${what} has no bounding box`);
  if (!viewport) throw new Error('the page has no viewport size');
  const left = Math.max(box.x, 0);
  const top = Math.max(box.y, 0);
  const right = Math.min(box.x + box.width, viewport.width);
  const bottom = Math.min(box.y + box.height, viewport.height);
  if (right - left < 4 || bottom - top < 4) throw new Error(`${what} is not in the viewport`);
  return { x: (left + right) / 2, y: (top + bottom) / 2 };
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
