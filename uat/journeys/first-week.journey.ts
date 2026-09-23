// J13 — "A newcomer finds their way around without writing anything"
// (KAIROS-I-0014). The start of the arc: bob has just been given access
// and spends an hour reading.
//
// Every other journey writes, which means the read paths — the boards
// overview by flight level, a parent's progress rollup, a team's in-flight
// work, an agent's queue narrowed to its repository — are the least
// exercised part of the product and the first thing a new person meets.
// Nothing here creates anything, so there is nothing to tear down: the
// journey's whole claim is "what a newcomer needs is where they would
// look for it".
import { expect } from '@playwright/test';
import { journey, step } from '../run/narrate';
import { openBoard, openTeam, panel } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

const TEAM = process.env.UAT_TEAM ?? 'platform';

journey(
  'first-week',
  'A newcomer finds their way around without writing anything',
  { humans: ['bob'] },
  async ({ cast }) => {
    const bob = cast.human('bob');

    await step(bob, 'lands on the boards page and sees the work sorted by flight level', async () => {
      const page = await bob.gui();
      await page.goto('/boards');
      // Strategy above initiatives above delivery is the product's whole
      // orientation claim (KAIROS-T-0063); if the bands are unordered a
      // newcomer has no way to read the org.
      const bands = page.locator('.kairos-board-band, section').filter({ hasText: 'Delivery' });
      await expect(page.getByText('Strategy', { exact: true }).first()).toBeVisible();
      await expect(page.getByText('Initiatives', { exact: true }).first()).toBeVisible();
      await expect(bands.first()).toBeVisible();
      const tiles = await page.locator('a.kairos-board-tile').count();
      expect(tiles).toBeGreaterThan(0);
      const headings = await page.locator('body').innerText();
      const order = ['Strategy', 'Initiatives', 'Delivery'].map((b) => headings.indexOf(b));
      expect(order[0]).toBeLessThan(order[1]);
      expect(order[1]).toBeLessThan(order[2]);
      return { boards_visible: tiles, band_order: 'strategy → initiatives → delivery' };
    });

    let initiative = '';
    await step(bob, 'opens an initiative and reads how far along it is', async () => {
      const page = await bob.gui();
      await openBoard(page, 'initiatives');
      const card = page.locator('article.kairos-card', { has: page.locator('.kairos-card__progress') }).first();
      await expect(card).toBeVisible();
      initiative = (await card.locator('a.kairos-card__code').innerText()).trim();
      // The card badge and the detail bar have to agree; a newcomer who
      // sees 1/5 on the board and something else inside stops trusting it.
      const badge = (await card.locator('.kairos-card__progress').innerText()).replace(/\s+/g, ' ').trim();
      await card.locator('a.kairos-card__code').click();
      await page.waitForURL(/\/items\//);
      const bar = page.locator('.kairos-progress').first();
      await expect(bar).toBeVisible();
      const detail = (await bar.innerText()).replace(/\s+/g, ' ').trim();
      const done = badge.match(/(\d+)\s*\/\s*(\d+)/);
      expect(done, `card badge reads N/M: ${badge}`).toBeTruthy();
      expect(detail).toContain(`${done![1]} of ${done![2]}`);
      return { initiative, card_badge: badge, detail_bar: detail };
    });

    await step(bob, 'follows a child task down to the work itself', async () => {
      const page = await bob.gui();
      const children = panel(page, 'Relationships');
      await expect(children).toBeVisible();
      const child = children.locator('a[href*="/items/"]').filter({ hasText: /-T-\d{4}/ }).first();
      await expect(child).toBeVisible();
      // The link text carries the title as well as the code, so take the
      // code from the href a newcomer would actually follow.
      const code = ((await child.getAttribute('href')) ?? '').split('/items/')[1] ?? '';
      expect(code).toMatch(/-T-\d{4}/);
      await child.click();
      await page.waitForURL(new RegExp(code));
      await expect(page.getByText(code).first()).toBeVisible();
      return { followed_to: code };
    });

    await step(bob, 'reads his team page: who is on it, what it owns, what is in flight', async () => {
      const page = await bob.gui();
      await openTeam(page, TEAM);
      await expect(panel(page, 'Charter').locator('.kairos-markdown')).toBeVisible();
      const repos = panel(page, 'Repositories');
      await expect(repos.locator('[data-repo]').first()).toBeVisible();
      const owned = await repos.locator('[data-repo]').count();
      const inFlight = panel(page, 'In flight');
      await expect(inFlight).toBeVisible();
      return { repositories_owned: owned, in_flight_panel: 'present' };
    });

    await step(bob, 'asks his agent-side tools the same questions and gets the same answers', async () => {
      const mcp = await bob.mcp();
      const boards = await mcp.call('my_boards');
      expect(boards).toContain(`${TEAM}-delivery`);
      // The queue a repo-scoped agent would work from — the newcomer's
      // "what should I pick up" question, asked the way the plugin asks it.
      const repo = await mcp.call('list_repositories', { team: TEAM });
      const [slug] = repo.match(/^- ([a-z0-9-]+) —/m)?.slice(1) ?? [];
      expect(slug).toBeTruthy();
      const queue = await mcp.call('board_items', { board: `${TEAM}-delivery`, repository: slug });
      return {
        my_board: `${TEAM}-delivery`,
        repository: slug,
        queue_size: shortCodes(queue).length,
      };
    });

    await step(bob, 'checks from the terminal that he is who he thinks he is', async () => {
      const cli = await bob.cli();
      const me = await cli.json(['whoami']);
      const teams = (me.teams as any[]).map((t) => t.slug);
      expect(teams).toContain(TEAM);
      // A new member holds no explicit grants; his powers come from team
      // membership (A-0006 / KAIROS-T-0072). Worth reading once, because
      // "why can I do this?" is the first question a newcomer asks.
      return {
        teams,
        explicit_grants: (me.capabilities ?? []).length,
        implicit: me.implicit,
      };
    });
  },
);
