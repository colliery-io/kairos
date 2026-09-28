// A scroll-safe drag for board cards (KAIROS-T-0124 #8, applied to the e2e
// tier in KAIROS-T-0126).
//
// `locator.dragTo` hovers the target AFTER the mouse is down, and a hover
// scrolls the target into view. On a board that overflows the viewport
// (two lanes, wrapped card heads) that scroll lands between `mousedown` and
// the first `mousemove`; Chromium then hit-tests the drag origin at the
// stale viewport point and either never starts the drag or drops wherever
// the pointer now is. A person never scrolls mid-press, so: bring both ends
// into view first, press, move to a point of the target that is on screen,
// release.
//
// COLLIERY-T-0239: the two ends are not always in one viewport. A column is
// as tall as its cards, so the card at the bottom of a column of 30 cards is
// some screens below the head of the adjacent column. A person then does
// this: press, move the card (the drag starts), turn the wheel until the
// target column shows, move to it, release. The helper does the same. The
// wheel turns only AFTER the drag starts, so the defect above does not
// come back: the drag origin is hit-tested before the page scrolls.
//
// COLLIERY-T-0253: each column is now as tall as the tallest column of its
// lane, so the adjacent column of the same lane is at the height of the
// card. `dragAcross` does that drag, and it fails if the page scrolls. The
// wheel stays in `dragTo` for a target that is not in the viewport: a
// column of a different lane.
import type { Locator, Page } from '@playwright/test';

interface Point {
  x: number;
  y: number;
}

/** The part of `target` that is inside the viewport, or null. */
async function visiblePoint(page: Page, target: Locator): Promise<Point | null> {
  const box = await target.boundingBox();
  const viewport = page.viewportSize();
  if (!box) throw new Error('drag target has no bounding box');
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
async function wheelTo(page: Page, target: Locator): Promise<Point> {
  const viewport = page.viewportSize();
  if (!viewport) throw new Error('the page has no viewport size');
  // Each turn is less than one viewport, so the target cannot go past.
  const turn = { x: viewport.width / 2, y: viewport.height / 2 };
  for (let turns = 0; turns < 200; turns += 1) {
    const at = await visiblePoint(page, target);
    if (at) return at;
    const box = await target.boundingBox();
    if (!box) throw new Error('drag target has no bounding box');
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
        throw new Error('the page does not scroll to the drag target during the drag');
      }
      await page.waitForTimeout(50);
    }
  }
  throw new Error('drag target is not in the viewport');
}

export async function dragTo(page: Page, source: Locator, target: Locator): Promise<void> {
  await target.scrollIntoViewIfNeeded();
  await source.scrollIntoViewIfNeeded();
  const from = await source.boundingBox();
  if (!from) throw new Error('drag source has no bounding box');
  const origin = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
  const near = await visiblePoint(page, target);
  await page.mouse.move(origin.x, origin.y);
  await page.mouse.down();
  if (near) {
    await page.mouse.move(near.x, near.y, { steps: 2 });
  } else {
    // Start the drag on the card, then scroll, then go to the target.
    await page.mouse.move(origin.x + 12, origin.y + 12, { steps: 2 });
    const far = await wheelTo(page, target);
    await page.mouse.move(far.x, far.y, { steps: 2 });
  }
  await page.mouse.up();
}

/**
 * Drag `source` to `target` at the height of `source`, with no scroll
 * (COLLIERY-T-0253): press on the card, move horizontally to the target
 * column, release. It throws if the target has no part at that height, and
 * if the page scrolls between the press and the release.
 */
export async function dragAcross(page: Page, source: Locator, target: Locator): Promise<void> {
  await source.scrollIntoViewIfNeeded();
  const from = await source.boundingBox();
  const to = await target.boundingBox();
  if (!from) throw new Error('drag source has no bounding box');
  if (!to) throw new Error('drag target has no bounding box');
  const origin = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
  if (origin.y < to.y || origin.y > to.y + to.height) {
    throw new Error(
      `the drag target is not at the height of the card: card y=${origin.y}, ` +
        `target y=${to.y} to ${to.y + to.height}`,
    );
  }
  const scroll = () => page.evaluate(() => ({ x: window.scrollX, y: window.scrollY }));
  const before = await scroll();
  await page.mouse.move(origin.x, origin.y);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, origin.y, { steps: 4 });
  await page.mouse.up();
  const after = await scroll();
  if (after.x !== before.x || after.y !== before.y) {
    throw new Error(`the page scrolled during the drag: ${JSON.stringify({ before, after })}`);
  }
}
