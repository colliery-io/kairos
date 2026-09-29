// KAIROS-T-0070 — the KAIROS-I-0006 "teams as a first-class lens" smoke.
//
// One serial flow as bob (a NON-ADMIN member of Platform — the seed fixture
// puts alice+bob on platform, carol on web; crates/kairos-db/src/seed.rs):
//
//   1. REAL PKCE login through the Dex login form (bob)
//   2. /boards shows the flight-level bands, delivery grouped by team
//   3. "My teams" in the nav shows Platform (and not Web); click through
//   4. /teams/platform: roster (alice, bob), delivery board link,
//      customer-portal stream — reachable without any admin capability
//   5. the delivery board link lands on the platform-delivery board
//   6. /teams directory lists both seeded teams
//   7. /activity: the filter by team is a filter of the server
//      (COLLIERY-T-0265), so the count below the table is the count of the
//      entries by the members of the team
//
// Conventions match smoke.spec.ts: visible-text/role selectors plus the
// stable `.kairos-*`/`.cl-*` class names (aurora Select renders
// `.cl-field > .cl-field__label + select.cl-select` — label and select are
// siblings). All navigation happens through in-app links because the SPA
// holds its token in memory only (A-0015).

import { test, expect, type Page } from '@playwright/test';
import { dragTo } from '../helpers/drag';
import { mintToken } from '../helpers/auth';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

const navbar = (page: Page) => page.locator('.cl-appshell__navbar');

test('team lens: bob → my teams → roster/board/stream → directory → activity filter', async ({
  page,
}) => {
  // The API token of step 7, and FIRST (see repositories.spec.ts). It is
  // the token of alice: a token of bob stops the session of the browser.
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });

  // 1. Real in-browser PKCE login via the Dex form (bob, non-admin) --------
  await test.step('login via Dex as bob (non-admin)', async () => {
    await page.goto('/');
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'bob@kairos.test');
    await page.fill('#password', 'bob-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
      timeout: 30_000,
    });
    const header = page.locator('.cl-appshell__header');
    await expect(header.getByText('bob', { exact: true })).toBeVisible();
    await expect(header.locator('.cl-pill', { hasText: 'member' })).toBeVisible();
  });

  // 1b. Reload survives (KAIROS-T-0071): the stored refresh token restores
  //     the session silently — no Dex form, no login page ------------------
  await test.step('page reload keeps the session', async () => {
    await page.reload();
    // The restore runs before the guard redirects; we must land back on
    // the app, never on the Dex form (#login) or /login.
    const header = page.locator('.cl-appshell__header');
    await expect(header.getByText('bob', { exact: true })).toBeVisible({
      timeout: 20_000,
    });
    expect(new URL(page.url()).pathname.startsWith('/boards')).toBe(true);
  });

  // 2. /boards: level bands, delivery grouped by owning team ---------------
  await test.step('boards page shows level bands with team grouping', async () => {
    const bands = page.locator('.kairos-board-band');
    // strategy, initiative, delivery, adr — all four seeded levels present.
    await expect(bands).toHaveCount(4);
    // The delivery band carries team headings linking into the team pages.
    const delivery = bands.filter({
      has: page.locator('.kairos-board-tile', { hasText: 'Platform Delivery' }),
    });
    await expect(
      delivery.getByRole('link', { name: 'Platform', exact: true }),
    ).toBeVisible();
    await expect(
      delivery.getByRole('link', { name: 'Web', exact: true }),
    ).toBeVisible();
    // All five seeded boards are still reachable as tiles.
    await expect(page.locator('.kairos-board-tile')).toHaveCount(5);
  });

  // 3. "My teams" shows bob's team and navigates to it ---------------------
  await test.step('my-teams nav entry → team page', async () => {
    const section = page.locator('.kairos-nav__section');
    await expect(section.getByText('My teams')).toBeVisible();
    // bob is on platform only — web must NOT appear here.
    await expect(
      section.getByRole('link', { name: 'Platform', exact: true }),
    ).toBeVisible();
    await expect(
      section.getByRole('link', { name: 'Web', exact: true }),
    ).toHaveCount(0);
    await section.getByRole('link', { name: 'Platform', exact: true }).click();
    await page.waitForURL(/\/teams\/platform/);
  });

  // 4. Team detail: roster, board, stream — as a plain member --------------
  await test.step('team page shows roster, delivery board, stream', async () => {
    const members = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Members' }),
    });
    await expect(members.getByText('alice', { exact: true })).toBeVisible();
    await expect(members.getByText('bob', { exact: true })).toBeVisible();
    await expect(
      page.getByRole('link', { name: 'Platform Delivery', exact: true }),
    ).toBeVisible();
    await expect(page.getByText('Customer Portal')).toBeVisible();
  });

  // 5. The delivery board link lands on the board --------------------------
  await test.step('delivery board link opens the board', async () => {
    await page
      .getByRole('link', { name: 'Platform Delivery', exact: true })
      .click();
    await page.waitForURL(/\/boards\/platform-delivery/);
    await expect(
      page.locator('section.kairos-board__column').first(),
    ).toBeVisible();
  });

  // 5b. KAIROS-T-0072: team membership implies delivery powers — bob (zero
  //     explicit grants) creates a task and drags it between columns.
  const createdTitle = `Bob's team card ${Date.now()}`;
  await test.step('bob creates and drags a card on his team board', async () => {
    // KAIROS-T-0077: scope to the Planned lane (defaults create there),
    // so drag targets stay unique on the two-lane delivery board.
    const lane = page.locator('section.kairos-board__lane--planned');
    const backlog = lane.locator('section.kairos-board__column', {
      has: page.locator('.kairos-board__column-head', { hasText: 'Backlog' }),
    });
    const todo = lane.locator('section.kairos-board__column', {
      has: page.locator('.kairos-board__column-head', { hasText: 'Todo' }),
    });
    // Implied manage_tasks: the global create action renders for bob
    // (KAIROS-T-0062: creation is intake — lands in Backlog).
    await page.getByRole('button', { name: 'New task', exact: true }).click();
    const modal = page.locator('.cl-modal');
    await modal.locator('input.cl-input').first().fill(createdTitle);
    await modal.getByRole('button', { name: 'Create' }).click();
    await expect(modal).toBeHidden();
    const card = backlog.locator('article.kairos-card', { hasText: createdTitle });
    await expect(card).toBeVisible();
    // Implied transition_items: draggable, and the drop lands.
    await expect(card).toHaveAttribute('draggable', 'true');
    await dragTo(page, card, todo);
    await expect(
      todo.locator('article.kairos-card', { hasText: createdTitle }),
    ).toBeVisible({ timeout: 15_000 });
  });

  // 5c. Not bob's team, no grants: web-delivery shows him NO mutating
  //     affordances (drag disabled, no create) — and the item detail's
  //     move control (the T-0075 keyboard path) is gated the same way.
  await test.step('web-delivery offers bob no mutating affordances', async () => {
    await navbar(page).getByRole('link', { name: 'Boards', exact: true }).click();
    await page.waitForURL(/\/boards$/);
    await page.locator('.kairos-board-tile', { hasText: 'Web Delivery' }).click();
    await page.waitForURL(/\/boards\/web-delivery/);
    const firstCard = page.locator('article.kairos-card').first();
    await expect(firstCard).toBeVisible();
    await expect(firstCard).toHaveAttribute('draggable', 'false');
    await expect(
      page.getByRole('button', { name: 'New task', exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'New document', exact: true }),
    ).toHaveCount(0);

    // The keyboard fallback is capability-gated exactly like the drag:
    // a web-delivery item's detail page offers bob no "Move to" control.
    // (Navigation via the short code — KAIROS-T-0076.)
    await firstCard.locator('a.kairos-card__code').click();
    await page.waitForURL(/\/items\//);
    const boardPanel = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Board' }),
    });
    // Placement renders (board name pill) — but no move select for bob.
    await expect(boardPanel.locator('.cl-pill').first()).toBeVisible();
    await expect(boardPanel.locator('select')).toHaveCount(0);
  });

  // 6. /teams directory lists both seeded teams ----------------------------
  await test.step('teams directory lists both teams', async () => {
    await navbar(page)
      .getByRole('link', { name: 'Teams', exact: true })
      .click();
    await page.waitForURL(/\/teams$/);
    const tiles = page.locator('.kairos-board-tile');
    await expect(tiles.filter({ hasText: 'Platform' })).toBeVisible();
    await expect(tiles.filter({ hasText: 'Web' })).toBeVisible();
    await expect(tiles).toHaveCount(2);
  });

  // 7. Activity: the server filters by the members of the team ------------
  await test.step('the activity filter by team is a filter of the server', async () => {
    await navbar(page)
      .getByRole('link', { name: 'Activity', exact: true })
      .click();
    await page.waitForURL(/\/activity$/);

    // The page gives the meaning of the filter before the person uses it.
    await expect(page.getByTestId('activity-team-rule')).toHaveText(
      'The filter Team shows the entries by the members of the team. ' +
        'The members are those that the team has now.',
    );
    // Apply the filter for Platform (aurora Select: label + sibling select
    // inside one .cl-field).
    const filters = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Filters' }),
    });
    const teamField = filters.locator('.cl-field', {
      has: page.locator('.cl-field__label', { hasText: 'Team (by members)' }),
    });
    await teamField.locator('select').selectOption({ label: 'Platform' });
    await filters.getByRole('button', { name: 'Apply' }).click();

    // The note names the team.
    await expect(page.getByTestId('activity-team-note')).toHaveText(
      'The list shows the entries by the members of Platform.',
      { timeout: 15_000 },
    );

    // The count of the pager is the count of the server after the filter,
    // and it is smaller than the count of the full feed.
    const get = async (query: string): Promise<any> => {
      const res = await fetch(`${GUI}/api/activity?${query}`, {
        headers: { authorization: `Bearer ${alice}` },
      });
      expect(res.status, `GET /api/activity?${query}`).toBe(200);
      return res.json();
    };
    const ofTeam = await get('team=platform&limit=25');
    const all = await get('limit=1');
    expect(ofTeam.total).toBeGreaterThan(0);
    expect(ofTeam.total).toBeLessThan(all.total);
    const shown = Math.min(25, ofTeam.total);
    const feed = page.locator('.cl-panel', {
      has: page.locator('.cl-panel__title', { hasText: 'Feed' }),
    });
    await expect(
      feed.getByText(`The list shows items 1 to ${shown} of ${ofTeam.total}.`),
    ).toBeVisible();
    await expect(feed.locator('tbody tr')).toHaveCount(shown);
    // Each entry of the page is by a member of the team.
    const members = new Set<string>(
      (ofTeam.items as any[]).map((entry) => entry.actor_id as string),
    );
    const roster = await (
      await fetch(`${GUI}/api/teams/by-slug/platform`, {
        headers: { authorization: `Bearer ${alice}` },
      })
    ).json();
    const team = await (
      await fetch(`${GUI}/api/teams/${roster.id}/members`, {
        headers: { authorization: `Bearer ${alice}` },
      })
    ).json();
    const ids = new Set<string>((team as any[]).map((member) => member.user_id as string));
    for (const actor of members) expect(ids.has(actor), `${actor} is a member`).toBe(true);
  });

  // 8. Logout then reload must NOT silently sign back in (KAIROS-T-0071:
  //    logout removes the stored refresh token) ----------------------------
  await test.step('logout, then reload stays logged out', async () => {
    await page.locator('.cl-appshell__header')
      .getByRole('button', { name: 'Log out' })
      .click();
    await page.waitForURL(/\/login/);
    await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();

    await page.reload();
    await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/login');
  });
});
