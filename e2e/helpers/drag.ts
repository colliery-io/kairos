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
import type { Locator, Page } from '@playwright/test';

export async function dragTo(page: Page, source: Locator, target: Locator): Promise<void> {
  await target.scrollIntoViewIfNeeded();
  await source.scrollIntoViewIfNeeded();
  const from = await source.boundingBox();
  if (!from) throw new Error('drag source has no bounding box');
  const box = await target.boundingBox();
  const viewport = page.viewportSize();
  if (!box) throw new Error('drag target has no bounding box');
  if (!viewport) throw new Error('the page has no viewport size');
  const left = Math.max(box.x, 0);
  const top = Math.max(box.y, 0);
  const right = Math.min(box.x + box.width, viewport.width);
  const bottom = Math.min(box.y + box.height, viewport.height);
  if (right - left < 4 || bottom - top < 4) throw new Error('drag target is not in the viewport');
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move((left + right) / 2, (top + bottom) / 2, { steps: 2 });
  await page.mouse.up();
}
