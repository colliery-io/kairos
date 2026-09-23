// J16 — "The leadership team reads the whole portfolio" (KAIROS-I-0014).
// The quarterly review: the one hour in the quarter when somebody reads
// every flight level at once and expects the numbers to add up.
//
// A rollup is a summary of a board somebody else owns. The failure mode is
// silent: the initiative card says 1/5, the two delivery boards underneath
// it say something else, and nobody notices because no single screen shows
// both. So this journey reads the SAME quantity three ways — the board's
// bulk rollup (what the card draws), the per-item rollup endpoint (a
// different query), and the actual cards sitting in the actual columns —
// and refuses to let them disagree. It also walks the blocked chain the
// review is really there to talk about, and asks the terminal the same
// question the boards were asked.
//
// Read-only from end to end: nothing is created, so there is nothing to
// tear down.
import { expect } from '@playwright/test';
import { journey, step } from '../run/narrate';
import { openBoard, openTeam, panel } from '../surfaces/gui';

/** `1/5 done` or `3 children` as the card badge renders it (KAIROS-T-0080). */
function readBadge(text: string): { done: number; total: number } {
  const fraction = text.match(/(\d+)\s*\/\s*(\d+)/);
  if (fraction) return { done: Number(fraction[1]), total: Number(fraction[2]) };
  const composition = text.match(/(\d+)\s+children/);
  if (composition) return { done: 0, total: Number(composition[1]) };
  throw new Error(`card progress badge reads neither "N/M done" nor "N children": ${text}`);
}

journey(
  'quarterly-review',
  'The leadership team reads the whole portfolio',
  { humans: ['alice', 'carol'] },
  async ({ cast }) => {
    const alice = cast.human('alice');
    const carol = cast.human('carol');

    let strategy = '';
    let initiatives: string[] = [];

    await step(alice, 'opens the review on the boards overview and counts the portfolio by flight level', async () => {
      const page = await alice.gui();
      await page.goto('/boards');
      await expect(page.locator('section.kairos-board-band').first()).toBeVisible();
      const bandOf = (label: string) =>
        page.locator('section.kairos-board-band').filter({ hasText: label });
      const counts: Record<string, number> = {};
      for (const label of ['Strategy', 'Initiatives', 'Delivery']) {
        counts[label] = await bandOf(label).first().locator('a.kairos-board-tile').count();
        expect(counts[label], `the ${label} band has boards`).toBeGreaterThan(0);
      }
      // A portfolio review only works if delivery is fanned out under one
      // initiative board: several teams, one place the bets are written
      // down (KAIROS-T-0063).
      expect(counts['Delivery'], 'more than one team delivers').toBeGreaterThan(1);
      expect(counts['Initiatives']).toBe(1);
      return {
        strategy_boards: counts['Strategy'],
        initiative_boards: counts['Initiatives'],
        delivery_boards: counts['Delivery'],
      };
    });

    await step(alice, 'opens the quarter\'s strategy and reads the bets hanging off it', async () => {
      const page = await alice.gui();
      await openBoard(page, 'strategy');
      const card = page.locator('article.kairos-card').first();
      await expect(card).toBeVisible();
      strategy = (await card.locator('a.kairos-card__code').innerText()).trim();
      await card.locator('a.kairos-card__code').click();
      await page.waitForURL(new RegExp(strategy));
      const relationships = panel(page, 'Relationships');
      await expect(relationships).toBeVisible();
      // The children carry their titles in the link text, so take the code
      // from the href a reader would follow (KAIROS-T-0137).
      const links = relationships.locator('a[href*="/items/"]');
      const hrefs = await links.evaluateAll((els) =>
        els.map((el) => (el as HTMLAnchorElement).getAttribute('href') ?? ''),
      );
      initiatives = [
        ...new Set(
          hrefs
            .map((h) => h.split('/items/')[1] ?? '')
            .filter((c) => /-I-\d{4}$/.test(c)),
        ),
      ];
      expect(initiatives.length, 'the strategy has initiatives under it').toBeGreaterThan(0);
      return { strategy, initiatives };
    });

    let lead = '';
    await step(alice, 'checks every initiative\'s progress badge against the rollup behind it', async () => {
      const page = await alice.gui();
      const api = await alice.api();
      await openBoard(page, 'initiatives');
      const cards = page.locator('article.kairos-card', {
        has: page.locator('.kairos-card__progress'),
      });
      const count = await cards.count();
      expect(count, 'initiatives on the board carry progress badges').toBeGreaterThan(0);
      const compared: string[] = [];
      let widest = -1;
      for (let i = 0; i < count; i++) {
        const card = cards.nth(i);
        const code = (await card.locator('a.kairos-card__code').innerText()).trim();
        const badge = readBadge((await card.locator('.kairos-card__progress').innerText()).trim());
        // The card is drawn from the BOARD's bulk rollup; this is the
        // per-item rollup, a different query over the same graph. A
        // portfolio read is worthless if the two can drift apart.
        const rollup = await api.get(`/api/initiatives/${code}/children-progress`);
        expect(rollup.total, `${code} total`).toBe(badge.total);
        expect(rollup.done, `${code} done`).toBe(badge.done);
        compared.push(`${code} ${badge.done}/${badge.total}`);
        if (badge.total > widest) {
          widest = badge.total;
          lead = code;
        }
      }
      return { initiatives_compared: count, badges: compared.join(', ') };
    });

    await step(alice, 'counts the lead initiative\'s cards herself, on the delivery boards that own them', async () => {
      const api = await alice.api();
      const rollup = await api.get(`/api/initiatives/${lead}/children-progress`);
      // The by-column breakdown names the boards the children live on —
      // the point of a portfolio rollup is that it crosses team lines.
      const boards = [...new Set((rollup.by_column as any[]).map((c) => c.board_id))];
      expect(boards.length, `${lead}'s children span more than one board`).toBeGreaterThan(1);
      // Now the ground truth: the cards actually sitting in the actual
      // columns of those boards, read the way the owning team reads them.
      const children = new Set<string>();
      const cli = await alice.cli();
      const traverse = await cli.json([
        'search', '--from', lead, '--relationships', 'parent', '--direction', 'outbound', '--depth', '1', '--limit', '100',
      ]);
      for (const task of traverse.results?.tasks ?? []) children.add(task.short_code);
      expect(children.size, 'the traverse found the children').toBe(rollup.total);
      let done = 0;
      const seen = new Set<string>();
      const boardNames: string[] = [];
      for (const boardId of boards) {
        const contents = await api.get(`/api/boards/${boardId}/items`);
        boardNames.push(contents.board.slug);
        for (const column of contents.columns ?? []) {
          for (const task of column.tasks ?? []) {
            if (!children.has(task.short_code)) continue;
            seen.add(task.short_code);
            if (column.column.is_done) done += 1;
          }
        }
      }
      expect(seen.size, 'every child was found on a board').toBe(rollup.total);
      expect(done, 'the done count is the cards in done columns').toBe(rollup.done);
      return {
        initiative: lead,
        boards: boardNames.sort().join(' + '),
        children: rollup.total,
        in_done_columns: done,
        rollup_says: `${rollup.done}/${rollup.total}`,
      };
    });

    let blocked = '';
    let blocker = '';
    await step(alice, 'finds the blocked chain the review is really about, and follows it into the graph', async () => {
      const page = await alice.gui();
      const api = await alice.api();
      const teams = (await api.get('/api/teams?limit=100')).items ?? [];
      const delivery = (await api.boards()).filter((b: any) => b.board_level === 'delivery');
      let boardSlug = '';
      for (const board of delivery) {
        const contents = await api.get(`/api/boards/${board.id}/items`);
        const summary = contents.blocks_summary ?? {};
        const entry = Object.entries(summary).find(([, v]: any) => v.blocked_by > 0);
        if (!entry) continue;
        blocked = entry[0];
        boardSlug = board.slug;
        // Which card is in the way is a question for the item, not the
        // board summary: the summary only counts.
        const edges = await api.get(`/api/tasks/${blocked}/relationships`);
        const incoming = (edges.incoming ?? []).find((g: any) => g.relationship === 'blocks');
        blocker = incoming?.items?.[0]?.short_code ?? '';
        break;
      }
      expect(blocked, 'a card on a delivery board is blocked').toBeTruthy();
      expect(blocker, 'and something is blocking it').toBeTruthy();
      await openBoard(page, boardSlug);
      const card = page.locator('article.kairos-card', { hasText: blocked });
      const pill = card.locator('a.kairos-card__blocks', { hasText: 'blocked by' }).first();
      await expect(pill).toBeVisible();
      const label = (await pill.innerText()).trim();
      // The pill is a door, not a decoration: it opens the item's graph,
      // which is where the chain is legible.
      await pill.click();
      await page.waitForURL(/view=graph/);
      // SVG `<text>` has no innerText — read text content, and wait for the
      // canvas to draw before counting anything on it.
      const drawn = page.locator('text.kairos-graph__code');
      await expect(drawn.first()).toBeVisible();
      const codes = (await drawn.allTextContents()).map((c) => c.trim());
      expect(codes).toContain(blocked);
      expect(codes).toContain(blocker);
      // `blocks` is the only relationship the canvas draws as an arrow.
      expect(await page.locator('path.kairos-graph__edge').count()).toBeGreaterThan(0);
      return {
        team_boards_read: delivery.length,
        teams: teams.length,
        blocked,
        pill: label,
        blocker_on_canvas: blocker,
      };
    });

    await step(alice, 'asks the terminal the same question and gets the same portfolio back', async () => {
      const cli = await alice.cli();
      const walked = await cli.json([
        'search', '--from', strategy, '--relationships', 'parent', '--direction', 'outbound', '--depth', '2', '--limit', '100',
      ]);
      const reached = {
        initiatives: (walked.results?.initiatives ?? []).map((i: any) => i.short_code),
        tasks: (walked.results?.tasks ?? []).map((t: any) => t.short_code),
      };
      for (const code of initiatives) {
        expect(reached.initiatives, `${code} is under ${strategy}`).toContain(code);
      }
      // The half of the portfolio a strategy review is blind to: standing
      // buckets (bugs, tech debt) hang off no bet, so their work never
      // appears in this walk. Worth seeing on the page, not discovering in
      // the meeting.
      const allTasks = (await cli.json(['search', '--type', 'task', '--limit', '100'])).results?.tasks ?? [];
      const unaccounted = allTasks
        .map((t: any) => t.short_code)
        .filter((c: string) => !reached.tasks.includes(c));
      expect(reached.tasks.length, 'the strategy accounts for delivery work').toBeGreaterThan(0);
      return {
        traverse: `${strategy} → parent → depth 2`,
        initiatives_reached: reached.initiatives.length,
        tasks_reached: reached.tasks.length,
        tasks_outside_the_strategy: unaccounted.length,
      };
    });

    await step(carol, 'reads the delivery stream: which teams are in it and what each has in flight', async () => {
      const page = await carol.gui();
      const api = await carol.api();
      const streams = (await api.get('/api/delivery-streams')).items ?? [];
      let streamName = '';
      let members: any[] = [];
      for (const stream of streams) {
        const teams = await api.get(`/api/delivery-streams/${stream.id}/teams`);
        if ((teams ?? []).length < 2) continue;
        streamName = stream.name;
        members = teams;
        break;
      }
      expect(members.length, 'a stream carries more than one team').toBeGreaterThan(1);
      const observed: string[] = [];
      let documents = 0;
      for (const team of members) {
        const docs = await api.get(`/api/teams/${team.id}/work-documents`);
        documents += (docs ?? []).length;
        await openTeam(page, team.slug);
        await expect(panel(page, 'Charter').locator('.kairos-markdown')).toBeVisible();
        // A stream review asks "what is open where" — the team page is
        // where that lives: the delivery board, the documents hanging off
        // its work, and the branches and PRs in flight.
        await expect(panel(page, 'Delivery streams')).toContainText(streamName);
        await expect(panel(page, 'In flight')).toBeVisible();
        const links = await api.get(`/api/teams/${team.id}/links`);
        observed.push(`${team.slug}: ${(links ?? []).length} in flight, ${(docs ?? []).length} docs`);
      }
      expect(documents, 'the stream has documents attached to its work').toBeGreaterThan(0);
      return { stream: streamName, teams: members.map((t: any) => t.slug).join(' + '), per_team: observed.join('; ') };
    });
  },
);
