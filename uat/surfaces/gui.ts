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
  // COLLIERY-T-1836: the Aurora AppShell has a <main>, and the lane is in
  // it. The union is in document order, so the last element is the lane
  // when the board has lanes, and <main> when it has none.
  const scope = lane.or(page.locator('main'));
  return scope.last().locator('section.kairos-board__column', {
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

// Lane-scoped selectors. `column()` resolves inside the PLANNED lane, which
// is right for a journey about planned work and useless for one about a
// request or an incident: that one needs to say which lane it means on every
// assertion, because "the card is in Active" is exactly the claim that hides
// a lane bug. They were local to the incident journey until COLLIERY-T-0218
// put every request in the Support lane, and the cross-team journey needed
// them too.
export type Lane = 'planned' | 'support';

export function laneColumn(page: Page, lane: Lane, name: string): Locator {
  return page.locator(`section.kairos-board__lane--${lane}`).locator('section.kairos-board__column', {
    has: page.locator('.kairos-board__column-head', { hasText: name }),
  });
}

export function laneCard(page: Page, lane: Lane, columnName: string, code: string): Locator {
  return laneColumn(page, lane, columnName).locator('article.kairos-card', { hasText: code });
}

/** Drag a card to a column IN A NAMED LANE (see `gui.dragCard` for why the
 * mouse is driven by hand rather than through `dragTo`). */
export async function dragInLane(page: Page, code: string, lane: Lane, toColumn: string): Promise<void> {
  const target = laneColumn(page, lane, toColumn);
  await dragToTarget(page, card(page, code).first(), target, `card ${code}`, `${lane}/${toColumn}`);
  await expect(laneCard(page, lane, toColumn, code)).toBeVisible({ timeout: 15_000 });
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
  const target = column(page, toColumn);
  await dragToTarget(page, card(page, code).first(), target, `card ${code}`, `column ${toColumn}`);
  await expect(cardIn(page, toColumn, code)).toBeVisible({ timeout: 15_000 });
}

/**
 * The drag of `dragCard` and `dragInLane` (COLLIERY-T-0253). The two ends
 * are not always in one viewport: a column of a different lane can be some
 * screens from the card. A person then does this: press, move the card
 * (the drag starts), turn the wheel until the target shows, move to it,
 * release. The wheel turns only AFTER the drag starts, so the defect of
 * KAIROS-T-0124 #8 does not come back. The same approach is in
 * `e2e/helpers/drag.ts`. The two packages share no code.
 */
async function dragToTarget(
  page: Page,
  source: Locator,
  target: Locator,
  sourceName: string,
  targetName: string,
): Promise<void> {
  await target.scrollIntoViewIfNeeded();
  await source.scrollIntoViewIfNeeded();
  const from = await source.boundingBox();
  if (!from) throw new Error(`${sourceName} has no bounding box`);
  const origin = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
  const near = await visiblePoint(page, target, targetName);
  await page.mouse.move(origin.x, origin.y);
  await page.mouse.down();
  if (near) {
    await page.mouse.move(near.x, near.y, { steps: 2 });
  } else {
    // Start the drag on the card, then scroll, then go to the target.
    await page.mouse.move(origin.x + 12, origin.y + 12, { steps: 2 });
    const far = await wheelTo(page, target, targetName);
    await page.mouse.move(far.x, far.y, { steps: 2 });
  }
  await page.mouse.up();
}

/** The centre of the part of `target` that is inside the viewport, or null. */
async function visiblePoint(
  page: Page,
  target: Locator,
  what: string,
): Promise<{ x: number; y: number } | null> {
  const box = await target.boundingBox();
  const viewport = page.viewportSize();
  if (!box) throw new Error(`${what} has no bounding box`);
  if (!viewport) throw new Error('the page has no viewport size');
  const left = Math.max(box.x, 0);
  const top = Math.max(box.y, 0);
  const right = Math.min(box.x + box.width, viewport.width);
  const bottom = Math.min(box.y + box.height, viewport.height);
  if (right - left < 4 || bottom - top < 4) return null;
  return { x: (left + right) / 2, y: (top + bottom) / 2 };
}

/**
 * Turn the wheel, with the mouse button down and the drag in progress,
 * until a part of `target` is in the viewport. Returns a point of that part.
 */
async function wheelTo(page: Page, target: Locator, what: string): Promise<{ x: number; y: number }> {
  const viewport = page.viewportSize();
  if (!viewport) throw new Error('the page has no viewport size');
  // Each turn is less than one viewport, so the target cannot go past.
  const turn = { x: viewport.width / 2, y: viewport.height / 2 };
  for (let turns = 0; turns < 200; turns += 1) {
    const at = await visiblePoint(page, target, what);
    if (at) return at;
    const box = await target.boundingBox();
    if (!box) throw new Error(`${what} has no bounding box`);
    const before = { x: box.x, y: box.y };
    const dx = box.x + box.width <= 0 ? -turn.x : box.x >= viewport.width ? turn.x : 0;
    const dy = box.y + box.height <= 0 ? -turn.y : box.y >= viewport.height ? turn.y : 0;
    await page.mouse.wheel(dx, dy);
    // The scroll is not synchronous with the wheel event. Wait until the
    // target moves; a target that does not move cannot be reached.
    const deadline = Date.now() + 2_000;
    for (;;) {
      const now = await target.boundingBox();
      if (now && (now.x !== before.x || now.y !== before.y)) break;
      if (Date.now() > deadline) {
        throw new Error(`the page does not scroll to ${what} during the drag`);
      }
      await page.waitForTimeout(50);
    }
  }
  throw new Error(`${what} is not in the viewport`);
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
